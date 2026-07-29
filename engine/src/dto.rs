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
    Text { peer: String, id: u64, text: String, time: i64 },
    Media {
        peer: String,
        id: u64,
        time: i64,
        kind: String,
        blob_id: String,
        mime: String,
        name: String,
        size: u64,
        duration_ms: u32,
        width: u32,
        height: u32,
        thumb_id: String,
        /// Байты уже лежат локально.
        downloaded: bool,
        /// Мелкое тянем сразу; крупное ждёт кнопки — клиенту это нужно, чтобы
        /// не показывать спиннер там, где докачки не будет.
        auto: bool,
    },
    Ack { peer: String, id: u64 },
    Undelivered { peer: String, id: u64 },
    ContactRequest { peer: String, new: bool },
    ContactsChanged,
    GroupsChanged,
    GroupInvitesChanged,
    GroupRemoved { gid: String },
    RoomInvite { peer: String, room_id: String, audio: bool, video: bool },
    RoomDecline { peer: String, room_id: String },
    RoomPresence { gid: String, peer: String, flags: u8, left: bool },
    BlobReceived { id: String, mime: String, size: u64 },
    BlobProgress { id: String, done: u32, total: u32 },
    /// Групповое сообщение. Форма одна на два случая — эхо своего (`mine`) и
    /// входящее: клиент различает их по `mine`, а не по набору полей.
    /// Медиа-поля пропускаются, когда сообщение текстовое, — так провод не
    /// меняется относительно прежнего варианта на литералах.
    GroupMsg {
        gid: String,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        mine: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sender: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        system: Option<bool>,
        /// Вложение: отдельным объектом, а не десятком плоских опций рядом с
        /// текстовыми полями — набор полей карточки один и тот же, где бы она ни
        /// показывалась.
        #[serde(skip_serializing_if = "Option::is_none")]
        media: Option<GroupMediaEv>,
    },
}

/// Вложение группового сообщения в событии. Повторяет поля карточки из DM: имя,
/// размер, превью — всё известно ДО загрузки байт, иначе нечего показывать вместо
/// автоматической докачки.
#[derive(Debug, Clone, Serialize)]
pub struct GroupMediaEv {
    pub kind: String,
    pub blob_id: String,
    pub mime: String,
    pub name: String,
    pub size: u64,
    pub duration_ms: u32,
    pub width: u32,
    pub height: u32,
    pub thumb_id: String,
    /// Байты уже лежат локально.
    pub downloaded: bool,
    /// Мелкое тянем сразу; крупное ждёт кнопки.
    pub auto: bool,
}

impl UiEvent {
    /// Эхо своего группового сообщения: клиент по нему перечитывает историю,
    /// потому что blob_id считается в движке (по очищенным байтам).
    pub fn group_msg_mine(gid: String) -> Self {
        Self::GroupMsg { gid, mine: true, text: None, sender: None, system: None, media: None }
    }
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

        // Эхо своего группового: только gid и mine — лишних полей на проводе нет.
        assert_eq!(
            UiEvent::group_msg_mine("ff".into()).to_json(),
            r#"{"type":"group_msg","gid":"ff","mine":true}"#
        );

        // Входящее групповое с вложением: карточка приезжает целиком (имя, размер,
        // превью, auto) — по ней клиент решает, рисовать «скачать» или спиннер.
        let ev = UiEvent::GroupMsg {
            gid: "ff".into(),
            mine: false,
            text: Some("подпись".into()),
            sender: Some("aa".into()),
            system: Some(false),
            media: Some(GroupMediaEv {
                kind: "file".into(),
                blob_id: "bb".into(),
                mime: "video/mp4".into(),
                name: "clip.mp4".into(),
                size: 900,
                duration_ms: 12,
                width: 1,
                height: 2,
                thumb_id: "cc".into(),
                downloaded: false,
                auto: false,
            }),
        };
        assert_eq!(
            ev.to_json(),
            r#"{"type":"group_msg","gid":"ff","text":"подпись","sender":"aa","system":false,"media":{"kind":"file","blob_id":"bb","mime":"video/mp4","name":"clip.mp4","size":900,"duration_ms":12,"width":1,"height":2,"thumb_id":"cc","downloaded":false,"auto":false}}"#
        );
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
