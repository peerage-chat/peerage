//! Протокол сообщений.
//!
//! [`Payload`] — внутреннее сообщение (тип + данные). Сериализуется в MessagePack,
//! добивается до размерного бакета ([`Payload::to_padded`]) и целиком шифруется
//! сессией 1:1 ([`crate::session`], ADR-012), поэтому ни тип сообщения, ни
//! отправитель снаружи не видны: на проводе только индекс сессии и счётчик.
//!
//! Отклонение от ТЗ: `type` сообщения убран в зашифрованную часть (приватность
//! метаданных); время отправки — только внутри шифра, у сообщений чата (`sent`): по нему
//! получатель отличает досланное после офлайна. Формат самоописывающий (MessagePack с именами полей):
//! неизвестный тип и новые поля читаются старым клиентом без ошибки, а байты едут
//! байтами — без hex и без JSON-массивов чисел.

use serde::{Deserialize, Serialize};

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
/// ключ, около сотни байт; уже двух хватает, чтобы вылезти из бакета CONTROL 256 Б, а
/// маячок присутствия обязан быть неотличим по размеру от ack (M5). Поэтому в маячке
/// едет лишь обязательство: хэш пачки и её размер, — а сами квитанции запрашивает
/// тот, кому есть смысл проверять. Это же ограничивает и объём вранья: заявленное
/// число подтверждается только предъявленной пачкой.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitClaim {
    /// BLAKE3 пачки квитанций за период.
    #[serde(with = "serde_bytes")]
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
        /// Время отправки по часам отправителя, мс (0 — старый клиент). Задаётся раз, при
        /// постановке в очередь: досланное после офлайна несёт свой возраст, и получатель
        /// не звенит и не поднимает шторку на старое (notifications.md, п. 3).
        #[serde(default, skip_serializing_if = "is_zero")]
        sent: i64,
    },
    /// Отзыв СВОЕГО ранее отправленного сообщения («удалить у всех»).
    ///
    /// `id` — для надёжной доставки (retry-until-Ack), `target` — id отзываемого
    /// сообщения. Честная граница: это просьба удалить, а не гарантия — получатель
    /// мог прочитать сообщение раньше, сделать снимок экрана или иметь патченый
    /// клиент. UI обязан говорить это прямо, а не обещать «удалено у всех».
    Retract { id: u64, target: u64 },
    /// Правка СВОЕГО ранее отправленного сообщения. Цель — всегда сообщение
    /// отправителя, поэтому «чьё» не передаётся: чужой текст переписать нельзя,
    /// это была бы подмена чужих слов, а не редактирование.
    ///
    /// Меняется только текст (у вложения — подпись); тип, блоб и время остаются.
    /// История правок НЕ хранится: «удалил и переписал» иначе становится ложью в
    /// обе стороны — и перед собеседником, и перед автором.
    Edit { id: u64, target: u64, text: String },
    /// Закрепить/открепить сообщение в личном чате. Закреп — свойство ЧАТА, а не
    /// одного из собеседников: у обоих он один и тот же, иначе «закрепил важное»
    /// значило бы «закрепил у себя», и вторая сторона об этом не узнала бы.
    ///
    /// `target` + `target_mine` — ссылка на сообщение (ADR-006); «чьё» едет с точки
    /// зрения ОТПРАВИТЕЛЯ и переворачивается на приёме, как у ответа. `on = false` —
    /// открепить. Бакет CONTROL: по размеру неотличимо от ack.
    Pin { id: u64, target: u64, target_mine: bool, on: bool },
    /// Реакция на сообщение: у человека одна на сообщение, пустой `emoji` — снять.
    /// `target` + `target_mine` — ссылка, как у закрепа (переворачивается на приёме).
    /// `at` — время отправителя в мс: из двух его изменений побеждает позднее, и
    /// опоздавший пакет не возвращает старое. Бакет CONTROL: неотличимо от ack.
    React { id: u64, target: u64, target_mine: bool, emoji: String, at: i64 },
    /// Отметка о прочтении (2.10, шаг 7c): «твои сообщения до id `upto` включительно
    /// прочитаны». Один номер вместо списка: свои id отправитель выдаёт по
    /// возрастанию, так что «до N» однозначно. Без id и Ack — потерянную отметку
    /// перекроет следующая, а надёжная доставка копила бы их в очереди. Шлётся,
    /// только если читающий включил отчёты; принимается, только если их включил и
    /// получатель (взаимность). Бакет CONTROL: неотличимо от ack.
    Read { upto: u64 },
    /// Индикатор набора: «сейчас печатаю». Эфемерный — без id, без очереди, без
    /// повторов; получатель гасит его сам через несколько секунд или с приходом
    /// сообщения. Частота ограничена отправителем. Взаимность — как у [`Payload::Read`].
    Typing,
    /// Медиа-сообщение чата (M2): ссылка на блоб по контент-хэшу. Байты получатель
    /// сам запрашивает блоб-каналом (pull по хэшу), сообщение несёт лишь
    /// ссылку → тот же надёжный путь, что и `Text` (id + `Ack`, ретраи, офлайн-очередь,
    /// дедуп). `mime` — тип (image/*, …), `name` — исходное имя файла (для не-картинок).
    Media {
        id: u64,
        #[serde(with = "serde_bytes")]
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
        #[serde(with = "serde_bytes")]
        thumb: Vec<u8>,
        /// Пересылка: hex-ключ автора исходного сообщения (пусто — своё).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        fwd_from: String,
        /// «Переслано, автора не называем» (см. `Text::fwd_anon`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        fwd_anon: bool,
        /// Ответ: id сообщения, на которое отвечают (0 — обычное). Те же поля, что
        /// у `Text`: ответить фотографией — такой же обычный случай, как ответить
        /// текстом, и хранить это по-разному значило бы писать фичу дважды.
        #[serde(default)]
        reply_to: u64,
        /// Чьё сообщение цитируется — с точки зрения ОТПРАВИТЕЛЯ (как у `Text`).
        #[serde(default)]
        reply_mine: bool,
        /// Время отправки, мс (см. `Text::sent`).
        #[serde(default, skip_serializing_if = "is_zero")]
        sent: i64,
        /// Альбом (п. 69): вложения, отправленные разом с «Группировать», несут один
        /// случайный номер. Необязательное поле (заморозка формата): старая версия его не
        /// знает и рисует снимки по одному. 0 — не в альбоме, на провод не идёт.
        #[serde(default, skip_serializing_if = "is_zero_u64")]
        album: u64,
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
    ///
    /// `active` — приложение открыто на экране («в сети», п. 9 списка с телефона; сама
    /// связь — «на связи»). Только своим и только если человек это не скрыл; ложное не
    /// сериализуется, так что пинг без него — те же байты, что в 1.0, а старый клиент
    /// незнакомое поле пропускает и видит лишь «на связи».
    Ping {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transit: Option<TransitClaim>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        active: bool,
    },
    /// Ответ на ping. `transit` и `active` — см. [`Payload::Ping`].
    Pong {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transit: Option<TransitClaim>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        active: bool,
    },
    /// Приглашение в голосовую комнату (DM-звонок). room_id = первые 16 байт
    /// SHA-256(min(keyA,keyB) ++ max(keyA,keyB)) — детерминирован для обеих сторон.
    /// `id` — надёжная доставка (retry-until-Ack): Ack шлётся при ДОСТАВКЕ (до ответа
    /// пользователя), поэтому ретрай гаснет, как только звонок долетел — чинит потерю
    /// первого пакета на холодной сессии («первый звонок не доходит»).
    RoomInvite { id: u64, #[serde(with = "serde_bytes")] room_id: Vec<u8>, audio: bool, video: bool },
    /// Отклонение входящего звонка (получатель нажал «Отклонить»). Надёжно —
    /// звонящий гарантированно узнаёт отказ и перестаёт звонить.
    RoomDecline { id: u64, #[serde(with = "serde_bytes")] room_id: Vec<u8> },
    /// Групповое MLS-сообщение (Commit/Welcome/прикладное) — для пересылки
    /// участникам группы. Содержимое уже зашифровано MLS. `id` — для надёжной
    /// доставки: получатель шлёт `Ack{id}`, отправитель повторяет до Ack/TTL
    /// (per-recipient, как у DM). Дедуп повторов — на стороне MLS (generation).
    Group { id: u64, #[serde(with = "serde_bytes")] data: Vec<u8> },
    /// Запрос KeyPackage у контакта (чтобы добавить его в группу). `id` — для
    /// надёжной доставки (retry-until-Ack): получатель эхо-нёт `Ack{id}`. Раньше был
    /// fire-and-forget → на холодной сессии терялся (инвайт «не срабатывал»).
    /// `gid` — в какую группу (resync): ответ обязан его вернуть.
    GroupKeyRequest { id: u64, #[serde(with = "serde_bytes")] gid: Vec<u8> },
    /// Ответ с KeyPackage (MLS) для добавления в группу `gid`. Группа названа явно:
    /// одного человека можно звать в несколько групп сразу, и KeyPackage без неё
    /// добавил бы его не туда. `id` — надёжная доставка.
    GroupKeyReply {
        id: u64,
        #[serde(with = "serde_bytes")]
        gid: Vec<u8>,
        #[serde(with = "serde_bytes")]
        key_package: Vec<u8>,
    },
    /// Приглашение в группу как отдельное сообщение (решение владельца 2026-06-28):
    /// вместо тихого KeyPackage-запроса получатель видит карточку «X зовёт в Y» и сам
    /// решает вступать. При согласии он шлёт `GroupKeyReply` пригласившему. `id` —
    /// надёжная доставка; `name` — название группы для карточки.
    GroupInvite { id: u64, #[serde(with = "serde_bytes")] gid: Vec<u8>, name: String },
    /// Маячок присутствия в комнате-звонке (Discord-style): периодический сигнал
    /// «я сейчас в комнате `gid`, A/V-флаги такие». `flags`: bit0 — аудио,
    /// bit1 — видео. Низкочастотный control-plane (не медиа).
    RoomPresence { #[serde(with = "serde_bytes")] gid: Vec<u8>, flags: u8 },
    /// Запрос повторной синхронизации группы (ADR-004): отправитель отстал
    /// (пропустил Commit / удалён секрет эпохи) и просит админа заново добавить
    /// его (Remove+Add). Админ игнорирует, если проситель не в текущем ростере.
    GroupResyncRequest { #[serde(with = "serde_bytes")] gid: Vec<u8> },
    /// Запрос на выход из группы (ADR-004): отправитель уходит и просит админа
    /// отозвать его (Remove + смена эпохи). Отправитель локально уже забыл группу.
    GroupLeaveRequest { #[serde(with = "serde_bytes")] gid: Vec<u8> },
    /// Анонс блоба (M2): манифест (JSON) — план докачки. Получатель качает чанки
    /// запросами `BlobRequest`. `manifest` непрозрачен для core (парсит слой blob).
    BlobManifest { manifest: String },
    /// Часть манифеста крупного блоба. Список хэшей чанков растёт линейно с размером
    /// файла и одним пакетом перестаёт влезать в датаграмму (это и держало потолок
    /// 4 МБ), поэтому манифест едет частями. Метаданные (`mime`/`size`) дублируются
    /// в каждой части: получатель может проверить лимиты и отказаться, не дожидаясь
    /// всех частей. `hashes` — сырые BLAKE3 чанков (кратно 32).
    BlobManifestPart {
        #[serde(with = "serde_bytes")]
        id: Vec<u8>,
        mime: String,
        size: u64,
        part: u32,
        total: u32,
        #[serde(with = "serde_bytes")]
        hashes: Vec<u8>,
    },
    /// Запрос чанка блоба по id (BLAKE3) и индексу (получатель-драйвен pull).
    BlobRequest {
        #[serde(with = "serde_bytes")]
        id: Vec<u8>,
        index: u32,
    },
    /// Ответ с чанком блоба. Шифруется как обычное сообщение (E2E), целостность
    /// чанка отдельно проверяется по хэшу в манифесте на слое blob.
    BlobChunk {
        #[serde(with = "serde_bytes")]
        id: Vec<u8>,
        index: u32,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// Обновление профиля (M2): самоназвание + хэш аватара. Рассылается контактам при
    /// смене и на коннекте (fire-and-forget, идемпотентно). `avatar_hash` пуст = аватара
    /// нет. Получатель тянет аватар-блоб по хэшу (`BlobManifestRequest`) ТОЛЬКО если хэш
    /// новый и блоба ещё нет — контент-адрес даёт авто-дедуп.
    ProfileUpdate {
        name: String,
        #[serde(with = "serde_bytes")]
        avatar_hash: Vec<u8>,
        /// «Пришли свой в ответ». Ставит тот, кто только что принял запрос в контакты:
        /// профиль, присланный ему ДО одобрения, он выбросил (от незнакомцев профиль не
        /// принимается), а сам пир повторил бы его лишь на следующем коннекте.
        /// Ответ идёт без флага — пинг-понга нет.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ask_back: bool,
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
        /// Подписанная запись адреса v2 ([`crate::record`]).
        #[serde(with = "serde_bytes")]
        record: Vec<u8>,
        #[serde(with = "serde_bytes")]
        sig: Vec<u8>,
    },
    /// Запрос манифеста блоба по id (BLAKE3) — pull-по-хэшу (аватар / ленивые картинки).
    /// Ответ — `BlobManifest`. Делает блобы запрашиваемыми, а не только push-анонсом.
    BlobManifestRequest {
        #[serde(with = "serde_bytes")]
        id: Vec<u8>,
    },
    /// Запрос в контакты (шаг 7i): приветствие и самоназвание. Шлёт тот, кто добавил по
    /// коду; пока нет [`Payload::ContactAccept`], остальное ему не уходит — ждёт в
    /// очереди отправителя. `greeting` — до [`MAX_GREETING`] байт, может быть пустым;
    /// фото — только после принятия, вместе с профилем. Надёжно (`id` + `Ack`).
    ///
    /// `record`/`sig` — подписанная запись адреса отправителя ([`crate::record`]), как в
    /// [`Payload::TransportRotate`]. Незнакомец приходит с транспортного ключа, а не с
    /// ключа личности, и получателю больше неоткуда узнать связку «личность → адрес»:
    /// без неё запрос не прошёл бы анти-спуфинг, а ответ (Ack, принятие) ушёл бы на
    /// ключ личности, где никого нет.
    ContactRequest {
        id: u64,
        greeting: String,
        name: String,
        #[serde(with = "serde_bytes")]
        record: Vec<u8>,
        #[serde(with = "serde_bytes")]
        sig: Vec<u8>,
    },
    /// Запрос `request` принят: приветствие легло в чат, очередь отправителя можно
    /// выпускать. Отклонение ответа не имеет — не раскрывать, что запрос прочли.
    ContactAccept { id: u64, request: u64 },
    /// Объявление адреса не принято: эпоха не новее известной (R-120). В ответе — запись
    /// адреса отправителя, которую получатель уже знает (подписана самим отправителем):
    /// по ней владелец поднимает эпоху выше и объявляет снова. Шлётся на адрес, с которого
    /// пришло объявление, — по известному адресу владельца уже нет.
    AddressStale {
        #[serde(with = "serde_bytes")]
        record: Vec<u8>,
        #[serde(with = "serde_bytes")]
        sig: Vec<u8>,
    },
    /// Неизвестный/будущий тип payload — forward-compat (ADR-006). Сообщения версий,
    /// которые этот клиент ещё НЕ понимает (напр. реакции/закрепы/правки — post-1.0),
    /// декодируются СЮДА и молча игнорируются, а не роняют `decode`. Так новые типы
    /// сообщений добавляются аддитивно, не ломая старые клиенты. Приёмник новых типов
    /// НЕ шлёт `Ack` — отправитель погасит ретрай по TTL (безопасно).
    #[serde(other)]
    Unknown,
}

impl Payload {
    /// Сериализация payload: MessagePack с именами полей. Та же форма — и на проводе
    /// (до паддинга и шифрования), и в офлайн-очереди.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        rmp_serde::to_vec_named(self).map_err(|e| ProtocolError::Codec(e.to_string()))
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self, ProtocolError> {
        rmp_serde::from_slice(b).map_err(|e| ProtocolError::Codec(e.to_string()))
    }

    /// id надёжной доставки (retry-until-Ack), если он у payload есть. Получатель
    /// по нему отсеивает повторы: ретрай после потерянного Ack и переотправка при
    /// смене сессии приносят то же сообщение ещё раз.
    pub fn reliable_id(&self) -> Option<u64> {
        match self {
            Payload::Text { id, .. }
            | Payload::Retract { id, .. }
            | Payload::Edit { id, .. }
            | Payload::Pin { id, .. }
            | Payload::React { id, .. }
            | Payload::Media { id, .. }
            | Payload::RoomInvite { id, .. }
            | Payload::RoomDecline { id, .. }
            | Payload::Group { id, .. }
            | Payload::GroupKeyRequest { id, .. }
            | Payload::GroupKeyReply { id, .. }
            | Payload::GroupInvite { id, .. }
            | Payload::ContactRequest { id, .. }
            | Payload::ContactAccept { id, .. }
            | Payload::TransportRotate { id, .. } => Some(*id),
            _ => None,
        }
    }

    /// Открытый текст пакета сессии: payload, добитый до бакета своего класса.
    /// M5: добивка ДО шифрования прячет длину по размеру пакета.
    pub fn to_padded(&self) -> Result<Vec<u8>, ProtocolError> {
        Ok(crate::pad::pad(&self.to_bytes()?, bucket_for(self)))
    }

    /// Обратно из расшифрованного текста пакета сессии.
    pub fn from_padded(padded: &[u8]) -> Result<Self, ProtocolError> {
        Self::from_bytes(&crate::pad::unpad(padded).ok_or(ProtocolError::BadPadding)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("сериализация payload: {0}")]
    Codec(String),
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

/// Потолок приветствия в запросе в контакты, в БАЙТАХ (~500 символов кириллицей): с
/// именем запрос обязан умещаться в бакет APP, как текст.
pub const MAX_GREETING: usize = 1024;

/// Потолок реакции в БАЙТАХ: хватает на семейный эмодзи из ZWJ-цепочки дважды, а
/// реакция остаётся в бакете CONTROL.
pub const MAX_REACTION: usize = 64;

/// Годится ли строка в реакцию: непустая, не длиннее [`MAX_REACTION`], без пробелов и
/// управляющих символов. Проверяется и на отправке, и на приёме — отправитель мог
/// быть пропатчен. Что именно это эмодзи, не проверяем: таблица эмодзи меняется
/// каждый год, а вред от «реакции буквой» — только вид.
pub fn reaction_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_REACTION
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub const CONTROL_BUCKET: usize = 256;
pub const APP_BUCKET: usize = 4096;
/// Чанк блоба 12 КБ (`peerage_blob::CHUNK_SIZE`) плюс запас на поля сообщения (id,
/// индекс, имена полей, префикс длины паддинга). Полный чанк обязан влезать в бакет
/// целиком — иначе хвостовой чанк стал бы отличим от полного; на это есть тест
/// `padded_blob_chunk_fits_datagram`.
const CHUNK_BUCKET: usize = 12 * 1024 + 256;

/// Ноль не пишется на провод и в копию: необязательное поле без значения — его отсутствие.
pub(crate) fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

fn bucket_for(payload: &Payload) -> usize {
    match payload {
        // Прикладные: текст и медиа обязаны быть в ОДНОМ бакете (DoD M5).
        Payload::Text { .. }
        // Правка несёт новый текст целиком. В control-бакете она округлялась до
        // кратного 256 Б и выдавала длину текста — и заодно сам факт правки.
        | Payload::Edit { .. }
        | Payload::Media { .. }
        | Payload::Group { .. }
        | Payload::GroupInvite { .. }
        | Payload::GroupKeyReply { .. }
        | Payload::ProfileUpdate { .. }
        // Запрос и принятие (7i) — в бакет текста: несут приветствие, а «человек
        // добавляет контакт» не должно отличаться по размеру от обычного сообщения.
        | Payload::ContactRequest { .. }
        | Payload::ContactAccept { .. }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    // Размер на проводе = открытый текст + постоянный заголовок и тег сессии
    // (`session::DATA_OVERHEAD`), поэтому равенство длин `to_padded` — это и есть
    // равенство размеров датаграмм.
    fn wire(p: &Payload) -> usize {
        p.to_padded().unwrap().len() + crate::session::DATA_OVERHEAD
    }

    fn roundtrip(p: &Payload) -> Payload {
        Payload::from_padded(&p.to_padded().unwrap()).unwrap()
    }

    #[test]
    fn padded_roundtrip() {
        let payload = Payload::Text {
            id: 42,
            text: "привет, как дела?".to_string(),
            reply_to: 0,
            reply_mine: false,
            fwd_from: String::new(),
            fwd_anon: false,
            sent: 1_790_000_000_123,
        };
        assert_eq!(roundtrip(&payload), payload);
        assert!(matches!(Payload::from_padded(&[0, 0, 0, 9, 1]), Err(ProtocolError::BadPadding)));
    }

    /// Объявление о смене адреса переживает упаковку целиком и проверяется у
    /// получателя ключом ЛИЧНОСТИ отправителя — то есть подписью, а не тем, откуда
    /// пакет пришёл. Иначе смена адреса не отличалась бы от захвата контакта.
    #[test]
    fn transport_rotate_roundtrip_and_verifies_by_identity() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let r = alice.address_record(5);
        let payload = Payload::TransportRotate { id: 77, record: r.bytes.clone(), sig: r.sig.to_vec() };
        let decoded = roundtrip(&payload);
        assert_eq!(decoded, payload);

        let Payload::TransportRotate { record, sig, .. } = decoded else {
            panic!("не тот вариант");
        };
        let sg = <[u8; 64]>::try_from(sig.as_slice()).unwrap();
        use crate::record::AddressRecord;
        let got = AddressRecord::verify(&alice.public_key_bytes(), &record, &sg).unwrap();
        assert_eq!((got.epoch, got.addr), (5, alice.transport_public(5).to_vec()));
        // Тот же пакет, приписанный другой личности, не проходит.
        assert!(AddressRecord::verify(&bob.public_key_bytes(), &record, &sg).is_none());
    }

    #[test]
    fn unknown_payload_type_is_ignored_not_error() {
        // Forward-compat (ADR-006): будущий тип, которого этот клиент ещё не знает
        // (напр. реакция post-1.0), декодируется в Unknown, а НЕ роняет разбор.
        #[derive(Serialize)]
        struct Reaction {
            r#type: &'static str,
            target: u64,
            emoji: &'static str,
        }
        let future = rmp_serde::to_vec_named(&Reaction { r#type: "reaction", target: 42, emoji: "fire" }).unwrap();
        assert_eq!(Payload::from_bytes(&future).unwrap(), Payload::Unknown);
    }

    #[test]
    fn all_payload_types_roundtrip() {
        for p in [
            Payload::Hello,
            Payload::Ping { transit: None, active: false },
            Payload::Pong { transit: None, active: false },
            Payload::Ack { id: 7 },
            Payload::Text { id: 1, text: "hi".into(), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false, sent: 1_790_000_000_000 },
            Payload::Media {
                id: 2, hash: vec![5u8; 32], mime: "image/jpeg".into(), name: "p.jpg".into(),
                size: 1234, duration_ms: 0, width: 800, height: 600, caption: String::new(), thumb: vec![9u8; 32], fwd_from: String::new(), fwd_anon: false, reply_to: 0, reply_mine: false, sent: 0, album: 0,
            },
            // Видео с длительностью и без превью — поля опциональны.
            Payload::Media {
                id: 3, hash: vec![6u8; 32], mime: "video/mp4".into(), name: "v.mp4".into(),
                size: 900_000_000, duration_ms: 125_000, width: 1920, height: 1080, caption: String::new(), thumb: Vec::new(), fwd_from: String::new(), fwd_anon: false, reply_to: 0, reply_mine: false, sent: 0, album: 0,
            },
            // Снимок альбома (п. 69).
            Payload::Media {
                id: 4, hash: vec![7u8; 32], mime: "image/jpeg".into(), name: "a.jpg".into(),
                size: 10, duration_ms: 0, width: 1, height: 1, caption: String::new(), thumb: Vec::new(), fwd_from: String::new(), fwd_anon: false, reply_to: 0, reply_mine: false, sent: 0, album: 0xA1B2_C3D4_E5F6_0718,
            },
            Payload::RoomInvite { id: 11, room_id: vec![1u8; 16], audio: true, video: false },
            Payload::RoomDecline { id: 12, room_id: vec![2u8; 16] },
            Payload::Group { id: 5, data: vec![1, 2, 3, 9, 9] },
            Payload::GroupKeyRequest { id: 3, gid: vec![1, 2] },
            Payload::GroupKeyReply { id: 4, gid: vec![1, 2], key_package: vec![7, 7, 7] },
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
            Payload::ProfileUpdate { name: "Алиса".into(), avatar_hash: vec![3u8; 32], ask_back: true },
            Payload::BlobManifestRequest { id: vec![4u8; 32] },
        ] {
            assert_eq!(roundtrip(&p), p);
        }
    }

    /// Всё, у чего есть id надёжной доставки, отдаёт его; Ack — нет (его id чужой).
    #[test]
    fn reliable_id_covers_reliable_payloads() {
        assert_eq!(Payload::Ack { id: 5 }.reliable_id(), None);
        assert_eq!(Payload::Ping { transit: None, active: false }.reliable_id(), None);
        assert_eq!(Payload::RoomInvite { id: 5, room_id: vec![], audio: true, video: false }.reliable_id(), Some(5));
        assert_eq!(Payload::Retract { id: 6, target: 1 }.reliable_id(), Some(6));
        assert_eq!(Payload::GroupKeyRequest { id: 7, gid: vec![] }.reliable_id(), Some(7));
    }

    #[test]
    fn padding_hides_text_length() {
        let short = Payload::Text { id: 1, text: "да".into(), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false, sent: 0 };
        let long = Payload::Text { id: 1, text: "a".repeat(500), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false, sent: 0 };
        // M5: разные по длине тексты → одинаковый размер датаграммы (бакет скрыл длину).
        assert_eq!(wire(&short), wire(&long), "паддинг должен скрыть длину текста");
        assert!(matches!(roundtrip(&short), Payload::Text { text, .. } if text == "да"));
    }

    /// DoD M5: размеры НА ПРОВОДЕ не различают текст / медиа / звонок, а пакеты
    /// рукопожатия сессии совпадают по размеру с прикладными и управляющими.
    /// Отказ по эпохе (R-120) служебный, как Ack: по размеру от подтверждения не отличить.
    #[test]
    fn address_stale_looks_like_an_ack() {
        let rec = crate::Identity::from_seed(&[1u8; 32]).unwrap().address_record(u32::MAX);
        let stale = Payload::AddressStale { record: rec.bytes, sig: rec.sig.to_vec() };
        assert_eq!(stale.to_padded().unwrap().len(), Payload::Ack { id: 1 }.to_padded().unwrap().len());
        assert_eq!(roundtrip(&stale), stale);
    }

    #[test]
    fn text_media_and_call_are_indistinguishable_by_size() {
        let enc = wire;

        // Прикладной класс: текст (короткий/длинный), картинка, голосовое, файл, группа.
        let sizes = [
            enc(&Payload::Text { id: 1, text: "да".into(), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false, sent: 0 }),
            enc(&Payload::Text { id: 1, text: "a".repeat(900), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false, sent: 0 }),
            enc(&Payload::Media {
                id: 2, hash: vec![5u8; 32], mime: "image/jpeg".into(),
                name: "secret-photo.jpg".into(), size: 2_400_000,
                duration_ms: 0, width: 4000, height: 3000, caption: String::new(), thumb: vec![1u8; 32], fwd_from: String::new(), fwd_anon: false, reply_to: 0, reply_mine: false, sent: 0, album: 0,
            }),
            enc(&Payload::Media {
                id: 3, hash: vec![6u8; 32], mime: "audio/mp4".into(),
                name: "voice.m4a".into(), size: 48_000,
                duration_ms: 7_400, width: 0, height: 0, caption: String::new(), thumb: Vec::new(), fwd_from: String::new(), fwd_anon: false, reply_to: 0, reply_mine: false, sent: 0, album: 0,
            }),
            enc(&Payload::Media {
                id: 4, hash: vec![7u8; 32], mime: "application/pdf".into(),
                name: "договор-2026.pdf".into(), size: 900_000_000,
                duration_ms: 0, width: 0, height: 0, caption: String::new(), thumb: Vec::new(), fwd_from: String::new(), fwd_anon: false, reply_to: 0, reply_mine: false, sent: 0, album: 0,
            }),
            enc(&Payload::Group { id: 5, data: vec![1u8; 200] }),
            // Правка своего сообщения: короткая и длинная неотличимы от текста.
            enc(&Payload::Edit { id: 6, target: 1, text: "ок".into() }),
            enc(&Payload::Edit { id: 7, target: 1, text: "b".repeat(900) }),
            // Смена адреса (ADR-007) едет здесь же: подпись 64 Б не влезает в
            // control-бакет, зато среди прикладных объявление неотличимо от
            // обычного сообщения — флудер не видит по проводу, что жертва прямо
            // сейчас уходит на новый адрес.
            enc(&Payload::TransportRotate {
                id: 14, record: crate::Identity::from_seed(&[1u8; 32]).unwrap().address_record(7).bytes, sig: vec![4u8; 64],
            }),
            // msg1 и msg2 рукопожатия (ADR-012).
            crate::session::APP_PACKET,
        ];
        assert!(
            sizes.iter().all(|s| *s == sizes[0]),
            "текст/правка/картинка/голосовое/файл/группа/смена-адреса/рукопожатие должны быть одного размера, получено {sizes:?}"
        );

        // Control-класс: начало звонка неотличимо от ack/ping/маячка присутствия.
        let ctl = [
            enc(&Payload::RoomInvite { id: 11, room_id: vec![1u8; 16], audio: true, video: true }),
            enc(&Payload::RoomDecline { id: 12, room_id: vec![2u8; 16] }),
            enc(&Payload::Ack { id: 13 }),
            enc(&Payload::Ping { transit: None, active: false }),
            enc(&Payload::RoomPresence { gid: vec![9u8; 16], flags: 0b11 }),
            // msg3 рукопожатия и пустой пакет подтверждения сессии.
            crate::session::CONTROL_PACKET,
        ];
        assert!(
            ctl.iter().all(|s| *s == ctl[0]),
            "звонок должен быть неотличим от ack/ping/присутствия, получено {ctl:?}"
        );
    }

    /// Реакция — надёжная (id для Ack и дедупа) и мелкая: самый длинный допустимый
    /// эмодзи всё равно неотличим по размеру от ack, как и закреп.
    #[test]
    fn reaction_is_reliable_and_hides_in_control_bucket() {
        let longest = "👨‍👩‍👧‍👦".repeat(2);
        assert!(longest.len() <= MAX_REACTION);
        let p = Payload::React { id: 9, target: 3, target_mine: true, emoji: longest, at: i64::MAX };
        assert_eq!(roundtrip(&p), p);
        assert_eq!(p.reliable_id(), Some(9));
        assert_eq!(wire(&p), wire(&Payload::Ack { id: 1 }));
    }

    /// Запрос в контакты и принятие (7i) — надёжные и по размеру неотличимы от текста,
    /// даже с самым длинным приветствием и именем и с записью адреса.
    #[test]
    fn contact_request_and_accept_hide_among_text() {
        let addr = crate::identity::Identity::from_seed(&[1; 32]).unwrap().address_record(u32::MAX);
        let req = Payload::ContactRequest {
            id: 5,
            greeting: "я".repeat(MAX_GREETING / 2),
            name: "Ж".repeat(64),
            record: addr.bytes.clone(),
            sig: addr.sig.to_vec(),
        };
        let acc = Payload::ContactAccept { id: 6, request: u64::MAX };
        let text = Payload::Text {
            id: 1, text: "привет".into(), reply_to: 0, reply_mine: false, fwd_from: String::new(), fwd_anon: false, sent: 0,
        };
        for p in [&req, &acc] {
            assert_eq!(&roundtrip(p), p);
            assert_eq!(wire(p), wire(&text));
        }
        assert_eq!((req.reliable_id(), acc.reliable_id()), (Some(5), Some(6)));
    }

    /// Отметка о прочтении и набор (2.10) — не надёжные (без id: потеря не страшна,
    /// следующее прочтение или набор пришлют своё) и по размеру неотличимы от ack.
    #[test]
    fn read_and_typing_are_fire_and_forget_control() {
        for p in [Payload::Read { upto: u64::MAX }, Payload::Typing] {
            assert_eq!(roundtrip(&p), p);
            assert_eq!(p.reliable_id(), None);
            assert_eq!(wire(&p), wire(&Payload::Ack { id: 1 }));
        }
    }

    #[test]
    fn reaction_ok_accepts_emoji_and_rejects_junk() {
        for ok in ["👍", "❤️", "👨‍👩‍👧‍👦", "🇷🇺"] {
            assert!(reaction_ok(ok), "{ok}");
        }
        let too_long = "😀".repeat(MAX_REACTION / 4 + 1);
        for bad in ["", " ", "a b", "\u{0}", "line\n", too_long.as_str()] {
            assert!(!reaction_ok(bad), "{bad:?}");
        }
    }

    /// Зарезервированное поле proof-of-transit (ADR-008) обязано умещаться в тот же
    /// бакет CONTROL. Смысл резервирования именно в этом: заполнив поле позже, мы не
    /// имеем права сделать маячок присутствия отличимым по размеру от ack — иначе
    /// наблюдатель начнёт видеть, кто в сети, а свойство M5 сломается задним числом.
    #[test]
    fn presence_with_transit_claim_stays_in_control_bucket() {
        let claim = TransitClaim { digest: vec![0xab; 32], count: u32::MAX, epoch: u32::MAX };
        assert_eq!(
            wire(&Payload::Ping { transit: Some(claim.clone()), active: true }),
            wire(&Payload::Ack { id: 1 }),
            "маячок с квитанциями обязан остаться неотличим от ack по размеру"
        );
        assert_eq!(wire(&Payload::Pong { transit: Some(claim), active: true }), wire(&Payload::Ack { id: 1 }));
    }

    /// Пустое поле не должно появляться на проводе: маячок 1.0 — это голый тип.
    #[test]
    fn empty_transit_field_is_absent_on_the_wire() {
        #[derive(Serialize)]
        struct Bare {
            r#type: &'static str,
        }
        let bare = rmp_serde::to_vec_named(&Bare { r#type: "ping" }).unwrap();
        assert_eq!(Payload::Ping { transit: None, active: false }.to_bytes().unwrap(), bare);
        // И обратно: пакет без поля читается (совместимость в обе стороны).
        assert_eq!(Payload::from_bytes(&bare).unwrap(), Payload::Ping { transit: None, active: false });
    }

    /// Флаг «приложение открыто» совместим в обе стороны: старый клиент (пинг без поля)
    /// читает новый пинг, пропуская поле, а новый читает старый как `active: false`.
    #[test]
    fn active_flag_is_compatible_with_old_ping() {
        #[derive(Deserialize, Debug, PartialEq)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Old {
            Ping {
                #[serde(default)]
                transit: Option<TransitClaim>,
            },
        }
        let new = Payload::Ping { transit: None, active: true }.to_bytes().unwrap();
        assert_eq!(rmp_serde::from_slice::<Old>(&new).unwrap(), Old::Ping { transit: None });
        #[derive(Serialize)]
        struct Bare {
            r#type: &'static str,
        }
        let old = rmp_serde::to_vec_named(&Bare { r#type: "ping" }).unwrap();
        assert_eq!(Payload::from_bytes(&old).unwrap(), Payload::Ping { transit: None, active: false });
        assert_eq!(Payload::from_bytes(&new).unwrap(), Payload::Ping { transit: None, active: true });
    }

    /// R-004: байты едут байтами. Полный чанк 12 КБ на проводе — чуть больше 12 КБ,
    /// а не вчетверо (hex в payload, потом hex всего шифртекста в JSON).
    #[test]
    fn blob_chunk_has_no_hex_overhead() {
        let chunk = Payload::BlobChunk { id: vec![1u8; 32], index: 0, data: vec![7u8; 12 * 1024] };
        assert!(wire(&chunk) < 12 * 1024 + 512, "чанк на проводе: {} Б", wire(&chunk));
        // MLS-данные — тоже байтами, а не JSON-массивом чисел.
        let g = Payload::Group { id: 1, data: vec![200u8; 1000] }.to_bytes().unwrap();
        assert!(g.len() < 1100, "Group: {} Б", g.len());
    }

    /// Последний (короткий) чанк блоба добивается до полного — иначе точный размер
    /// файла утекал бы остатком. И при этом пакет обязан влезать в датаграмму.
    #[test]
    fn padded_blob_chunk_fits_datagram() {
        let enc = |data: Vec<u8>| wire(&Payload::BlobChunk { id: vec![1u8; 32], index: 0, data });
        let full = enc(vec![9u8; 12 * 1024]); // полный чанк
        let tail = enc(vec![9u8; 37]); // куцый хвост файла
        assert_eq!(full, tail, "хвостовой чанк неотличим от полного");
        assert!(full < 65535, "чанк с паддингом не влезает в датаграмму: {full}");
    }
}
