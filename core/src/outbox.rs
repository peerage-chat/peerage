//! Персистентная офлайн-очередь исходящих (веха M3/H3).
//!
//! In-memory надёжная доставка мессенджера (retry-until-Ack, ~5 мин) переживает
//! короткие провалы, но НЕ переживает перезапуск приложения и долгий офлайн пира.
//! Этот слой персистит недоставленное в SQLite (`outbox`); ставит в очередь, снимает
//! по Ack и досылает сам мессенджер, окном по порядку постановки. Здесь — хранилище
//! и то, что нужно движку и UI: сроки по тиру, статистика, ручной сброс.
//!
//! Хранит непрозрачные `(payload_type, payload)` — кодирование выбирает мессенджер.
//!
//! Выброшенное из досылки (срок по тиру, сброс пользователем) не стирается, если это
//! своё сообщение переписки: запись помечается `failed` и больше не уходит, но
//! «Повторить» возвращает её в очередь с тем же `msg_id` — получатель дедуплицирует,
//! а вложение не нужно собирать заново. Тело записи мало: у вложения это ссылка на
//! блоб и миниатюра, сами байты живут отдельно.

use crate::budget::Tier;
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
            "INSERT INTO outbox (peer, msg_id, payload_type, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![peer.as_slice(), msg_id as i64, payload_type, payload, created_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Ожидающие записи для конкретного пира (для досылки при его онлайне),
    /// в порядке постановки.
    pub fn pending_for(&self, peer: &[u8; 32]) -> Result<Vec<OutboxEntry>, StoreError> {
        self.query(Some(peer))
    }

    /// Все ожидающие записи (статистика, прунинг по сроку).
    pub fn pending_all(&self) -> Result<Vec<OutboxEntry>, StoreError> {
        self.query(None)
    }

    /// Первые `limit` записей пира в порядке постановки — окно, которое должно быть
    /// в полёте. Порядок — по rowid: часы могут прыгнуть назад, очередь — нет.
    pub fn head_for(&self, peer: &[u8; 32], limit: usize) -> Result<Vec<OutboxEntry>, StoreError> {
        let conn = db::lock(&self.db);
        let mut stmt = conn.prepare(
            "SELECT id, peer, msg_id, payload_type, payload, created_at FROM outbox
             WHERE peer = ?1 AND failed = 0 ORDER BY id LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![peer.as_slice(), limit as i64], |row| {
                let peer_blob: Vec<u8> = row.get(1)?;
                Ok(OutboxEntry {
                    id: row.get(0)?,
                    peer: <[u8; 32]>::try_from(peer_blob.as_slice()).unwrap_or([0u8; 32]),
                    msg_id: row.get::<_, i64>(2)? as u64,
                    payload_type: row.get(3)?,
                    payload: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Кому вообще что-то ждёт — с них начинается досылка на старте.
    pub fn peers(&self) -> Result<Vec<[u8; 32]>, StoreError> {
        let conn = db::lock(&self.db);
        let mut stmt = conn.prepare("SELECT DISTINCT peer FROM outbox WHERE failed = 0")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))?
            .filter_map(|r| r.ok().and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()))
            .collect();
        Ok(rows)
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
                let mut stmt = conn
                    .prepare(&format!("{COLS} WHERE peer = ?1 AND failed = 0 ORDER BY created_at, id"))?;
                let rows = stmt
                    .query_map(rusqlite::params![p.as_slice()], map)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
            None => {
                let mut stmt =
                    conn.prepare(&format!("{COLS} WHERE failed = 0 ORDER BY created_at, id"))?;
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

    /// Выбросить запись из досылки, оставив её для «Повторить».
    pub fn mark_failed(&self, id: i64) -> Result<(), StoreError> {
        db::lock(&self.db).execute("UPDATE outbox SET failed = 1 WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    /// «Повторить»: вернуть выброшенную запись в очередь. Срок по тиру считается
    /// заново — с момента повтора. false — такой выброшенной записи нет.
    pub fn retry(&self, peer: &[u8; 32], msg_id: u64, now: i64) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "UPDATE outbox SET failed = 0, created_at = ?3 WHERE peer = ?1 AND msg_id = ?2 AND failed = 1",
            rusqlite::params![peer.as_slice(), msg_id as i64, now],
        )?;
        Ok(n > 0)
    }

    /// Убрать запись по (peer, msg_id) — вызывается на Ack доставки.
    pub fn remove_by_msg(&self, peer: &[u8; 32], msg_id: u64) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM outbox WHERE peer = ?1 AND msg_id = ?2",
            rusqlite::params![peer.as_slice(), msg_id as i64],
        )?;
        Ok(())
    }

    /// Стереть все записи пира, и ждущие, и выброшенные, — вместе с перепиской
    /// («Очистить историю»). Возвращает число удалённых.
    pub fn clear_peer(&self, peer: &[u8; 32]) -> Result<usize, StoreError> {
        let n = db::lock(&self.db).execute(
            "DELETE FROM outbox WHERE peer = ?1",
            rusqlite::params![peer.as_slice()],
        )?;
        Ok(n)
    }

    /// Число ожидающих записей (метрика/тесты).
    pub fn count(&self) -> Result<i64, StoreError> {
        let n = db::lock(&self.db)
            .query_row("SELECT COUNT(*) FROM outbox WHERE failed = 0", [], |r| r.get(0))?;
        Ok(n)
    }
}

/// Порог, с которого UI показывает предупреждение «очередь разрослась» (число
/// ожидающих сообщений). НЕ лимит: ничего не удаляется, пользователь лишь узнаёт,
/// что кому-то накопилось много недоставленного, и сам решает, что делать.
pub const OUTBOX_WARN_PENDING: usize = 1000;

/// Снимок офлайн-очереди для UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutboxStats {
    /// Сколько сообщений ждёт доставки.
    pub pending: usize,
    /// Сколько байт они занимают (только тела сообщений).
    pub bytes: usize,
    /// Скольким разным получателям.
    pub peers: usize,
}

impl OutboxStats {
    /// Стоит ли предупредить пользователя о разросшейся очереди.
    pub fn should_warn(&self) -> bool {
        self.pending >= OUTBOX_WARN_PENDING
    }
}

/// Взгляд движка на очередь: сроки хранения по тиру, статистика для UI, ручной
/// сброс. Ставит и досылает мессенджер — он же владеет окном в полёте.
pub struct OutboxManager {
    store: OutboxStore,
    /// Тир пира — единый бюджет (M3). Задаёт, сколько и как долго хранить.
    /// Движок отдаёт актуальный тир из контактов/блок-листа.
    tier_of: Box<dyn Fn(&[u8; 32]) -> Tier + Send>,
}

impl OutboxManager {
    pub fn new(store: OutboxStore, tier_of: Box<dyn Fn(&[u8; 32]) -> Tier + Send>) -> Self {
        Self { store, tier_of }
    }

    /// Снимок очереди для UI: сколько ждёт и сколько занимает. По нему клиент решает,
    /// показывать ли предупреждение о разросшейся очереди.
    pub fn stats(&self) -> OutboxStats {
        let entries = self.store.pending_all().unwrap_or_default();
        let bytes: usize = entries.iter().map(|e| e.payload.len()).sum();
        let mut peers: Vec<[u8; 32]> = entries.iter().map(|e| e.peer).collect();
        peers.sort_unstable();
        peers.dedup();
        OutboxStats { pending: entries.len(), bytes, peers: peers.len() }
    }

    /// Разбивка очереди по получателям: (пир, сколько ждёт, сколько байт, время
    /// самой старой записи). Экран очереди показывает именно её — сумма «1248»
    /// бесполезна, пока не видно, кому именно не доходит.
    pub fn per_peer(&self) -> Vec<([u8; 32], usize, usize, i64)> {
        let entries = self.store.pending_all().unwrap_or_default();
        let mut by: std::collections::HashMap<[u8; 32], (usize, usize, i64)> = std::collections::HashMap::new();
        for e in entries {
            let slot = by.entry(e.peer).or_insert((0, 0, i64::MAX));
            slot.0 += 1;
            slot.1 += e.payload.len();
            slot.2 = slot.2.min(e.created_at);
        }
        let mut out: Vec<_> = by.into_iter().map(|(p, (n, b, t))| (p, n, b, t)).collect();
        // Самый «тяжёлый» получатель первым: с него и начнут разбираться.
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }

    /// Что ждёт досылки — всем (`None`) или одному пиру: это и выбрасывает «Сбросить».
    pub fn waiting(&self, peer: Option<&[u8; 32]>) -> Vec<OutboxEntry> {
        match peer {
            Some(p) => self.store.pending_for(p),
            None => self.store.pending_all(),
        }
        .unwrap_or_default()
    }

    /// Выбросить запись из досылки, оставив для «Повторить» (своё сообщение переписки).
    pub fn fail(&self, id: i64) {
        let _ = self.store.mark_failed(id);
    }

    /// Стереть запись насовсем (служебный пакет: повторять его вручную незачем).
    pub fn remove(&self, id: i64) {
        let _ = self.store.remove(id);
    }

    /// «Повторить»: вернуть выброшенное в очередь. false — повторять нечего.
    pub fn retry(&self, peer: &[u8; 32], msg_id: u64, now: i64) -> bool {
        self.store.retry(peer, msg_id, now).unwrap_or(false)
    }

    /// Число ожидающих записей (диагностика).
    pub fn pending_count(&self) -> i64 {
        self.store.count().unwrap_or(-1)
    }

    /// Стереть всё пира вместе с перепиской («Очистить историю»). Возвращает удалённых.
    pub fn clear_peer(&self, peer: &[u8; 32]) -> usize {
        self.store.clear_peer(peer).unwrap_or(0)
    }

    /// Старт приложения, ДО того как мессенджер начнёт досылку: что пора выбросить по
    /// сроку. TTL — per-peer по тиру: контакту бессрочно, незнакомцу сутки,
    /// заблокированному ничего. Выбрасывает движок ([`Self::fail`] / [`Self::remove`]):
    /// только он знает, своё ли это сообщение переписки.
    pub fn expired(&self, now: i64) -> Vec<OutboxEntry> {
        self.waiting(None)
            .into_iter()
            .filter(|e| {
                let tier = (self.tier_of)(&e.peer);
                // Заблокированному не досылаем вообще — это решение пользователя, а не срок.
                tier.is_blocked()
                    || tier.outbox_ttl_secs().is_some_and(|ttl| now.saturating_sub(e.created_at) > ttl)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn store() -> OutboxStore {
        OutboxStore::new(db::open_in_memory().unwrap())
    }

    /// Менеджер с заданным тиром для ВСЕХ пиров — для проверки градуировки бюджета.
    fn mgr_with_tier(store: OutboxStore, tier: Tier) -> OutboxManager {
        OutboxManager::new(store, Box::new(move |_| tier))
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
    fn binary_payload_survives() {
        let s = store();
        let blob = vec![0u8, 1, 2, 255, 254, 0, 42];
        s.enqueue(&peer(9), 1, "blob_ref", &blob, 5).unwrap();
        assert_eq!(s.pending_for(&peer(9)).unwrap()[0].payload, blob);
    }

    /// Сброс очереди пира — только по явной команде (кнопка по предупреждению),
    /// и только его: чужие очереди не трогаем.
    #[test]
    fn clear_peer_removes_only_that_peer() {
        let s = store();
        for i in 0..5 {
            s.enqueue(&peer(1), i as u64, "text", format!("m{i}").as_bytes(), i).unwrap();
        }
        s.enqueue(&peer(2), 99, "text", b"other", 0).unwrap();
        assert_eq!(s.clear_peer(&peer(1)).unwrap(), 5);
        assert!(s.pending_for(&peer(1)).unwrap().is_empty());
        assert_eq!(s.pending_for(&peer(2)).unwrap().len(), 1);
    }

    /// Выброшенное не досылается и не считается ждущим, но «Повторить» возвращает его
    /// в очередь с тем же msg_id — ровно один раз.
    #[test]
    fn failed_rows_leave_the_queue_until_retried() {
        let s = store();
        let row = s.enqueue(&peer(1), 7, "text", b"a", 1).unwrap();
        s.enqueue(&peer(1), 8, "text", b"b", 2).unwrap();
        s.mark_failed(row).unwrap();
        let head: Vec<u64> = s.head_for(&peer(1), 10).unwrap().iter().map(|e| e.msg_id).collect();
        assert_eq!(head, vec![8], "выброшенное не уходит");
        assert_eq!(s.count().unwrap(), 1);
        assert!(!s.retry(&peer(1), 8, 50).unwrap(), "ждущее повторять нечего");
        assert!(s.retry(&peer(1), 7, 50).unwrap());
        let back = s.pending_for(&peer(1)).unwrap();
        assert_eq!(back.iter().map(|e| e.msg_id).collect::<Vec<_>>(), vec![8, 7], "срок — с повтора");
        assert!(!s.retry(&peer(1), 7, 60).unwrap(), "второй раз — нечего");
        assert_eq!(s.clear_peer(&peer(1)).unwrap(), 2);
    }

    /// Окно в полёте — голова очереди пира по порядку постановки, не больше `limit`.
    #[test]
    fn head_for_is_ordered_and_limited() {
        let s = store();
        for i in 0..5u64 {
            // created_at убывает: порядок задаёт постановка, а не часы.
            s.enqueue(&peer(1), 100 + i, "payload", b"x", 50 - i as i64).unwrap();
        }
        s.enqueue(&peer(2), 1, "payload", b"y", 0).unwrap();
        let head: Vec<u64> = s.head_for(&peer(1), 3).unwrap().iter().map(|e| e.msg_id).collect();
        assert_eq!(head, vec![100, 101, 102]);
        let mut peers = s.peers().unwrap();
        peers.sort();
        assert_eq!(peers, vec![peer(1), peer(2)]);
    }

    // ── OutboxManager ────────────────────────────────────────────────────────

    /// Прунинг по сроку остался только у тиров со сроком (незнакомец): старое
    /// выбрасывается, свежее остаётся ждать.
    #[test]
    fn manager_prunes_expired_only() {
        let s = store();
        let ttl = Tier::Stranger.outbox_ttl_secs().expect("у незнакомца срок есть");
        s.enqueue(&peer(1), 1, "payload", b"old", 0).unwrap();
        let now = ttl + 1000;
        s.enqueue(&peer(1), 2, "payload", b"fresh", now - 10).unwrap();
        let m = mgr_with_tier(s, Tier::Stranger);
        let old: Vec<u64> = m.expired(now).iter().map(|e| e.msg_id).collect();
        assert_eq!(old, vec![1]);
    }

    /// M3: срок хранения — из бюджета тира. Контакту НЕ истекает (сервера, который
    /// сохранит за нас, нет — иначе уехавший на месяц молча теряет сообщение);
    /// незнакомцу сутки, заблокированному — ничего.
    #[test]
    fn ttl_follows_peer_tier() {
        let month = 30 * 24 * 3600; // «человек вернулся через месяц»
        let kept_after = |tier: Tier, age: i64| {
            let s = store();
            s.enqueue(&peer(1), 1, "payload", b"msg", 0).unwrap();
            let m = mgr_with_tier(s, tier);
            m.expired(age).is_empty()
        };
        assert!(kept_after(Tier::Verified, month), "сверенному храним бессрочно");
        assert!(kept_after(Tier::Contact, month), "контакт вернулся через месяц — сообщение ждёт");
        assert!(!kept_after(Tier::Stranger, month), "незнакомцу — сутки");
        assert!(!kept_after(Tier::Blocked, 0), "заблокированному не храним вовсе");
    }

    /// Разрастание очереди — повод предупредить, а не удалять.
    #[test]
    fn stats_report_growth_and_warn_threshold() {
        let s = store();
        s.enqueue(&peer(1), 1, "payload", b"one", 10).unwrap();
        s.enqueue(&peer(2), 2, "payload", b"two", 10).unwrap();
        let m = mgr_with_tier(s, Tier::Contact);
        let st = m.stats();
        assert_eq!(st.pending, 2);
        assert_eq!(st.peers, 2, "считаем и получателей");
        assert!(st.bytes > 0);
        assert!(!st.should_warn(), "две штуки — не повод для плашки");
    }
}
