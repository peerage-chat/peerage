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
    /// Подтверждение доставки сообщения с данным id (✓✓ в ТЗ).
    Ack { id: u64 },
    /// Keep-alive / проверка присутствия.
    Ping,
    /// Ответ на ping.
    Pong,
    /// Приглашение в голосовую комнату (DM-звонок). room_id = первые 16 байт
    /// SHA-256(min(keyA,keyB) ++ max(keyA,keyB)) — детерминирован для обеих сторон.
    RoomInvite { room_id: Vec<u8>, audio: bool, video: bool },
    /// Отклонение входящего звонка (получатель нажал «Отклонить»).
    RoomDecline { room_id: Vec<u8> },
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
    let sealed = crypto::seal(me, peer_ed_pub, &plaintext)?;
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
    let plaintext = crypto::open(me, &from, &sealed)?;
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
    fn all_payload_types_roundtrip() {
        let a = Identity::generate();
        let b = Identity::generate();
        for p in [
            Payload::Hello,
            Payload::Ping,
            Payload::Pong,
            Payload::Ack { id: 7 },
            Payload::Text { id: 1, text: "hi".into() },
            Payload::RoomInvite { room_id: vec![1u8; 16], audio: true, video: false },
            Payload::RoomDecline { room_id: vec![2u8; 16] },
            Payload::Group { id: 5, data: vec![1, 2, 3, 9, 9] },
            Payload::GroupKeyRequest { id: 3 },
            Payload::GroupKeyReply { id: 4, key_package: vec![7, 7, 7] },
            Payload::RoomPresence { gid: vec![5, 6, 7], flags: 0b11 },
            Payload::GroupResyncRequest { gid: vec![8, 9, 10] },
            Payload::GroupLeaveRequest { gid: vec![11, 12] },
            Payload::BlobManifest { manifest: "{}".into() },
            Payload::BlobRequest { id: vec![1u8; 32], index: 2 },
            Payload::BlobChunk { id: vec![2u8; 32], index: 3, data: vec![9, 8, 7] },
        ] {
            let d = encode(&a, &b.public_key_bytes(), &p, 0).unwrap();
            let (_, got) = decode(&b, &d).unwrap();
            assert_eq!(got, p);
        }
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
