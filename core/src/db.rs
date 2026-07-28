//! SQLCipher-соединение с базой данных мессенджера.
//!
//! Ключ шифрования берётся из [`crate::vault`] (должен быть инициализирован
//! до вызова [`open`]). При отсутствии ключа база остаётся незашифрованной
//! (только в dev/test окружении).
//!
//! Схема:
//!   contacts   — TOFU-контакты
//!   messages   — история сообщений
//!   outbox     — очередь исходящих (H3, сейчас только таблица)

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

pub type Db = Arc<Mutex<Connection>>;

/// Открыть (или создать) зашифрованную базу в `dir/messenger.db`.
/// Применяет SQLCipher-ключ из vault, WAL-mode и накатывает миграцию схемы.
pub fn open(dir: &Path) -> rusqlite::Result<Db> {
    let conn = Connection::open(dir.join("messenger.db"))?;

    #[cfg(feature = "sqlcipher")]
    if let Some(key) = crate::vault::key_bytes() {
        let hex = hex::encode(key);
        conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))?;
    }

    conn.execute_batch(
        // busy_timeout: ждать освобождения блокировки до 5с вместо мгновенного
        // SQLITE_BUSY. У нас одно соединение под Arc<Mutex> (записи и так
        // сериализованы), но это страхует от контеншена WAL-checkpoint и от
        // будущего второго соединения (напр. read-only) — см. docs/architecture/invariants.md.
        "PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys  = ON;",
    )?;

    migrate(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

/// In-memory база без шифрования — только для тестов.
#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Db> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;")?;
    migrate(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

/// Получить соединение, восстанавливая poisoned mutex.
pub fn lock(db: &Db) -> MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|p| p.into_inner())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS contacts (
            key          BLOB PRIMARY KEY,
            name         TEXT NOT NULL,
            fingerprint  TEXT NOT NULL,
            added        INTEGER NOT NULL,
            verified     INTEGER NOT NULL DEFAULT 0,
            profile_name TEXT NOT NULL DEFAULT '',
            avatar_id    TEXT NOT NULL DEFAULT '',
            helpful      INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS messages (
            peer    BLOB    NOT NULL,
            id      INTEGER NOT NULL,
            mine    INTEGER NOT NULL,
            text    TEXT    NOT NULL,
            time    INTEGER NOT NULL,
            status  TEXT    NOT NULL DEFAULT '',
            kind    TEXT    NOT NULL DEFAULT 'text',
            blob_id TEXT    NOT NULL DEFAULT '',
            mime    TEXT    NOT NULL DEFAULT '',
            name    TEXT    NOT NULL DEFAULT '',
            size        INTEGER NOT NULL DEFAULT 0,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            width       INTEGER NOT NULL DEFAULT 0,
            height      INTEGER NOT NULL DEFAULT 0,
            thumb_id    TEXT    NOT NULL DEFAULT '',
            downloaded  INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (peer, id, mine)
        );
        CREATE INDEX IF NOT EXISTS messages_peer_time ON messages(peer, time);

        CREATE TABLE IF NOT EXISTS outbox (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            peer         BLOB    NOT NULL,
            msg_id       INTEGER NOT NULL DEFAULT 0,
            payload_type TEXT    NOT NULL,
            payload      BLOB    NOT NULL,
            created_at   INTEGER NOT NULL,
            status       TEXT    NOT NULL DEFAULT 'pending'
        );

        -- Гейт контактов (замена TOFU-автодобавления): первое сообщение незнакомца
        -- ждёт одобрения здесь, а не сыплется в чат. blocklist — тихий дроп.
        CREATE TABLE IF NOT EXISTS contact_requests (
            key         BLOB PRIMARY KEY,
            fingerprint TEXT    NOT NULL,
            preview     TEXT    NOT NULL DEFAULT '',
            time        INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS blocklist (
            key     BLOB    PRIMARY KEY,
            blocked INTEGER NOT NULL
        );

        -- Журнал звонков. Локальный: наружу не уходит и никем не синхронизируется —
        -- это то, что отличает нас от обычного мессенджера, и повод сказать об этом
        -- на пустом экране.
        --   peer     — собеседник (для комнаты — любой, gid важнее)
        --   gid      — id комнаты (пусто у личного звонка)
        --   incoming — 1 входящий
        --   outcome  — answered | missed | declined | unanswered
        CREATE TABLE IF NOT EXISTS calls (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            peer     BLOB    NOT NULL,
            gid      BLOB    NOT NULL DEFAULT x'',
            incoming INTEGER NOT NULL,
            outcome  TEXT    NOT NULL,
            started  INTEGER NOT NULL,
            duration INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS calls_started ON calls(started DESC);

        -- Запрошенные пользователем вложения («скачать»). Намерение персистентно:
        -- пир может быть офлайн — докачка продолжится сама, когда он появится, и
        -- переживёт перезапуск. Следить за пиром вручную пользователь не должен.
        CREATE TABLE IF NOT EXISTS wanted_blobs (
            blob_id    TEXT PRIMARY KEY,
            peer       BLOB NOT NULL,
            requested  INTEGER NOT NULL
        );
    ")?;
    // Миграция для установок с outbox без msg_id (идемпотентно: дубль-колонка → игнор).
    let _ = conn.execute("ALTER TABLE outbox ADD COLUMN msg_id INTEGER NOT NULL DEFAULT 0", []);
    // Репутация (M3): счётчик «полезных дел» пира — отданных нам блобов. Влияет на
    // бюджет ресурсов (см. p2p_core::budget). Идемпотентно.
    let _ = conn.execute("ALTER TABLE contacts ADD COLUMN helpful INTEGER NOT NULL DEFAULT 0", []);
    // Профиль контакта (M2): самоназвание + хэш аватара (идемпотентно).
    let _ = conn.execute("ALTER TABLE contacts ADD COLUMN profile_name TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE contacts ADD COLUMN avatar_id TEXT NOT NULL DEFAULT ''", []);
    // Медиа-сообщения (M2): тип + ссылка на блоб вложения (идемпотентно).
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN blob_id TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN mime TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN name TEXT NOT NULL DEFAULT ''", []);
    // Карточка вложения: размер/длительность/кадр/превью + признак «скачано».
    // Крупные вложения не тянутся автоматически — сначала карточка, байты по кнопке.
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN size INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN duration_ms INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN width INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN height INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN thumb_id TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN downloaded INTEGER NOT NULL DEFAULT 0", []);
    Ok(())
}
