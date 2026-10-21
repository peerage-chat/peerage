//! Формы данных, которые движок отдаёт клиенту (снимки и события).
//!
//! **Зачем отдельный модуль.** Раньше эти формы собирались литералами
//! `serde_json::json!{...}` прямо в местах вызова, а клиент разбирал их по строковым
//! ключам. Схема жила в двух местах и ни одним компилятором не проверялась:
//! переименовал поле в Rust — узнал об этом на устройстве. Здесь схема — ТИП, и
//! ошибка ловится сборкой.
//!
//! **Формат провода не меняется.** Это ровно тот же JSON, что и раньше: типы
//! подобраны под существующие имена полей, чтобы клиент продолжал работать без
//! правок. Тесты внизу фиксируют форму — если поле переименуют, тест упадёт раньше,
//! чем это увидит пользователь.

use serde::Serialize;

// ── Снимки (Query → Snapshot) ───────────────────────────────────────────────

/// Сводка офлайн-очереди. `warn` — только сигнал клиенту показать предупреждение:
/// движок ничего не удаляет сам.
#[derive(Debug, Clone, Serialize)]
pub struct OutboxStats {
    pub pending: usize,
    pub bytes: usize,
    pub peers: usize,
    pub warn: bool,
}

/// Кто сейчас флудит: источник, сколько его пакетов отброшено бюджетом и с каким
/// темпом. Остановить поток на устройстве нельзя (threat-model N8), но пользователь
/// имеет право знать, почему у него тает трафик.
#[derive(Debug, Clone, Serialize)]
pub struct FloodPeer {
    /// hex публичного ключа источника.
    pub peer: String,
    /// Всего отброшено пакетов за время жизни процесса.
    pub dropped: u64,
    /// Оценка темпа в последнем секундном окне, пакетов/с.
    pub pps: f64,
    /// Источник уже заблокирован (его режет и транспорт, до FFI).
    pub blocked: bool,
}

/// Строка очереди по получателю. `oldest` — unix-секунды самой старой записи.
#[derive(Debug, Clone, Serialize)]
pub struct OutboxPeer {
    pub peer: String,
    pub count: usize,
    pub bytes: usize,
    pub oldest: i64,
}

/// Сколько вложений от конкретного чата лежит на диске.
#[derive(Debug, Clone, Serialize)]
pub struct ChatStorage {
    pub peer: String,
    pub files: u64,
    pub bytes: u64,
}

/// Что занимает место. `by_kind` — байты по типу вложения (image/video/voice/file).
#[derive(Debug, Clone, Serialize)]
pub struct StorageStats {
    pub blobs: u64,
    pub blob_bytes: u64,
    pub outbox_pending: usize,
    pub outbox_bytes: usize,
    pub by_kind: std::collections::HashMap<String, u64>,
    pub chats: Vec<ChatStorage>,
}

/// Участник комнаты по маячку присутствия.
#[derive(Debug, Clone, Serialize)]
pub struct RoomMember {
    pub peer: String,
    pub audio: bool,
    pub video: bool,
}

/// Параметры аудио-сессии звонка (клиент настраивает по ним AudioTrack/Record).
#[derive(Debug, Clone, Serialize)]
pub struct CallParams {
    pub call_id: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_len: usize,
}

/// Приёмная сторона диагностического замера.
#[derive(Debug, Clone, Serialize)]
pub struct BenchRecv {
    pub pkts: u64,
    pub bytes: u64,
    pub secs: f64,
    pub mbps: f64,
}

/// Результат диагностического замера скорости до пира.
#[derive(Debug, Clone, Serialize)]
pub struct BenchResult {
    pub sent_pkts: u64,
    pub sent_bytes: u64,
    pub secs: f64,
    pub mbps: f64,
}

/// Запись журнала звонков. `outcome`: answered | missed | declined | unanswered.
#[derive(Debug, Clone, Serialize)]
pub struct CallEntry {
    pub peer: String,
    pub gid: String,
    pub incoming: bool,
    pub outcome: String,
    pub started: i64,
    pub duration: i64,
}

// ── События (Event: движок → клиент) ────────────────────────────────────────

/// Асинхронное уведомление клиенту. Сериализуется как объект с полем `type` —
/// ровно то, что клиент уже разбирает.
///
/// Варианты без полей (`GroupsChanged` и подобные) — «перечитай снимок»: движок
/// не тащит состояние в событие, чтобы не было двух источников правды.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    Presence { peer: String, online: bool },
    /// Новая строка переписки — личной или групповой, своей или входящей. Едет
    /// ровно та строка, что легла в историю (подпись, «переслано», ответ, номер
    /// строки): одна форма для ленты и для истории, и открытый чат не расходится с
    /// тем, что покажет перезаход.
    Message { chat: String, group: bool, msg: p2p_core::store::StoredMessage },
    Ack { peer: String, id: u64 },
    /// Собеседник отозвал своё сообщение — строка исчезла из истории.
    Retracted { peer: String, id: u64 },
    /// В группе кто-то отозвал сообщение — историю нужно перечитать.
    GroupRetracted { gid: String },
    Undelivered { peer: String, id: u64 },
    ContactRequest { peer: String, new: bool },
    ContactsChanged,
    GroupsChanged,
    GroupInvitesChanged,
    /// Список закреплённых в чате изменился (hex ключа собеседника или gid).
    PinsChanged { chat: String },
    /// Сообщение исправлено автором: текст в ленте надо заменить на месте.
    MessageEdited { chat: String, id: u64, sender: String, text: String },
    GroupRemoved { gid: String },
    RoomInvite { peer: String, room_id: String, audio: bool, video: bool },
    RoomDecline { peer: String, room_id: String },
    RoomPresence { gid: String, peer: String, flags: u8, left: bool },
    BlobReceived { id: String, mime: String, size: u64 },
    /// Наш адрес сменился (ADR-007): новый адрес Yggdrasil и номер эпохи. Личность
    /// при этом прежняя — контакты, верификации и группы не затронуты.
    TransportRotated { address: String, epoch: u32 },
    BlobProgress { id: String, done: u32, total: u32 },
    /// Докачка не началась: `too_large` (больше предела приёма) или `no_space`
    /// (на диске не хватает места). Заказ снят — кнопка «Скачать» снова доступна.
    BlobFailed { id: String, reason: String },
}

impl UiEvent {
    /// Сериализовать в строку для очереди событий. Пустой JSON-объект вместо паники:
    /// потеря одного события лучше, чем падение потока-актора.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Фиксируем ФОРМУ провода: клиент разбирает эти имена по строкам, поэтому
    /// переименование поля в Rust обязано ломать тест, а не устройство.
    #[test]
    fn event_wire_shape_is_stable() {
        let ev = UiEvent::Presence { peer: "aa".into(), online: true };
        assert_eq!(ev.to_json(), r#"{"type":"presence","peer":"aa","online":true}"#);

        let ev = UiEvent::Ack { peer: "bb".into(), id: 7 };
        assert_eq!(ev.to_json(), r#"{"type":"ack","peer":"bb","id":7}"#);

        // Вариант без полей — только тег: клиент по нему перечитывает снимок.
        assert_eq!(UiEvent::GroupsChanged.to_json(), r#"{"type":"groups_changed"}"#);

        let ev = UiEvent::BlobProgress { id: "cc".into(), done: 3, total: 10 };
        assert_eq!(ev.to_json(), r#"{"type":"blob_progress","id":"cc","done":3,"total":10}"#);

        // Строка переписки едет целиком — теми же полями, что и в истории.
        let msg = p2p_core::store::StoredMessage {
            id: 5,
            sender: "aa".into(),
            text: "подпись".into(),
            time: 100,
            kind: "file".into(),
            blob_id: "bb".into(),
            fwd_from: "cc".into(),
            row: 9,
            ..Default::default()
        };
        let json = UiEvent::Message { chat: "ff".into(), group: true, msg }.to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "message");
        assert_eq!(v["chat"], "ff");
        assert_eq!(v["group"], true);
        assert_eq!(v["msg"]["text"], "подпись");
        assert_eq!(v["msg"]["fwd_from"], "cc");
        assert_eq!(v["msg"]["blob_id"], "bb");
        assert_eq!(v["msg"]["row"], 9);
    }

    #[test]
    fn snapshot_wire_shape_is_stable() {
        let s = OutboxStats { pending: 2, bytes: 30, peers: 1, warn: false };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"pending":2,"bytes":30,"peers":1,"warn":false}"#
        );

        let p = OutboxPeer { peer: "dd".into(), count: 5, bytes: 90, oldest: 111 };
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"peer":"dd","count":5,"bytes":90,"oldest":111}"#
        );

        let m = RoomMember { peer: "ee".into(), audio: true, video: false };
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r#"{"peer":"ee","audio":true,"video":false}"#
        );
    }
}
