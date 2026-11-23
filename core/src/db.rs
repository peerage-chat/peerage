//! SQLCipher-соединение с базой данных мессенджера.
//!
//! Ключ шифрования берётся из [`crate::vault`] (должен быть инициализирован
//! до вызова [`open`]). При отсутствии ключа база остаётся незашифрованной
//! (только в dev/test окружении).
//!
//! Версия схемы — `PRAGMA user_version` (8.5). Схема на момент заморозки формата —
//! версия 1: её создаёт [`schema_v1`].
//! Каждое изменение после — шаг в [`STEPS`] под следующим номером, в своей транзакции
//! вместе с номером: прерванная миграция повторяется целиком. База новее приложения
//! не открывается — явная ошибка вместо молчаливой порчи.

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

/// Миграции после заморозки: `STEPS[i]` переводит схему `i + 1` в `i + 2`.
const STEPS: &[fn(&Connection) -> rusqlite::Result<()>] = &[];

/// Версия схемы этой сборки.
pub const SCHEMA_VERSION: i64 = 1 + STEPS.len() as i64;

/// База создана сборкой новее этой (схема `found`, эта знает до `SCHEMA_VERSION`).
#[derive(Debug, thiserror::Error)]
#[error("база данных новее приложения: схема {found}, эта сборка знает до {SCHEMA_VERSION}")]
pub struct NewerSchema {
    pub found: i64,
}

pub(crate) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let found: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if found > SCHEMA_VERSION {
        let code = rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN);
        return Err(rusqlite::Error::SqliteFailure(code, Some(NewerSchema { found }.to_string())));
    }
    for to in found + 1..=SCHEMA_VERSION {
        let tx = conn.unchecked_transaction()?;
        if to == 1 {
            schema_v1(&tx)?;
        } else {
            STEPS[(to - 2) as usize](&tx)?;
        }
        tx.pragma_update(None, "user_version", to)?;
        tx.commit()?;
    }
    search_index(conn);
    Ok(())
}

/// Схема 1 (заморозка формата, шаг 8): база без номера — новая. Базы, созданные до
/// заморозки, не доводятся (решение владельца 2026-11-23: до раздачи беты — `pm clear`).
fn schema_v1(conn: &Connection) -> rusqlite::Result<()> {
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
            -- ADR-007 / ADR-015 F3: адрес контакта — его подписанная запись адреса v2
            -- (вид сети, эпоха, адрес) и её подпись; единственный источник адреса.
            -- NULL — адрес неизвестен, ждём объявления. Ею же контакт доказывает
            -- владельцу, какую эпоху его адреса знает (R-120).
            transport_rec   BLOB,
            transport_sig   BLOB,
            -- Когда и как сверен ключ (unix-секунды; 'code' | 'set'). Пусто — не
            -- сверен или сверен до того, как это стали хранить.
            verified_at     INTEGER NOT NULL DEFAULT 0,
            verified_how    TEXT    NOT NULL DEFAULT '',
            -- 7i: послан запрос в контакты, ждём принятия (очередь к нему держится).
            awaiting        INTEGER NOT NULL DEFAULT 0
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
        -- Входящие запросы в контакты (шаг 7i): имя и приветствие из запроса, id
        -- запроса у отправителя (под ним приветствие встанет в чат).
        CREATE TABLE IF NOT EXISTS contact_requests (
            key         BLOB    PRIMARY KEY,
            fingerprint TEXT    NOT NULL,
            name        TEXT    NOT NULL DEFAULT '',
            greeting    TEXT    NOT NULL DEFAULT '',
            request_id  INTEGER NOT NULL DEFAULT 0,
            time        INTEGER NOT NULL
        );
        -- Блок-лист. name — как пир был показан в момент блокировки (блок удаляет
        -- контакт, иначе строку в «Заблокированных» нечем подписать); contact — был ли
        -- он контактом. Самоназвание незнакомца не хранится: оно не проверено.
        CREATE TABLE IF NOT EXISTS blocklist (
            key     BLOB    PRIMARY KEY,
            blocked INTEGER NOT NULL,
            name    TEXT    NOT NULL DEFAULT '',
            contact INTEGER NOT NULL DEFAULT 0
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
            -- Просмотрена ли (капсула «❤», 5.7). Запись реакции ставит 0; «новыми» считаются
            -- только чужие на мои сообщения — это решает запрос, а не запись.
            seen       INTEGER NOT NULL DEFAULT 1,
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

        -- Локальные пометки чата: закреплён в списке, режим уведомлений. Это НЕ часть
        -- переписки — решение только этого устройства, собеседнику оно не уходит.
        -- Отдельной таблицей, а не колонкой в контактах: пометки одинаковы для
        -- личного чата и группы, а те живут в разных местах.
        CREATE TABLE IF NOT EXISTS chat_flags (
            chat      BLOB    NOT NULL,
            scope     INTEGER NOT NULL DEFAULT 0,
            pinned_at INTEGER NOT NULL DEFAULT 0,  -- 0 = не закреплён; иначе когда
            autodel   INTEGER NOT NULL DEFAULT -1, -- своё правило автоудаления, −1 = как в общих
            notify    INTEGER NOT NULL DEFAULT -1, -- свой режим (`NotifyMode::code`), −1 = умолчание чата
            notify_until INTEGER NOT NULL DEFAULT 0, -- до какого времени (с), 0 = пока не включит
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
        --   photo      — фото группы: hex блоба (пусто — нет), ставит админ
        --   notify_default — режим уведомлений участникам по умолчанию (`NotifyMode::code`)
        CREATE TABLE IF NOT EXISTS group_meta (
            gid        BLOB    PRIMARY KEY,
            name       TEXT    NOT NULL,
            admin      INTEGER NOT NULL DEFAULT 0,
            admin_key  BLOB    NOT NULL DEFAULT x'',
            rekeyed_at INTEGER NOT NULL DEFAULT 0,
            photo      TEXT    NOT NULL DEFAULT '',
            notify_default INTEGER NOT NULL DEFAULT 0 -- режим уведомлений по умолчанию, ставит админ
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
    Ok(())
}

/// Полнотекстовый индекс — при каждом открытии: его может не быть в сборке SQLite, и
/// тогда поиск обязан деградировать, а не ронять запуск.
fn search_index(conn: &Connection) {
    // Полнотекстовый индекс переписки: текст и имя файла вложения (поиск в чате, 2.9).
    // `content='chat_messages'` — индекс без копии текста: строки остаются в основной
    // таблице, FTS хранит только словарь. Всё в `let _`: на сборке SQLite без FTS5
    // поиск обязан деградировать до LIKE, а не ронять запуск (см. `MessageStore::search_page`).
    let _ = conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chat_search
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

#[cfg(test)]
mod migration {
    use rusqlite::Connection;

    fn version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
    }

    /// Новая база получает номер схемы; повторное открытие ничего не ломает.
    #[test]
    fn fresh_base_is_numbered_and_reopens() {
        let conn = Connection::open_in_memory().unwrap();
        super::migrate(&conn).unwrap();
        assert_eq!(version(&conn), super::SCHEMA_VERSION);
        conn.execute("INSERT INTO blocklist (key, blocked) VALUES (x'01', 1)", []).unwrap();
        super::migrate(&conn).unwrap();
        assert_eq!(version(&conn), super::SCHEMA_VERSION);
        let n: i64 = conn.query_row("SELECT count(*) FROM blocklist", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "данные на месте");
    }

    /// База от сборки новее — понятный отказ, данные не тронуты.
    #[test]
    fn newer_base_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        super::migrate(&conn).unwrap();
        conn.pragma_update(None, "user_version", super::SCHEMA_VERSION + 1).unwrap();
        let err = super::migrate(&conn).unwrap_err();
        assert!(err.to_string().contains("новее приложения"), "{err}");
        assert_eq!(version(&conn), super::SCHEMA_VERSION + 1);
    }
}
