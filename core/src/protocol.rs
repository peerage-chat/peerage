//! Протокол сообщений.
//!
//! Два слоя:
//!   * [`Payload`] — внутреннее сообщение (тип + данные). Сериализуется и
//!     ШИФРУЕТСЯ (NaCl Box), поэтому тип сообщения не виден снаружи.
//!   * [`Envelope`] — внешний конверт (версия, отправитель, время + запечатанный
//!     payload). Передаётся как датаграмма через транспорт.
//!
//! Отклонение от ТЗ: `type` сообщения убран в зашифрованную часть (приватность
//! метаданных), а `payload` = `nonce || ciphertext`. Поля `from`/`time`/версия —
//! как в ТЗ. Кодирование конверта — JSON (отлаживаемо; для медиа позже бинарь).

use serde::{Deserialize, Serialize};

use crate::crypto::{self, CryptoError};
use crate::identity::Identity;

/// Версия формата конверта.
pub const PROTOCOL_VERSION: u8 = 1;

/// Внутреннее сообщение (шифруется целиком).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
    /// Рукопожатие при установлении сессии (нужно двунаправленно — см. README).
    Hello,
    /// Текстовое сообщение чата.
    Text { id: u64, text: String },
    /// Медиа-сообщение чата (M2): ссылка на блоб по контент-хэшу. Байты идут отдельно
    /// блоб-каналом (push при отправке + pull-по-хэшу при промахе), сообщение несёт лишь
    /// ссылку → тот же надёжный путь, что и `Text` (id + `Ack`, ретраи, офлайн-очередь,
    /// дедуп). `mime` — тип (image/*, …), `name` — исходное имя файла (для не-картинок).
    Media {
        id: u64,
        #[serde(with = "hex_bytes")]
        hash: Vec<u8>,
        mime: String,
        name: String,
    },
    /// Подтверждение доставки сообщения с данным id (✓✓ в ТЗ).
    Ack { id: u64 },
    /// Keep-alive / проверка присутствия.
    Ping,
    /// Ответ на ping.
    Pong,
    /// Приглашение в голосовую комнату (DM-звонок). room_id = первые 16 байт
    /// SHA-256(min(keyA,keyB) ++ max(keyA,keyB)) — детерминирован для обеих сторон.
    /// `id` — надёжная доставка (retry-until-Ack): Ack шлётся при ДОСТАВКЕ (до ответа
    /// пользователя), поэтому ретрай гаснет, как только звонок долетел — чинит потерю
    /// первого пакета на холодной сессии («первый звонок не доходит»).
    RoomInvite { id: u64, room_id: Vec<u8>, audio: bool, video: bool },
    /// Отклонение входящего звонка (получатель нажал «Отклонить»). Надёжно —
    /// звонящий гарантированно узнаёт отказ и перестаёт звонить.
    RoomDecline { id: u64, room_id: Vec<u8> },
    /// Групповое MLS-сообщение (Commit/Welcome/прикладное) — для пересылки
    /// участникам группы. Содержимое уже зашифровано MLS. `id` — для надёжной
    /// доставки: получатель шлёт `Ack{id}`, отправитель повторяет до Ack/TTL
    /// (per-recipient, как у DM). Дедуп повторов — на стороне MLS (generation).
    Group { id: u64, data: Vec<u8> },
    /// Запрос KeyPackage у контакта (чтобы добавить его в группу). `id` — для
    /// надёжной доставки (retry-until-Ack): получатель эхо-нёт `Ack{id}`. Раньше был
    /// fire-and-forget → на холодной сессии терялся (инвайт «не срабатывал»).
    GroupKeyRequest { id: u64 },
    /// Ответ с KeyPackage (MLS) для добавления в группу. `id` — надёжная доставка.
    GroupKeyReply { id: u64, key_package: Vec<u8> },
    /// Приглашение в группу как отдельное сообщение (решение владельца 2026-06-28):
    /// вместо тихого KeyPackage-запроса получатель видит карточку «X зовёт в Y» и сам
    /// решает вступать. При согласии он шлёт `GroupKeyReply` пригласившему. `id` —
    /// надёжная доставка; `name` — название группы для карточки.
    GroupInvite { id: u64, gid: Vec<u8>, name: String },
    /// Маячок присутствия в комнате-звонке (Discord-style): периодический сигнал
    /// «я сейчас в комнате `gid`, A/V-флаги такие». `flags`: bit0 — аудио,
    /// bit1 — видео. Низкочастотный control-plane (не медиа).
    RoomPresence { gid: Vec<u8>, flags: u8 },
    /// Запрос повторной синхронизации группы (ADR-004): отправитель отстал
    /// (пропустил Commit / удалён секрет эпохи) и просит админа заново добавить
    /// его (Remove+Add). Админ игнорирует, если проситель не в текущем ростере.
    GroupResyncRequest { gid: Vec<u8> },
    /// Запрос на выход из группы (ADR-004): отправитель уходит и просит админа
    /// отозвать его (Remove + смена эпохи). Отправитель локально уже забыл группу.
    GroupLeaveRequest { gid: Vec<u8> },
    /// Анонс блоба (M2): манифест (JSON) — план докачки. Получатель качает чанки
    /// запросами `BlobRequest`. `manifest` непрозрачен для core (парсит слой blob).
    BlobManifest { manifest: String },
    /// Запрос чанка блоба по id (BLAKE3) и индексу (получатель-драйвен pull).
    BlobRequest {
        #[serde(with = "hex_bytes")]
        id: Vec<u8>,
        index: u32,
    },
    /// Ответ с чанком блоба. Шифруется как обычное сообщение (E2E), целостность
    /// чанка отдельно проверяется по хэшу в манифесте на слое blob. `id`/`data` —
    /// hex, а не JSON-массив чисел (иначе payload раздувался бы в ~3.5× и не влезал
    /// в датаграмму).
    BlobChunk {
        #[serde(with = "hex_bytes")]
        id: Vec<u8>,
        index: u32,
        #[serde(with = "hex_bytes")]
        data: Vec<u8>,
    },
    /// Обновление профиля (M2): самоназвание + хэш аватара. Рассылается контактам при
    /// смене и на коннекте (fire-and-forget, идемпотентно). `avatar_hash` пуст = аватара
    /// нет. Получатель тянет аватар-блоб по хэшу (`BlobManifestRequest`) ТОЛЬКО если хэш
    /// новый и блоба ещё нет — контент-адрес даёт авто-дедуп.
    ProfileUpdate {
        name: String,
        #[serde(with = "hex_bytes")]
        avatar_hash: Vec<u8>,
    },
    /// Запрос манифеста блоба по id (BLAKE3) — pull-по-хэшу (аватар / ленивые картинки).
    /// Ответ — `BlobManifest`. Делает блобы запрашиваемыми, а не только push-анонсом.
    BlobManifestRequest {
        #[serde(with = "hex_bytes")]
        id: Vec<u8>,
    },
    /// Неизвестный/будущий тип payload — forward-compat (ADR-006). Сообщения версий,
    /// которые этот клиент ещё НЕ понимает (напр. реакции/закрепы/правки — post-1.0),
    /// декодируются СЮДА и молча игнорируются, а не роняют `decode`. Так новые типы
    /// сообщений добавляются аддитивно, не ломая старые клиенты. Приёмник новых типов
    /// НЕ шлёт `Ack` — отправитель погасит ретрай по TTL (безопасно).
    #[serde(other)]
    Unknown,
}

/// Компактная hex-сериализация байтовых полей блоб-сообщений.
mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Внешний конверт, передаваемый по транспорту.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Версия формата.
    pub v: u8,
    /// Публичный ключ отправителя (ed25519, hex).
    pub from: String,
    /// Unix-время отправки (секунды).
    pub time: i64,
    /// Запечатанный payload: `nonce(24) || ciphertext`, в hex.
    pub data: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("крипто: {0}")]
    Crypto(#[from] CryptoError),
    #[error("ошибка JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("неверный hex: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("неподдерживаемая версия протокола: {0}")]
    Version(u8),
    #[error("неверная длина публичного ключа отправителя")]
    BadFrom,
    #[error("повреждённый паддинг сообщения")]
    BadPadding,
}

/// Размерный бакет паддинга (M5) по типу payload: текст DM/группы добиваем до 4 КБ
/// (прячем длину сообщения по размеру), мелкий control/keepalive — только префикс
/// длины (0), чтобы не амплифицировать частые пакеты.
const TEXT_BUCKET: usize = 4096;
fn bucket_for(payload: &Payload) -> usize {
    match payload {
        Payload::Text { .. } | Payload::Group { .. } => TEXT_BUCKET,
        _ => 0,
    }
}

/// Закодировать и зашифровать `payload` для контакта `peer_ed_pub`.
/// Результат — готовая к отправке датаграмма (JSON-конверт).
pub fn encode(
    me: &Identity,
    peer_ed_pub: &[u8; 32],
    payload: &Payload,
    time: i64,
) -> Result<Vec<u8>, ProtocolError> {
    let plaintext = serde_json::to_vec(payload)?;
    // M5: добить до размерного бакета ДО шифрования (прячем длину по размеру).
    let padded = crate::pad::pad(&plaintext, bucket_for(payload));
    let sealed = crypto::seal(me, peer_ed_pub, &padded)?;
    let envelope = Envelope {
        v: PROTOCOL_VERSION,
        from: hex::encode(me.public_key_bytes()),
        time,
        data: hex::encode(&sealed),
    };
    Ok(serde_json::to_vec(&envelope)?)
}

/// Разобрать и расшифровать датаграмму. Возвращает `(ключ_отправителя, payload)`.
///
/// Расшифровка идёт по ключу `from` из конверта. Вызывающий слой ДОЛЖЕН сверить
/// возвращённый ключ отправителя с источником транспорта (Yggdrasil src key),
/// чтобы конверт нельзя было приписать чужому ключу.
pub fn decode(me: &Identity, datagram: &[u8]) -> Result<([u8; 32], Payload), ProtocolError> {
    let envelope: Envelope = serde_json::from_slice(datagram)?;
    if envelope.v != PROTOCOL_VERSION {
        return Err(ProtocolError::Version(envelope.v));
    }
    let from_bytes = hex::decode(&envelope.from)?;
    let from: [u8; 32] = from_bytes
        .as_slice()
        .try_into()
        .map_err(|_| ProtocolError::BadFrom)?;
    let sealed = hex::decode(&envelope.data)?;
    let padded = crypto::open(me, &from, &sealed)?;
    let plaintext = crate::pad::unpad(&padded).ok_or(ProtocolError::BadPadding)?;
    let payload: Payload = serde_json::from_slice(&plaintext)?;
    Ok((from, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let payload = Payload::Text {
            id: 42,
            text: "привет, как дела?".to_string(),
        };

        let datagram = encode(&alice, &bob.public_key_bytes(), &payload, 1_700_000_000).unwrap();
        let (from, decoded) = decode(&bob, &datagram).unwrap();

        assert_eq!(from, alice.public_key_bytes());
        assert_eq!(decoded, payload);
    }

    #[test]
    fn unknown_payload_type_is_ignored_not_error() {
        // Forward-compat (ADR-006): будущий тип, которого этот клиент ещё не знает
        // (напр. реакция post-1.0), декодируется в Unknown, а НЕ роняет разбор.
        let future = r#"{"type":"reaction","target":42,"emoji":"fire"}"#;
        let p: Payload = serde_json::from_str(future).unwrap();
        assert_eq!(p, Payload::Unknown);
    }

    #[test]
    fn all_payload_types_roundtrip() {
        let a = Identity::generate();
        let b = Identity::generate();
        for p in [
            Payload::Hello,
            Payload::Ping,
            Payload::Pong,
            Payload::Ack { id: 7 },
            Payload::Text { id: 1, text: "hi".into() },
            Payload::Media { id: 2, hash: vec![5u8; 32], mime: "image/jpeg".into(), name: "p.jpg".into() },
            Payload::RoomInvite { id: 11, room_id: vec![1u8; 16], audio: true, video: false },
            Payload::RoomDecline { id: 12, room_id: vec![2u8; 16] },
            Payload::Group { id: 5, data: vec![1, 2, 3, 9, 9] },
            Payload::GroupKeyRequest { id: 3 },
            Payload::GroupKeyReply { id: 4, key_package: vec![7, 7, 7] },
            Payload::GroupInvite { id: 15, gid: vec![1, 2, 3], name: "Группа".into() },
            Payload::RoomPresence { gid: vec![5, 6, 7], flags: 0b11 },
            Payload::GroupResyncRequest { gid: vec![8, 9, 10] },
            Payload::GroupLeaveRequest { gid: vec![11, 12] },
            Payload::BlobManifest { manifest: "{}".into() },
            Payload::BlobRequest { id: vec![1u8; 32], index: 2 },
            Payload::BlobChunk { id: vec![2u8; 32], index: 3, data: vec![9, 8, 7] },
            Payload::ProfileUpdate { name: "Алиса".into(), avatar_hash: vec![3u8; 32] },
            Payload::BlobManifestRequest { id: vec![4u8; 32] },
        ] {
            let d = encode(&a, &b.public_key_bytes(), &p, 0).unwrap();
            let (_, got) = decode(&b, &d).unwrap();
            assert_eq!(got, p);
        }
    }

    #[test]
    fn padding_hides_text_length() {
        let a = Identity::generate();
        let b = Identity::generate();
        let short = encode(&a, &b.public_key_bytes(), &Payload::Text { id: 1, text: "да".into() }, 0).unwrap();
        let long = encode(&a, &b.public_key_bytes(), &Payload::Text { id: 1, text: "a".repeat(500) }, 0).unwrap();
        // M5: разные по длине тексты → одинаковый размер датаграммы (бакет скрыл длину).
        assert_eq!(short.len(), long.len(), "паддинг должен скрыть длину текста");
        // Расшифровка по-прежнему работает.
        let (_, p) = decode(&b, &short).unwrap();
        assert!(matches!(p, Payload::Text { text, .. } if text == "да"));
    }

    #[test]
    fn wrong_recipient_fails_to_decode() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let eve = Identity::generate();
        let d = encode(&alice, &bob.public_key_bytes(), &Payload::Hello, 0).unwrap();
        assert!(decode(&eve, &d).is_err());
    }

    #[test]
    fn envelope_is_valid_json_with_expected_fields() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let d = encode(&alice, &bob.public_key_bytes(), &Payload::Ping, 123).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&d).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["time"], 123);
        assert_eq!(v["from"], hex::encode(alice.public_key_bytes()));
        assert!(v["data"].as_str().unwrap().len() >= crypto::NONCE_LEN * 2);
    }
}
