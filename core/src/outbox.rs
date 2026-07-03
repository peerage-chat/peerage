//! Персистентная офлайн-очередь исходящих (веха M3/H3).
//!
//! In-memory надёжная доставка мессенджера (retry-until-Ack, ~5 мин) переживает
//! короткие провалы, но НЕ переживает перезапуск приложения и долгий офлайн пира.
//! Этот слой персистит недоставленное в SQLite (`outbox`), досылает при возвращении
//! пира и на старте, выбрасывает протухшее по TTL. Опирается на общий [`crate::db::Db`].
//!
//! Хранит непрозрачные `(payload_type, payload)` — кодирование выбирает верхний слой
//! (движок): что именно сериализуется в `payload`, здесь не важно.

use crate::db::{self, Db};
use crate::store::StoreError;

/// Одна запись очереди исходящих.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    pub id: i64,
    pub peer: [u8; 32],
    /// id сообщения в мессенджере (для снятия по Ack и СТАБИЛЬНОЙ пере-отправки —
    /// получатель дедуплицирует по (peer, msg_id), поэтому досылка не создаёт дублей).
    pub msg_id: u64,
    pub payload_type: String,
    pub payload: Vec<u8>,
    pub created_at: i64,
}

/// SQLite-очередь исходящих (без кэша — прямые запросы).
pub struct OutboxStore {
    db: Db,
}

impl OutboxStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Поставить исходящее в очередь. Возвращает rowid записи.
    pub fn enqueue(
        &self,
        peer: &[u8; 32],
        msg_id: u64,
        payload_type: &str,
        payload: &[u8],
        created_at: i64,
    ) -> Result<i64, StoreError> {
        let conn = db::lock(&self.db);
        conn.execute(
            "INSERT INTO outbox (peer, msg_id, payload_type, payload, created_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
            rusqlite::params![peer.as_slice(), msg_id as i64, payload_type, payload, created_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Ожидающие записи для конкретного пира (для досылки при его онлайне),
    /// в порядке постановки.
    pub fn pending_for(&self, peer: &[u8; 32]) -> Result<Vec<OutboxEntry>, StoreError> {
        self.query(Some(peer))
    }

    /// Все ожидающие записи (для досылки при старте приложения).
    pub fn pending_all(&self) -> Result<Vec<OutboxEntry>, StoreError> {
        self.query(None)
    }

    fn query(&self, peer: Option<&[u8; 32]>) -> Result<Vec<OutboxEntry>, StoreError> {
        let conn = db::lock(&self.db);
        let map = |row: &rusqlite::Row| -> rusqlite::Result<OutboxEntry> {
            let peer_blob: Vec<u8> = row.get(1)?;
            Ok(OutboxEntry {
                id: row.get(0)?,
                peer: <[u8; 32]>::try_from(peer_blob.as_slice()).unwrap_or([0u8; 32]),
                msg_id: row.get::<_, i64>(2)? as u64,
                payload_type: row.get(3)?,
                payload: row.get(4)?,
                created_at: row.get(5)?,
            })
        };
        const COLS: &str = "SELECT id, peer, msg_id, payload_type, payload, created_at FROM outbox";
        match peer {
            Some(p) => {
                let mut stmt =
                    conn.prepare(&format!("{COLS} WHERE peer = ?1 AND status = 'pending' ORDER BY created_at, id"))?;
                let rows = stmt
                    .query_map(rusqlite::params![p.as_slice()], map)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
            None => {
                let mut stmt =
                    conn.prepare(&format!("{COLS} WHERE status = 'pending' ORDER BY created_at, id"))?;
                let rows = stmt.query_map([], map)?.collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        }
    }

    /// Убрать запись по её rowid.
    pub fn remove(&self, id: i64) -> Result<(), StoreError> {
        db::lock(&self.db).execute("DELETE FROM outbox WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    /// Убрать запись по (peer, msg_id) — вызывается на Ack доставки.
    pub fn remove_by_msg(&self, peer: &[u8; 32], msg_id: u64) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM outbox WHERE peer = ?1 AND msg_id = ?2",
            rusqlite::params![peer.as_slice(), msg_id as i64],
        )?;
        Ok(())
    }

    /// Выбросить протухшие записи (created_at < `cutoff`). Возвращает число удалённых.
    pub fn prune_older_than(&self, cutoff: i64) -> Result<usize, StoreError> {
        let n = db::lock(&self.db)
            .execute("DELETE FROM outbox WHERE created_at < ?1", rusqlite::params![cutoff])?;
        Ok(n)
    }

    /// Число ожидающих записей (метрика/тесты).
    pub fn count(&self) -> Result<i64, StoreError> {
        let n = db::lock(&self.db)
            .query_row("SELECT COUNT(*) FROM outbox WHERE status = 'pending'", [], |r| r.get(0))?;
        Ok(n)
    }
}

/// TTL офлайн-очереди — 7 дней. Старше — выбрасываем (не досылаем вечно).
pub const OUTBOX_TTL_SECS: i64 = 7 * 24 * 3600;

/// Оркестратор офлайн-очереди: персистит исходящий текст, снимает по Ack, досылает
/// при возвращении пира и на старте, чистит по TTL. Отправку делает через
/// callback (в движке = `Messenger::resend_text`), поэтому вся логика тестируема без
/// сети. Досылка — с тем же `msg_id`, так что получатель дедуплицирует (нет дублей).
pub struct OutboxManager {
    store: OutboxStore,
    send: Box<dyn FnMut(&[u8; 32], u64, &str) + Send>,
}

impl OutboxManager {
    pub fn new(store: OutboxStore, send: Box<dyn FnMut(&[u8; 32], u64, &str) + Send>) -> Self {
        Self { store, send }
    }

    /// Запомнить отправленный текст (на случай, если не дойдёт).
    pub fn on_sent(&self, peer: &[u8; 32], msg_id: u64, text: &str, now: i64) {
        let _ = self.store.enqueue(peer, msg_id, "text", text.as_bytes(), now);
    }

    /// Доставка подтверждена (Ack) — снять из очереди.
    pub fn on_ack(&self, peer: &[u8; 32], msg_id: u64) {
        let _ = self.store.remove_by_msg(peer, msg_id);
    }

    /// Пир вышел в онлайн — дослать всё ожидающее ему (тем же msg_id).
    pub fn on_online(&mut self, peer: &[u8; 32]) {
        if let Ok(entries) = self.store.pending_for(peer) {
            self.resend(entries);
        }
    }

    /// Старт приложения: выбросить протухшее (>TTL) и дослать оставшееся.
    pub fn on_start(&mut self, now: i64) {
        let _ = self.store.prune_older_than(now - OUTBOX_TTL_SECS);
        if let Ok(entries) = self.store.pending_all() {
            self.resend(entries);
        }
    }

    fn resend(&mut self, entries: Vec<OutboxEntry>) {
        for e in entries {
            if e.payload_type == "text" {
                if let Ok(text) = String::from_utf8(e.payload) {
                    (self.send)(&e.peer, e.msg_id, &text);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::sync::{Arc, Mutex};

    fn store() -> OutboxStore {
        OutboxStore::new(db::open_in_memory().unwrap())
    }

    type Log = Arc<Mutex<Vec<([u8; 32], u64, String)>>>;

    /// Менеджер над свежим стором + журнал вызовов отправки (записывающий callback).
    fn mgr_with(store: OutboxStore) -> (OutboxManager, Log) {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        let send = Box::new(move |p: &[u8; 32], id: u64, t: &str| {
            l2.lock().unwrap().push((*p, id, t.to_string()));
        });
        (OutboxManager::new(store, send), log)
    }

    fn peer(n: u8) -> [u8; 32] {
        [n; 32]
    }

    #[test]
    fn enqueue_list_remove_roundtrip() {
        let s = store();
        let id = s.enqueue(&peer(1), 7, "text", b"hello", 100).unwrap();
        let pending = s.pending_for(&peer(1)).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].payload, b"hello");
        assert_eq!(pending[0].msg_id, 7);
        assert_eq!(pending[0].payload_type, "text");
        assert_eq!(pending[0].created_at, 100);
        assert_eq!(s.count().unwrap(), 1);

        s.remove(id).unwrap();
        assert!(s.pending_for(&peer(1)).unwrap().is_empty());
        assert_eq!(s.count().unwrap(), 0);
    }

    #[test]
    fn remove_by_msg_targets_peer_and_id() {
        let s = store();
        s.enqueue(&peer(1), 100, "text", b"a", 1).unwrap();
        s.enqueue(&peer(1), 101, "text", b"b", 2).unwrap();
        s.enqueue(&peer(2), 100, "text", b"c", 3).unwrap(); // тот же msg_id, другой пир
        s.remove_by_msg(&peer(1), 100).unwrap();
        // Ушла только (peer1,100); (peer1,101) и (peer2,100) на месте.
        assert_eq!(s.pending_for(&peer(1)).unwrap().len(), 1);
        assert_eq!(s.pending_for(&peer(1)).unwrap()[0].msg_id, 101);
        assert_eq!(s.pending_for(&peer(2)).unwrap().len(), 1);
    }

    #[test]
    fn pending_for_filters_by_peer_and_orders() {
        let s = store();
        s.enqueue(&peer(1), 1, "text", b"a", 30).unwrap();
        s.enqueue(&peer(2), 2, "text", b"other", 10).unwrap();
        s.enqueue(&peer(1), 3, "text", b"b", 20).unwrap();

        let p1 = s.pending_for(&peer(1)).unwrap();
        assert_eq!(p1.len(), 2);
        // по created_at: b(20) раньше a(30)
        assert_eq!(p1[0].payload, b"b");
        assert_eq!(p1[1].payload, b"a");

        // pending_all видит все три
        assert_eq!(s.pending_all().unwrap().len(), 3);
    }

    #[test]
    fn prune_drops_expired_only() {
        let s = store();
        s.enqueue(&peer(1), 1, "text", b"old", 100).unwrap();
        s.enqueue(&peer(1), 2, "text", b"fresh", 1000).unwrap();
        // TTL cutoff = 500: старое (100) уходит, свежее (1000) остаётся.
        let dropped = s.prune_older_than(500).unwrap();
        assert_eq!(dropped, 1);
        let left = s.pending_for(&peer(1)).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].payload, b"fresh");
    }

    #[test]
    fn binary_payload_survives() {
        let s = store();
        let blob = vec![0u8, 1, 2, 255, 254, 0, 42];
        s.enqueue(&peer(9), 1, "blob_ref", &blob, 5).unwrap();
        assert_eq!(s.pending_for(&peer(9)).unwrap()[0].payload, blob);
    }

    // ── OutboxManager ────────────────────────────────────────────────────────

    #[test]
    fn manager_resends_on_online_with_stable_id() {
        let (mut m, log) = mgr_with(store());
        m.on_sent(&peer(1), 5, "привет", 100);
        m.on_online(&peer(1));
        // Дослано с тем же msg_id (получатель дедуплицирует).
        assert_eq!(*log.lock().unwrap(), vec![(peer(1), 5, "привет".to_string())]);
    }

    #[test]
    fn manager_ack_stops_resend() {
        let (mut m, log) = mgr_with(store());
        m.on_sent(&peer(1), 5, "hi", 100);
        m.on_ack(&peer(1), 5); // доставлено → снято из очереди
        log.lock().unwrap().clear();
        m.on_online(&peer(1));
        assert!(log.lock().unwrap().is_empty(), "после Ack досылки быть не должно");
    }

    #[test]
    fn manager_online_targets_only_that_peer() {
        let (mut m, log) = mgr_with(store());
        m.on_sent(&peer(1), 1, "to1", 10);
        m.on_sent(&peer(2), 2, "to2", 10);
        m.on_online(&peer(1));
        assert_eq!(*log.lock().unwrap(), vec![(peer(1), 1, "to1".to_string())]);
    }

    #[test]
    fn manager_start_prunes_expired_and_resends_rest() {
        let s = store();
        // Протухшее (старше TTL) и свежее.
        s.enqueue(&peer(1), 1, "text", b"old", 0).unwrap();
        let now = OUTBOX_TTL_SECS + 1000;
        s.enqueue(&peer(1), 2, "text", b"fresh", now - 10).unwrap();
        let (mut m, log) = mgr_with(s);
        m.on_start(now);
        // old (created_at 0 < now-TTL) выброшено; дослан только fresh.
        assert_eq!(*log.lock().unwrap(), vec![(peer(1), 2, "fresh".to_string())]);
    }
}
