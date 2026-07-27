//! Персистентная офлайн-очередь исходящих (веха M3/H3).
//!
//! In-memory надёжная доставка мессенджера (retry-until-Ack, ~5 мин) переживает
//! короткие провалы, но НЕ переживает перезапуск приложения и долгий офлайн пира.
//! Этот слой персистит недоставленное в SQLite (`outbox`), досылает при возвращении
//! пира и на старте, выбрасывает протухшее по TTL. Опирается на общий [`crate::db::Db`].
//!
//! Хранит непрозрачные `(payload_type, payload)` — кодирование выбирает верхний слой
//! (движок): что именно сериализуется в `payload`, здесь не важно.

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

    /// Удалить всю очередь пира — например, когда пользователь сам решил её сбросить
    /// по предупреждению о разросшейся очереди. Возвращает число удалённых.
    /// (Автоматического вытеснения НЕТ: свои сообщения не выбрасываем молча.)
    pub fn clear_peer(&self, peer: &[u8; 32]) -> Result<usize, StoreError> {
        let n = db::lock(&self.db).execute(
            "DELETE FROM outbox WHERE peer = ?1",
            rusqlite::params![peer.as_slice()],
        )?;
        Ok(n)
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

/// Оркестратор офлайн-очереди: персистит исходящий текст, снимает по Ack, досылает
/// при возвращении пира и на старте, чистит по TTL. Отправку делает через
/// callback (в движке = `Messenger::resend_text`), поэтому вся логика тестируема без
/// сети. Досылка — с тем же `msg_id`, так что получатель дедуплицирует (нет дублей).
/// Что досылать: тип payload + его тело (кодирование выбирает верхний слой —
/// для текста это UTF-8, для медиа JSON-ссылка на блоб).
pub type ResendFn = Box<dyn FnMut(&[u8; 32], u64, &str, &[u8]) + Send>;

pub struct OutboxManager {
    store: OutboxStore,
    send: ResendFn,
    /// Тир пира — единый бюджет (M3). Задаёт, сколько и как долго хранить.
    /// Движок отдаёт актуальный тир из контактов/блок-листа.
    tier_of: Box<dyn Fn(&[u8; 32]) -> Tier + Send>,
}

impl OutboxManager {
    pub fn new(
        store: OutboxStore,
        send: ResendFn,
        tier_of: Box<dyn Fn(&[u8; 32]) -> Tier + Send>,
    ) -> Self {
        Self { store, send, tier_of }
    }

    /// Запомнить отправленный текст (на случай, если не дойдёт).
    ///
    /// НИЧЕГО НЕ ВЫТЕСНЯЕМ: очередь состоит из СОБСТВЕННЫХ сообщений пользователя, и
    /// удалить их — значит соврать задним числом (они уже показаны как отправленные).
    /// Раньше здесь стоял cap с выбрасыванием старейших, т.е. терялось начало
    /// разговора. Объём не страшен: в очереди только тексты (сотни байт), гигабайт —
    /// это миллионы сообщений. Разрастание — повод ПРЕДУПРЕДИТЬ (см. [`Self::stats`]),
    /// а не молча удалять.
    pub fn on_sent(&self, peer: &[u8; 32], msg_id: u64, text: &str, now: i64) {
        let _ = self.store.enqueue(peer, msg_id, "text", text.as_bytes(), now);
    }

    /// Запомнить отправленное ВЛОЖЕНИЕ. В очередь кладём только ССЫЛКУ (хэш блоба +
    /// mime + имя) — сами байты уже лежат в blob-хранилище, дублировать их незачем,
    /// поэтому запись остаётся крошечной. При досылке верхний слой снова предложит
    /// блоб и повторит медиа-сообщение с тем же id.
    pub fn on_sent_media(
        &self,
        peer: &[u8; 32],
        msg_id: u64,
        blob_id_hex: &str,
        mime: &str,
        name: &str,
        now: i64,
    ) {
        let body = serde_json::json!({ "blob": blob_id_hex, "mime": mime, "name": name });
        let Ok(payload) = serde_json::to_vec(&body) else { return };
        let _ = self.store.enqueue(peer, msg_id, "media", &payload, now);
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

    /// Доставка подтверждена (Ack) — снять из очереди.
    pub fn on_ack(&self, peer: &[u8; 32], msg_id: u64) {
        let _ = self.store.remove_by_msg(peer, msg_id);
    }

    /// Число ожидающих записей (диагностика).
    pub fn pending_count(&self) -> i64 {
        self.store.count().unwrap_or(-1)
    }

    /// Сбросить очередь пира по явному решению пользователя. Возвращает удалённых.
    pub fn clear_peer(&self, peer: &[u8; 32]) -> usize {
        self.store.clear_peer(peer).unwrap_or(0)
    }

    /// Пир вышел в онлайн — дослать всё ожидающее ему (тем же msg_id).
    pub fn on_online(&mut self, peer: &[u8; 32]) {
        if let Ok(entries) = self.store.pending_for(peer) {
            self.resend(entries);
        }
    }

    /// Старт приложения: выбросить протухшее и дослать оставшееся. TTL — per-peer по
    /// тиру: доверенному держим дольше, незнакомцу сутки, заблокированному ничего.
    pub fn on_start(&mut self, now: i64) {
        let Ok(entries) = self.store.pending_all() else { return };
        let mut alive = Vec::with_capacity(entries.len());
        for e in entries {
            let tier = (self.tier_of)(&e.peer);
            // Заблокированному не досылаем вообще — это решение пользователя, а не срок.
            if tier.is_blocked() {
                let _ = self.store.remove(e.id);
                continue;
            }
            // None = без срока (контакты): держим, пока влезает в cap.
            match tier.outbox_ttl_secs() {
                Some(ttl) if now.saturating_sub(e.created_at) > ttl => {
                    let _ = self.store.remove(e.id);
                }
                _ => alive.push(e),
            }
        }
        self.resend(alive);
    }

    /// Досылка: тип payload передаём наверх как есть — там знают, как его отправить
    /// (текст → Text, медиа → повторный blob_offer + ссылка). Неизвестные типы просто
    /// проходят мимо, поэтому старые записи не роняют досылку после апгрейда.
    fn resend(&mut self, entries: Vec<OutboxEntry>) {
        for e in entries {
            (self.send)(&e.peer, e.msg_id, &e.payload_type, &e.payload);
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
    /// Все пиры — `Contact` (историческое поведение: cap 500 / TTL 7 дней).
    fn mgr_with(store: OutboxStore) -> (OutboxManager, Log) {
        mgr_with_tier(store, Tier::Contact)
    }

    /// То же, но с заданным тиром для ВСЕХ пиров — для проверки градуировки бюджета.
    fn mgr_with_tier(store: OutboxStore, tier: Tier) -> (OutboxManager, Log) {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        // В журнал пишем тело как строку — тестам хватает (текст и JSON-ссылка медиа).
        let send = Box::new(move |p: &[u8; 32], id: u64, _kind: &str, body: &[u8]| {
            l2.lock().unwrap().push((*p, id, String::from_utf8_lossy(body).into_owned()));
        });
        (OutboxManager::new(store, send, Box::new(move |_| tier)), log)
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

    /// Прунинг по сроку остался только у тиров со сроком (незнакомец): старое
    /// выбрасывается, свежее досылается.
    #[test]
    fn manager_start_prunes_expired_and_resends_rest() {
        let s = store();
        let ttl = Tier::Stranger.outbox_ttl_secs().expect("у незнакомца срок есть");
        s.enqueue(&peer(1), 1, "text", b"old", 0).unwrap();
        let now = ttl + 1000;
        s.enqueue(&peer(1), 2, "text", b"fresh", now - 10).unwrap();
        let (mut m, log) = mgr_with_tier(s, Tier::Stranger);
        m.on_start(now);
        assert_eq!(*log.lock().unwrap(), vec![(peer(1), 2, "fresh".to_string())]);
    }

    /// M3: срок хранения — из бюджета тира. Контакту НЕ истекает (сервера, который
    /// сохранит за нас, нет — иначе уехавший на месяц молча теряет сообщение);
    /// незнакомцу сутки, заблокированному — ничего.
    #[test]
    fn start_ttl_follows_peer_tier() {
        let month = 30 * 24 * 3600; // «человек вернулся через месяц»
        let resent_after = |tier: Tier, age: i64| {
            let s = store();
            s.enqueue(&peer(1), 1, "text", b"msg", 0).unwrap();
            let (mut m, log) = mgr_with_tier(s, tier);
            m.on_start(age);
            let resent = !log.lock().unwrap().is_empty();
            resent
        };
        assert!(resent_after(Tier::Verified, month), "сверенному храним бессрочно");
        assert!(resent_after(Tier::Contact, month), "контакт вернулся через месяц — сообщение ждёт");
        assert!(!resent_after(Tier::Stranger, month), "незнакомцу — сутки");
        assert!(!resent_after(Tier::Blocked, 0), "заблокированному не храним вовсе");
    }

    /// Свои сообщения НЕ вытесняются: сколько написал — столько и ждёт доставки.
    /// (Раньше тут стоял cap, выбрасывавший начало разговора.)
    #[test]
    fn own_messages_are_never_evicted() {
        let (m, _log) = mgr_with(store());
        let n = 2_000; // заведомо больше любого прежнего cap
        for i in 0..n {
            m.on_sent(&peer(1), i as u64, "письмо", 100);
        }
        assert_eq!(m.pending_count() as usize, n, "ни одно своё сообщение не потеряно");
    }

    /// Вложение тоже переживает офлайн: в очередь кладём ССЫЛКУ на блоб (не байты),
    /// и при возвращении пира она досылается вместе с текстом.
    #[test]
    fn media_is_queued_as_reference_and_resent() {
        let (mut m, log) = mgr_with(store());
        m.on_sent(&peer(1), 1, "привет", 10);
        m.on_sent_media(&peer(1), 2, "ab12cd", "image/jpeg", "photo.jpg", 11);

        // Запись компактная — байты картинки в очередь не попали.
        assert!(m.stats().bytes < 200, "в очереди ссылка, а не содержимое файла");

        m.on_online(&peer(1));
        let sent = log.lock().unwrap().clone();
        assert_eq!(sent.len(), 2, "дослано и текстовое, и вложение");
        assert!(sent[1].2.contains("ab12cd"), "ссылка на блоб доехала: {:?}", sent[1]);
        assert!(sent[1].2.contains("photo.jpg"));
    }

    /// Разрастание очереди — повод предупредить, а не удалять.
    #[test]
    fn stats_report_growth_and_warn_threshold() {
        let (m, _log) = mgr_with(store());
        m.on_sent(&peer(1), 1, "раз", 10);
        m.on_sent(&peer(2), 2, "два", 10);
        let s = m.stats();
        assert_eq!(s.pending, 2);
        assert_eq!(s.peers, 2, "считаем и получателей");
        assert!(s.bytes > 0);
        assert!(!s.should_warn(), "две штуки — не повод для плашки");

        for i in 0..OUTBOX_WARN_PENDING {
            m.on_sent(&peer(3), 100 + i as u64, "много", 10);
        }
        assert!(m.stats().should_warn(), "после порога предупреждаем");
    }
}
