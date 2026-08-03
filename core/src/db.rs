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

        -- Вся переписка одной таблицей: личные чаты, «Сохранённое» и группы. Раньше
        -- группы жили отдельно (список в памяти + JSON-файл), и каждая фича чата
        -- писалась дважды — а копия данных о группах не знала вовсе.
        --   chat   — с кем/где: ключ собеседника (32 Б) или gid группы
        --   scope  — 0 личный чат (в т.ч. заметки себе), 1 группа
        --   sender — hex-ключ автора: ссылка на сообщение это пара «автор + id»
        --            (ADR-006), одинаково читаемая в личном чате и в группе
        --   mine   — ярлык для UI («моё»), не часть ключа
        CREATE TABLE IF NOT EXISTS chat_messages (
            chat    BLOB    NOT NULL,
            scope   INTEGER NOT NULL DEFAULT 0,
            id      INTEGER NOT NULL,
            sender  TEXT    NOT NULL DEFAULT '',
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
            reply_to     INTEGER NOT NULL DEFAULT 0,
            reply_sender TEXT    NOT NULL DEFAULT '',
            system       INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (chat, scope, sender, id)
        );
        -- Порядок в чате и курсор пагинации. Курсор двойной, (time, rowid), но в
        -- индексе достаточно времени: rowid лежит в каждой записи индекса сам.
        CREATE INDEX IF NOT EXISTS chat_messages_time ON chat_messages(chat, scope, time);
        -- Пометка «скачано» ищет строки по контент-адресу вложения, а не по чату.
        CREATE INDEX IF NOT EXISTS chat_messages_blob ON chat_messages(blob_id);

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

        -- Название чата. Нужно ровно для одного: групповая переписка, восстановленная
        -- из копии, должна где-то называться. MLS-состояние в копию не входит
        -- (ADR-004), поэтому такая группа живёт как архив: история есть, писать
        -- нельзя — а без имени она выглядела бы как чат с шестнадцатеричным id.
        CREATE TABLE IF NOT EXISTS chat_titles (
            chat  BLOB    NOT NULL,
            scope INTEGER NOT NULL DEFAULT 0,
            name  TEXT    NOT NULL,
            PRIMARY KEY (chat, scope)
        );

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
    // Ответ на сообщение (ADR-006, «фича-по-ссылке»): ссылка = (автор, id). Здесь
    // автор кодируется флагом reply_mine, потому что в DM собеседник всего один.
    // Идемпотентно: повторный ALTER на существующей колонке игнорируется.
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN reply_to INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN reply_mine INTEGER NOT NULL DEFAULT 0", []);
    // Репутация (M3): счётчик «полезных дел» пира — отданных нам блобов. Влияет на
    // бюджет ресурсов (см. p2p_core::budget). Идемпотентно.
    let _ = conn.execute("ALTER TABLE contacts ADD COLUMN helpful INTEGER NOT NULL DEFAULT 0", []);
    // Профиль контакта (M2): самоназвание + хэш аватара (идемпотентно).
    let _ = conn.execute("ALTER TABLE contacts ADD COLUMN profile_name TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE contacts ADD COLUMN avatar_id TEXT NOT NULL DEFAULT ''", []);
    // ADR-007: адрес контакта отделён от его личности. Пусто = эпоха 0, то есть
    // адрес совпадает с ключом личности (как было до разделения) — существующие
    // записи мигрировать не нужно.
    let _ = conn.execute(
        "ALTER TABLE contacts ADD COLUMN transport_key TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE contacts ADD COLUMN transport_epoch INTEGER NOT NULL DEFAULT 0",
        [],
    );
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
