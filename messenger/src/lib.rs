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

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::media::{
    parse_media, write_media_header, CHAN_CHAT, CHAN_MEDIA, MAX_MEDIA_PAYLOAD, MEDIA_HEADER_LEN,
};
use p2p_core::protocol::{decode, encode, Payload, ProtocolError};
use p2p_core::ratelimit::RateLimiter;
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
    fn stop(&self) {
        Node::stop(self)
    }
}

/// События, доставляемые приложению из фоновых потоков.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Изменение онлайн-статуса пира (первый контакт — `online: true`).
    Presence { peer: PeerKey, online: bool },
    /// Входящее текстовое сообщение.
    Text { peer: PeerKey, id: u64, text: String },
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
    /// Контакт просит наш KeyPackage (хочет добавить в группу).
    GroupKeyRequest { peer: PeerKey },
    /// Контакт прислал свой KeyPackage (для добавления в группу).
    GroupKeyReply { peer: PeerKey, key_package: Vec<u8> },
    /// Маячок присутствия участника в комнате-звонке (Discord-style).
    RoomPresence { peer: PeerKey, gid: Vec<u8>, flags: u8 },
    /// Отставший участник просит админа заново синхронизировать его в группе (ADR-004).
    GroupResyncRequest { peer: PeerKey, gid: Vec<u8> },
    /// Участник просит админа отозвать его — выход из группы (ADR-004).
    GroupLeaveRequest { peer: PeerKey, gid: Vec<u8> },
    /// Анонс блоба: пришёл манифест (JSON) — можно начинать докачку (M2).
    BlobManifest { peer: PeerKey, manifest: String },
    /// Пир запросил чанк нашего блоба.
    BlobRequest { peer: PeerKey, id: Vec<u8>, index: u32 },
    /// Пришёл чанк блоба, который мы качаем.
    BlobChunk { peer: PeerKey, id: Vec<u8>, index: u32, data: Vec<u8> },
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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ping_interval: Duration::from_secs(15),
            offline_after: Duration::from_secs(45),
            retry_interval: Duration::from_secs(3),
            message_ttl: Duration::from_secs(300),
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
}

struct PeerState {
    online: bool,
    last_seen: Instant,
    last_ping: Instant,
}

struct Pending {
    peer: PeerKey,
    payload: Payload,
    created: Instant,
    last_sent: Instant,
    /// Уведомлять ли UI о доставке/недоставке (`Ack`/`Undelivered`). true для DM,
    /// false для групповых (статус по участникам в UI не показывается).
    notify: bool,
}

struct Shared {
    identity: Identity,
    node: Arc<dyn Transport>,
    cfg: Config,
    peers: Mutex<HashMap<PeerKey, PeerState>>,
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    running: AtomicBool,
    tx: Sender<Event>,
    media_tx: Mutex<Option<SyncSender<MediaPacket>>>,
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

    fn send(&self, peer: &PeerKey, payload: &Payload) -> Result<(), MessengerError> {
        let body = encode(&self.identity, peer, payload, Self::now_unix())?;
        // Префикс канала чата (демукс с медиа на приёме).
        let mut dg = Vec::with_capacity(1 + body.len());
        dg.push(CHAN_CHAT);
        dg.extend_from_slice(&body);
        self.node.write(peer, &dg)?;
        Ok(())
    }

    /// Зарегистрировать пир как отслеживаемый (для keepalive), если ещё нет.
    fn track(&self, peer: &PeerKey) {
        let now = Instant::now();
        self.peers.lock().unwrap().entry(*peer).or_insert(PeerState {
            online: false,
            last_seen: now,
            last_ping: now,
        });
    }

    /// ЕДИНЫЙ примитив надёжной отправки: выделяет id, шлёт и кладёт в `pending` —
    /// `maintenance_loop` повторяет до `Ack{id}` или TTL. `mk` строит payload с этим
    /// id (его получатель эхо-нёт в Ack). `notify` — уведомлять ли UI о доставке
    /// (DM: true → Ack/Undelivered-события; группы: false — молча).
    /// Чтобы сделать надёжным новый тип сообщения: добавь ему поле `id`, шли через
    /// этот метод и ответь `Ack{id}` на приёме. Возвращает id.
    fn send_reliable(
        &self,
        peer: &PeerKey,
        notify: bool,
        mk: impl FnOnce(u64) -> Payload,
    ) -> Result<u64, MessengerError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = mk(id);
        self.track(peer);
        self.send(peer, &payload)?;
        let now = Instant::now();
        self.pending.lock().unwrap().insert(
            id,
            Pending { peer: *peer, payload, created: now, last_sent: now, notify },
        );
        Ok(id)
    }

    /// Как [`send_reliable`], но с ЗАДАННЫМ `id` (для пере-отправки из офлайн-очереди:
    /// тот же id → получатель дедуплицирует, дублей нет). Пере-вставка в `pending` с
    /// тем же id идемпотентна.
    fn send_reliable_with_id(
        &self,
        peer: &PeerKey,
        id: u64,
        notify: bool,
        mk: impl FnOnce(u64) -> Payload,
    ) -> Result<(), MessengerError> {
        let payload = mk(id);
        self.track(peer);
        self.send(peer, &payload)?;
        let now = Instant::now();
        self.pending.lock().unwrap().insert(
            id,
            Pending { peer: *peer, payload, created: now, last_sent: now, notify },
        );
        Ok(())
    }

    fn is_online(&self, peer: &PeerKey) -> bool {
        self.peers
            .lock()
            .unwrap()
            .get(peer)
            .map(|s| s.online)
            .unwrap_or(false)
    }

    /// Отметить приход трафика от пира; вернуть true, если пир ПЕРЕШЁЛ в онлайн.
    fn mark_seen(&self, peer: &PeerKey) -> bool {
        let now = Instant::now();
        let mut peers = self.peers.lock().unwrap();
        let st = peers.entry(*peer).or_insert(PeerState {
            online: false,
            last_seen: now,
            last_ping: now,
        });
        st.last_seen = now;
        if !st.online {
            st.online = true;
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
            identity,
            node,
            cfg,
            peers: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            running: AtomicBool::new(true),
            tx,
            media_tx: Mutex::new(None),
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
    pub fn public_key(&self) -> PeerKey {
        self.shared.identity.public_key_bytes()
    }

    /// Адрес Yggdrasil нашего узла.
    pub fn address(&self) -> String {
        self.shared.node.address()
    }

    /// Онлайн ли пир (по последнему трафику / keepalive).
    pub fn is_online(&self, peer: &PeerKey) -> bool {
        self.shared.is_online(peer)
    }

    /// Число прямых пиров транспорта (для индикатора сети).
    pub fn net_peers(&self) -> i32 {
        self.shared.node.peer_count()
    }

    /// Число узлов в дереве маршрутизации транспорта.
    pub fn net_tree(&self) -> i32 {
        self.shared.node.tree_count()
    }

    /// JSON-массив пиров транспорта (адрес/направление/пинг/трафик) для стата сети.
    pub fn net_peers_json(&self) -> String {
        self.shared.node.peers_json()
    }

    /// Повторно добавить пир по URI: обходит внутренний backoff Yggdrasil и
    /// форсирует немедленную попытку реконнекта (например, после флика WiFi).
    pub fn add_peer(&self, uri: &str) {
        let _ = self.shared.node.add_peer(uri);
    }

    /// Остановить мессенджер: останавливает транспортный узел и фоновые потоки.
    pub fn shutdown(&self) {
        self.shared.running.store(false, Ordering::Relaxed);
        self.shared.node.stop();
    }

    /// Установить сессию: шлём `Hello`, пока пир не станет онлайн (двунаправленный
    /// обмен поднимает шифр-сессию ironwood). После — пир под keepalive.
    pub fn connect(&self, peer: &PeerKey, timeout: Duration) -> Result<(), MessengerError> {
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
        let text = text.to_string();
        // DM: notify=true → UI получает Ack/Undelivered (✓✓ / ошибка).
        self.shared.send_reliable(peer, true, move |id| Payload::Text { id, text })
    }

    /// Пере-отправить текст с ТЕМ ЖЕ `id` (офлайн-досылка): получатель дедуплицирует
    /// по (peer, id), поэтому дублей нет. Используется [`p2p_core`]-очередью через движок.
    pub fn resend_text(&self, peer: &PeerKey, id: u64, text: &str) -> Result<(), MessengerError> {
        let text = text.to_string();
        self.shared.send_reliable_with_id(peer, id, true, move |id| Payload::Text { id, text })
    }

    /// Отправить медиа-датаграмму (реалтайм-канал): без Ack/повторов/шифрования
    /// приложения — конфиденциальность даёт E2E-шифрование транспорта Yggdrasil.
    /// Собирается в стековый буфер без аллокаций кучи.
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
        let mut buf = vec![0u8; total];
        write_media_header(&mut buf, track, seq, ts);
        buf[MEDIA_HEADER_LEN..total].copy_from_slice(payload);
        if let Err(e) = self.shared.node.write(peer, &buf) {
            log::warn!("p2p-media: node.write failed for track={} len={}: {:?}", track, total, e);
            return Err(MessengerError::Transport(e));
        }
        Ok(())
    }

    /// Пригласить пира в голосовую комнату (DM-звонок).
    pub fn send_room_invite(
        &self,
        peer: &PeerKey,
        room_id: &[u8],
        audio: bool,
        video: bool,
    ) -> Result<(), MessengerError> {
        // Надёжно (retry-until-Ack): чинит потерю первого пакета на холодной сессии.
        let room_id = room_id.to_vec();
        self.shared
            .send_reliable(peer, false, move |id| Payload::RoomInvite { id, room_id, audio, video })?;
        Ok(())
    }

    /// Отклонить входящий звонок — надёжно (звонящий гарантированно узнаёт отказ).
    pub fn send_room_decline(&self, peer: &PeerKey, room_id: &[u8]) -> Result<(), MessengerError> {
        let room_id = room_id.to_vec();
        self.shared
            .send_reliable(peer, false, move |id| Payload::RoomDecline { id, room_id })?;
        Ok(())
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
        self.shared.send_reliable(peer, false, move |id| Payload::Group { id, data })?;
        Ok(())
    }

    /// Попросить у контакта KeyPackage (для добавления в группу) — НАДЁЖНО
    /// (retry-until-Ack): на холодной сессии первый пакет мог теряться, из-за чего
    /// инвайт «не срабатывал». Дедуп повторов на приёме — по идемпотентности
    /// (повторный запрос лишь пере-присылает KeyPackage).
    pub fn send_group_key_request(&self, peer: &PeerKey) -> Result<(), MessengerError> {
        self.shared.send_reliable(peer, false, |id| Payload::GroupKeyRequest { id })?;
        Ok(())
    }

    /// Прислать контакту свой KeyPackage — надёжно (retry-until-Ack).
    pub fn send_group_key_reply(&self, peer: &PeerKey, key_package: &[u8]) -> Result<(), MessengerError> {
        let kp = key_package.to_vec();
        self.shared
            .send_reliable(peer, false, move |id| Payload::GroupKeyReply { id, key_package: kp })?;
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

fn receive_loop(shared: Arc<Shared>) {
    let mut buf = vec![0u8; 65535];
    // M3: анти-replay для надёжного канала — отпечатки недавних датаграмм.
    // Каждое легитимное сообщение содержит случайный nonce, поэтому повтор = точная
    // копия байт → совпадение отпечатка. Кольцо ограничено.
    let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut seen_order: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
    const SEEN_CAP: usize = 4096;

    // Приёмный rate-limit (M1, анти-abuse): per-source token-bucket, навязывается
    // ПРИНИМАЮЩИМ (модификация отправителя не обходит). Применяется по src-ключу
    // Yggdrasil ДО расшифровки — флуд дропается до дорогого крипто. Каналы разные:
    //  · control/сообщения (CHAN_CHAT) — строгий бюджет (спам-вектор);
    //  · медиа (CHAN_MEDIA) — щедрый (звонок легитимно высокочастотен: ~50 ауд/с +
    //    видео), но абсурдный флуд (>~600/с устойчиво) всё равно режется.
    // Градуировка по доверию (Sybil-устойчивость) + репутация — M3.
    let mut chat_rl = RateLimiter::new(60.0, 30.0);
    let mut media_rl = RateLimiter::new(1200.0, 600.0);
    let mut pkt_count: u64 = 0;
    let mut dropped: u64 = 0;
    loop {
        let (n, src) = match shared.node.read(&mut buf) {
            Ok(v) => v,
            Err(_) => return, // узел остановлен
        };
        if n == 0 {
            continue;
        }
        // Ingress rate-limit по источнику (до расшифровки). Превышение → дроп.
        let now = Instant::now();
        let allowed = if buf[0] == CHAN_MEDIA {
            media_rl.allow(&src, now)
        } else {
            chat_rl.allow(&src, now)
        };
        if !allowed {
            dropped = dropped.wrapping_add(1);
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
        // Демукс по байту канала: медиа — лёгкий путь без крипто/Ack.
        if buf[0] == CHAN_MEDIA {
            if let Some(f) = parse_media(&buf[..n]) {
                if let Some(tx) = shared.media_tx.lock().unwrap().as_ref() {
                    let err = tx.try_send(MediaPacket {
                        src,
                        track: f.track,
                        seq: f.seq,
                        ts: f.ts,
                        payload: f.payload.to_vec(),
                    });
                    if err.is_err() {
                        log::error!("p2p-media: channel full, dropped packet track={}", f.track);
                    }
                } else {
                    log::warn!("p2p-media: received packet but media_tx is None (track={})", f.track);
                }
            } else {
                log::warn!("p2p-media: parse_media failed for n={}", n);
            }
            continue;
        }
        if buf[0] != CHAN_CHAT {
            continue;
        }
        // Анти-replay: отбрасываем точные повторы датаграмм надёжного канала.
        {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            buf[1..n].hash(&mut h);
            let fp = h.finish();
            if !seen.insert(fp) {
                continue; // уже видели — повтор
            }
            seen_order.push_back(fp);
            if seen_order.len() > SEEN_CAP {
                if let Some(old) = seen_order.pop_front() {
                    seen.remove(&old);
                }
            }
        }
        let (from, payload) = match decode(&shared.identity, &buf[1..n]) {
            Ok(v) => v,
            Err(_) => continue, // не наш конверт / не расшифровать
        };
        // Анти-спуфинг: ключ в конверте обязан совпадать с источником транспорта.
        if from != src {
            continue;
        }

        // Любой трафик от пира = он онлайн.
        if shared.mark_seen(&src) {
            shared.emit(Event::Presence { peer: src, online: true });
        }

        match payload {
            Payload::Hello => {}
            Payload::Text { id, text } => {
                shared.emit(Event::Text { peer: src, id, text });
                let _ = shared.send(&src, &Payload::Ack { id }); // ✓✓ доставлено
            }
            Payload::Ack { id } => {
                // Снимаем pending только если ack пришёл от того, КОМУ слали (анти-
                // спуфинг). Событие Ack в UI — лишь для DM (notify); группы молчат.
                let acked = {
                    let mut pending = shared.pending.lock().unwrap();
                    if pending.get(&id).map(|p| p.peer) == Some(src) {
                        pending.remove(&id).map(|p| p.notify)
                    } else {
                        None
                    }
                };
                if acked == Some(true) {
                    shared.emit(Event::Ack { peer: src, id });
                }
            }
            Payload::Ping => {
                let _ = shared.send(&src, &Payload::Pong);
            }
            Payload::Pong => {}
            Payload::RoomInvite { id, room_id, audio, video } => {
                shared.emit(Event::RoomInvite { peer: src, room_id, audio, video });
                let _ = shared.send(&src, &Payload::Ack { id }); // гасим ретрай инвайта
            }
            Payload::RoomDecline { id, room_id } => {
                shared.emit(Event::RoomDecline { peer: src, room_id });
                let _ = shared.send(&src, &Payload::Ack { id });
            }
            Payload::Group { id, data } => {
                log::debug!("group recv id={} from={:02x?} len={}", id, &src[..4], data.len());
                shared.emit(Event::Group { peer: src, data });
                // ✓ доставлено — гасим повтор у отправителя (дубли отсеет MLS).
                let _ = shared.send(&src, &Payload::Ack { id });
            }
            Payload::GroupKeyRequest { id } => {
                shared.emit(Event::GroupKeyRequest { peer: src });
                let _ = shared.send(&src, &Payload::Ack { id }); // гасим ретрай инвайта
            }
            Payload::GroupKeyReply { id, key_package } => {
                shared.emit(Event::GroupKeyReply { peer: src, key_package });
                let _ = shared.send(&src, &Payload::Ack { id });
            }
            Payload::RoomPresence { gid, flags } => {
                shared.emit(Event::RoomPresence { peer: src, gid, flags });
            }
            Payload::GroupResyncRequest { gid } => {
                shared.emit(Event::GroupResyncRequest { peer: src, gid });
            }
            Payload::GroupLeaveRequest { gid } => {
                shared.emit(Event::GroupLeaveRequest { peer: src, gid });
            }
            Payload::BlobManifest { manifest } => {
                shared.emit(Event::BlobManifest { peer: src, manifest });
            }
            Payload::BlobRequest { id, index } => {
                shared.emit(Event::BlobRequest { peer: src, id, index });
            }
            Payload::BlobChunk { id, index, data } => {
                shared.emit(Event::BlobChunk { peer: src, id, index, data });
            }
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

        // Presence-таймауты + расписание keepalive-ping.
        let mut went_offline = Vec::new();
        let mut to_ping = Vec::new();
        {
            let mut peers = shared.peers.lock().unwrap();
            for (key, st) in peers.iter_mut() {
                if st.online && now.duration_since(st.last_seen) > shared.cfg.offline_after {
                    st.online = false;
                    went_offline.push(*key);
                }
                if now.duration_since(st.last_ping) >= shared.cfg.ping_interval {
                    st.last_ping = now;
                    to_ping.push(*key);
                }
            }
        }
        for key in went_offline {
            shared.emit(Event::Presence { peer: key, online: false });
        }
        for key in to_ping {
            let _ = shared.send(&key, &Payload::Ping);
        }

        // Повтор недоставленных сообщений; отсев по TTL.
        let mut to_retry = Vec::new();
        let mut expired = Vec::new();
        {
            let mut pending = shared.pending.lock().unwrap();
            pending.retain(|id, p| {
                if now.duration_since(p.created) > shared.cfg.message_ttl {
                    expired.push((*id, p.peer, p.notify));
                    return false;
                }
                if now.duration_since(p.last_sent) >= shared.cfg.retry_interval {
                    p.last_sent = now;
                    to_retry.push((p.peer, p.payload.clone()));
                }
                true
            });
        }
        for (id, peer, notify) in expired {
            if notify {
                shared.emit(Event::Undelivered { peer, id });
            }
        }
        for (peer, payload) in to_retry {
            let _ = shared.send(&peer, &payload);
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
        /// Ронять каждую N-ю датаграмму (0 = без потерь) — для тестов ретрая.
        drop_every: Arc<AtomicU32>,
        counter: Arc<AtomicU32>,
    }

    impl LoopbackNet {
        pub fn new() -> Self {
            Self::default()
        }

        /// Ронять каждую `n`-ю доставку (эмуляция потерь; 0 — выключить).
        pub fn set_drop_every(&self, n: u32) {
            self.drop_every.store(n, Ordering::Relaxed);
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
            let drop_every = self.net.drop_every.load(Ordering::Relaxed);
            if drop_every > 0 {
                let n = self.net.counter.fetch_add(1, Ordering::Relaxed) + 1;
                if n % drop_every == 0 {
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
        fn stop(&self) {}
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
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(50)) {
                if pred(&ev) {
                    return Some(ev);
                }
            }
        }
        None
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
        let got = wait_for(&b_rx, |e| matches!(e, Event::Text { text, .. } if text == "сквозь потери"));
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

        a.send_group_key_request(&b_key).unwrap();
        let got = wait_for(&b_rx, |e| matches!(e, Event::GroupKeyRequest { .. }));
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
        let got = wait_for(&b_rx, |e| matches!(e, Event::RoomInvite { audio: true, .. }));
        assert!(got.is_some(), "RoomInvite должен дойти несмотря на потери (ретрай)");
    }
}
