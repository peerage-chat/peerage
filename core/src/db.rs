//! SQLCipher-соединение с базой данных мессенджера.
//!
//! Ключ шифрования берётся из [`crate::vault`] (должен быть инициализирован
//! до вызова [`open`]). При отсутствии ключа база остаётся незашифрованной
//! (только в dev/test окружении).
//!
//! Схема описана ОДИН раз — в `migrate()`, без ALTER: совместимость со старыми
//! базами не поддерживается, новая колонка добавляется прямо в `CREATE TABLE`.

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

pub(crate) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS contacts (
            key          BLOB PRIMARY KEY,
            name         TEXT NOT NULL,
            fingerprint  TEXT NOT NULL,
            added        INTEGER NOT NULL,
            verified     INTEGER NOT NULL DEFAULT 0,
            profile_name TEXT NOT NULL DEFAULT '',
            avatar_id    TEXT NOT NULL DEFAULT '',
            -- Репутация (M3): отданные нам блобы, влияет на бюджет (peerage_core::budget).
            helpful      INTEGER NOT NULL DEFAULT 0,
            -- ADR-007: адрес контакта отделён от личности. Пусто = эпоха 0, адрес
            -- совпадает с ключом личности.
            transport_key   TEXT    NOT NULL DEFAULT '',
            transport_epoch INTEGER NOT NULL DEFAULT 0
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
            fwd_from     TEXT    NOT NULL DEFAULT '',
            fwd_anon     INTEGER NOT NULL DEFAULT 0,
            edited_at    INTEGER NOT NULL DEFAULT 0,
            -- Упоминания (JSON `[{at,len,key}]`, пусто — нет): ссылки на людей
            -- поверх участков текста, см. peerage_core::mention.
            mentions     TEXT    NOT NULL DEFAULT '',
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
            msg_id       INTEGER NOT NULL,
            payload_type TEXT    NOT NULL,
            payload      BLOB    NOT NULL,
            created_at   INTEGER NOT NULL,
            -- 1 — выброшено из досылки (срок, сброс пользователем), но сообщение
            -- можно «Повторить»: запись с тем же msg_id вернётся в очередь.
            failed       INTEGER NOT NULL DEFAULT 0
        );

        -- Гейт контактов (замена TOFU-автодобавления): первое сообщение незнакомца
        -- ждёт одобрения здесь, а не сыплется в чат. blocklist — тихий дроп.
        CREATE TABLE IF NOT EXISTS contact_requests (
            key         BLOB PRIMARY KEY,
            fingerprint TEXT    NOT NULL,
            preview     TEXT    NOT NULL DEFAULT '',
            time        INTEGER NOT NULL,
            -- Первое сообщение незнакомца целиком: при одобрении оно ложится в чат
            -- (отправитель уже получил Ack и не перешлёт его).
            first_id    INTEGER NOT NULL DEFAULT 0,
            first_text  TEXT    NOT NULL DEFAULT '',
            first_time  INTEGER NOT NULL DEFAULT 0
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
            duration INTEGER NOT NULL DEFAULT 0,
            video    INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS calls_started ON calls(started DESC);

        -- Закреплённые сообщения. Side-таблица, а не флаг в самой строке истории:
        -- закреп — свойство ЧАТА, а не сообщения (его ставит и снимает другой
        -- человек, и он должен переживать удаление сообщения у себя).
        --   chat/scope — где закреплено
        --   msg_id + sender — ссылка на сообщение (ADR-006)
        --   pinned_at — когда закрепили (шапка показывает последнее)
        --   by — hex того, кто закрепил
        -- Реакции: у человека одна на сообщение (ключ — до who). Пустой emoji — снятая
        -- реакция: строка остаётся с её временем, чтобы опоздавшая постановка не
        -- вернула снятое. Сообщение — пара «автор + id» (ADR-006).
        CREATE TABLE IF NOT EXISTS reactions (
            chat       BLOB    NOT NULL,
            scope      INTEGER NOT NULL DEFAULT 0,
            msg_id     INTEGER NOT NULL,
            msg_sender TEXT    NOT NULL,
            who        TEXT    NOT NULL,
            emoji      TEXT    NOT NULL,
            at         INTEGER NOT NULL,
            PRIMARY KEY (chat, scope, msg_id, msg_sender, who)
        );

        CREATE TABLE IF NOT EXISTS pinned (
            chat      BLOB    NOT NULL,
            scope     INTEGER NOT NULL DEFAULT 0,
            msg_id    INTEGER NOT NULL,
            sender    TEXT    NOT NULL,
            pinned_at INTEGER NOT NULL,
            by        TEXT    NOT NULL DEFAULT '',
            PRIMARY KEY (chat, scope, msg_id, sender)
        );

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

        -- Локальные пометки чата: закреплён в списке, без звука. Это НЕ часть
        -- переписки — решение только этого устройства, собеседнику оно не уходит.
        -- Отдельной таблицей, а не колонкой в контактах: пометки одинаковы для
        -- личного чата и группы, а те живут в разных местах.
        CREATE TABLE IF NOT EXISTS chat_flags (
            chat      BLOB    NOT NULL,
            scope     INTEGER NOT NULL DEFAULT 0,
            pinned_at INTEGER NOT NULL DEFAULT 0,  -- 0 = не закреплён; иначе когда
            muted     INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (chat, scope)
        );

        -- Запрошенные пользователем вложения («скачать»). Намерение персистентно:
        -- пир может быть офлайн — докачка продолжится сама, когда он появится, и
        -- переживёт перезапуск. Следить за пиром вручную пользователь не должен.
        -- Группы, в которых мы состоим. MLS-состояние — в своём файле
        -- (`groups/<gid>.group`), здесь всё остальное:
        --   admin      — мы админ/секвенсор (создатель)
        --   admin_key  — ключ админа (пусто — неизвестен): права на действия извне
        --   rekeyed_at — последняя ротация своего ключа (катящийся PCS, ADR-004)
        CREATE TABLE IF NOT EXISTS group_meta (
            gid        BLOB    PRIMARY KEY,
            name       TEXT    NOT NULL,
            admin      INTEGER NOT NULL DEFAULT 0,
            admin_key  BLOB    NOT NULL DEFAULT x'',
            rekeyed_at INTEGER NOT NULL DEFAULT 0
        );
        -- Кого и в какую группу мы позвали: ждём от него KeyPackage с этим gid.
        -- На диске, потому что «Вступить» могут нажать и через сутки, после нашего
        -- перезапуска.
        CREATE TABLE IF NOT EXISTS group_invites_sent (
            peer BLOB NOT NULL,
            gid  BLOB NOT NULL,
            PRIMARY KEY (peer, gid)
        );
        -- Входящие приглашения, ждут решения. accepted — «Вступить» нажато, ждём
        -- Welcome (карточка не исчезает, пока он не придёт).
        CREATE TABLE IF NOT EXISTS group_invites (
            gid      BLOB    PRIMARY KEY,
            name     TEXT    NOT NULL,
            inviter  BLOB    NOT NULL,
            accepted INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS wanted_blobs (
            blob_id    TEXT PRIMARY KEY,
            peer       BLOB NOT NULL,
            requested  INTEGER NOT NULL
        );
    ")?;
    // Колонки, появившиеся после первых установок: на старой базе ALTER добавит их
    // один раз, на новой (колонка уже в CREATE) — тихо ничего не сделает.
    let _ = conn.execute("ALTER TABLE outbox ADD COLUMN failed INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE calls ADD COLUMN video INTEGER NOT NULL DEFAULT 0", []);
    // Полнотекстовый индекс переписки: текст и имя файла вложения (поиск в чате, 2.9).
    // `content='chat_messages'` — индекс без копии текста: строки остаются в основной
    // таблице, FTS хранит только словарь. Всё в `let _`: на сборке SQLite без FTS5
    // поиск обязан деградировать до LIKE, а не ронять запуск (см. `MessageStore::search_page`).
    //
    // Прежний индекс `messages_fts` знал только текст: убираем его с триггерами, а новый
    // при первом создании наполняем из истории (`rebuild`) — иначе старая переписка
    // осталась бы ненайденной.
    let fresh: bool = conn
        .query_row(
            "SELECT count(*) = 0 FROM sqlite_master WHERE name = 'chat_search'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    let _ = conn.execute_batch(
        "DROP TRIGGER IF EXISTS chat_messages_ai;
         DROP TRIGGER IF EXISTS chat_messages_ad;
         DROP TRIGGER IF EXISTS chat_messages_au;
         DROP TABLE IF EXISTS messages_fts;

         CREATE VIRTUAL TABLE IF NOT EXISTS chat_search
             USING fts5(text, name, content='chat_messages', content_rowid='rowid');

         CREATE TRIGGER IF NOT EXISTS chat_search_ai AFTER INSERT ON chat_messages BEGIN
             INSERT INTO chat_search(rowid, text, name) VALUES (new.rowid, new.text, new.name);
         END;
         CREATE TRIGGER IF NOT EXISTS chat_search_ad AFTER DELETE ON chat_messages BEGIN
             INSERT INTO chat_search(chat_search, rowid, text, name)
             VALUES ('delete', old.rowid, old.text, old.name);
         END;
         CREATE TRIGGER IF NOT EXISTS chat_search_au AFTER UPDATE ON chat_messages BEGIN
             INSERT INTO chat_search(chat_search, rowid, text, name)
             VALUES ('delete', old.rowid, old.text, old.name);
             INSERT INTO chat_search(rowid, text, name) VALUES (new.rowid, new.text, new.name);
         END;",
    );
    if fresh {
        let _ = conn.execute("INSERT INTO chat_search(chat_search) VALUES ('rebuild')", []);
    }
    Ok(())
}


#[cfg(test)]
mod fts_probe {
    #[test]
    fn fts5_is_available_in_this_build() {
        let db = super::open_in_memory().unwrap();
        let conn = super::lock(&db);
        conn.execute_batch("CREATE VIRTUAL TABLE probe USING fts5(x);")
            .expect("FTS5 должен быть в сборке SQLite");
    }
}
