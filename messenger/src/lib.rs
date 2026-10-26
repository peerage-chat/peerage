//! p2p-messenger — слой сессии: связывает транспорт Yggdrasil ([`p2p_ygg`]) и
//! протокол/крипто ядра ([`p2p_core`]) в надёжный зашифрованный канал сообщений.
//!
//! Обязанности:
//!   * приёмный цикл: читает датаграммы, расшифровывает, диспетчеризует;
//!   * анти-спуфинг: ключ отправителя в конверте обязан совпадать с источником
//!     транспорта (Yggdrasil src key);
//!   * рукопожатие `Hello` в обе стороны (нужно для шифр-сессии ironwood);
//!   * keepalive-ping и статус онлайн/офлайн пиров (presence);
//!   * очередь исходящих с повтором до подтверждения (`Ack`);
//!   * переподключение: keepalive продолжает слать ping офлайн-пирам, и при их
//!     возвращении сессия восстанавливается, а недоставленные сообщения уходят.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::media::{
    parse_media, write_media_header, CHAN_CHAT, CHAN_MEDIA, MAX_MEDIA_PAYLOAD, MEDIA_HEADER_LEN,
};
use p2p_core::protocol::{Payload, ProtocolError};
use p2p_core::session::{Received, Sessions};
use rand_core::OsRng;
use p2p_core::budget::Tier;
use p2p_core::ratelimit::{RateLimiter, TieredRateLimiter};
use p2p_core::Identity;
use p2p_ygg::{Node, YggError};

/// Публичный ключ ed25519 пира — он же его идентификатор.
pub type PeerKey = [u8; 32];

/// Абстракция транспорта датаграмм по ключу пира. Реальный меш — [`p2p_ygg::Node`];
/// для тестов — loopback-двойник (см. [`loopback`]), что позволяет прогонять весь
/// сетевой слой (инвайт/группы/resync/доставка) в одном процессе без устройств.
pub trait Transport: Send + Sync {
    fn write(&self, dst_pub: &PeerKey, data: &[u8]) -> Result<usize, YggError>;
    /// Блокирующе читает одну датаграмму; возвращает (длину, ключ отправителя).
    fn read(&self, buf: &mut [u8]) -> Result<(usize, PeerKey), YggError>;
    fn address(&self) -> String;
    fn peer_count(&self) -> i32;
    fn tree_count(&self) -> i32;
    fn peers_json(&self) -> String;
    fn add_peer(&self, uri: &str) -> Result<(), YggError>;
    fn stop(&self);
    /// Отбрасывать пакеты от этого ключа как можно раньше (в транспорте), не
    /// поднимая их в прикладной слой. Приёмный бюджет режет их и так, но каждый
    /// пакет до него стоит работы; под флудом она лишняя. По умолчанию — no-op:
    /// транспорту-двойнику из тестов это не нужно.
    fn block(&self, _key: &PeerKey) {}
    /// Снять раннюю блокировку источника. По умолчанию — no-op.
    fn unblock(&self, _key: &PeerKey) {}
}

impl Transport for Node {
    fn write(&self, dst_pub: &PeerKey, data: &[u8]) -> Result<usize, YggError> {
        Node::write(self, dst_pub, data)
    }
    fn read(&self, buf: &mut [u8]) -> Result<(usize, PeerKey), YggError> {
        Node::read(self, buf)
    }
    fn address(&self) -> String {
        Node::address(self)
    }
    fn peer_count(&self) -> i32 {
        Node::peer_count(self)
    }
    fn tree_count(&self) -> i32 {
        Node::tree_count(self)
    }
    fn peers_json(&self) -> String {
        Node::peers_json(self)
    }
    fn add_peer(&self, uri: &str) -> Result<(), YggError> {
        Node::add_peer(self, uri)
    }
    fn block(&self, key: &PeerKey) {
        Node::block(self, key)
    }
    fn unblock(&self, key: &PeerKey) {
        Node::unblock(self, key)
    }
    fn stop(&self) {
        Node::stop(self)
    }
}

/// События, доставляемые приложению из фоновых потоков.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Изменение онлайн-статуса пира (первый контакт — `online: true`).
    Presence { peer: PeerKey, online: bool },
    /// Входящее текстовое сообщение. `reply_to` = 0 — обычное; иначе это ответ на
    /// сообщение с таким id, а `reply_mine` уже ПЕРЕВЁРНУТ под нашу сторону
    /// (true = отвечают на наше сообщение).
    Text {
        peer: PeerKey,
        id: u64,
        text: String,
        reply_to: u64,
        reply_mine: bool,
        /// Пересылка: hex автора исходного сообщения (пусто — сообщение своё).
        fwd_from: String,
        /// «Переслано, автора не называем».
        fwd_anon: bool,
    },
    /// Пир просит удалить своё сообщение `target` («удалить у всех»).
    Retract { peer: PeerKey, target: u64 },
    /// Пир исправил своё сообщение `target` — заменяем текст в истории.
    Edit { peer: PeerKey, target: u64, text: String },
    /// Пир закрепил (или открепил) сообщение в общем чате. `target_mine` уже
    /// перевёрнут: true — закреплено НАШЕ сообщение.
    Pin { peer: PeerKey, target: u64, target_mine: bool, on: bool },
    /// Входящее медиа-сообщение (M2): ссылка на блоб-вложение по хэшу.
    Media {
        peer: PeerKey,
        id: u64,
        hash: Vec<u8>,
        mime: String,
        name: String,
        /// Размер вложения — карточка показывает его ДО скачивания.
        size: u64,
        duration_ms: u32,
        width: u32,
        height: u32,
        /// Подпись к вложению (может быть пустой).
        caption: String,
        /// Хэш блоба-миниатюры (пусто — нет превью).
        thumb: Vec<u8>,
        /// Пересылка: hex автора исходного сообщения (пусто — своё).
        fwd_from: String,
        /// «Переслано, автора не называем».
        fwd_anon: bool,
        /// Ответ: id цитируемого сообщения (0 — обычное вложение).
        reply_to: u64,
        /// Чьё сообщение цитируется — уже с НАШЕЙ точки зрения (флаг перевёрнут).
        reply_mine: bool,
    },
    /// Пир подтвердил доставку нашего сообщения с данным id (✓✓).
    Ack { peer: PeerKey, id: u64 },
    /// Не удалось доставить наше сообщение за отведённое время (TTL).
    Undelivered { peer: PeerKey, id: u64 },
    /// Входящее приглашение в голосовую комнату (DM-звонок).
    RoomInvite { peer: PeerKey, room_id: Vec<u8>, audio: bool, video: bool },
    /// Пир отклонил входящий звонок.
    RoomDecline { peer: PeerKey, room_id: Vec<u8> },
    /// Входящее групповое MLS-сообщение (переслать в групповой слой).
    Group { peer: PeerKey, data: Vec<u8> },
    /// Админ группы `gid` просит наш KeyPackage (resync).
    GroupKeyRequest { peer: PeerKey, gid: Vec<u8> },
    /// Контакт прислал свой KeyPackage для группы `gid`.
    GroupKeyReply { peer: PeerKey, gid: Vec<u8>, key_package: Vec<u8> },
    /// Приглашение в группу как сообщение (карточка на одобрение): `peer` — кто зовёт,
    /// `gid`/`name` — группа. Получатель решает вступать (тогда шлёт KeyPackage).
    GroupInvite { peer: PeerKey, gid: Vec<u8>, name: String },
    /// Маячок присутствия участника в комнате-звонке (Discord-style).
    RoomPresence { peer: PeerKey, gid: Vec<u8>, flags: u8 },
    /// Отставший участник просит админа заново синхронизировать его в группе (ADR-004).
    GroupResyncRequest { peer: PeerKey, gid: Vec<u8> },
    /// Участник просит админа отозвать его — выход из группы (ADR-004).
    GroupLeaveRequest { peer: PeerKey, gid: Vec<u8> },
    /// Анонс блоба: пришёл манифест (JSON) — можно начинать докачку (M2).
    BlobManifest { peer: PeerKey, manifest: String },
    /// Часть манифеста крупного блоба (список хэшей не влезает в одну датаграмму).
    BlobManifestPart {
        peer: PeerKey,
        id: Vec<u8>,
        mime: String,
        size: u64,
        part: u32,
        total: u32,
        hashes: Vec<u8>,
    },
    /// Пир запросил чанк нашего блоба.
    BlobRequest { peer: PeerKey, id: Vec<u8>, index: u32 },
    /// Пришёл чанк блоба, который мы качаем.
    BlobChunk { peer: PeerKey, id: Vec<u8>, index: u32, data: Vec<u8> },
    /// Обновление профиля контакта (M2): его самоназвание + хэш аватара.
    ProfileUpdate { peer: PeerKey, name: String, avatar_hash: Vec<u8> },
    /// Пир запросил манифест нашего блоба по id (pull-по-хэшу).
    BlobManifestRequest { peer: PeerKey, id: Vec<u8> },
    /// Контакт объявил СМЕНУ АДРЕСА (ADR-007): личность прежняя (`peer`), адрес
    /// теперь `transport_pub`. Подпись уже проверена приёмным слоем (иначе пакет не
    /// прошёл бы анти-спуфинг), но применять её должен верхний слой: монотонность
    /// эпохи и запись в стор — его зона.
    TransportRotate { peer: PeerKey, epoch: u32, transport_pub: Vec<u8>, sig: Vec<u8> },
}

/// Метаданные вложения для карточки: их видно ДО скачивания байтов, поэтому
/// получатель решает осознанно («видео, 12 мин, 800 МБ» вместо «файл»).
/// `thumb` — хэш блоба-миниатюры (пусто = нет): превью едет отдельным мелким блобом,
/// инлайн раздуло бы сообщение и сломало размерную неразличимость M5.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaMeta {
    /// Пересылка: hex автора исходного сообщения (пусто — своё).
    pub fwd_from: String,
    /// «Переслано, автора не называем» — ключ автора при этом не отправляется.
    pub fwd_anon: bool,
    pub size: u64,
    pub duration_ms: u32,
    pub width: u32,
    pub height: u32,
    pub thumb: Vec<u8>,
    /// Подпись к вложению (обрезается до `MAX_CAPTION` на отправке).
    pub caption: String,
    /// Ответ: id цитируемого сообщения (0 — обычное вложение).
    pub reply_to: u64,
    /// Чьё сообщение цитируется, с точки зрения ОТПРАВИТЕЛЯ (ADR-006).
    pub reply_mine: bool,
}

/// Обрезать подпись до `MAX_CAPTION` БАЙТ по границе символа: рвать UTF-8 посередине
/// нельзя, а считать в символах бессмысленно — в бакет мы упираемся байтами.
pub(crate) fn truncate_caption(s: &str) -> String {
    if s.len() <= p2p_core::protocol::MAX_CAPTION {
        return s.to_string();
    }
    let mut end = p2p_core::protocol::MAX_CAPTION;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// Входящий медиа-пакет (реалтайм-канал: голос/видео/экран).
#[derive(Debug, Clone)]
pub struct MediaPacket {
    pub src: PeerKey,
    pub track: u8,
    pub seq: u16,
    pub ts: u32,
    pub payload: Vec<u8>,
}

/// Параметры надёжности.
#[derive(Debug, Clone)]
pub struct Config {
    /// Период keepalive-ping к отслеживаемым пирам.
    pub ping_interval: Duration,
    /// Нет трафика от пира дольше этого — считаем офлайн.
    pub offline_after: Duration,
    /// Период повторной отправки неподтверждённых сообщений.
    pub retry_interval: Duration,
    /// После этого времени бросаем попытки доставить сообщение.
    pub message_ttl: Duration,
    /// Сколько живёт приглашение в звонок (и отказ от него): время дозвона. Звонок,
    /// доставленный позже, звонил бы человеку, который давно положил трубку.
    pub ring_ttl: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ping_interval: Duration::from_secs(15),
            offline_after: Duration::from_secs(45),
            retry_interval: Duration::from_secs(3),
            message_ttl: Duration::from_secs(300),
            ring_ttl: Duration::from_secs(45),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MessengerError {
    #[error("протокол: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("транспорт: {0}")]
    Transport(#[from] YggError),
    #[error("рукопожатие с пиром не завершилось за отведённое время")]
    HandshakeTimeout,
    #[error("медиа-пакет больше лимита датаграммы")]
    MediaTooLarge,
    #[error("очередь исходящих: {0}")]
    Spool(String),
}

struct PeerState {
    online: bool,
    last_seen: Instant,
    last_ping: Instant,
    /// Следующая проба офлайн-пира и текущий шаг backoff (растёт до [`PROBE_MAX`]).
    next_probe: Instant,
    backoff: Duration,
}

impl PeerState {
    fn new(now: Instant) -> Self {
        PeerState { online: false, last_seen: now, last_ping: now, next_probe: now, backoff: PROBE_MIN }
    }
}

/// Первая и предельная пауза между пробами офлайн-пира. Пропавший контакт
/// спрашиваем часто только поначалу: дальше — раз в несколько минут, а не пять
/// пакетов в секунду круглые сутки.
const PROBE_MIN: Duration = Duration::from_secs(2);
const PROBE_MAX: Duration = Duration::from_secs(300);

struct Pending {
    peer: PeerKey,
    payload: Payload,
    created: Instant,
    last_sent: Instant,
    /// Сколько пытаемся доставить: обычно `message_ttl`, у звонка — `ring_ttl`.
    ttl: Duration,
    /// Уведомлять ли UI о доставке/недоставке (`Ack`/`Undelivered`). true для DM,
    /// false для групповых (статус по участникам в UI не показывается).
    notify: bool,
    /// Лежит в персистентной очереди: на Ack снимаем и оттуда и подтягиваем
    /// следующее в окно.
    durable: bool,
    /// Должно уйти заново по новой сессии с собеседником (прежняя отправка ушла
    /// туда, где её не прочтут). Уходит через `pump` — темпом и по порядку.
    owed: bool,
}

/// Сколько сообщений одному пиру держим в полёте. Остальные ждут в очереди и
/// уходят по мере подтверждений: тысяча накопленных сообщений не вылетает разом.
const SEND_WINDOW: usize = 32;

/// Сколько НОВЫХ сообщений из очереди уходит одному пиру в секунду. Ниже
/// ingress-бюджета контакта (30/с): иначе приёмник режет всплеск, отрезанное
/// приходит ретраем позже — и сообщения в чате встают не по порядку.
const SEND_RATE: usize = 20;

/// Тип записи в очереди: сериализованный `Payload` + уведомлять ли UI о доставке.
const SPOOL_NOTIFY: &str = "payload";
const SPOOL_QUIET: &str = "payload-quiet";

struct Shared {
    identity: Identity,
    /// Сессии 1:1 (ADR-012): всё, что идёт по чат-каналу, шифруется ими. Ключи
    /// только в памяти — перезапуск означает новое рукопожатие, а не потерю связи.
    sessions: Mutex<Sessions>,
    /// Транспорт под замком: узел ЗАМЕНЯЕМ на ходу при смене адреса (ADR-007).
    /// Читатели берут снимок (`node()`), поэтому замена не рвёт ни одну операцию:
    /// приёмный цикл проснётся ошибкой на погашенном узле и переключится на новый.
    node: std::sync::RwLock<Arc<dyn Transport>>,
    cfg: Config,
    peers: Mutex<HashMap<PeerKey, PeerState>>,
    pending: Mutex<HashMap<u64, Pending>>,
    /// Персистентная очередь исходящих (ставит движок). Без неё — только память
    /// (`message_ttl`), как у голого мессенджера в тестах.
    outbox: std::sync::OnceLock<p2p_core::outbox::OutboxStore>,
    /// Темп досылки по пирам: (начало текущей секунды, сколько ушло в ней).
    pace: Mutex<HashMap<PeerKey, (Instant, usize)>>,
    /// Пиры, кому из очереди осталось что слать сверх темпа: дотягивает maintenance.
    backlog: Mutex<std::collections::HashSet<PeerKey>>,
    /// Ретраи и досылка шлют под одним замком. Иначе досылка по Ack из приёмного
    /// потока вклинивается в середину пачки ретраев, и новое сообщение обгоняет
    /// старое, которое как раз переотправляется.
    send_order: Mutex<()>,
    next_id: AtomicU64,
    running: AtomicBool,
    tx: Sender<Event>,
    media_tx: Mutex<Option<SyncSender<MediaPacket>>>,
    /// Бюджет пиров (M3): тир доверия + счётчик заслуг. Кого тут нет, тот
    /// [`Tier::Stranger`] с нулевыми заслугами (общая корзина класса, анти-Sybil).
    /// Обновляется движком (`set_peer_tiers`) при смене контактов/блок-листа/репутации.
    tiers: Mutex<std::collections::HashMap<PeerKey, (Tier, u32)>>,
    /// Пиры, которым ВРЕМЕННО поднят ingress-бюджет: пользователь сам запросил у них
    /// файл, значит этот трафик ожидаемый и резать его своим же анти-флуд лимитом
    /// незачем. Адресно и на срок — не «поднять лимиты всем».
    boosted: Mutex<std::collections::HashMap<PeerKey, Instant>>,
    /// Что сейчас реально заблокировано в транспорте (адреса). Нужно, чтобы при
    /// смене адреса или снятии блока убрать прежнюю запись.
    blocked_at_transport: Mutex<std::collections::HashSet<PeerKey>>,
    /// Адреса контактов (ADR-007): ключ ЛИЧНОСТИ → текущий ТРАНСПОРТНЫЙ ключ.
    /// Кого тут нет — эпоха 0, адрес совпадает с личностью (как было до разделения).
    /// Наполняет движок из стора контактов при старте и после каждого принятого
    /// объявления о смене адреса.
    transports: Mutex<std::collections::HashMap<PeerKey, PeerKey>>,
    /// Обратная карта: адрес → личность. Приёмный слой видит только адрес, а тир
    /// доверия и блок-лист заведены на личность — без этой карты контакт после
    /// смены адреса стал бы для бюджета «незнакомцем», а заблокированный ушёл бы
    /// из-под блока, просто сменив ключ.
    identities: Mutex<std::collections::HashMap<PeerKey, PeerKey>>,
    /// Кто и насколько интенсивно долбится в нас сверх бюджета. Нужен, чтобы
    /// пользователю можно было СКАЗАТЬ, что его флудят: остановить поток на
    /// устройстве нельзя (threat-model N8), но молчать об этом — худший вариант.
    flood: Mutex<std::collections::HashMap<PeerKey, FloodEntry>>,
    /// Недавно принятые надёжные сообщения (пир, id) — отсев повторов. Повтор
    /// подтверждаем снова (его Ack мог потеряться), но наверх не отдаём: второй
    /// `RoomInvite` — это второй звонок в истории.
    delivered: Mutex<HashMap<(PeerKey, u64), Instant>>,
    /// С кем держим связь (контакты и участники групп — задаёт движок): только им
    /// идут пробы и keepalive, и только им отвечаем на Ping. Остальным мы не
    /// сообщаем, онлайн ли мы.
    targets: Mutex<std::collections::HashSet<PeerKey>>,
}

/// Учёт отброшенного по одному источнику. `pps` — оценка за последнее окно.
#[derive(Debug, Clone)]
struct FloodEntry {
    dropped: u64,
    window_start: Instant,
    in_window: u32,
    pps: f64,
    last_seen: Instant,
}

/// Сколько недавно принятых надёжных сообщений помним для отсева повторов.
const DELIVERED_CAP: usize = 8192;

/// Сколько источников держим в учёте. Больше не нужно: экран показывает верхних,
/// а неограниченная карта — это вектор роста памяти от Sybil-флуда.
const FLOOD_CAP: usize = 64;
/// Простаивающие записи выбрасываем: «долбился час назад» пользователю не новость.
const FLOOD_IDLE: Duration = Duration::from_secs(120);

/// Интенсивность флуда от одного источника (для UI и диагностики).
#[derive(Debug, Clone, Copy)]
pub struct FloodStat {
    pub peer: PeerKey,
    /// Всего отброшено пакетов от источника за время жизни процесса.
    pub dropped: u64,
    /// Оценка темпа в последнем секундном окне, пакетов/с.
    pub pps: f64,
}

impl Shared {
    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn emit(&self, ev: Event) {
        let _ = self.tx.send(ev);
    }

    /// Личность за этим адресом. Неизвестный адрес — сам себе личность (эпоха 0
    /// либо незнакомец).
    fn identity_of(&self, src: &PeerKey) -> PeerKey {
        self.identities.lock().unwrap().get(src).copied().unwrap_or(*src)
    }

    /// Текущий транспорт. Клонируем Arc и сразу отпускаем замок: держать его на
    /// время блокирующего `read` нельзя — замена узла встала бы намертво.
    fn node(&self) -> Arc<dyn Transport> {
        self.node.read().unwrap().clone()
    }

    /// Куда физически слать пиру: его транспортный ключ, если он объявлял смену
    /// адреса, иначе сам ключ личности (ADR-007, эпоха 0).
    fn transport_of(&self, peer: &PeerKey) -> PeerKey {
        self.transports.lock().unwrap().get(peer).copied().unwrap_or(*peer)
    }

    /// Разовая отправка: без сессии payload подождёт рукопожатия и уйдёт за ним.
    fn send(&self, peer: &PeerKey, payload: &Payload) -> Result<(), MessengerError> {
        self.send_with(peer, payload, true)
    }

    /// Проба присутствия: без сессии само рукопожатие и есть проба (ответ на него
    /// — признак жизни), держать Ping до его конца незачем.
    fn probe(&self, peer: &PeerKey) {
        let _ = self.send_with(peer, &Payload::Ping { transit: None }, false);
    }

    /// Отправка того, что лежит в `pending`: без сессии — только начать
    /// рукопожатие. Payload дошлёт ретрай (или `resend_pending` сразу по
    /// готовности сессии), и снятое к тому времени уже не уйдёт.
    fn send_pending(&self, peer: &PeerKey, payload: &Payload) -> Result<(), MessengerError> {
        self.send_with(peer, payload, false)
    }

    fn send_with(&self, peer: &PeerKey, payload: &Payload, hold: bool) -> Result<(), MessengerError> {
        // ВАЖНО: сессия заведена на ЛИЧНОСТЬ, а пакеты идут на текущий АДРЕС. Это
        // разные ключи с ADR-007: сессия переживает смену адреса, а слать по
        // ключу личности — значит слать на покинутый адрес.
        let plaintext = payload.to_padded()?;
        let out = self.sessions.lock().unwrap().send(peer, &plaintext, hold, Instant::now(), &mut OsRng);
        self.write_chat(&self.transport_of(peer), out)
    }

    /// Сессия с `peer` только что поднялась: всё, что ждёт его Ack, уходит заново,
    /// не дожидаясь очередного ретрая. Через `pump` — темпом [`SEND_RATE`] и по
    /// порядку: залп всего окна сверх ingress-бюджета получателя срезал бы хвост,
    /// и новые сообщения обогнали бы его (R-011).
    fn resend_pending(&self, peer: &PeerKey) {
        for p in self.pending.lock().unwrap().values_mut().filter(|p| p.peer == *peer) {
            p.owed = true;
        }
        self.pump(peer);
    }

    /// Отправить пакеты сессии на адрес `to` с префиксом чат-канала (демукс с
    /// медиа на приёме).
    fn write_chat(&self, to: &PeerKey, packets: Vec<Vec<u8>>) -> Result<(), MessengerError> {
        let node = self.node();
        for p in packets {
            let mut dg = Vec::with_capacity(1 + p.len());
            dg.push(CHAN_CHAT);
            dg.extend_from_slice(&p);
            node.write(to, &dg)?;
        }
        Ok(())
    }

    /// Зарегистрировать пир как отслеживаемый (для keepalive), если ещё нет.
    fn track(&self, peer: &PeerKey) {
        let now = Instant::now();
        self.peers.lock().unwrap().entry(*peer).or_insert(PeerState::new(now));
    }

    /// ЕДИНЫЙ примитив надёжной отправки: выделяет id, шлёт и кладёт в `pending` —
    /// `maintenance_loop` повторяет до `Ack{id}` или TTL. `mk` строит payload с этим
    /// id (его получатель эхо-нёт в Ack). `notify` — уведомлять ли UI о доставке
    /// (DM: true → Ack/Undelivered-события; группы: false — молча).
    /// Чтобы сделать надёжным новый тип сообщения: добавь ему поле `id`, шли через
    /// этот метод и ответь `Ack{id}` на приёме. Возвращает id.
    ///
    /// Ошибка записи в транспорт — не повод терять сообщение: оно уже в `pending`,
    /// ретрай дожмёт, когда узел поднимется (например, посреди смены адреса).
    fn send_reliable(
        &self,
        peer: &PeerKey,
        notify: bool,
        mk: impl FnOnce(u64) -> Payload,
    ) -> Result<u64, MessengerError> {
        self.send_reliable_for(peer, notify, self.cfg.message_ttl, mk)
    }

    /// То же со своим сроком доставки: у приглашения в звонок он — время дозвона.
    fn send_reliable_for(
        &self,
        peer: &PeerKey,
        notify: bool,
        ttl: Duration,
        mk: impl FnOnce(u64) -> Payload,
    ) -> Result<u64, MessengerError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = mk(id);
        self.track(peer);
        let now = Instant::now();
        self.pending.lock().unwrap().insert(
            id,
            Pending {
                peer: *peer,
                payload: payload.clone(),
                created: now,
                last_sent: now,
                ttl,
                notify,
                durable: false,
                owed: false,
            },
        );
        if let Err(e) = self.send_pending(peer, &payload) {
            log::warn!("send id={id}: транспорт не принял ({e}), дожмёт ретрай");
        }
        Ok(id)
    }

    /// Надёжно И переживая офлайн получателя и наш перезапуск: сообщение целиком
    /// ложится в персистентную очередь и уходит в порядке постановки, не больше
    /// [`SEND_WINDOW`] в полёте. Для всего, что меняет переписку (текст, вложение,
    /// правка, отзыв, закреп, групповое MLS): потеря такого пакета — расхождение
    /// историй, которое уже ничем не чинится.
    fn send_durable(
        &self,
        peer: &PeerKey,
        notify: bool,
        mk: impl FnOnce(u64) -> Payload,
    ) -> Result<u64, MessengerError> {
        let Some(outbox) = self.outbox.get() else {
            return self.send_reliable(peer, notify, mk);
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = mk(id);
        let body = payload.to_bytes()?;
        let kind = if notify { SPOOL_NOTIFY } else { SPOOL_QUIET };
        outbox
            .enqueue(peer, id, kind, &body, Self::now_unix())
            .map_err(|e| MessengerError::Spool(e.to_string()))?;
        self.track(peer);
        self.pump(peer);
        Ok(id)
    }

    /// Дотянуть окно пира из очереди: голова очереди (первые [`SEND_WINDOW`]) —
    /// это ровно то, что должно быть в полёте; чего из неё нет в `pending`, то и
    /// отправляем, не быстрее [`SEND_RATE`]. Зовётся на постановку, Ack, появление
    /// пира и старт; упёрлись в темп — пира дотянет maintenance.
    ///
    /// Сперва — должное заново (`owed`, после смены сессии), и пока оно не ушло
    /// всё, новое из очереди не трогаем: иначе новое обогнало бы старое.
    fn pump(&self, peer: &PeerKey) {
        let _order = self.send_order.lock().unwrap();
        let now = Instant::now();
        let budget = {
            let mut pace = self.pace.lock().unwrap();
            let slot = pace.entry(*peer).or_insert((now, 0));
            if now.duration_since(slot.0) >= Duration::from_secs(1) {
                *slot = (now, 0);
            }
            SEND_RATE.saturating_sub(slot.1)
        };
        // Кандидаты на отправку — по id, то есть в порядке постановки: должное
        // заново (уже в `pending`) и не отправленное из головы очереди (в том числе
        // выпавшее из `pending` по сроку). Отдельными проходами новое обгоняло бы
        // старое (R-011).
        let outbox = self.outbox.get();
        let head = outbox.and_then(|o| o.head_for(peer, SEND_WINDOW).ok()).unwrap_or_default();
        let mut to_send = Vec::new();
        let mut broken = Vec::new();
        let deferred = {
            let mut pending = self.pending.lock().unwrap();
            // (id, None) — должное из `pending`; (id, Some(i)) — head[i].
            let mut cands: Vec<(u64, Option<usize>)> =
                pending.iter().filter(|(_, p)| p.peer == *peer && p.owed).map(|(id, _)| (*id, None)).collect();
            cands.extend(
                head.iter().enumerate().filter(|(_, e)| !pending.contains_key(&e.msg_id)).map(|(i, e)| (e.msg_id, Some(i))),
            );
            cands.sort_unstable_by_key(|c| c.0);
            let deferred = cands.len() > budget;
            for (id, src) in cands.into_iter().take(budget) {
                match src {
                    None => {
                        if let Some(p) = pending.get_mut(&id) {
                            p.owed = false;
                            p.last_sent = now;
                            to_send.push(p.payload.clone());
                        }
                    }
                    Some(i) => {
                        let e = &head[i];
                        let Ok(payload) = Payload::from_bytes(&e.payload) else {
                            broken.push(e.id);
                            continue;
                        };
                        let notify = e.payload_type == SPOOL_NOTIFY;
                        pending.insert(
                            e.msg_id,
                            Pending {
                                peer: *peer,
                                payload: payload.clone(),
                                created: now,
                                last_sent: now,
                                ttl: self.cfg.message_ttl,
                                notify,
                                durable: true,
                                owed: false,
                            },
                        );
                        to_send.push(payload);
                    }
                }
            }
            deferred
        };
        // Нечитаемая запись (формат сменился) застряла бы в голове навсегда и
        // заперла окно — выбрасываем.
        if let Some(outbox) = outbox {
            for id in broken {
                let _ = outbox.remove(id);
            }
        }
        if let Some(slot) = self.pace.lock().unwrap().get_mut(peer) {
            slot.1 += to_send.len();
        }
        if deferred {
            self.backlog.lock().unwrap().insert(*peer);
        }
        for payload in to_send {
            if let Err(e) = self.send_pending(peer, &payload) {
                log::warn!("досылка пиру {:02x?}: транспорт не принял ({e})", &peer[..4]);
            }
        }
    }

    fn is_online(&self, peer: &PeerKey) -> bool {
        self.peers
            .lock()
            .unwrap()
            .get(peer)
            .map(|s| s.online)
            .unwrap_or(false)
    }

    /// Первая ли это доставка надёжного сообщения `id` от `peer`. Помним дольше
    /// срока ретраев отправителя; карта ограничена, чтобы флуд её не раздул.
    fn first_delivery(&self, peer: &PeerKey, id: u64, now: Instant) -> bool {
        let keep = self.cfg.message_ttl * 2;
        let mut d = self.delivered.lock().unwrap();
        if d.len() >= DELIVERED_CAP {
            d.retain(|_, t| now.duration_since(*t) < keep);
            if d.len() >= DELIVERED_CAP {
                // Всё свежее: забываем самое старое. Худшее последствие — повтор
                // наверх, который движок для текста и так отсеет.
                if let Some(old) = d.iter().min_by_key(|(_, t)| **t).map(|(k, _)| *k) {
                    d.remove(&old);
                }
            }
        }
        d.insert((*peer, id), now).is_none()
    }

    /// Пакет от `peer` пришёл с адреса `src`, который за этой личностью числится.
    fn is_bound(&self, peer: &PeerKey, src: &PeerKey) -> bool {
        peer == src || self.transport_of(peer) == *src
    }

    /// От пира пришёл подлинный трафик: он онлайн. Вернулся — досылаем накопленное.
    fn peer_alive(&self, peer: &PeerKey) {
        if self.mark_seen(peer) {
            self.emit(Event::Presence { peer: *peer, online: true });
            self.pump(peer);
        }
    }

    /// Отметить приход трафика от пира; вернуть true, если пир ПЕРЕШЁЛ в онлайн.
    fn mark_seen(&self, peer: &PeerKey) -> bool {
        let now = Instant::now();
        let mut peers = self.peers.lock().unwrap();
        let st = peers.entry(*peer).or_insert(PeerState::new(now));
        st.last_seen = now;
        if !st.online {
            st.online = true;
            st.backoff = PROBE_MIN;
            true
        } else {
            false
        }
    }
}

/// Мессенджер. `Send + Sync`; поток событий отдаётся отдельным `Receiver`.
pub struct Messenger {
    shared: Arc<Shared>,
    // TODO: грациозное завершение фоновых потоков (сейчас живут до выхода процесса).
    _rx_thread: thread::JoinHandle<()>,
    _maint_thread: thread::JoinHandle<()>,
}

impl Messenger {
    /// Запустить с параметрами надёжности по умолчанию.
    pub fn start(identity: Identity, node: Node) -> (Messenger, Receiver<Event>) {
        Self::start_with_config(identity, node, Config::default())
    }

    /// Запустить с заданной конфигурацией (реальный меш-узел).
    pub fn start_with_config(
        identity: Identity,
        node: Node,
        cfg: Config,
    ) -> (Messenger, Receiver<Event>) {
        Self::start_with_transport(identity, Arc::new(node), cfg)
    }

    /// Запустить поверх произвольного транспорта (реальный `Node` или loopback-двойник
    /// в тестах). Ядро логики надёжности/диспетчеризации не зависит от вида транспорта.
    pub fn start_with_transport(
        identity: Identity,
        node: Arc<dyn Transport>,
        cfg: Config,
    ) -> (Messenger, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            sessions: Mutex::new(Sessions::new(&identity)),
            identity,
            node: std::sync::RwLock::new(node),
            cfg,
            peers: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            outbox: std::sync::OnceLock::new(),
            pace: Mutex::new(HashMap::new()),
            backlog: Mutex::new(std::collections::HashSet::new()),
            send_order: Mutex::new(()),
            // Сеем от времени (мс с эпохи), а НЕ с 1: id монотонно растут между
            // перезапусками и не рециклируются. Иначе после рестарта id снова 1..N
            // сталкивались бы со старыми сообщениями в базе получателя → затирание
            // истории (INSERT OR REPLACE) и ложный дедуп «уже видел» (сообщение не
            // показывается, хотя доставлено). Внутри сессии просто инкрементируется.
            next_id: AtomicU64::new(
                SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1),
            ),
            running: AtomicBool::new(true),
            tx,
            media_tx: Mutex::new(None),
            tiers: Mutex::new(std::collections::HashMap::new()),
            boosted: Mutex::new(std::collections::HashMap::new()),
            blocked_at_transport: Mutex::new(std::collections::HashSet::new()),
            transports: Mutex::new(std::collections::HashMap::new()),
            identities: Mutex::new(std::collections::HashMap::new()),
            flood: Mutex::new(std::collections::HashMap::new()),
            delivered: Mutex::new(HashMap::new()),
            targets: Mutex::new(std::collections::HashSet::new()),
        });
        let s1 = shared.clone();
        let rx_thread = thread::spawn(move || receive_loop(s1));
        let s2 = shared.clone();
        let maint_thread = thread::spawn(move || maintenance_loop(s2));
        (
            Messenger {
                shared,
                _rx_thread: rx_thread,
                _maint_thread: maint_thread,
            },
            rx,
        )
    }

    /// Наш публичный ключ.
    /// Подключить персистентную очередь исходящих и дослать всё, что в ней ждёт.
    /// Один раз за жизнь мессенджера; до этого надёжные сообщения живут только в
    /// памяти.
    pub fn set_outbox(&self, store: p2p_core::outbox::OutboxStore) {
        let peers = store.peers().unwrap_or_default();
        if self.shared.outbox.set(store).is_err() {
            return;
        }
        for peer in peers {
            self.shared.track(&peer);
            self.shared.pump(&peer);
        }
    }

    /// Больше не доставлять сообщение `id` пиру `peer` (например, его отозвали, пока
    /// оно ждало в очереди): убрать и из очереди, и из ретраев.
    pub fn forget(&self, peer: &PeerKey, id: u64) {
        let removed = {
            let mut pending = self.shared.pending.lock().unwrap();
            pending.get(&id).filter(|p| p.peer == *peer).is_some() && pending.remove(&id).is_some()
        };
        if let Some(ob) = self.shared.outbox.get() {
            let _ = ob.remove_by_msg(peer, id);
        }
        if removed {
            self.shared.pump(peer);
        }
    }

    /// С кем держим связь: пробы офлайн-пиров с backoff, keepalive онлайн-пиров,
    /// ответ на их Ping. Кто выпал из набора (удалён, заблокирован, исключён из
    /// группы), тот перестаёт получать от нас что-либо служебное сразу.
    pub fn set_targets(&self, targets: std::collections::HashSet<PeerKey>) {
        *self.shared.targets.lock().unwrap() = targets;
    }

    /// Попробовать достучаться до пира прямо сейчас (пользователь открыл чат,
    /// сеть поднялась): сбросить backoff и послать пробу, не дожидаясь расписания.
    pub fn poke(&self, peer: &PeerKey) {
        if let Some(st) = self.shared.peers.lock().unwrap().get_mut(peer) {
            st.backoff = PROBE_MIN;
            st.next_probe = Instant::now() + PROBE_MIN;
        }
        self.shared.probe(peer);
    }

    /// Сеть сменилась: всех офлайн-пиров из набора пробуем заново с короткой паузы.
    pub fn reset_probes(&self) {
        let now = Instant::now();
        for st in self.shared.peers.lock().unwrap().values_mut() {
            if !st.online {
                st.backoff = PROBE_MIN;
                st.next_probe = now;
            }
        }
    }

    pub fn public_key(&self) -> PeerKey {
        self.shared.identity.public_key_bytes()
    }

    /// Временно поднять ingress-бюджет пира: мы сами запросили у него данные
    /// (докачка вложения по кнопке). Истекает само — продлевается новыми запросами.
    pub fn boost_peer(&self, peer: &PeerKey, until: Duration) {
        self.shared.boosted.lock().unwrap().insert(*peer, Instant::now() + until);
    }

    /// Кто сейчас флудит: источники с наибольшим отброшенным темпом, по убыванию.
    /// Пустой список = флуда нет. Читает UI, чтобы честно показать происходящее.
    pub fn flood_stats(&self) -> Vec<FloodStat> {
        let map = self.shared.flood.lock().unwrap();
        let mut v: Vec<FloodStat> = map
            .iter()
            .map(|(peer, e)| FloodStat { peer: *peer, dropped: e.dropped, pps: e.pps })
            .collect();
        v.sort_by(|a, b| b.pps.total_cmp(&a.pps).then(b.dropped.cmp(&a.dropped)));
        v
    }

    /// Погасить текущий транспорт, НЕ останавливая мессенджер: приёмный цикл
    /// переживает это и ждёт нового узла. Первая половина смены адреса — между
    /// гашением и подъёмом нового обязана быть пауза офлайн (ADR-007).
    pub fn stop_transport(&self) {
        self.shared.node().stop();
    }

    /// Заменить транспорт на ходу — смена нашего адреса (ADR-007).
    ///
    /// Старый узел гасится вызывающим: между гашением и подъёмом нового ОБЯЗАНА
    /// быть пауза офлайн, иначе новый узел унаследует маршрутную позицию старого
    /// ключа и поток флудера продолжит приходить (проверено на публичном меше).
    /// Приёмный цикл переживает замену сам.
    pub fn swap_transport(&self, node: Arc<dyn Transport>) {
        *self.shared.node.write().unwrap() = node;
    }

    /// Обновить карту адресов контактов (ADR-007): личность → транспортный ключ.
    /// Список полный: чего в нём нет, то считается эпохой 0 (адрес = личность).
    pub fn set_peer_transports(&self, map: &[(PeerKey, PeerKey)]) {
        *self.shared.transports.lock().unwrap() = map.iter().copied().collect();
        *self.shared.identities.lock().unwrap() = map.iter().map(|(ik, tk)| (*tk, *ik)).collect();
        // Блокировки заведены на личность, а транспорт режет по адресу: после смены
        // адреса блок надо переставить, иначе заблокированный вернётся под новым
        // ключом.
        self.apply_transport_blocks();
    }

    /// Переставить ранний дроп (libygg) на АКТУАЛЬНЫЕ адреса заблокированных.
    /// Снимаем прежние — иначе за брошенными ключами копился бы мусор, а
    /// разблокированный остался бы отрезан там, где прикладной слой его не видит.
    fn apply_transport_blocks(&self) {
        let blocked_ids: Vec<PeerKey> = self
            .shared
            .tiers
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, (t, _))| t.is_blocked())
            .map(|(k, _)| *k)
            .collect();
        let want: std::collections::HashSet<PeerKey> =
            blocked_ids.iter().map(|ik| self.shared.transport_of(ik)).collect();
        let node = self.shared.node();
        let mut applied = self.shared.blocked_at_transport.lock().unwrap();
        for stale in applied.difference(&want) {
            node.unblock(stale);
        }
        for k in want.difference(&applied) {
            node.block(k);
        }
        *applied = want;
    }

    /// Залить бюджеты пиров (M3): тир доверия + заслуги (`helpful`). Кого нет в
    /// списке — [`Tier::Stranger`] без заслуг. Зовётся движком при старте и любой
    /// смене контактов/блок-листа/репутации.
    pub fn set_peer_tiers(&self, tiers: &[(PeerKey, Tier, u32)]) {
        *self.shared.tiers.lock().unwrap() =
            tiers.iter().map(|(k, t, h)| (*k, (*t, *h))).collect();
        // Блокировку дублируем в транспорт: там пакет отбрасывается ещё до
        // пересечения FFI, не тратя копирование и разбор заголовка.
        self.apply_transport_blocks();
    }

    /// Адрес Yggdrasil нашего узла.
    pub fn address(&self) -> String {
        self.shared.node().address()
    }

    /// Онлайн ли пир (по последнему трафику / keepalive).
    pub fn is_online(&self, peer: &PeerKey) -> bool {
        self.shared.is_online(peer)
    }

    /// Число прямых пиров транспорта (для индикатора сети).
    pub fn net_peers(&self) -> i32 {
        self.shared.node().peer_count()
    }

    /// Число узлов в дереве маршрутизации транспорта.
    pub fn net_tree(&self) -> i32 {
        self.shared.node().tree_count()
    }

    /// JSON-массив пиров транспорта (адрес/направление/пинг/трафик) для стата сети.
    pub fn net_peers_json(&self) -> String {
        self.shared.node().peers_json()
    }

    /// Повторно добавить пир по URI: обходит внутренний backoff Yggdrasil и
    /// форсирует немедленную попытку реконнекта (например, после флика WiFi).
    pub fn add_peer(&self, uri: &str) {
        let _ = self.shared.node().add_peer(uri);
    }

    /// Остановить мессенджер: останавливает транспортный узел и фоновые потоки.
    pub fn shutdown(&self) {
        self.shared.running.store(false, Ordering::Relaxed);
        self.shared.node().stop();
    }

    /// Установить сессию: шлём `Hello`, пока пир не станет онлайн (двунаправленный
    /// обмен поднимает шифр-сессию ironwood). После — пир под keepalive.
    pub fn connect(&self, peer: &PeerKey, timeout: Duration) -> Result<(), MessengerError> {
        self.shared.targets.lock().unwrap().insert(*peer);
        self.shared.track(peer);
        let start = Instant::now();
        loop {
            self.shared.send(peer, &Payload::Hello)?;
            if self.shared.is_online(peer) {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(MessengerError::HandshakeTimeout);
            }
            thread::sleep(Duration::from_millis(150));
        }
    }

    /// Отправить текст пиру. Возвращает id; доставка повторяется до `Ack` или TTL
    /// (см. [`Event::Ack`] / [`Event::Undelivered`]).
    pub fn send_text(&self, peer: &PeerKey, text: &str) -> Result<u64, MessengerError> {
        self.send_text_reply(peer, text, 0, false, "", false)
    }

    /// То же с ответом на сообщение (`reply_to` = 0 — обычное сообщение).
    /// `reply_mine` — чьё цитируемое сообщение с точки зрения ОТПРАВИТЕЛЯ; получатель
    /// перевернёт флаг (см. `Payload::Text`).
    pub fn send_text_reply(
        &self,
        peer: &PeerKey,
        text: &str,
        reply_to: u64,
        reply_mine: bool,
        fwd_from: &str,
        fwd_anon: bool,
    ) -> Result<u64, MessengerError> {
        let text = text.to_string();
        let fwd_from = fwd_from.to_string();
        // DM: notify=true → UI получает Ack/Undelivered (✓✓ / ошибка).
        self.shared.send_durable(peer, true, move |id| Payload::Text {
            id,
            text,
            reply_to,
            reply_mine,
            fwd_from,
            fwd_anon,
        })
    }

    /// Попросить получателя удалить наше сообщение `target` («удалить у всех»).
    /// Надёжно, как текст: просьба, отправленная в момент, когда собеседник офлайн,
    /// обязана доехать позже — иначе «удалил» означало бы «удалил, если повезло».
    pub fn send_retract(&self, peer: &PeerKey, target: u64) -> Result<u64, MessengerError> {
        self.shared.send_durable(peer, false, move |id| Payload::Retract { id, target })
    }

    /// Исправить СВОЁ сообщение. Надёжно, как отзыв: правка, сделанная пока
    /// собеседник офлайн, обязана доехать позже — иначе у двоих разный текст одного
    /// сообщения, и это хуже, чем отсутствие правки вовсе.
    pub fn send_edit(&self, peer: &PeerKey, target: u64, text: &str) -> Result<u64, MessengerError> {
        let text = text.to_string();
        self.shared.send_durable(peer, false, move |id| Payload::Edit { id, target, text })
    }

    /// Закрепить/открепить сообщение в личном чате. Надёжно, как отзыв: закреп,
    /// сделанный пока собеседник офлайн, обязан доехать позже — иначе у двоих
    /// разные шапки одного чата.
    pub fn send_pin(
        &self,
        peer: &PeerKey,
        target: u64,
        target_mine: bool,
        on: bool,
    ) -> Result<u64, MessengerError> {
        self.shared
            .send_durable(peer, false, move |id| Payload::Pin { id, target, target_mine, on })
    }

    /// Объявить контакту смену своего АДРЕСА (ADR-007). Надёжно, как текст:
    /// контакт, отсутствовавший в момент смены, обязан узнать новый адрес по
    /// возвращении — иначе связь с ним рвётся навсегда.
    ///
    /// `sig` строит владелец личности ([`p2p_core::Identity::sign_transport_rotate`]):
    /// подпись — единственное, что связывает старую личность с новым адресом.
    /// `notify=false` — это служебное объявление, в UI как «доставлено сообщение»
    /// оно показываться не должно.
    pub fn send_transport_rotate(
        &self,
        peer: &PeerKey,
        epoch: u32,
        transport_pub: &[u8; 32],
        sig: &[u8; 64],
    ) -> Result<u64, MessengerError> {
        let (tp, sg) = (transport_pub.to_vec(), sig.to_vec());
        self.shared.send_reliable(peer, false, move |id| Payload::TransportRotate {
            id,
            epoch,
            transport_pub: tp,
            sig: sg,
        })
    }

    /// Отправить медиа-СООБЩЕНИЕ чата (M2): ссылка на блоб по хэшу. Надёжно
    /// (retry-until-Ack), как текст; сами байты идут блоб-каналом отдельно.
    /// (Отличать от [`Messenger::send_media`] — realtime-датаграммы звонка.)
    pub fn send_media_message(
        &self,
        peer: &PeerKey,
        hash: &[u8],
        mime: &str,
        name: &str,
        meta: MediaMeta,
    ) -> Result<u64, MessengerError> {
        let (hash, mime, name) = (hash.to_vec(), mime.to_string(), name.to_string());
        // Режем подпись здесь, а не в UI: за размер пакета отвечает отправляющий
        // слой, и обойти его через другой клиент не должно получаться.
        let caption = truncate_caption(&meta.caption);
        self.shared.send_durable(peer, true, move |id| Payload::Media {
            id, hash, mime, name,
            size: meta.size,
            duration_ms: meta.duration_ms,
            width: meta.width,
            height: meta.height,
            caption: caption.clone(),
            thumb: meta.thumb.clone(),
            fwd_from: meta.fwd_from.clone(),
            fwd_anon: meta.fwd_anon,
            reply_to: meta.reply_to,
            reply_mine: meta.reply_mine,
        })
    }

    /// Отправить медиа-датаграмму (реалтайм-канал, без Ack и повторов). Кадр
    /// шифруется ключом медиа сессии 1:1 с этим собеседником (SEC-4) — и в
    /// личном, и в групповом звонке: звонок — это полная сетка пар. Нет сессии —
    /// кадр отбрасывается (опоздавшее медиа бесполезно) и начинается рукопожатие.
    pub fn send_media(
        &self,
        peer: &PeerKey,
        track: u8,
        seq: u16,
        ts: u32,
        payload: &[u8],
    ) -> Result<(), MessengerError> {
        if payload.len() > MAX_MEDIA_PAYLOAD {
            return Err(MessengerError::MediaTooLarge);
        }
        let total = MEDIA_HEADER_LEN + payload.len();
        let mut frame = vec![0u8; total];
        write_media_header(&mut frame, track, seq, ts);
        frame[MEDIA_HEADER_LEN..total].copy_from_slice(payload);
        let sealed = self.shared.sessions.lock().unwrap().seal_media(peer, &frame, Instant::now());
        let Some(sealed) = sealed else {
            self.shared.probe(peer);
            return Ok(());
        };
        let mut dg = Vec::with_capacity(1 + sealed.len());
        dg.push(CHAN_MEDIA);
        dg.extend_from_slice(&sealed);
        if let Err(e) = self.shared.node().write(&self.shared.transport_of(peer), &dg) {
            log::warn!("p2p-media: node.write failed for track={} len={}: {:?}", track, dg.len(), e);
            return Err(MessengerError::Transport(e));
        }
        Ok(())
    }

    /// Пригласить пира в голосовую комнату. Надёжно (retry-until-Ack — чинит
    /// потерю первого пакета на холодной сессии), но только пока идёт дозвон
    /// ([`Config::ring_ttl`]): позже звонок уже никто не ждёт. Возвращает id —
    /// положил трубку раньше, сними приглашение через [`Self::cancel`].
    pub fn send_room_invite(
        &self,
        peer: &PeerKey,
        room_id: &[u8],
        audio: bool,
        video: bool,
    ) -> Result<u64, MessengerError> {
        let room_id = room_id.to_vec();
        self.shared.send_reliable_for(peer, false, self.shared.cfg.ring_ttl, move |id| Payload::RoomInvite {
            id,
            room_id,
            audio,
            video,
        })
    }

    /// Отклонить входящий звонок (или отменить свой дозвон) — надёжно, с тем же
    /// сроком, что и приглашение: отказ, пришедший позже, отвечает на звонок,
    /// которого уже нет.
    pub fn send_room_decline(&self, peer: &PeerKey, room_id: &[u8]) -> Result<(), MessengerError> {
        let room_id = room_id.to_vec();
        self.shared
            .send_reliable_for(peer, false, self.shared.cfg.ring_ttl, move |id| Payload::RoomDecline { id, room_id })?;
        Ok(())
    }

    /// Перестать доставлять неподтверждённое сообщение `id` (не из офлайн-очереди).
    /// Для звонка: положили трубку — приглашение, ещё не дошедшее до собеседника,
    /// не должно зазвонить у него потом.
    pub fn cancel(&self, id: u64) {
        let mut pending = self.shared.pending.lock().unwrap();
        if pending.get(&id).is_some_and(|p| !p.durable) {
            pending.remove(&id);
        }
    }

    /// Переслать групповое MLS-сообщение конкретному участнику — НАДЁЖНО:
    /// повтор до `Ack` или TTL (как DM), per-recipient. Каждому участнику —
    /// свой `id` (одно и то же `data`), поэтому общая схема `pending[id]` не
    /// меняется. Дедуп повторов на приёме делает MLS (generation). Это чинит и
    /// потерю прикладных сообщений, и рассинхрон эпох от потерянных Commit'ов.
    pub fn send_group(&self, peer: &PeerKey, data: &[u8]) -> Result<(), MessengerError> {
        let data = data.to_vec();
        // Группа: notify=false (статус по участникам в UI не показываем). Каждому
        // получателю — свой id, одно и то же data; дедуп повторов делает MLS.
        self.shared.send_durable(peer, false, move |id| Payload::Group { id, data })?;
        Ok(())
    }

    /// Попросить у контакта KeyPackage (для добавления в группу) — НАДЁЖНО
    /// (retry-until-Ack): на холодной сессии первый пакет мог теряться, из-за чего
    /// инвайт «не срабатывал». Дедуп повторов на приёме — по идемпотентности
    /// (повторный запрос лишь пере-присылает KeyPackage).
    pub fn send_group_key_request(&self, peer: &PeerKey, gid: &[u8]) -> Result<(), MessengerError> {
        let gid = gid.to_vec();
        self.shared.send_reliable(peer, false, move |id| Payload::GroupKeyRequest { id, gid })?;
        Ok(())
    }

    /// Пригласить контакт в группу отдельным сообщением-карточкой — надёжно
    /// (retry-until-Ack). Получатель решает вступать; тогда шлёт KeyPackage.
    pub fn send_group_invite(&self, peer: &PeerKey, gid: &[u8], name: &str) -> Result<(), MessengerError> {
        let (gid, name) = (gid.to_vec(), name.to_string());
        self.shared.send_durable(peer, false, move |id| Payload::GroupInvite { id, gid, name })?;
        Ok(())
    }

    /// Прислать свой KeyPackage для вступления в группу `gid` — надёжно
    /// (retry-until-Ack).
    pub fn send_group_key_reply(&self, peer: &PeerKey, gid: &[u8], key_package: &[u8]) -> Result<(), MessengerError> {
        let (gid, kp) = (gid.to_vec(), key_package.to_vec());
        self.shared
            .send_reliable(peer, false, move |id| Payload::GroupKeyReply { id, gid, key_package: kp })?;
        Ok(())
    }

    /// Маячок присутствия в комнате-звонке участнику группы (один выстрел,
    /// без очереди повтора — следующий маячок придёт сам через интервал).
    pub fn send_room_presence(&self, peer: &PeerKey, gid: &[u8], flags: u8) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::RoomPresence { gid: gid.to_vec(), flags })
    }

    /// Запрос повторной синхронизации группы админу (ADR-004). Fire-and-forget:
    /// потеряется — следующий рассинхрон запросит снова.
    pub fn send_group_resync_request(&self, peer: &PeerKey, gid: &[u8]) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::GroupResyncRequest { gid: gid.to_vec() })
    }

    /// Анонсировать блоб пиру: отправить манифест (M2). Fire-and-forget — если
    /// потеряется, отправитель может анонсировать повторно (или получатель уже качает).
    pub fn send_blob_manifest(&self, peer: &PeerKey, manifest: &str) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::BlobManifest { manifest: manifest.to_string() })
    }

    /// Отправить ЧАСТЬ манифеста крупного блоба (список хэшей не влезает в одну
    /// датаграмму). Fire-and-forget: потерянную часть получатель перезапросит вместе
    /// с недостающими чанками, как и всё остальное в блоб-протоколе.
    #[allow(clippy::too_many_arguments)]
    pub fn send_blob_manifest_part(
        &self,
        peer: &PeerKey,
        id: &[u8],
        mime: &str,
        size: u64,
        part: u32,
        total: u32,
        hashes: &[u8],
    ) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::BlobManifestPart {
            id: id.to_vec(),
            mime: mime.to_string(),
            size,
            part,
            total,
            hashes: hashes.to_vec(),
        })
    }

    /// Рассылка обновления профиля (M2): самоназвание + хэш аватара. Fire-and-forget
    /// (идемпотентно, повторяется на коннекте).
    pub fn send_profile_update(&self, peer: &PeerKey, name: &str, avatar_hash: &[u8]) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::ProfileUpdate { name: name.to_string(), avatar_hash: avatar_hash.to_vec() })
    }

    /// Запросить манифест блоба по id (pull-по-хэшу, M2). Fire-and-forget — при потере
    /// запрашивается снова.
    pub fn request_blob_manifest(&self, peer: &PeerKey, id: &[u8]) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::BlobManifestRequest { id: id.to_vec() })
    }

    /// Запросить чанк блоба (получатель-драйвен pull). Fire-and-forget: недошедший
    /// запрос/чанк повторно запрашивается по таймауту (см. blob-менеджер).
    pub fn send_blob_request(&self, peer: &PeerKey, id: &[u8], index: u32) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::BlobRequest { id: id.to_vec(), index })
    }

    /// Отдать чанк блоба. E2E-шифруется как обычное сообщение; целостность чанка —
    /// по хэшу в манифесте на приёме.
    pub fn send_blob_chunk(&self, peer: &PeerKey, id: &[u8], index: u32, data: &[u8]) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::BlobChunk { id: id.to_vec(), index, data: data.to_vec() })
    }

    /// Запрос на выход из группы админу (ADR-004). Fire-and-forget.
    pub fn send_group_leave_request(&self, peer: &PeerKey, gid: &[u8]) -> Result<(), MessengerError> {
        self.shared.send(peer, &Payload::GroupLeaveRequest { gid: gid.to_vec() })
    }

    /// Включить приём медиа: возвращает bounded-канал. При переполнении новые
    /// пакеты отбрасываются (для реалтайма ронять опоздавшие лучше, чем копить).
    pub fn media_channel(&self, capacity: usize) -> Receiver<MediaPacket> {
        let (tx, rx) = mpsc::sync_channel(capacity);
        *self.shared.media_tx.lock().unwrap() = Some(tx);
        rx
    }
}

/// Учесть отброшенный пакет от источника: счётчик и оценка темпа за секундное
/// окно. Карта ограничена по размеру и чистится от простоя — иначе Sybil-флуд
/// с тысячи ключей растил бы её без предела.
fn note_flood(shared: &Arc<Shared>, src: &PeerKey, now: Instant) {
    let mut map = shared.flood.lock().unwrap();
    let e = map.entry(*src).or_insert(FloodEntry {
        dropped: 0,
        window_start: now,
        in_window: 0,
        pps: 0.0,
        last_seen: now,
    });
    e.dropped = e.dropped.saturating_add(1);
    e.in_window = e.in_window.saturating_add(1);
    e.last_seen = now;
    let elapsed = now.saturating_duration_since(e.window_start);
    if elapsed >= Duration::from_secs(1) {
        e.pps = e.in_window as f64 / elapsed.as_secs_f64();
        e.in_window = 0;
        e.window_start = now;
    }
    if map.len() > FLOOD_CAP {
        map.retain(|_, e| now.saturating_duration_since(e.last_seen) < FLOOD_IDLE);
        while map.len() > FLOOD_CAP {
            // Выбрасываем самый тихий источник: экран всё равно показывает верхних.
            if let Some(quietest) =
                map.iter().min_by(|a, b| a.1.pps.total_cmp(&b.1.pps)).map(|(k, _)| *k)
            {
                map.remove(&quietest);
            } else {
                break;
            }
        }
    }
}

fn receive_loop(shared: Arc<Shared>) {
    let mut buf = vec![0u8; 65535];
    // Повтор датаграммы отсекает окно повторов сессии (ADR-012). Повторную
    // ДОСТАВКУ (ретрай после потерянного Ack — это новый пакет) отсекает движок —
    // дедупом по id сообщения.

    // Приёмный rate-limit (M1, анти-abuse): per-source token-bucket, навязывается
    // ПРИНИМАЮЩИМ (модификация отправителя не обходит). Применяется по src-ключу
    // Yggdrasil ДО расшифровки — флуд дропается до дорогого крипто. Каналы разные:
    //  · control/сообщения (CHAN_CHAT) — строгий бюджет (спам-вектор);
    //  · медиа (CHAN_MEDIA) — щедрый (звонок легитимно высокочастотен: ~50 ауд/с +
    //    видео), но абсурдный флуд (>~600/с устойчиво) всё равно режется.
    // M3: бюджет градуирован по доверию, конкретные числа — в ЕДИНОМ источнике
    // `p2p_core::budget::Tier` (сверенный > контакт > чужой; чужие делят одну корзину
    // класса → Sybil из 1000 ключей не расширяет бюджет; заблокированный режется до
    // расшифровки). Медиа — щедрый per-source (звонок легитимно высокочастотен).
    let mut chat_rl = TieredRateLimiter::new();
    let mut media_rl = RateLimiter::new(1200.0, 600.0);
    let mut pkt_count: u64 = 0;
    let mut dropped: u64 = 0;
    // Медиа, которое некуда деть (очередь звонка полна или звонка уже нет, а
    // собеседник ещё шлёт). Счётчики — по той же причине, что и у `dropped`:
    // строка лога на каждый пакет сама становится штормом в сотни строк в секунду.
    let mut media_full: u64 = 0;
    let mut media_orphan: u64 = 0;
    let mut media_bad: u64 = 0;
    loop {
        // Снимок берём КАЖДУЮ итерацию: при смене адреса узел заменяется, и цикл
        // обязан продолжить работу на новом — иначе после ротации приём молча
        // умирает, а приложение выглядит «в сети, но ничего не приходит».
        let node = shared.node();
        let (n, src) = match node.read(&mut buf) {
            Ok(v) => v,
            Err(_) => {
                if !shared.running.load(Ordering::Relaxed) {
                    return; // штатная остановка мессенджера
                }
                // Узел погашен ротацией (или ещё не поднят во время паузы офлайн) —
                // ждём появления нового и пробуем снова.
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        if n == 0 {
            continue;
        }
        // Ingress rate-limit по источнику (до расшифровки). Превышение → дроп.
        let now = Instant::now();
        // Бюджет источника (M3): блок режется здесь, ДО расшифровки; заслуженный
        // контакт получает повышенный лимит.
        // Тир ищем по ЛИЧНОСТИ за этим адресом: иначе контакт, сменивший адрес,
        // разжаловался бы в незнакомцы, а заблокированный вышел бы из-под блока.
        let sender_id = shared.identity_of(&src);
        let (tier, helpful) =
            shared.tiers.lock().unwrap().get(&sender_id).copied().unwrap_or((Tier::default(), 0));
        // Мы сами просили у него файл → пропускаем без анти-флуд бюджета (он защищает
        // от НЕЖДАННОГО потока, а этот поток заказан пользователем).
        let boosted = {
            let mut b = shared.boosted.lock().unwrap();
            match b.get(&sender_id) {
                Some(until) if *until > now => true,
                Some(_) => {
                    b.remove(&sender_id);
                    false
                }
                None => false,
            }
        };
        let allowed = if tier.is_blocked() {
            false // заблокированный не тратит наше крипто ни на одном канале
        } else if boosted {
            true
        } else if buf[0] == CHAN_MEDIA {
            media_rl.allow(&src, now)
        } else {
            chat_rl.allow_with_reputation(&src, tier, helpful, now)
        };
        if !allowed {
            dropped = dropped.wrapping_add(1);
            note_flood(&shared, &sender_id, now);
            // Логируем редко, чтобы лог сам не стал амплификацией флуда.
            if dropped % 512 == 1 {
                log::warn!(
                    "ingress rate-limit: дроп флуда от {:02x?}… (ch={}, всего дропнуто {})",
                    &src[..4], buf[0], dropped
                );
            }
            continue;
        }
        // Периодически чистим простаивающие бакеты (анти-рост памяти).
        pkt_count = pkt_count.wrapping_add(1);
        if pkt_count % 4096 == 0 {
            chat_rl.evict_idle(now);
            media_rl.evict_idle(now);
        }
        // Демукс по байту канала: медиа — лёгкий путь без Ack, но с шифрованием
        // сессии (SEC-4).
        if buf[0] == CHAN_MEDIA {
            // Звонка нет — и расшифровывать незачем.
            let Some(tx) = shared.media_tx.lock().unwrap().clone() else {
                media_orphan = media_orphan.wrapping_add(1);
                if media_orphan % 512 == 1 {
                    log::info!("p2p-media: пакет вне звонка, дроп (всего {media_orphan})");
                }
                continue;
            };
            let opened = shared.sessions.lock().unwrap().open_media(&buf[1..n], now);
            let frame = opened.as_ref().and_then(|(peer, pt)| parse_media(pt).map(|f| (*peer, f)));
            if let Some((peer, f)) = frame {
                {
                    // Отправитель — ЛИЧНОСТЬ, доказанная рукопожатием сессии, а не
                    // адрес: по ней движок решает, чей это голос, и сверяет её с
                    // участниками комнаты.
                    let err = tx.try_send(MediaPacket {
                        src: peer,
                        track: f.track,
                        seq: f.seq,
                        ts: f.ts,
                        payload: f.payload.to_vec(),
                    });
                    if err.is_err() {
                        media_full = media_full.wrapping_add(1);
                        if media_full % 512 == 1 {
                            log::warn!("p2p-media: очередь полна, дроп (track={}, всего {media_full})", f.track);
                        }
                    }
                }
            } else {
                media_bad = media_bad.wrapping_add(1);
                if media_bad % 512 == 1 {
                    log::warn!("p2p-media: не расшифровать или битый медиа-пакет (n={n}, всего {media_bad})");
                }
            }
            continue;
        }
        if buf[0] != CHAN_CHAT {
            continue;
        }
        let received = {
            // Новая сессия не должна появиться посреди пачки ретраев или досылки
            // (они идут под `send_order`): иначе голова пачки уходит по старой
            // сессии, которой у собеседника уже нет, хвост — по новой, и после
            // переотправки голова приходит позже хвоста (R-011).
            let _order = shared.send_order.lock().unwrap();
            shared.sessions.lock().unwrap().receive(&buf[1..n], now, &mut OsRng)
        };
        let (from, payload) = match received {
            Received::Data { peer, plaintext } => match Payload::from_padded(&plaintext) {
                Ok(p) => (peer, p),
                Err(_) => continue,
            },
            Received::Keepalive { peer } => {
                if shared.is_bound(&peer, &src) {
                    shared.peer_alive(&peer);
                }
                continue;
            }
            // Ответ на рукопожатие — туда, откуда пришёл msg1: кто там, узнаем
            // только из msg3.
            Received::Reply(m2) => {
                let _ = shared.write_chat(&src, vec![m2]);
                continue;
            }
            // Рукопожатие закончено: msg3, подтверждение и всё, что ждало
            // сессии, — по адресу, с которого собеседник только что ответил.
            Received::Established { peer, out } => {
                let _ = shared.write_chat(&src, out);
                shared.resend_pending(&peer);
                if shared.is_bound(&peer, &src) {
                    shared.peer_alive(&peer);
                }
                continue;
            }
            // Шлют по сессии, которой у нас нет (мы перезапустились). Если это
            // свой и сессии с ним правда нет — начинаем её сами, не дожидаясь,
            // пока он заметит молчание. Чужим не отвечаем ничем.
            Received::UnknownIndex => {
                let peer = shared.identity_of(&src);
                let ours = shared.targets.lock().unwrap().contains(&peer);
                if ours && !shared.sessions.lock().unwrap().has_session(&peer, now) {
                    shared.probe(&peer);
                }
                continue;
            }
            Received::Rejected => continue,
        };
        // Анти-спуфинг с учётом разделения личности и адреса (ADR-007).
        //
        // `from` — личность, доказанная рукопожатием сессии. Адрес (src) с ней
        // не обязан совпадать, поэтому проверяем ПРИВЯЗКУ. Принять пакет можно
        // в трёх случаях:
        //   * from == src — эпоха 0 или незнакомец;
        //   * src — известный нам адрес этой личности (связка подтверждена ранее
        //     принятым подписанным объявлением);
        //   * это само объявление о смене адреса, и его подпись связывает
        //     from → src (самоподтверждающийся случай, иначе новый адрес
        //     невозможно узнать: он приходит уже С нового адреса).
        let bound = shared.is_bound(&from, &src);
        let self_authenticating = matches!(
            &payload,
            Payload::TransportRotate { epoch, transport_pub, sig, .. }
                if transport_pub.as_slice() == src.as_slice()
                    && <[u8; 64]>::try_from(sig.as_slice())
                        .map(|s| p2p_core::identity::verify_transport_rotate(&from, *epoch, &src, &s))
                        .unwrap_or(false)
        );
        if !bound && !self_authenticating {
            continue;
        }
        // Дальше пир везде идентифицируется ЛИЧНОСТЬЮ (`from`), а не адресом:
        // контакты, история, бюджет и события завязаны на неё, и после смены
        // адреса эта связь обязана сохраниться.
        let peer_id = from;

        // Любой трафик от пира = он онлайн.
        shared.peer_alive(&peer_id);

        // Отсев повторов надёжных сообщений. Групповые (MLS) — мимо: у MLS свой
        // отсев по поколению, а ошибка нашего (лишний «повтор») стоила бы группе
        // рассинхрона. Повтор — это ТОТ ЖЕ id от того же пира: id выделяются заново
        // на каждое сообщение и переиспользуются только переотправкой его самого.
        let dedup = payload.reliable_id().filter(|_| !matches!(payload, Payload::Group { .. }));
        if let Some(id) = dedup {
            if !shared.first_delivery(&peer_id, id, now) {
                let _ = shared.send(&peer_id, &Payload::Ack { id });
                continue;
            }
        }

        match payload {
            Payload::Hello => {}
            Payload::Text { id, text, reply_to, reply_mine, fwd_from, fwd_anon } => {
                // Флаг «чьё сообщение» приходит с точки зрения ОТПРАВИТЕЛЯ — здесь он
                // переворачивается: его «моё» для нас «его», и наоборот.
                shared.emit(Event::Text {
                    peer: peer_id,
                    id,
                    text,
                    reply_to,
                    reply_mine: reply_to != 0 && !reply_mine,
                    fwd_from,
                    fwd_anon,
                });
                let _ = shared.send(&peer_id, &Payload::Ack { id }); // ✓✓ доставлено
            }
            Payload::Retract { id, target } => {
                // Отзыв касается сообщения ОТПРАВИТЕЛЯ — чужое удалить нельзя.
                shared.emit(Event::Retract { peer: peer_id, target });
                let _ = shared.send(&peer_id, &Payload::Ack { id });
            }
            Payload::Edit { id, target, text } => {
                // Правка касается сообщения ОТПРАВИТЕЛЯ: чужой текст переписать нельзя.
                shared.emit(Event::Edit { peer: peer_id, target, text });
                let _ = shared.send(&peer_id, &Payload::Ack { id });
            }
            Payload::Pin { id, target, target_mine, on } => {
                // «Чьё» приходит с точки зрения отправителя и переворачивается, как
                // у ответа: его «моё» для нас — его сообщение.
                shared.emit(Event::Pin { peer: peer_id, target, target_mine: !target_mine, on });
                let _ = shared.send(&peer_id, &Payload::Ack { id });
            }
            Payload::Media {
                id, hash, mime, name, size, duration_ms, width, height, caption, thumb, fwd_from,
                fwd_anon, reply_to, reply_mine,
            } => {
                shared.emit(Event::Media {
                    peer: peer_id, id, hash, mime, name, size, duration_ms, width, height,
                    caption: truncate_caption(&caption),
                    thumb,
                    fwd_from,
                    fwd_anon,
                    reply_to,
                    // Как у текста: «моё» отправителя — это «его» для нас.
                    reply_mine: !reply_mine,
                });
                let _ = shared.send(&peer_id, &Payload::Ack { id }); // ✓✓ доставлено
            }
            Payload::Ack { id } => {
                // Снимаем pending только если ack пришёл от того, КОМУ слали (анти-
                // спуфинг). Событие Ack в UI — лишь для DM (notify); группы молчат.
                let acked = {
                    let mut pending = shared.pending.lock().unwrap();
                    if pending.get(&id).map(|p| p.peer) == Some(peer_id) {
                        pending.remove(&id).map(|p| (p.notify, p.durable))
                    } else {
                        None
                    }
                };
                if let Some((notify, durable)) = acked {
                    if durable {
                        if let Some(ob) = shared.outbox.get() {
                            let _ = ob.remove_by_msg(&peer_id, id);
                        }
                        shared.pump(&peer_id); // место в окне освободилось
                    }
                    if notify {
                        shared.emit(Event::Ack { peer: peer_id, id });
                    }
                }
            }
            Payload::Ping { .. } => {
                // ADR-008: заявка о транзите (`transit`) в 1.0 не читается — её нечем
                // проверить, а непроверенной заявке цена ноль.
                // Отвечаем только своим: Pong кому угодно — готовый оракул «онлайн
                // ли этот ключ» для любого, кто знает наш адрес.
                let ours = shared.targets.lock().unwrap().contains(&peer_id);
                if ours {
                    let _ = shared.send(&peer_id, &Payload::Pong { transit: None });
                }
            }
            Payload::Pong { .. } => {}
            Payload::RoomInvite { id, room_id, audio, video } => {
                shared.emit(Event::RoomInvite { peer: peer_id, room_id, audio, video });
                let _ = shared.send(&peer_id, &Payload::Ack { id }); // гасим ретрай инвайта
            }
            Payload::RoomDecline { id, room_id } => {
                shared.emit(Event::RoomDecline { peer: peer_id, room_id });
                let _ = shared.send(&peer_id, &Payload::Ack { id });
            }
            Payload::Group { id, data } => {
                log::debug!("group recv id={} from={:02x?} len={}", id, &peer_id[..4], data.len());
                shared.emit(Event::Group { peer: peer_id, data });
                // ✓ доставлено — гасим повтор у отправителя (дубли отсеет MLS).
                let _ = shared.send(&peer_id, &Payload::Ack { id });
            }
            Payload::GroupKeyRequest { id, gid } => {
                shared.emit(Event::GroupKeyRequest { peer: peer_id, gid });
                let _ = shared.send(&peer_id, &Payload::Ack { id }); // гасим ретрай инвайта
            }
            Payload::GroupKeyReply { id, gid, key_package } => {
                shared.emit(Event::GroupKeyReply { peer: peer_id, gid, key_package });
                let _ = shared.send(&peer_id, &Payload::Ack { id });
            }
            Payload::GroupInvite { id, gid, name } => {
                shared.emit(Event::GroupInvite { peer: peer_id, gid, name });
                let _ = shared.send(&peer_id, &Payload::Ack { id }); // гасим ретрай приглашения
            }
            Payload::RoomPresence { gid, flags } => {
                shared.emit(Event::RoomPresence { peer: peer_id, gid, flags });
            }
            Payload::GroupResyncRequest { gid } => {
                shared.emit(Event::GroupResyncRequest { peer: peer_id, gid });
            }
            Payload::GroupLeaveRequest { gid } => {
                shared.emit(Event::GroupLeaveRequest { peer: peer_id, gid });
            }
            Payload::BlobManifest { manifest } => {
                shared.emit(Event::BlobManifest { peer: peer_id, manifest });
            }
            Payload::BlobManifestPart { id, mime, size, part, total, hashes } => {
                shared.emit(Event::BlobManifestPart { peer: peer_id, id, mime, size, part, total, hashes });
            }
            Payload::BlobRequest { id, index } => {
                shared.emit(Event::BlobRequest { peer: peer_id, id, index });
            }
            Payload::BlobChunk { id, index, data } => {
                shared.emit(Event::BlobChunk { peer: peer_id, id, index, data });
            }
            Payload::ProfileUpdate { name, avatar_hash } => {
                shared.emit(Event::ProfileUpdate { peer: peer_id, name, avatar_hash });
            }
            Payload::BlobManifestRequest { id } => {
                shared.emit(Event::BlobManifestRequest { peer: peer_id, id });
            }
            // Контакт сменил адрес (ADR-007). Подпись здесь уже проверена выше —
            // без этого пакет не прошёл бы анти-спуфинг, — но решение «принять и
            // запомнить» принимает верхний слой: только он знает, какая эпоха уже
            // известна, и он же перезапишет стор и карту адресов.
            Payload::TransportRotate { id, epoch, transport_pub, sig } => {
                shared.emit(Event::TransportRotate {
                    peer: peer_id,
                    epoch,
                    transport_pub,
                    sig,
                });
                let _ = shared.send(&peer_id, &Payload::Ack { id });
            }
            // Forward-compat (ADR-006): будущий тип, который этот клиент не знает —
            // молча игнорируем (Ack не шлём, отправитель погасит ретрай по TTL).
            Payload::Unknown => {}
        }
    }
}

fn maintenance_loop(shared: Arc<Shared>) {
    let tick = Duration::from_millis(500);
    loop {
        thread::sleep(tick);
        if !shared.running.load(Ordering::Relaxed) {
            return;
        }
        let now = Instant::now();

        // Таймеры рукопожатий сессий: повтор msg3 и новое рукопожатие для тех, кому
        // ждут данные. Первый пакет на холодной сессии иначе держался бы на том,
        // что его кто-нибудь пошлёт ещё раз.
        let handshakes = shared.sessions.lock().unwrap().tick(now, &mut OsRng);
        for (peer, dg) in handshakes {
            let _ = shared.write_chat(&shared.transport_of(&peer), vec![dg]);
        }

        // Presence-таймауты + расписание keepalive-ping.
        let mut went_offline = Vec::new();
        let mut to_ping = Vec::new();
        {
            let targets = shared.targets.lock().unwrap().clone();
            let mut peers = shared.peers.lock().unwrap();
            for t in &targets {
                peers.entry(*t).or_insert_with(|| PeerState::new(now));
            }
            for (key, st) in peers.iter_mut() {
                if st.online && now.duration_since(st.last_seen) > shared.cfg.offline_after {
                    st.online = false;
                    st.backoff = PROBE_MIN;
                    st.next_probe = now;
                    went_offline.push(*key);
                }
                // Чужих не пингуем: пир, приславший один пакет, не повод слать ему
                // keepalive до конца жизни процесса.
                if !targets.contains(key) {
                    continue;
                }
                if st.online {
                    if now.duration_since(st.last_ping) >= shared.cfg.ping_interval {
                        st.last_ping = now;
                        to_ping.push(*key);
                    }
                } else if now >= st.next_probe {
                    // Офлайн: проба с растущей паузой, а не непрерывный Hello.
                    st.last_ping = now;
                    st.next_probe = now + st.backoff;
                    st.backoff = (st.backoff * 2).min(PROBE_MAX);
                    to_ping.push(*key);
                }
            }
            // Ушедших в офлайн чужих не держим: их состояние больше никому не нужно.
            peers.retain(|k, st| st.online || targets.contains(k));
        }
        for key in went_offline {
            shared.emit(Event::Presence { peer: key, online: false });
        }
        for key in to_ping {
            shared.probe(&key);
        }

        // Повтор недоставленных сообщений; отсев по TTL.
        //
        // Повтор — для пира ЦЕЛИКОМ: подошёл срок хоть у одного сообщения — заново
        // уходят все неподтверждённые этому пиру, через `pump`, строго по id. Раньше
        // у каждого сообщения был свой таймер, сдвинутый на момент отправки; после
        // обрыва связи сообщения доходили в порядке «чей таймер сработал первым»
        // (живой прогон 2026-10-26: 2, 5, 1, 3, 4). Тот же `pump` подтягивает из
        // очереди и выпавшее из памяти по сроку, так что новое не обгоняет и его.
        let mut due: HashSet<PeerKey> = HashSet::new();
        let mut expired = Vec::new();
        {
            let mut pending = shared.pending.lock().unwrap();
            pending.retain(|id, p| {
                if now.duration_since(p.created) > p.ttl {
                    expired.push((*id, p.peer, p.notify));
                    return false;
                }
                if !p.owed && now.duration_since(p.last_sent) >= shared.cfg.retry_interval {
                    due.insert(p.peer);
                }
                true
            });
            for p in pending.values_mut().filter(|p| due.contains(&p.peer)) {
                p.owed = true;
            }
        }
        for (id, peer, notify) in expired {
            if notify {
                shared.emit(Event::Undelivered { peer, id });
            }
        }
        // Очередь, упёршаяся в темп досылки, — дотягиваем вместе с повторами.
        due.extend(shared.backlog.lock().unwrap().drain());
        for peer in due {
            shared.pump(&peer);
        }
    }
}

/// Loopback-транспорт: маршрутизирует датаграммы по ключу получателя в памяти.
/// Позволяет прогонять весь сетевой слой (доставка/инвайт/группы/resync) в одном
/// процессе без Yggdrasil и устройств. Публичный, чтобы им пользовались и
/// интеграционные тесты движка. Поддерживает инъекцию потерь (для проверки ретраев).
pub mod loopback {
    use super::*;
    use std::sync::atomic::AtomicU32;

    type Inbox = Sender<(PeerKey, Vec<u8>)>;

    /// Общая «сеть» в памяти. Клонируется дёшево (Arc внутри).
    #[derive(Clone, Default)]
    pub struct LoopbackNet {
        inboxes: Arc<Mutex<HashMap<PeerKey, Inbox>>>,
        /// Ронять в среднем каждую N-ю датаграмму (0 = без потерь) — для тестов ретрая.
        drop_every: Arc<AtomicU32>,
        counter: Arc<AtomicU32>,
        /// Обрыв связи: всё пропадает, но узлы и их сессии живы — как телефон,
        /// у которого выключили интернет.
        cut: Arc<AtomicBool>,
    }

    impl LoopbackNet {
        pub fn new() -> Self {
            Self::default()
        }

        /// Ронять в среднем каждую `n`-ю доставку (эмуляция потерь; 0 — выключить).
        /// Потери псевдослучайные, но воспроизводимые (по номеру датаграммы), а не
        /// строго периодические: строгое «каждый N-й» с рукопожатием из трёх пакетов
        /// даёт вырожденные узоры — при N = 2 msg2 вечно падает на чётный номер.
        pub fn set_drop_every(&self, n: u32) {
            self.drop_every.store(n, Ordering::Relaxed);
        }

        /// Оборвать (`true`) или вернуть связь для всех узлов сети.
        pub fn set_cut(&self, cut: bool) {
            self.cut.store(cut, Ordering::Relaxed);
        }

        /// Эндпоинт-транспорт для узла с ключом `key`.
        pub fn endpoint(&self, key: PeerKey) -> Arc<LoopbackTransport> {
            let (tx, rx) = mpsc::channel();
            self.inboxes.lock().unwrap().insert(key, tx);
            Arc::new(LoopbackTransport { key, net: self.clone(), rx: Mutex::new(rx) })
        }
    }

    pub struct LoopbackTransport {
        key: PeerKey,
        net: LoopbackNet,
        rx: Mutex<Receiver<(PeerKey, Vec<u8>)>>,
    }

    impl Transport for LoopbackTransport {
        fn write(&self, dst_pub: &PeerKey, data: &[u8]) -> Result<usize, YggError> {
            if self.net.cut.load(Ordering::Relaxed) {
                return Ok(data.len());
            }
            let drop_every = self.net.drop_every.load(Ordering::Relaxed);
            if drop_every > 0 {
                let n = self.net.counter.fetch_add(1, Ordering::Relaxed) as u64 + 1;
                // splitmix64 от номера: равномерно и детерминированно.
                let mut z = n.wrapping_mul(0x9e37_79b9_7f4a_7c15);
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                if (z ^ (z >> 31)) % u64::from(drop_every) == 0 {
                    return Ok(data.len()); // «потеряли» пакет
                }
            }
            if let Some(tx) = self.net.inboxes.lock().unwrap().get(dst_pub) {
                let _ = tx.send((self.key, data.to_vec()));
            }
            Ok(data.len())
        }
        fn read(&self, buf: &mut [u8]) -> Result<(usize, PeerKey), YggError> {
            let (src, data) = self.rx.lock().unwrap().recv().map_err(|_| YggError::OpFailed)?;
            let n = data.len().min(buf.len());
            buf[..n].copy_from_slice(&data[..n]);
            Ok((n, src))
        }
        fn address(&self) -> String {
            String::new()
        }
        fn peer_count(&self) -> i32 {
            self.net.inboxes.lock().unwrap().len() as i32
        }
        fn tree_count(&self) -> i32 {
            self.peer_count()
        }
        fn peers_json(&self) -> String {
            "[]".into()
        }
        fn add_peer(&self, _uri: &str) -> Result<(), YggError> {
            Ok(())
        }
        fn stop(&self) {
            // Снимаемся из сети: наш Sender в inboxes дропается → recv у нашего rx
            // вернёт Err → receive_loop завершится (чистая остановка для тестов рестарта).
            self.net.inboxes.lock().unwrap().remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::loopback::LoopbackNet;
    use super::*;
    use p2p_core::Identity;
    use std::time::Duration;

    fn id(n: u8) -> Identity {
        Identity::from_seed(&[n; 32]).unwrap()
    }

    /// Дождаться конкретного события до таймаута (фон-потоки доставляют асинхронно).
    fn wait_for(rx: &Receiver<Event>, pred: impl Fn(&Event) -> bool) -> Option<Event> {
        wait_long(rx, Duration::from_secs(2), pred)
    }

    /// Под потерями холодная сессия — это рукопожатие из трёх пакетов, и каждый
    /// может потеряться: при 50 % потерь на это уходят секунды, а не один ретрай.
    const LOSSY: Duration = Duration::from_secs(10);

    fn wait_long(rx: &Receiver<Event>, within: Duration, pred: impl Fn(&Event) -> bool) -> Option<Event> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(50)) {
                if pred(&ev) {
                    return Some(ev);
                }
            }
        }
        None
    }

    /// R-010 в мире сессий (ADR-012): адрес сам по себе не отвечает. Кто знает
    /// лишь адрес A, но не его личность, не получает ничего — msg1 без верного
    /// `mac1` встречает молчание, и нельзя узнать ни что A онлайн, ни чей это адрес.
    /// Кто знает личность, узнаёт, что A в сети, из самого рукопожатия (как раньше —
    /// из Ack на любое сообщение); без этого не дошёл бы и запрос в контакты.
    #[test]
    fn address_alone_gets_no_answer() {
        let net = LoopbackNet::new();
        let (a_id, b_id) = (id(41), id(42));
        let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
        let (_a, _a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), Config::default());
        let (b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), Config::default());

        // B думает, что по адресу A живёт кто-то другой (X): mac1 под X.
        let x_key = id(49).public_key_bytes();
        b.set_peer_transports(&[(x_key, a_key)]);
        b.poke(&x_key);
        let seen = wait_for(&b_rx, |e| matches!(e, Event::Presence { online: true, .. }));
        assert!(seen.is_none(), "по одному адресу A не ответил");

        b.poke(&a_key);
        let seen = wait_for(&b_rx, |e| matches!(e, Event::Presence { peer, online: true } if *peer == a_key));
        assert!(seen.is_some(), "знающему личность — рукопожатие");
    }

    /// R-022: офлайн-пира из набора пробуем с растущей паузой, а не непрерывно.
    #[test]
    fn offline_target_is_probed_with_backoff() {
        let net = LoopbackNet::new();
        let (a_id, x_id) = (id(43), id(44));
        let (a_key, x_key) = (a_id.public_key_bytes(), x_id.public_key_bytes());
        // X — только эндпоинт без мессенджера: считаем, сколько ему пришло.
        let x = net.endpoint(x_key);
        let got = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let g = got.clone();
        thread::spawn(move || {
            let mut buf = vec![0u8; 65536];
            while x.read(&mut buf).is_ok() {
                g.fetch_add(1, Ordering::Relaxed);
            }
        });
        let (a, _rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), Config::default());
        a.set_targets([x_key].into_iter().collect());
        thread::sleep(Duration::from_millis(9500));
        // Пробы в 0, 2, 6 с (пауза 2 → 4 → 8): три, а не ~60 (Hello каждые 150 мс).
        let n = got.load(Ordering::Relaxed);
        assert!((2..=4).contains(&n), "проб за 9,5 с: {n}");
    }

    /// Два мессенджера поверх loopback обмениваются текстом; приходит Ack (✓✓).
    #[test]
    fn text_delivered_and_acked_over_loopback() {
        let net = LoopbackNet::new();
        let (a_id, b_id) = (id(1), id(2));
        let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
        let (a, a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), Config::default());
        let (_b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), Config::default());

        let mid = a.send_text(&b_key, "привет").unwrap();
        // B получает текст…
        let got = wait_for(&b_rx, |e| matches!(e, Event::Text { text, .. } if text == "привет"));
        assert!(got.is_some(), "B должен получить текст");
        // …а A — подтверждение доставки.
        let ack = wait_for(&a_rx, |e| matches!(e, Event::Ack { id, .. } if *id == mid));
        assert!(ack.is_some(), "A должен получить Ack");
    }

    /// Надёжная доставка переживает потери: роняем каждый 2-й пакет — ретрай дожимает.
    #[test]
    fn reliable_delivery_survives_packet_loss() {
        let net = LoopbackNet::new();
        net.set_drop_every(2); // половина датаграмм теряется
        let (a_id, b_id) = (id(3), id(4));
        let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
        let cfg = Config { retry_interval: Duration::from_millis(50), ..Config::default() };
        let (a, _a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
        let (_b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);

        a.send_text(&b_key, "сквозь потери").unwrap();
        let got = wait_long(&b_rx, LOSSY, |e| matches!(e, Event::Text { text, .. } if text == "сквозь потери"));
        assert!(got.is_some(), "ретрай должен дожать сообщение несмотря на потери");
    }

    /// Инвайт-хендшейк надёжен: `GroupKeyRequest` доходит несмотря на потери — это
    /// чинит «инвайт не срабатывает на холодной сессии» (был fire-and-forget).
    #[test]
    fn group_key_request_survives_packet_loss() {
        let net = LoopbackNet::new();
        net.set_drop_every(2);
        let (a_id, b_id) = (id(5), id(6));
        let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
        let cfg = Config { retry_interval: Duration::from_millis(50), ..Config::default() };
        let (a, _a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
        let (_b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);

        a.send_group_key_request(&b_key, &[1, 2, 3]).unwrap();
        let got = wait_long(&b_rx, LOSSY, |e| matches!(e, Event::GroupKeyRequest { .. }));
        assert!(got.is_some(), "GroupKeyRequest должен дойти несмотря на потери (ретрай)");
    }

    /// Приглашение в звонок надёжно: `RoomInvite` доходит несмотря на потери —
    /// чинит «первый звонок не доходит» на холодной сессии.
    #[test]
    fn room_invite_survives_packet_loss() {
        let net = LoopbackNet::new();
        net.set_drop_every(2);
        let (a_id, b_id) = (id(7), id(8));
        let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
        let cfg = Config { retry_interval: Duration::from_millis(50), ..Config::default() };
        let (a, _a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
        let (_b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);

        a.send_room_invite(&b_key, &[9u8; 16], true, false).unwrap();
        let got = wait_long(&b_rx, LOSSY, |e| matches!(e, Event::RoomInvite { audio: true, .. }));
        assert!(got.is_some(), "RoomInvite должен дойти несмотря на потери (ретрай)");
    }

    /// R-007: приглашение живёт только время дозвона. Собеседник, появившийся
    /// позже, звонка не получает.
    #[test]
    fn room_invite_expires_after_ring_time() {
        let net = LoopbackNet::new();
        let (a_id, b_id) = (id(45), id(46));
        let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
        let cfg = Config {
            retry_interval: Duration::from_millis(50),
            ring_ttl: Duration::from_millis(600),
            ..Config::default()
        };
        let (a, _a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
        // B пока офлайн: его эндпоинта в сети нет.
        a.send_room_invite(&b_key, &[1u8; 16], true, false).unwrap();
        a.send_text(&b_key, "контроль").unwrap(); // обычное сообщение живёт дольше
        thread::sleep(Duration::from_millis(1600));
        let (_b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);
        let mut ring = false;
        let mut text = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !text {
            if let Ok(ev) = b_rx.recv_timeout(Duration::from_millis(50)) {
                ring |= matches!(ev, Event::RoomInvite { .. });
                text |= matches!(ev, Event::Text { .. });
            }
        }
        assert!(text, "обычное сообщение дошло");
        assert!(!ring, "просроченный звонок не зазвонил");
    }

    /// Повтор надёжного сообщения (тот же id от того же пира) доходит до движка
    /// один раз — иначе второй `RoomInvite` стал бы вторым звонком в истории. Но
    /// только повтор: другой id или другой отправитель — новое сообщение, а
    /// групповые (MLS) отсевает сам MLS.
    #[test]
    fn duplicate_reliable_payload_is_delivered_once() {
        let net = LoopbackNet::new();
        let (a_id, b_id, c_id) = (id(51), id(52), id(53));
        let (a_key, b_key, c_key) = (a_id.public_key_bytes(), b_id.public_key_bytes(), c_id.public_key_bytes());
        let (a, _a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), Config::default());
        let (_b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), Config::default());
        let (c, _c_rx) = Messenger::start_with_transport(c_id, net.endpoint(c_key), Config::default());
        let invite = |id: u64| Payload::RoomInvite { id, room_id: vec![7u8; 16], audio: true, video: false };
        a.shared.send(&b_key, &invite(77)).unwrap();
        a.shared.send(&b_key, &invite(77)).unwrap(); // повтор
        a.shared.send(&b_key, &invite(78)).unwrap(); // новое
        c.shared.send(&b_key, &invite(77)).unwrap(); // тот же id, другой отправитель
        a.shared.send(&b_key, &Payload::Group { id: 90, data: vec![1] }).unwrap();
        a.shared.send(&b_key, &Payload::Group { id: 90, data: vec![1] }).unwrap();
        let (mut invites, mut groups) = (Vec::new(), 0);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            match b_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Event::RoomInvite { peer, .. }) => invites.push(peer),
                Ok(Event::Group { .. }) => groups += 1,
                _ => {}
            }
        }
        invites.sort();
        let mut want = vec![a_key, a_key, c_key];
        want.sort();
        assert_eq!(invites, want, "повтор отсеян, новое и чужое — нет");
        assert_eq!(groups, 2, "групповые не отсеиваем — это дело MLS");
    }

    /// Живой прогон 2026-10-26: у получателя пропал интернет, отправитель набрал пять
    /// сообщений с паузами, связь вернулась — пришло 2, 5, 1, 3, 4. Сессия при этом
    /// жива, и повторы шли по таймеру каждого сообщения, сдвинутому на момент его
    /// отправки. Теперь повтор — пачкой по пиру, строго по id.
    #[test]
    fn messages_queued_during_outage_arrive_in_order() {
        let net = LoopbackNet::new();
        let (a_id, b_id) = (id(61), id(62));
        let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
        // Интервал повтора длиннее тика обслуживания (500 мс), как в жизни (3 с):
        // иначе каждый тик повторяет всё разом и баг не виден.
        let cfg = Config { retry_interval: Duration::from_millis(2000), ..Config::default() };
        let (a, _a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
        let (_b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);
        a.send_text(&b_key, "до обрыва").unwrap();
        assert!(wait_for(&b_rx, |e| matches!(e, Event::Text { .. })).is_some(), "сессия поднялась");

        net.set_cut(true);
        for i in 1..=5 {
            a.send_text(&b_key, &i.to_string()).unwrap();
            // Паузы короче интервала повтора и длиннее тика: таймеры сообщений
            // расходятся по фазе, как у человека, набирающего текст.
            std::thread::sleep(Duration::from_millis(600));
        }
        std::thread::sleep(Duration::from_millis(700));
        net.set_cut(false);

        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(8);
        while got.len() < 5 && Instant::now() < deadline {
            if let Ok(Event::Text { text, .. }) = b_rx.recv_timeout(Duration::from_millis(50)) {
                got.push(text);
            }
        }
        assert_eq!(got, ["1", "2", "3", "4", "5"], "после обрыва — в порядке отправки");
    }

    /// R-007: положили трубку до того, как собеседник появился, — снятое
    /// приглашение не доставляется, а неснятое доходит.
    #[test]
    fn cancelled_room_invite_is_not_delivered() {
        let net = LoopbackNet::new();
        let (a_id, b_id) = (id(47), id(48));
        let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
        let cfg = Config { retry_interval: Duration::from_millis(50), ..Config::default() };
        let (a, _a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
        let dropped = a.send_room_invite(&b_key, &[1u8; 16], true, false).unwrap();
        a.send_room_invite(&b_key, &[2u8; 16], true, false).unwrap();
        a.cancel(dropped);
        let (_b, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);
        let mut rooms = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(Event::RoomInvite { room_id, .. }) = b_rx.recv_timeout(Duration::from_millis(50)) {
                rooms.push(room_id);
            }
        }
        assert_eq!(rooms, vec![vec![2u8; 16]], "дошло только неснятое");
    }
}
