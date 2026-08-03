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

/// Заявка узла о выполненной транзитной работе — **зарезервировано под ADR-008,
/// в 1.0 не выдаётся и не проверяется**.
///
/// Код открыт (GPL-3.0), поэтому любой может вырезать ретрансляцию, оставив флаг
/// «я транзитный узел», и получать привилегии (приоритет очереди, длинный TTL) даром.
/// Единственная защита, которую нельзя обойти правкой своего же кода, — квитанции:
/// получатель подписывает своим ключом факт «пакет пришёл через узел A», и подделать
/// эту подпись A не может, не выполнив работу.
///
/// **Почему здесь дайджест, а не сами квитанции.** Одна квитанция — подпись 64 Б плюс
/// ключ; в hex-JSON это ~200 Б, то есть уже одна не влезает в бакет CONTROL 256 Б, а
/// маячок присутствия обязан быть неотличим по размеру от ack (M5). Поэтому в маячке
/// едет лишь обязательство: хэш пачки и её размер, — а сами квитанции запрашивает
/// тот, кому есть смысл проверять. Это же ограничивает и объём вранья: заявленное
/// число подтверждается только предъявленной пачкой.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitClaim {
    /// BLAKE3 пачки квитанций за период.
    #[serde(with = "hex_bytes")]
    pub digest: Vec<u8>,
    /// Сколько квитанций в пачке (заявка; проверяется по предъявлению).
    pub count: u32,
    /// Номер расчётного периода — чтобы старую пачку нельзя было предъявить снова.
    pub epoch: u32,
}

/// Внутреннее сообщение (шифруется целиком).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
    /// Рукопожатие при установлении сессии (нужно двунаправленно — см. README).
    Hello,
    /// Текстовое сообщение чата.
    ///
    /// `reply_to` — id сообщения, на которое отвечают (0 = обычное сообщение), а
    /// `reply_mine` — чьё оно **с точки зрения отправителя** (см. ADR-006: ссылка на
    /// сообщение — пара «автор + id»). Получатель переворачивает флаг: то, что для
    /// отправителя «моё», для него — «его». Без этого флага ответ на своё и ответ на
    /// чужое сообщение с одинаковым id были бы неразличимы.
    Text {
        id: u64,
        text: String,
        #[serde(default)]
        reply_to: u64,
        #[serde(default)]
        reply_mine: bool,
        /// Пересылка: hex-ключ автора ИСХОДНОГО сообщения (пусто — своё). Едет
        /// вместе с текстом, а не отдельной строкой в нём, чтобы клиент мог
        /// показать источник честно и не давал подделать подпись «переслано от».
        /// Пустое поле не сериализуется — байты 1.0 не меняются.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        fwd_from: String,
        /// «Переслано, автора не называем»: пересылающий скрыл источник. Ключ
        /// автора при этом НЕ отправляется вовсе — иначе «скрыл» означало бы
        /// «спрятал в интерфейсе, но отдал по проводу».
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        fwd_anon: bool,
    },
    /// Отзыв СВОЕГО ранее отправленного сообщения («удалить у всех»).
    ///
    /// `id` — для надёжной доставки (retry-until-Ack), `target` — id отзываемого
    /// сообщения. Честная граница: это просьба удалить, а не гарантия — получатель
    /// мог прочитать сообщение раньше, сделать снимок экрана или иметь патченый
    /// клиент. UI обязан говорить это прямо, а не обещать «удалено у всех».
    Retract { id: u64, target: u64 },
    /// Закрепить/открепить сообщение в личном чате. Закреп — свойство ЧАТА, а не
    /// одного из собеседников: у обоих он один и тот же, иначе «закрепил важное»
    /// значило бы «закрепил у себя», и вторая сторона об этом не узнала бы.
    ///
    /// `target` + `target_mine` — ссылка на сообщение (ADR-006); «чьё» едет с точки
    /// зрения ОТПРАВИТЕЛЯ и переворачивается на приёме, как у ответа. `on = false` —
    /// открепить. Бакет CONTROL: по размеру неотличимо от ack.
    Pin { id: u64, target: u64, target_mine: bool, on: bool },
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
        /// Размер содержимого в байтах. Нужен, чтобы показать карточку «видео, 800 МБ»
        /// ДО скачивания: сам размер иначе приезжает только с манифестом, т.е. когда
        /// докачка уже началась.
        size: u64,
        /// Длительность аудио/видео в миллисекундах (0 — не применимо/неизвестно).
        duration_ms: u32,
        /// Размеры кадра картинки/видео (0 — неизвестно).
        width: u32,
        height: u32,
        /// Подпись к вложению. Едет ВНУТРИ медиа-сообщения, а не отдельным
        /// сообщением: иначе фото с подписью — это два пакета, и наблюдатель видит
        /// характерную пару вместо одного. Длина ограничена [`MAX_CAPTION`], чтобы
        /// сообщение оставалось в бакете APP 4 КБ и не выдавало себя размером (M5).
        caption: String,
        /// Хэш блоба-МИНИАТЮРЫ (пусто — нет превью). Именно ссылка, а не сами байты:
        /// превью инлайн раздуло бы медиа-сообщение и сломало размерную
        /// неразличимость «текст ≡ медиа» (M5). Миниатюра мелкая, качается сразу.
        #[serde(with = "hex_bytes")]
        thumb: Vec<u8>,
        /// Пересылка: hex-ключ автора исходного сообщения (пусто — своё).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        fwd_from: String,
        /// «Переслано, автора не называем» (см. `Text::fwd_anon`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        fwd_anon: bool,
    },
    /// Подтверждение доставки сообщения с данным id (✓✓ в ТЗ).
    Ack { id: u64 },
    /// Keep-alive / проверка присутствия.
    ///
    /// `transit` — ЗАРЕЗЕРВИРОВАНО под proof-of-transit (ADR-008); в 1.0 всегда
    /// `None` и ни на что не влияет. Поле заведено заранее, потому что позже его
    /// уже некуда будет деть: маячок присутствия обязан оставаться в бакете
    /// CONTROL 256 Б (M5), а отдельный пакет «я транзитный узел» был бы новым
    /// различимым классом трафика.
    Ping {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transit: Option<TransitClaim>,
    },
    /// Ответ на ping. `transit` — см. [`Payload::Ping`].
    Pong {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transit: Option<TransitClaim>,
    },
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
    /// Часть манифеста крупного блоба. Список хэшей чанков растёт линейно с размером
    /// файла и одним пакетом перестаёт влезать в датаграмму (это и держало потолок
    /// 4 МБ), поэтому манифест едет частями. Метаданные (`mime`/`size`) дублируются
    /// в каждой части: получатель может проверить лимиты и отказаться, не дожидаясь
    /// всех частей. `hashes` — сырые BLAKE3 чанков (кратно 32), hex на проводе.
    BlobManifestPart {
        #[serde(with = "hex_bytes")]
        id: Vec<u8>,
        mime: String,
        size: u64,
        part: u32,
        total: u32,
        #[serde(with = "hex_bytes")]
        hashes: Vec<u8>,
    },
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
    /// Объявление о смене АДРЕСА (ADR-007): «моя личность прежняя, слать теперь сюда».
    ///
    /// Адрес Yggdrasil = функция ключа узла, поэтому оборвать чужой флуд можно только
    /// сменой ключа. Чтобы это не стоило личности, транспортный ключ отделён от неё:
    /// здесь едет новый транспортный ключ, номер эпохи и подпись **ключом личности**.
    ///
    /// Получатель принимает объявление, только если подпись сходится с уже известной
    /// ему личностью отправителя, а эпоха строго новее последней известной
    /// (`ContactStore::apply_transport_rotate`). Без первого условия это вектор
    /// захвата контакта, без второго — переигрывание старого адреса.
    ///
    /// `id` — чтобы объявление шло надёжным путём (retry-until-Ack + офлайн-очередь):
    /// контакт, отсутствовавший в момент смены, обязан узнать новый адрес по
    /// возвращении, иначе связь с ним теряется навсегда.
    TransportRotate {
        id: u64,
        epoch: u32,
        #[serde(with = "hex_bytes")]
        transport_pub: Vec<u8>,
        #[serde(with = "hex_bytes")]
        sig: Vec<u8>,
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

/// Размерные бакеты паддинга (M5, анти-трафик-анализ). Наблюдатель меша видит только
/// РАЗМЕР шифртекста, поэтому цель — сделать классы сообщений неразличимыми ВНУТРИ
/// бакета, не раздувая частые пакеты.
///
/// * [`CONTROL_BUCKET`] — мелкий control: ack/ping/pong/маячок присутствия И
///   приглашение-отклонение звонка. Ключевое: «начался звонок» неотличимо от
///   «пришёл ack» — оба ровно 256 Б.
/// * [`APP_BUCKET`] — прикладные сообщения: текст, медиа-ссылка, MLS-группа,
///   приглашение в группу, профиль. Ключевое: картинка/файл/голосовое неотличимы
///   от текста, а длина текста скрыта в пределах бакета.
/// * [`CHUNK_BUCKET`] — тело блоба: последний (короткий) чанк добивается до полного,
///   иначе точный размер файла утекал бы остатком `size mod CHUNK_SIZE`.
///
/// Границы (в threat-model честно): скрыт РАЗМЕР, но не тайминг, не число пакетов и
/// не байт канала (chat/media) — см. `docs/protocol-spec.md`.
/// Потолок подписи к вложению в БАЙТАХ. Вместе с именем файла и хэшами подпись
/// обязана умещаться в бакет APP (4 КБ) — иначе медиа-сообщение вылезет в следующий
/// бакет и станет отличимым от текстового по размеру.
pub const MAX_CAPTION: usize = 1024;

const CONTROL_BUCKET: usize = 256;
const APP_BUCKET: usize = 4096;
/// Чанк блоба 12 КБ → hex удваивает (~24.6 КБ JSON). Бакет с запасом на JSON-оверхед;
/// конверт (hex шифртекста ≈ ×2) остаётся заметно ниже датаграммы 65535 — на это есть
/// тест `padded_blob_chunk_fits_datagram`.
const CHUNK_BUCKET: usize = 25 * 1024;

fn bucket_for(payload: &Payload) -> usize {
    match payload {
        // Прикладные: текст и медиа обязаны быть в ОДНОМ бакете (DoD M5).
        Payload::Text { .. }
        | Payload::Media { .. }
        | Payload::Group { .. }
        | Payload::GroupInvite { .. }
        | Payload::GroupKeyReply { .. }
        | Payload::ProfileUpdate { .. }
        // Смена адреса (ADR-007) не влезает в CONTROL: одна только подпись 64 Б,
        // плюс ключ — с бакетом 256 Б это несовместимо. Прячем среди прикладных:
        // на проводе объявление неотличимо от обычного текста или картинки, что
        // даже лучше control-класса — флудер не видит момент ротации.
        | Payload::TransportRotate { .. }
        | Payload::BlobManifest { .. } => APP_BUCKET,
        // Тело блоба: прячем остаток последнего чанка.
        Payload::BlobChunk { .. } => CHUNK_BUCKET,
        // Всё остальное — мелкий и/или частый control (в т.ч. RoomInvite/RoomDecline:
        // так факт звонка не выдаётся размером пакета).
        _ => CONTROL_BUCKET,
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
            reply_to: 0,
            reply_mine: false,
            fwd_from: String::new(),
            fwd_anon: false,
        };

        let datagram = encode(&alice, &bob.public_key_bytes(), &payload, 1_700_000_000).unwrap();
        let (from, decoded) = decode(&bob, &datagram).unwrap();

        assert_eq!(from, alice.public_key_bytes());
        assert_eq!(decoded, payload);
    }

    /// Объявление о смене адреса переживает шифрование целиком и проверяется у
    /// получателя ключом ЛИЧНОСТИ отправителя — то есть подписью, а не тем, откуда
    /// пакет пришёл. Иначе смена адреса не отличалась бы от захвата контакта.
    #[test]
    fn transport_rotate_roundtrip_and_verifies_by_identity() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let tk = alice.transport_public(5);
        let sig = alice.sign_transport_rotate(5, &tk);
        let payload = Payload::TransportRotate {
            id: 77,
            epoch: 5,
            transport_pub: tk.to_vec(),
            sig: sig.to_vec(),
        };

        let datagram = encode(&alice, &bob.public_key_bytes(), &payload, 0).unwrap();
        let (from, decoded) = decode(&bob, &datagram).unwrap();
        assert_eq!(from, alice.public_key_bytes());
        assert_eq!(decoded, payload);

        let Payload::TransportRotate { epoch, transport_pub, sig, .. } = decoded else {
            panic!("не тот вариант");
        };
        let tp = <[u8; 32]>::try_from(transport_pub.as_slice()).unwrap();
        let sg = <[u8; 64]>::try_from(sig.as_slice()).unwrap();
        assert!(crate::identity::verify_transport_rotate(&from, epoch, &tp, &sg));
        // Тот же пакет, приписанный другой личности, не проходит.
        assert!(!crate::identity::verify_transport_rotate(
            &bob.public_key_bytes(),
            epoch,
            &tp,
            &sg
        ));
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
            Payload::Ping { transit: None },
            Payload::Pong { transit: None },
            Payload::Ack { id: 7 },
            Payload::Text { id: 1, text: "hi".into(), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false },
            Payload::Media {
                id: 2, hash: vec![5u8; 32], mime: "image/jpeg".into(), name: "p.jpg".into(),
                size: 1234, duration_ms: 0, width: 800, height: 600, caption: String::new(), thumb: vec![9u8; 32], fwd_from: String::new(), fwd_anon: false,
            },
            // Видео с длительностью и без превью — поля опциональны.
            Payload::Media {
                id: 3, hash: vec![6u8; 32], mime: "video/mp4".into(), name: "v.mp4".into(),
                size: 900_000_000, duration_ms: 125_000, width: 1920, height: 1080, caption: String::new(), thumb: Vec::new(), fwd_from: String::new(), fwd_anon: false,
            },
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
            Payload::BlobManifestPart {
                id: vec![1u8; 32], mime: "video/mp4".into(), size: 12345,
                part: 2, total: 7, hashes: vec![9u8; 64],
            },
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
        let short = encode(&a, &b.public_key_bytes(), &Payload::Text { id: 1, text: "да".into(), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false }, 0).unwrap();
        let long = encode(&a, &b.public_key_bytes(), &Payload::Text { id: 1, text: "a".repeat(500), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false }, 0).unwrap();
        // M5: разные по длине тексты → одинаковый размер датаграммы (бакет скрыл длину).
        assert_eq!(short.len(), long.len(), "паддинг должен скрыть длину текста");
        // Расшифровка по-прежнему работает.
        let (_, p) = decode(&b, &short).unwrap();
        assert!(matches!(p, Payload::Text { text, .. } if text == "да"));
    }

    /// DoD M5: размеры НА ПРОВОДЕ не различают текст / медиа / звонок.
    #[test]
    fn text_media_and_call_are_indistinguishable_by_size() {
        let a = Identity::generate();
        let b = Identity::generate();
        let enc = |p: &Payload| encode(&a, &b.public_key_bytes(), p, 0).unwrap().len();

        // Прикладной класс: текст (короткий/длинный), картинка, голосовое, файл, группа.
        let sizes = [
            enc(&Payload::Text { id: 1, text: "да".into(), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false }),
            enc(&Payload::Text { id: 1, text: "a".repeat(900), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false }),
            enc(&Payload::Media {
                id: 2, hash: vec![5u8; 32], mime: "image/jpeg".into(),
                name: "secret-photo.jpg".into(), size: 2_400_000,
                duration_ms: 0, width: 4000, height: 3000, caption: String::new(), thumb: vec![1u8; 32], fwd_from: String::new(), fwd_anon: false,
            }),
            enc(&Payload::Media {
                id: 3, hash: vec![6u8; 32], mime: "audio/mp4".into(),
                name: "voice.m4a".into(), size: 48_000,
                duration_ms: 7_400, width: 0, height: 0, caption: String::new(), thumb: Vec::new(), fwd_from: String::new(), fwd_anon: false,
            }),
            enc(&Payload::Media {
                id: 4, hash: vec![7u8; 32], mime: "application/pdf".into(),
                name: "договор-2026.pdf".into(), size: 900_000_000,
                duration_ms: 0, width: 0, height: 0, caption: String::new(), thumb: Vec::new(), fwd_from: String::new(), fwd_anon: false,
            }),
            enc(&Payload::Group { id: 5, data: vec![1u8; 200] }),
            // Смена адреса (ADR-007) едет здесь же: подпись 64 Б не влезает в
            // control-бакет, зато среди прикладных объявление неотличимо от
            // обычного сообщения — флудер не видит по проводу, что жертва прямо
            // сейчас уходит на новый адрес.
            enc(&Payload::TransportRotate {
                id: 14, epoch: 7, transport_pub: vec![3u8; 32], sig: vec![4u8; 64],
            }),
        ];
        assert!(
            sizes.iter().all(|s| *s == sizes[0]),
            "текст/картинка/голосовое/файл/группа/смена-адреса должны быть одного размера, получено {sizes:?}"
        );

        // Control-класс: начало звонка неотличимо от ack/ping/маячка присутствия.
        let ctl = [
            enc(&Payload::RoomInvite { id: 11, room_id: vec![1u8; 16], audio: true, video: true }),
            enc(&Payload::RoomDecline { id: 12, room_id: vec![2u8; 16] }),
            enc(&Payload::Ack { id: 13 }),
            enc(&Payload::Ping { transit: None }),
            enc(&Payload::RoomPresence { gid: vec![9u8; 16], flags: 0b11 }),
        ];
        assert!(
            ctl.iter().all(|s| *s == ctl[0]),
            "звонок должен быть неотличим от ack/ping/присутствия, получено {ctl:?}"
        );
    }

    /// Зарезервированное поле proof-of-transit (ADR-008) обязано умещаться в тот же
    /// бакет CONTROL. Смысл резервирования именно в этом: заполнив поле позже, мы не
    /// имеем права сделать маячок присутствия отличимым по размеру от ack — иначе
    /// наблюдатель начнёт видеть, кто в сети, а свойство M5 сломается задним числом.
    #[test]
    fn presence_with_transit_claim_stays_in_control_bucket() {
        let a = Identity::generate();
        let b = Identity::generate();
        let enc = |p: &Payload| encode(&a, &b.public_key_bytes(), p, 0).unwrap().len();
        let claim = TransitClaim { digest: vec![0xab; 32], count: u32::MAX, epoch: u32::MAX };
        assert_eq!(
            enc(&Payload::Ping { transit: Some(claim.clone()) }),
            enc(&Payload::Ack { id: 1 }),
            "маячок с квитанциями обязан остаться неотличим от ack по размеру"
        );
        assert_eq!(
            enc(&Payload::Pong { transit: Some(claim) }),
            enc(&Payload::Ack { id: 1 }),
        );
    }

    /// Пустое поле не должно менять байты на проводе: 1.0 обязан слать ровно то же,
    /// что слал до резервирования.
    #[test]
    fn empty_transit_field_is_absent_on_the_wire() {
        let json = serde_json::to_string(&Payload::Ping { transit: None }).unwrap();
        assert_eq!(json, r#"{"type":"ping"}"#);
        // И обратно: пакет без поля читается (совместимость в обе стороны).
        let p: Payload = serde_json::from_str(r#"{"type":"ping"}"#).unwrap();
        assert_eq!(p, Payload::Ping { transit: None });
    }

    /// Последний (короткий) чанк блоба добивается до полного — иначе точный размер
    /// файла утекал бы остатком. И при этом конверт обязан влезать в датаграмму.
    #[test]
    fn padded_blob_chunk_fits_datagram() {
        let a = Identity::generate();
        let b = Identity::generate();
        let enc = |data: Vec<u8>| {
            encode(&a, &b.public_key_bytes(), &Payload::BlobChunk { id: vec![1u8; 32], index: 0, data }, 0).unwrap()
        };
        let full = enc(vec![9u8; 12 * 1024]); // полный чанк
        let tail = enc(vec![9u8; 37]); // куцый хвост файла
        assert_eq!(full.len(), tail.len(), "хвостовой чанк неотличим от полного");
        // Датаграмма транспорта — 65535; конверт с hex-оверхедом обязан влезать.
        assert!(full.len() < 65535, "чанк с паддингом не влезает в датаграмму: {}", full.len());
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
        let d = encode(&alice, &bob.public_key_bytes(), &Payload::Ping { transit: None }, 123).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&d).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["time"], 123);
        assert_eq!(v["from"], hex::encode(alice.public_key_bytes()));
        assert!(v["data"].as_str().unwrap().len() >= crypto::NONCE_LEN * 2);
    }
}
