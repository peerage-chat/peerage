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
            key         BLOB PRIMARY KEY,
            name        TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            added       INTEGER NOT NULL,
            verified    INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS messages (
            peer   BLOB    NOT NULL,
            id     INTEGER NOT NULL,
            mine   INTEGER NOT NULL,
            text   TEXT    NOT NULL,
            time   INTEGER NOT NULL,
            status TEXT    NOT NULL DEFAULT '',
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
    ")?;
    // Миграция для установок с outbox без msg_id (идемпотентно: дубль-колонка → игнор).
    let _ = conn.execute("ALTER TABLE outbox ADD COLUMN msg_id INTEGER NOT NULL DEFAULT 0", []);
    Ok(())
}
