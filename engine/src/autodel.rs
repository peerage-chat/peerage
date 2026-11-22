//! Автоудаление у себя (план 5.4). Правило и план — в `peerage_core::autodel`;
//! здесь — где лежит общее правило, файлы блобов и когда прогон идёт сам.
//!
//! Прогон: сразу при смене правила (после подтверждения в клиенте) и раз в
//! [`AUTODEL_EVERY`] из потока-актора — сообщения стареют и без участия человека.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use peerage_core::autodel::{ChatRule, Plan, Rule};
use serde::Serialize;

use crate::{dto, now, push_event, ChatKey, Engine, GroupRowJson, LockRecover};

/// Как часто прогон идёт сам. Порог правила — неделя и больше: чаще часа незачем.
pub(crate) const AUTODEL_EVERY: Duration = Duration::from_secs(60 * 60);

/// Своё правило одного чата — для экрана «Автоудаление» («Свои правила»).
#[derive(Serialize)]
struct ChatRuleJson {
    chat: String,
    group: bool,
    age: peerage_core::autodel::Age,
}

#[derive(Serialize)]
struct StateJson {
    rule: Rule,
    chats: Vec<ChatRuleJson>,
}

/// Разобрать своё правило чата с провода: пусто — «как в общих».
pub fn parse_chat_rule(s: &str) -> Option<ChatRule> {
    if s.is_empty() {
        return Some(None);
    }
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .ok()
        .map(Some)
}

impl Engine {
    fn autodel_path(&self) -> PathBuf {
        self.data_dir.join("autodel.json")
    }

    /// Общее правило. Нет файла или он битый — выключено: удалять молча нельзя.
    pub fn autodel_rule(&self) -> Rule {
        std::fs::read(self.autodel_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Правило и чаты со своим: JSON `{rule, chats: [{chat, group, age}]}`.
    pub fn autodel_json(&self) -> String {
        let chats = self
            .messages
            .lock_safe()
            .chat_autodel()
            .into_iter()
            .map(|(chat, scope, age)| ChatRuleJson {
                chat: hex::encode(chat),
                group: scope == 1,
                age,
            })
            .collect();
        let state = StateJson {
            rule: self.autodel_rule(),
            chats,
        };
        serde_json::to_string(&state).unwrap_or_else(|_| "{}".into())
    }

    /// Что удалит правило `rule` (и своё правило чата `own` поверх сохранённых),
    /// ничего не трогая. Для шторки «Удалить старые сообщения?».
    pub fn autodel_preview(&self, rule: &Rule, own: Option<(ChatKey, ChatRule)>) -> dto::Pruned {
        let plan = self.autodel_plan(now(), rule, own.as_ref());
        dto::Pruned {
            messages: plan.messages,
            bytes: self.files_bytes(&plan.blobs),
        }
    }

    /// Записать общее правило и сразу применить. Возвращает удалённое.
    pub fn set_autodel(&self, rule: &Rule) -> dto::Pruned {
        if let Ok(v) = serde_json::to_vec(rule) {
            let _ = peerage_core::fsutil::write_atomic(self.autodel_path(), v);
        }
        self.autodel_run()
    }

    /// Записать своё правило чата и сразу применить.
    pub fn set_chat_autodel(&self, chat: ChatKey, own: ChatRule) -> dto::Pruned {
        let _ = self
            .messages
            .lock_safe()
            .set_chat_autodel(chat.as_chat(), own);
        self.autodel_run()
    }

    /// Прогон по текущим правилам. Сообщает клиенту, если что-то ушло.
    pub fn autodel_run(&self) -> dto::Pruned {
        self.autodel_run_at(now())
    }

    /// Прогон так, будто сейчас `at` (unix-секунды). Отдельно — ради тестов: недельный
    /// порог иначе не проверить.
    #[doc(hidden)]
    pub fn autodel_run_at(&self, at: i64) -> dto::Pruned {
        let plan = self.autodel_plan(at, &self.autodel_rule(), None);
        if plan.messages == 0 && plan.blobs.is_empty() {
            return dto::Pruned::default();
        }
        let messages = self.messages.lock_safe().prune(&plan).unwrap_or(0);
        let keep = self.identity_blobs();
        let mut bytes = 0u64;
        for id in plan.blobs.iter().filter(|id| !keep.contains(*id)) {
            let path = self.blob_dir.join(id);
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if std::fs::remove_file(&path).is_ok() {
                bytes += meta.len();
            }
        }
        let out = dto::Pruned { messages, bytes };
        if messages > 0 || bytes > 0 {
            push_event(self, dto::UiEvent::HistoryPruned { messages, bytes });
        }
        out
    }

    fn autodel_plan(&self, at: i64, rule: &Rule, own: Option<&(ChatKey, ChatRule)>) -> Plan {
        let own = own.map(|(c, r)| (c.as_chat(), *r));
        self.messages
            .lock_safe()
            .prune_plan(at, rule, &self.me_pub, own)
            .unwrap_or_default()
    }

    /// Сколько весят на диске файлы из списка — то, что реально освободится.
    fn files_bytes(&self, ids: &[String]) -> u64 {
        let keep = self.identity_blobs();
        ids.iter()
            .filter(|id| !keep.contains(*id))
            .filter_map(|id| std::fs::metadata(self.blob_dir.join(id)).ok())
            .map(|m| m.len())
            .sum()
    }

    /// Файлы, которые держат лица, а не переписку: свой аватар (его у нас запрашивают
    /// контакты), аватары контактов и фото групп. Ни очистка, ни автоудаление их не
    /// трогают — иначе люди и группы остались бы без лица до следующей смены.
    pub(crate) fn identity_blobs(&self) -> HashSet<String> {
        let mut keep: HashSet<String> = HashSet::new();
        keep.extend(self.self_avatar_id());
        let cached = self.group_cache.lock_safe().list.clone();
        let rows: Vec<GroupRowJson> = serde_json::from_str(&cached).unwrap_or_default();
        keep.extend(rows.into_iter().map(|r| r.photo).filter(|p| !p.is_empty()));
        keep.extend(
            self.contacts
                .lock_safe()
                .list()
                .iter()
                .map(|c| c.avatar_id.clone())
                .filter(|a| !a.is_empty()),
        );
        keep
    }
}
