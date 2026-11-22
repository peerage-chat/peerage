//! Автоудаление у себя (план 5.4, макеты `docs/design/mockups/redesign/autodel/`).
//!
//! **Только на этом телефоне.** Собеседнику ничего не уходит: это уборка локальной
//! истории, а не «исчезающие сообщения». Поэтому и живёт целиком в хранилище.
//!
//! Правило одно на всё приложение ([`Rule`]): сообщения старше N; вложения старше N
//! **и** от X МБ (размер отключается вариантом «Любого» — остаётся только возраст).
//! Чат может задать своё правило для сообщений ([`ChatRule`]). Не удаляются никогда:
//! «Сохранённое» и закреплённые сообщения — вместе с их вложениями.
//!
//! Вложение удаляется как при очистке «Хранилища»: сообщение остаётся, карточка снова
//! предлагает «Скачать». Один файл может быть в нескольких чатах (контент-адресация),
//! поэтому файл уходит, только если ВСЕ ссылки на него попадают под правило; это
//! решает чистая [`plan_blobs`].

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::db;
use crate::store::{Chat, MessageStore, StoreError};

const DAY: i64 = 24 * 60 * 60;

/// Порог возраста. «Месяц» и «год» — календарно приблизительные (30 и 365 дней):
/// правило про «давно», а не про дату.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Age {
    #[default]
    Never,
    Week,
    Month,
    Year,
}

impl Age {
    /// Сколько секунд должно пройти; `None` — не удалять.
    pub fn secs(self) -> Option<i64> {
        match self {
            Age::Never => None,
            Age::Week => Some(7 * DAY),
            Age::Month => Some(30 * DAY),
            Age::Year => Some(365 * DAY),
        }
    }

    /// Код в `chat_flags.autodel`. −1 там — «как в общих» (см. [`chat_rule_code`]).
    fn code(self) -> i64 {
        match self {
            Age::Never => 0,
            Age::Week => 1,
            Age::Month => 2,
            Age::Year => 3,
        }
    }

    fn from_code(c: i64) -> Option<Age> {
        match c {
            0 => Some(Age::Never),
            1 => Some(Age::Week),
            2 => Some(Age::Month),
            3 => Some(Age::Year),
            _ => None,
        }
    }
}

/// «Только файлы от»: нижняя граница размера вложения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MinSize {
    #[default]
    #[serde(rename = "any")]
    Any,
    #[serde(rename = "10mb")]
    Mb10,
    #[serde(rename = "100mb")]
    Mb100,
}

impl MinSize {
    pub fn bytes(self) -> i64 {
        match self {
            MinSize::Any => 0,
            MinSize::Mb10 => 10 * 1024 * 1024,
            MinSize::Mb100 => 100 * 1024 * 1024,
        }
    }
}

/// Общее правило. По умолчанию выключено целиком: удалять историю молча нельзя.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub messages: Age,
    pub attachments: Age,
    pub min_size: MinSize,
}

/// Своё правило чата для сообщений: `None` — как в общих.
pub type ChatRule = Option<Age>;

/// Что действует в чате: `(возраст сообщений, возраст вложений)` в секундах.
///
/// Своё «Никогда» снимает с чата и правило вложений: чат, отмеченный «не трогать»,
/// не должен терять и файлы. Своё «старше N» меняет только сообщения — вложения
/// остаются по общему правилу (а вместе с удалённым сообщением уходят и так).
pub fn effective(rule: &Rule, own: ChatRule) -> (Option<i64>, Option<i64>) {
    match own {
        None => (rule.messages.secs(), rule.attachments.secs()),
        Some(Age::Never) => (None, None),
        Some(a) => (a.secs(), rule.attachments.secs()),
    }
}

/// Строка истории, ссылающаяся на файл (вложение или миниатюра).
#[derive(Debug, Clone)]
pub struct BlobRef {
    pub chat: Vec<u8>,
    pub scope: i64,
    pub time: i64,
    pub size: i64,
    pub blob_id: String,
    pub thumb_id: String,
    pub downloaded: bool,
    pub pinned: bool,
}

/// Пороги чата: время, раньше которого удаляются сообщения и вложения.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cutoffs {
    pub messages: Option<i64>,
    pub attachments: Option<i64>,
}

/// Какие файлы удалить. Файл уходит, если каждая ссылка на него либо удаляется
/// вместе с сообщением, либо — как вложение — попадает под правило вложений.
/// Миниатюра оставшегося сообщения держит свой файл: без неё карточка пуста.
/// Закреплённое держит всё своё.
pub fn plan_blobs(
    rows: &[BlobRef],
    cut: impl Fn(&[u8], i64) -> Cutoffs,
    min_size: MinSize,
) -> Vec<String> {
    let mut touched: HashSet<&str> = HashSet::new();
    let mut held: HashSet<&str> = HashSet::new();
    for r in rows {
        let c = cut(&r.chat, r.scope);
        let gone = !r.pinned && c.messages.is_some_and(|t| r.time < t);
        let att_due = !r.pinned
            && r.downloaded
            && r.size >= min_size.bytes()
            && c.attachments.is_some_and(|t| r.time < t);
        for (id, as_blob) in [(r.blob_id.as_str(), true), (r.thumb_id.as_str(), false)] {
            if id.is_empty() {
                continue;
            }
            if gone || (as_blob && att_due) {
                touched.insert(id);
            } else {
                held.insert(id);
            }
        }
    }
    let mut out: Vec<String> = touched
        .difference(&held)
        .map(|s| s.to_string())
        .collect();
    out.sort();
    out
}

/// Код своего правила для `chat_flags.autodel`: −1 — как в общих.
fn chat_rule_code(own: ChatRule) -> i64 {
    own.map_or(-1, Age::code)
}

/// Что попадает под правило: сколько сообщений уйдёт и какие файлы.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub messages: usize,
    /// Пороги по чатам (те, где хоть что-то удаляется) — их же применит [`MessageStore::prune`].
    pub cutoffs: Vec<(Vec<u8>, i64, Cutoffs)>,
    pub blobs: Vec<String>,
}

impl MessageStore {
    /// Своё правило чата.
    pub fn set_chat_autodel(&self, chat: Chat, own: ChatRule) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT INTO chat_flags (chat, scope, autodel) VALUES (?1, ?2, ?3)
             ON CONFLICT(chat, scope) DO UPDATE SET autodel = excluded.autodel",
            rusqlite::params![chat.id(), chat.scope(), chat_rule_code(own)],
        )?;
        Ok(())
    }

    /// Чаты со своим правилом: `(идентификатор, вид, правило)`.
    pub fn chat_autodel(&self) -> Vec<(Vec<u8>, i64, Age)> {
        let conn = db::lock(&self.db);
        let Ok(mut stmt) = conn.prepare("SELECT chat, scope, autodel FROM chat_flags WHERE autodel >= 0")
        else {
            return Vec::new();
        };
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)?)))
            .map(|rows| {
                rows.filter_map(|r| r.ok())
                    .filter_map(|(c, s, a)| Some((c, s, Age::from_code(a)?)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Посчитать, что удалит правило на момент `now`. `saved` — «Сохранённое»
    /// (личный чат с собой): его не трогаем никогда. `own` — своё правило одного чата
    /// поверх сохранённых (предпросмотр выбора в профиле чата до его записи).
    pub fn prune_plan(
        &self,
        now: i64,
        rule: &Rule,
        saved: &[u8; 32],
        own: Option<(Chat, ChatRule)>,
    ) -> Result<Plan, StoreError> {
        let mut rules: HashMap<(Vec<u8>, i64), Age> = self
            .chat_autodel()
            .into_iter()
            .map(|(c, s, a)| ((c, s), a))
            .collect();
        if let Some((chat, r)) = own {
            let k = (chat.id().to_vec(), chat.scope());
            match r {
                Some(a) => rules.insert(k, a),
                None => rules.remove(&k),
            };
        }
        let saved_id = Chat::Dm(saved);
        let cut_of = |chat: &[u8], scope: i64| -> Cutoffs {
            if chat == saved_id.id() && scope == saved_id.scope() {
                return Cutoffs::default();
            }
            let (m, a) = effective(rule, rules.get(&(chat.to_vec(), scope)).copied());
            Cutoffs {
                messages: m.map(|s| now - s),
                attachments: a.map(|s| now - s),
            }
        };

        let conn = db::lock(&self.db);
        let chats: Vec<(Vec<u8>, i64)> = conn
            .prepare("SELECT DISTINCT chat, scope FROM chat_messages")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut plan = Plan::default();
        for (chat, scope) in chats {
            let c = cut_of(&chat, scope);
            if c == Cutoffs::default() {
                continue;
            }
            if let Some(t) = c.messages {
                let n: i64 = conn.query_row(
                    &format!("SELECT count(*) FROM chat_messages m WHERE m.chat=?1 AND m.scope=?2 AND m.time<?3 AND {NOT_PINNED}"),
                    rusqlite::params![chat, scope, t],
                    |r| r.get(0),
                )?;
                plan.messages += n as usize;
            }
            plan.cutoffs.push((chat, scope, c));
        }
        let rows: Vec<BlobRef> = conn
            .prepare(&format!(
                "SELECT m.chat, m.scope, m.time, m.size, m.blob_id, m.thumb_id, m.downloaded,
                        NOT {NOT_PINNED}
                 FROM chat_messages m WHERE m.blob_id <> '' OR m.thumb_id <> ''"
            ))?
            .query_map([], |r| {
                Ok(BlobRef {
                    chat: r.get(0)?,
                    scope: r.get(1)?,
                    time: r.get(2)?,
                    size: r.get(3)?,
                    blob_id: r.get(4)?,
                    thumb_id: r.get(5)?,
                    downloaded: r.get::<_, i64>(6)? != 0,
                    pinned: r.get::<_, i64>(7)? != 0,
                })
            })?
            .collect::<Result<_, _>>()?;
        plan.blobs = plan_blobs(&rows, cut_of, rule.min_size);
        Ok(plan)
    }

    /// Применить план: удалить сообщения (с их реакциями) по порогам и снять пометку
    /// «скачано» с файлов плана. Сами файлы удаляет вызывающий — у хранилища нет
    /// каталога блобов. Возвращает число удалённых сообщений.
    pub fn prune(&self, plan: &Plan) -> Result<usize, StoreError> {
        let mut conn = db::lock(&self.db);
        let tx = conn.transaction()?;
        let mut n = 0;
        for (chat, scope, c) in &plan.cutoffs {
            let Some(t) = c.messages else { continue };
            let params = rusqlite::params![chat, scope, t];
            tx.execute(
                &format!(
                    "DELETE FROM reactions WHERE chat=?1 AND scope=?2 AND EXISTS (
                         SELECT 1 FROM chat_messages m WHERE m.chat=?1 AND m.scope=?2
                             AND m.id=reactions.msg_id AND m.sender=reactions.msg_sender
                             AND m.time<?3 AND {NOT_PINNED})"
                ),
                params,
            )?;
            n += tx.execute(
                &format!(
                    "DELETE FROM chat_messages AS m WHERE m.chat=?1 AND m.scope=?2 AND m.time<?3 AND {NOT_PINNED}"
                ),
                params,
            )?;
        }
        for id in &plan.blobs {
            tx.execute(
                "UPDATE chat_messages SET downloaded=0 WHERE blob_id=?1",
                rusqlite::params![id],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }
}

/// Условие «сообщение `m` не закреплено».
const NOT_PINNED: &str = "NOT EXISTS (SELECT 1 FROM pinned p WHERE p.chat=m.chat AND p.scope=m.scope
                                      AND p.msg_id=m.id AND p.sender=m.sender)";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::store::StoredMessage;

    const NOW: i64 = 1_000 * DAY;

    fn row(chat: u8, time: i64, size: i64, blob: &str, thumb: &str) -> BlobRef {
        BlobRef {
            chat: vec![chat],
            scope: 0,
            time,
            size,
            blob_id: blob.into(),
            thumb_id: thumb.into(),
            downloaded: true,
            pinned: false,
        }
    }

    /// Чат 1 — правило «вложения старше недели», чат 2 — «никогда».
    fn cut(chat: &[u8], _scope: i64) -> Cutoffs {
        match chat {
            [1] => Cutoffs { messages: None, attachments: Some(NOW - 7 * DAY) },
            [3] => Cutoffs { messages: Some(NOW - 30 * DAY), attachments: None },
            _ => Cutoffs::default(),
        }
    }

    #[test]
    fn old_attachment_goes_new_stays() {
        let rows = [row(1, NOW - 8 * DAY, 5, "old", ""), row(1, NOW - DAY, 5, "new", "")];
        assert_eq!(plan_blobs(&rows, cut, MinSize::Any), vec!["old".to_string()]);
    }

    #[test]
    fn size_threshold_and_condition() {
        let rows = [
            row(1, NOW - 8 * DAY, 5 * 1024 * 1024, "small", ""),
            row(1, NOW - 8 * DAY, 20 * 1024 * 1024, "big", ""),
        ];
        assert_eq!(plan_blobs(&rows, cut, MinSize::Mb10), vec!["big".to_string()]);
    }

    #[test]
    fn shared_file_kept_while_any_reference_is_protected() {
        // Тот же файл в чате без правила (или в «Сохранённом») — остаётся.
        let rows = [row(1, NOW - 8 * DAY, 5, "f", ""), row(2, NOW - 8 * DAY, 5, "f", "")];
        assert!(plan_blobs(&rows, cut, MinSize::Any).is_empty());
    }

    #[test]
    fn pinned_and_not_downloaded_are_kept() {
        let mut pinned = row(1, NOW - 8 * DAY, 5, "p", "");
        pinned.pinned = true;
        let mut remote = row(1, NOW - 8 * DAY, 5, "r", "");
        remote.downloaded = false;
        assert!(plan_blobs(&[pinned, remote], cut, MinSize::Any).is_empty());
    }

    #[test]
    fn thumb_follows_its_message() {
        // Вложение ушло по правилу вложений — миниатюра остаётся карточке.
        let att = [row(1, NOW - 8 * DAY, 5, "b", "t")];
        assert_eq!(plan_blobs(&att, cut, MinSize::Any), vec!["b".to_string()]);
        // Сообщение удалено целиком — уходят и файл, и миниатюра, даже нескачанный.
        let mut gone = row(3, NOW - 31 * DAY, 5, "b", "t");
        gone.downloaded = false;
        assert_eq!(plan_blobs(&[gone], cut, MinSize::Any), vec!["b".to_string(), "t".to_string()]);
    }

    #[test]
    fn chat_never_disables_attachments_too() {
        let rule = Rule { messages: Age::Month, attachments: Age::Week, min_size: MinSize::Any };
        assert_eq!(effective(&rule, Some(Age::Never)), (None, None));
        assert_eq!(effective(&rule, Some(Age::Year)), (Age::Year.secs(), Age::Week.secs()));
        assert_eq!(effective(&rule, None), (Age::Month.secs(), Age::Week.secs()));
    }

    #[test]
    fn rule_wire_form() {
        let r = Rule { messages: Age::Month, attachments: Age::Week, min_size: MinSize::Mb10 };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, r#"{"messages":"month","attachments":"week","min_size":"10mb"}"#);
        // Старый или неполный файл — выключенное правило, а не ошибка.
        assert_eq!(serde_json::from_str::<Rule>("{}").unwrap(), Rule::default());
    }

    fn msg(id: u64, sender: &str, time: i64) -> StoredMessage {
        StoredMessage { id, sender: sender.into(), text: format!("m{id}"), time, ..Default::default() }
    }

    #[test]
    fn prune_keeps_saved_pinned_and_own_never() {
        let store = MessageStore::open(open_in_memory().unwrap());
        let me = [9u8; 32];
        let (alice, bob) = ([1u8; 32], [2u8; 32]);
        let old = NOW - 40 * DAY;
        for k in [&me, &alice, &bob] {
            store.append(Chat::Dm(k), &msg(1, "x", old)).unwrap();
            store.append(Chat::Dm(k), &msg(2, "x", old)).unwrap();
            store.append(Chat::Dm(k), &msg(3, "x", NOW - DAY)).unwrap();
        }
        store.pin(Chat::Dm(&alice), 2, "x", NOW, "x").unwrap();
        store.set_chat_autodel(Chat::Dm(&bob), Some(Age::Never)).unwrap();
        let rule = Rule { messages: Age::Month, ..Default::default() };

        let plan = store.prune_plan(NOW, &rule, &me, None).unwrap();
        assert_eq!(plan.messages, 1, "только старое незакреплённое у Алисы");
        // Предпросмотр: Боб переводится на «как в общих» — добавятся его два старых.
        let what_if = store.prune_plan(NOW, &rule, &me, Some((Chat::Dm(&bob), None))).unwrap();
        assert_eq!(what_if.messages, 3);

        assert_eq!(store.prune(&plan).unwrap(), 1);
        assert!(!store.contains(Chat::Dm(&alice), "x", 1));
        assert!(store.contains(Chat::Dm(&alice), "x", 2), "закреплённое");
        assert!(store.contains(Chat::Dm(&me), "x", 1), "Сохранённое");
        assert!(store.contains(Chat::Dm(&bob), "x", 1), "своё «никогда»");
        // Повтор ничего не находит: прогон идемпотентен.
        assert_eq!(store.prune_plan(NOW, &rule, &me, None).unwrap().messages, 0);
    }

    #[test]
    fn prune_resets_downloaded_of_planned_files() {
        let store = MessageStore::open(open_in_memory().unwrap());
        let me = [9u8; 32];
        let alice = [1u8; 32];
        let mut m = msg(1, "x", NOW - 8 * DAY);
        m.kind = "file".into();
        m.blob_id = "aa".into();
        m.size = 20 * 1024 * 1024;
        m.downloaded = true;
        store.append(Chat::Dm(&alice), &m).unwrap();
        let rule = Rule { attachments: Age::Week, min_size: MinSize::Mb10, ..Default::default() };
        let plan = store.prune_plan(NOW, &rule, &me, None).unwrap();
        assert_eq!((plan.messages, plan.blobs.clone()), (0, vec!["aa".to_string()]));
        store.prune(&plan).unwrap();
        let again = store.prune_plan(NOW, &rule, &me, None).unwrap();
        assert!(again.blobs.is_empty(), "пометка снята — файл больше не в плане");
        assert!(store.contains(Chat::Dm(&alice), "x", 1), "сообщение остаётся");
    }
}
