//! Движок мессенджера — device-independent ядро (ADR-003). Владеет состоянием
//! сессии (контакты, сообщения, группы, комнаты-звонки), исполняет команды и
//! отдаёт события. НЕ зависит от JNI/Android — `androidjni` лишь маршалит вызовы.
//! Десктоп может использовать тот же крейт.
//!
//! Контракт: клиент шлёт команды (методы `Engine`) и читает снапшоты; движок
//! асинхронно копит события (`poll_event`). Домейн-логику (агрегаты, презенс,
//! маршрутизацию) считает движок, не клиент. Поэтому ядро юнит-тестируется без
//! устройства (см. `net.rs`, тесты презенса ниже).

pub mod backup;
pub mod blobs;
pub mod dto;
pub mod groups;
pub mod net;
pub mod receipts;
pub mod room_call;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::budget::Tier;
pub use peerage_core::mention::Mention;
use peerage_core::settings::AudioSettings;
use peerage_core::store::{
    Chat, ContactStore, FirstMessage, GateStore, MessageStore, Profile, StoredMessage,
};
use peerage_core::{vault, Identity};
/// Реэкспорт: медиа-вложение группового сообщения — часть контракта движка
/// (клиенты и тесты собирают его, чтобы отправить вложение в группу).
pub use peerage_groupchat::MediaBody;
use peerage_messenger::{Event, Messenger};
use peerage_ygg::Node;

use groups::GroupManager;
use net::{GroupPresence, NetStats, PeerStat};
use room_call::RoomCallEngine;

/// Отказоустойчивый захват мьютекса: если другой поток паниковал держа его,
/// `.unwrap()` тоже паникует → cascade crash. Забираем внутреннее значение,
/// игнорируя poison — данные остаются консистентными.
pub trait LockRecover<T> {
    fn lock_safe(&self) -> std::sync::MutexGuard<'_, T>;
}
impl<T> LockRecover<T> for Mutex<T> {
    fn lock_safe(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Команда группового актора (исполняется в потоке event_pump, т.к. GroupChat !Send).
/// Все команды — fire-and-forget. Read-данные UI читает из кэша [`Engine::group_cache`].
pub(crate) enum GroupCmd {
    Create {
        name: String,
    },
    Invite {
        gid: Vec<u8>,
        peer: [u8; 32],
    },
    /// Принять входящее приглашение в группу (карточка).
    AcceptInvite {
        gid: Vec<u8>,
    },
    /// Отклонить входящее приглашение.
    DeclineInvite {
        gid: Vec<u8>,
    },
    SendText {
        gid: Vec<u8>,
        text: String,
        reply_to: u64,
        reply_sender: String,
        /// Пересылка: hex автора исходного сообщения (пусто — своё).
        fwd_from: String,
        /// «Переслано, автора не называем».
        fwd_anon: bool,
        /// Упоминания поверх участков `text` (peerage_core::mention).
        mentions: Vec<Mention>,
    },
    /// Отозвать свои сообщения у участников («удалить у всех»).
    Retract {
        gid: Vec<u8>,
        ids: Vec<u64>,
    },
    /// Исправить своё сообщение в группе.
    Edit {
        gid: Vec<u8>,
        id: u64,
        text: String,
        mentions: Vec<Mention>,
    },
    /// Закрепить/открепить сообщение в группе (только админ).
    Pin {
        gid: Vec<u8>,
        id: u64,
        sender: String,
        on: bool,
    },
    /// Своя реакция на сообщение группы (пустой `emoji` — снять).
    React {
        gid: Vec<u8>,
        id: u64,
        sender: String,
        emoji: String,
        at: i64,
    },
    /// Удалить сообщения только у себя: пары `(id, автор-hex)`.
    DeleteLocal {
        gid: Vec<u8>,
        refs: Vec<(u64, String)>,
    },
    /// Пир появился в сети — проверить незавершённые вступления в группы.
    PeerOnline {
        peer: [u8; 32],
    },
    /// Отозвать участника (ADR-004, только админ).
    RemoveMember {
        gid: Vec<u8>,
        peer: [u8; 32],
    },
    /// Ротация ключа группы (PCS, ADR-004).
    Rekey {
        gid: Vec<u8>,
    },
    /// Выйти из группы (ADR-004).
    Leave {
        gid: Vec<u8>,
    },
}

/// Работа для потока event_pump: событие мессенджера, групповая команда либо
/// рассылка маячка присутствия.
pub(crate) enum Job {
    Ev(Event),
    Cmd(GroupCmd),
    Beacon,
    /// Рассылка «я вышел» членам комнаты `gid` (мгновенное исчезновение).
    Leave(Vec<u8>),
    /// Отправить вложение в группу: подготовка (см. [`Engine::store_media`]) идёт в
    /// своём потоке, готовое возвращается сюда как `GroupMediaRef`.
    GroupMedia {
        gid: Vec<u8>,
        source: MediaSource,
        mime: String,
        name: String,
        info: MediaInfo,
    },
    /// Ссылка на блоб, который уже у нас (пересылка): байты не трогаем.
    GroupMediaRef {
        gid: Vec<u8>,
        media: MediaBody,
    },
    /// Бросить идущие докачки (очистка вложений); ответ — когда брошены.
    CancelDownloads(Sender<()>),
}

/// До этого размера вложение скачивается автоматически (фото, голосовое — их ждут
/// сразу). Крупнее — сначала карточка с именем/размером/превью, байты по кнопке:
/// решение «тянуть ли 800 МБ» принимает получатель, а не отправитель.
const AUTO_DOWNLOAD_LIMIT: u64 = 2 * 1024 * 1024;

/// На сколько поднимаем ingress-бюджет пира после явного «скачать». Продлевается
/// каждым новым запросом, поэтому длинная докачка не обрывается на середине.
const BLOB_BOOST_FOR: Duration = Duration::from_secs(120);

/// Сколько ждём манифест на свой запрос. Дольше — ответ уже не наш: повторный
/// запрос (докачка по кнопке, возвращение пира) заведёт ожидание заново.
const BLOB_EXPECT_TTL: Duration = Duration::from_secs(600);

/// Откуда взять байты своего вложения.
pub(crate) enum MediaSource {
    /// Уже в памяти (картинка, голосовое).
    Bytes(Vec<u8>),
    /// Файлом на диске: в память не поднимается.
    File(PathBuf),
}

/// Метаданные вложения, известные отправителю (их извлекает клиент: на Android —
/// `MediaMetadataRetriever`). Нужны, чтобы карточка была информативной до скачивания.
#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    pub duration_ms: u32,
    pub width: u32,
    pub height: u32,
    /// Байты миниатюры (JPEG). Уйдёт отдельным мелким блобом.
    pub thumb: Option<Vec<u8>>,
    /// Подпись к вложению. Едет ВНУТРИ медиа-сообщения: фото с подписью не должно
    /// превращаться в пару пакетов, по которой наблюдатель узнаёт «отправили фото».
    pub caption: String,
    /// Пересылка: hex автора ИСХОДНОГО сообщения (пусто — своё вложение).
    pub fwd_from: String,
    /// «Переслано, автора не называем».
    pub fwd_anon: bool,
    /// Ответ вложением: id цитируемого сообщения (0 — обычное).
    pub reply_to: u64,
    /// hex автора цитируемого сообщения (пусто — обычное). Ссылка «автор + id»
    /// (ADR-006): на проводе едет флаг «моё/не моё», здесь — явный автор.
    pub reply_sender: String,
}

/// Сколько сообщений отдаётся клиенту за один заход в чат. Переписка бывает
/// в сотни тысяч строк: поднимать её целиком в память (да ещё гнать через JNI
/// одним JSON) нельзя. Остальное клиент дозапрашивает страницами вверх.
pub const HISTORY_PAGE: usize = 200;

/// Чат как адресат операции: личный (ключ собеседника) или группа (gid). Владеющая
/// версия [`Chat`] — её удобно собирать из строк, пришедших через FFI.
#[derive(Debug, Clone)]
pub enum ChatKey {
    Dm([u8; 32]),
    Group(Vec<u8>),
}

impl ChatKey {
    /// Разобрать пару «hex + это группа?» из клиента.
    pub fn parse(id_hex: &str, group: bool) -> Option<ChatKey> {
        if group {
            hex::decode(id_hex.trim())
                .ok()
                .filter(|v| !v.is_empty())
                .map(ChatKey::Group)
        } else {
            parse_key(id_hex).map(ChatKey::Dm)
        }
    }

    /// Идентификатор чата байтами (для событий наружу).
    pub(crate) fn id_bytes(&self) -> &[u8] {
        match self {
            ChatKey::Dm(k) => k.as_slice(),
            ChatKey::Group(g) => g,
        }
    }

    pub(crate) fn as_chat(&self) -> Chat<'_> {
        match self {
            ChatKey::Dm(k) => Chat::Dm(k),
            ChatKey::Group(g) => Chat::Group(g),
        }
    }
}

/// Страница поиска в чате: всего совпадений и находки этой страницы.
#[derive(serde::Serialize)]
struct SearchPageJson {
    total: usize,
    hits: Vec<SearchHit>,
}

/// Находка поиска: сама строка истории плюс чат, в котором она лежит.
#[derive(serde::Serialize)]
struct SearchHit {
    chat: String,
    group: bool,
    #[serde(flatten)]
    msg: StoredMessage,
}

/// Строка списка чатов на главной: где чат, его пометки и последнее сообщение.
/// `last = null` — чат без переписки (закреплён или заглушен, но пуст).
#[derive(serde::Serialize)]
struct ChatSummaryJson {
    chat: String,
    group: bool,
    pinned_at: i64,
    muted: bool,
    last: Option<StoredMessage>,
}

/// Строка списка групп в том виде, в каком её читает клиент. Живёт здесь, потому
/// что список склеивается из двух источников: ростер MLS (слой групп) и архивные
/// группы из базы.
#[derive(serde::Serialize, serde::Deserialize)]
struct GroupRowJson {
    gid: String,
    name: String,
    #[serde(default)]
    members: usize,
    #[serde(default)]
    admin: bool,
    /// Ключ админа (hex; пусто — неизвестен, у архивных всегда пусто).
    #[serde(default)]
    admin_key: String,
    /// Мы НЕ состоим в группе: история из копии, отправка недоступна.
    #[serde(default)]
    archived: bool,
}

/// Период рассылки маячка присутствия в комнатах (пакеты крошечные).
const BEACON_INTERVAL: Duration = Duration::from_secs(2);
/// Не видели маячок дольше — участник считается вышедшим (страховка на вылет/потерю
/// связи). 12с = 6 интервалов: переживаем многосекундные провалы доставки, не
/// выкидывая собеседника. Явный выход (PRESENCE_LEFT) убирает участника мгновенно.
pub const PRESENCE_TTL: Duration = Duration::from_secs(12);
/// Бит в `flags`: «я выхожу из комнаты» — получатель убирает участника сразу.
const PRESENCE_LEFT: u8 = 0x80;

pub const BENCH_TRACK: u8 = 200;

const PUBLIC_PEERS: &str = include_str!("../../peers/public-peers.txt");

/// Присутствие в комнатах-звонках (Discord-style).
#[derive(Default)]
pub(crate) struct RoomState {
    /// Комнаты, в которых МЫ сейчас находимся: gid → наши A/V-флаги.
    pub(crate) joined: HashMap<Vec<u8>, u8>,
    /// Присутствие других: gid → (peer → (время последнего маячка, флаги)).
    pub(crate) remote: HashMap<Vec<u8>, HashMap<[u8; 32], (Instant, u8)>>,
    /// DM-комнаты: room_id → peer (для маячков и LEFT-сигнала без MLS-группы).
    pub(crate) dm_peers: HashMap<Vec<u8>, [u8; 32]>,
}

impl RoomState {
    /// Живые участники `gid` (маячок не старше `ttl` относительно `now`).
    /// Чистая логика презенса — тестируется без устройства (инъекция времени).
    pub(crate) fn live_with_flags(
        &self,
        gid: &[u8],
        now: Instant,
        ttl: Duration,
    ) -> Vec<([u8; 32], u8)> {
        self.remote
            .get(gid)
            .map(|m| {
                m.iter()
                    .filter(|(_, (t, _))| now.saturating_duration_since(*t) < ttl)
                    .map(|(p, (_, f))| (*p, *f))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Выкинуть протухшее присутствие (старше `ttl`) из всех комнат.
    pub(crate) fn prune(&mut self, now: Instant, ttl: Duration) {
        for peers in self.remote.values_mut() {
            peers.retain(|_, (t, _)| now.saturating_duration_since(*t) < ttl);
        }
    }
}

/// Активный мультипати аудио-звонок комнаты (один на процесс).
pub(crate) struct RoomCall {
    pub(crate) gid: Vec<u8>,
    pub(crate) engine: Arc<RoomCallEngine>,
    /// Цели мультикаста — синхронизируется с живыми участниками (см. sync).
    pub(crate) members: Arc<Mutex<Vec<[u8; 32]>>>,
}

pub(crate) struct BenchRecv {
    pub(crate) bytes: AtomicU64,
    pub(crate) pkts: AtomicU64,
    pub(crate) running: AtomicBool,
    pub(crate) start: Instant,
}

/// Кэш групповых read-данных для UI. event_pump поддерживает его актуальным после
/// каждой мутации. Снапшоты `group_*_json` читают кэш мгновенно.
#[derive(Default)]
struct GroupCache {
    list: String,
    /// Ростер MLS — он живёт в потоке актора, поэтому его снапшот кэшируется.
    /// Истории здесь нет: она в общей базе, и читается напрямую (ADR-009).
    members: HashMap<Vec<u8>, String>,
    /// Входящие приглашения в группы (JSON) — для синхронного чтения из UI.
    invites: String,
}

/// Текущий звонок в журнале: запись, время старта, направление и комната.
struct ActiveCall {
    id: i64,
    /// Длительность меряем монотонно: настенные часы двигает синхронизация времени,
    /// и разговор, во время которого их подвели назад, получал бы ноль.
    since: Instant,
    incoming: bool,
    room: Vec<u8>,
}

/// Состояние сессии приложения — корень контракта движок↔клиент.
pub struct Engine {
    messenger: Arc<Messenger>,
    contacts: Mutex<ContactStore>,
    /// Гейт контактов (замена TOFU): входящие запросы незнакомцев + блок-лист.
    gate: Mutex<GateStore>,
    /// Общее хранилище переписки. `Arc`, потому что тем же хранилищем пользуется
    /// менеджер групп — с объединением истории у групп нет своего.
    messages: Arc<Mutex<MessageStore>>,
    me_pub: [u8; 32],
    /// Личность для экспорта секретной сид-фразы (M4 key backup). Секрет уже живёт в
    /// messenger; здесь — только для `seed_phrase()` по запросу пользователя.
    identity: Identity,
    events: Mutex<VecDeque<String>>,
    group_cache: Mutex<GroupCache>,
    jobs: Sender<Job>,
    room_call: Mutex<Option<RoomCall>>,
    rooms: Mutex<RoomState>,
    /// id наших недоставленных приглашений в звонок по комнате: уходя из комнаты,
    /// снимаем их (R-007). Доставленные снимает сам Ack — здесь они просто лишние.
    rings: Mutex<HashMap<Vec<u8>, Vec<u64>>>,
    /// Блобы, которые мы сами попросили: id → (у кого, когда). Манифест чего-то
    /// другого — это файл без спроса, его не качаем (R-020).
    blob_expect: Mutex<HashMap<peerage_blob::Hash, ([u8; 32], Instant)>>,
    bench: Mutex<Option<Arc<BenchRecv>>>,
    /// Каталог хранения блобов (контент по hex-id). M2.
    blob_dir: PathBuf,
    /// Персистентная офлайн-очередь исходящих (M3): досылка после офлайна/рестарта.
    outbox: Mutex<peerage_core::outbox::OutboxManager>,
    /// Журнал звонков (локальный) + id текущего активного звонка и время старта.
    calls: Mutex<peerage_core::store::CallStore>,
    /// Текущий звонок — активных звонков не бывает больше одного.
    active_call: Mutex<Option<ActiveCall>>,
    /// Звонки, пришедшие посреди другого разговора (комната → строка журнала
    /// «пропущен»): ответят на такой — строка станет отвеченной, а не задвоится.
    missed_calls: Mutex<HashMap<Vec<u8>, i64>>,
    /// Запрошенные пользователем вложения — докачиваем сами, когда пир в сети.
    wanted: Mutex<peerage_core::store::WantedStore>,
    /// Единый бюджет (M3): тир каждого известного пира. Читают ingress-лимитер
    /// (через messenger) и офлайн-очередь (через замыкание) — одна точка истины.
    tier_cache: Arc<Mutex<HashMap<[u8; 32], Tier>>>,
    /// Каталог данных: нужен смене адреса (ADR-007), которая хранит свою эпоху.
    data_dir: PathBuf,
    /// Кому ещё не доехало объявление о смене адреса (был офлайн). Догоняем по
    /// presence: пока контакт не появился, слать некуда, а не узнав нового адреса,
    /// он потеряет с нами связь навсегда.
    rotate_pending: Mutex<std::collections::HashSet<[u8; 32]>>,
    /// Мы намеренно вне сети: пользователь ушёл в офлайн либо идёт пауза смены
    /// адреса. Отличается от «нет связи» — это осознанное состояние, и UI обязан
    /// его показывать.
    offline: AtomicBool,
    /// Отчёты о прочтении и набор: переключатели и отправленное (2.10).
    receipts: Mutex<receipts::Receipts>,
    /// Чем поднимать новый узел при смене адреса. По умолчанию — настоящий
    /// Yggdrasil; тесты подставляют loopback, иначе ротацию нельзя прогнать без
    /// реальной сети.
    transport_factory: Mutex<Option<TransportFactory>>,
}

/// Создатель транспорта из seed узла: `None` — не удалось поднять.
pub type TransportFactory =
    Arc<dyn Fn(&[u8; 32]) -> Option<Arc<dyn peerage_messenger::Transport>> + Send + Sync>;

/// Свой профиль на диске (M2): самоназвание + аватар (id блоба + mime). Не секрет —
/// уходит контактам; хранится plaintext в profile_meta.json.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct ProfileMeta {
    #[serde(default)]
    name: String,
    #[serde(default)]
    avatar_id: String,
    #[serde(default)]
    avatar_mime: String,
}

/// Результат инициализации: движок + публичная идентичность для UI.
pub struct StartOk {
    pub engine: Arc<Engine>,
    pub pubkey: [u8; 32],
    pub address: String,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Время своей реакции, мс: строго растёт в пределах процесса. Два быстрых нажатия
/// в одну миллисекунду иначе дали бы равные `at`, и второе молча проиграло бы.
pub(crate) fn react_clock() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static LAST: AtomicI64 = AtomicI64::new(0);
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let prev = LAST.fetch_max(wall, Ordering::SeqCst);
    if prev >= wall {
        LAST.fetch_add(1, Ordering::SeqCst) + 1
    } else {
        wall
    }
}

/// hex(32 байта) → ключ. Кодек-хелпер (строка↔ключ); экспортируется для androidjni.
pub fn parse_key(s: &str) -> Option<[u8; 32]> {
    hex::decode(s.trim()).ok()?.as_slice().try_into().ok()
}

/// Самое длинное самоназвание из кода обмена, в символах. Код приходит от кого
/// угодно (переслали в переписке) — без предела он раздул бы карточку контакта.
const CODE_NAME_MAX: usize = 64;

/// Самоназвание из хвоста кода обмена (`#<hex UTF-8>`). Кривой хвост — не повод
/// отказать в добавлении: ключ разобран отдельно, имя просто не берём. Управляющие
/// символы выбрасываем — им не место в имени.
fn code_name(tail: &str) -> Option<String> {
    let bytes = hex::decode(tail.trim()).ok()?;
    let name: String = String::from_utf8(bytes)
        .ok()?
        .chars()
        .filter(|c| !c.is_control())
        .take(CODE_NAME_MAX)
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

// ── Публичный API движка (команды/снапшоты) ─────────────────────────────────
// Каждый метод — операция контракта. androidjni декодирует аргументы (hex→ключ)
// и маршалит результат; домейн-логику считает здесь движок.
impl Engine {
    // — Жизненный цикл —
    /// Следующее событие (JSON) или None, если очередь пуста.
    pub fn poll_event(&self) -> Option<String> {
        self.events.lock_safe().pop_front()
    }

    /// Корректно остановить транспорт/мессенджер.
    pub fn shutdown(&self) {
        self.messenger.shutdown();
    }

    pub fn me_pub(&self) -> [u8; 32] {
        self.me_pub
    }

    /// Визуальный отпечаток ключа для сверки глазами: 12 байт в hex.
    /// Пусто, если ключ не разобрать. Свой — [`visual_fingerprint_self`].
    pub fn visual_fingerprint(&self, key: &[u8; 32]) -> String {
        hex::encode(peerage_core::identity::visual_fingerprint(key))
    }

    pub fn visual_fingerprint_self(&self) -> String {
        hex::encode(peerage_core::identity::visual_fingerprint(&self.me_pub))
    }

    /// Секретная сид-фраза (24 слова BIP39) — по запросу пользователя (онбординг M4
    /// «Показать фразу»). СЕКРЕТ: показывать только на FLAG_SECURE-экране.
    pub fn seed_phrase(&self) -> String {
        self.identity.seed_phrase()
    }

    /// Проверка копии ключа: слова на номерах (с 1) совпадают с сид-фразой. Фраза
    /// наружу не выходит — только ответ.
    pub fn seed_words_match(&self, picks: &[(usize, &str)]) -> bool {
        self.identity.seed_words_match(picks)
    }

    // — Сообщения 1:1 —
    /// Отправить текст. Возвращает id (≥0) или None при ошибке.
    pub fn send_text(&self, key: &[u8; 32], text: &str) -> Option<i64> {
        self.send_text_reply(key, text, 0, false)
    }

    /// Отправить текст ответом на сообщение `reply_to` (`reply_mine` — чьё оно у НАС).
    /// `reply_to = 0` — обычное сообщение.
    pub fn send_text_reply(
        &self,
        key: &[u8; 32],
        text: &str,
        reply_to: u64,
        reply_mine: bool,
    ) -> Option<i64> {
        self.send_text_full(key, text, reply_to, reply_mine, "", false)
    }

    /// Общий путь отправки текста: обычного, ответом и пересланного. `fwd_from` —
    /// hex автора ИСХОДНОГО сообщения (пусто — своё).
    pub(crate) fn send_text_full(
        &self,
        key: &[u8; 32],
        text: &str,
        reply_to: u64,
        reply_mine: bool,
        fwd_from: &str,
        fwd_anon: bool,
    ) -> Option<i64> {
        match self
            .messenger
            .send_text_reply(key, text, reply_to, reply_mine, fwd_from, fwd_anon)
        {
            Ok(id) => {
                let t = now();
                let _ = self.messages.lock_safe().append(
                    Chat::Dm(key),
                    &StoredMessage {
                        id,
                        text: text.to_string(),
                        mine: true,
                        time: t,
                        status: "sent".into(),
                        sender: self.me_hex(),
                        reply_to,
                        reply_sender: self.reply_author(key, reply_to, reply_mine),
                        fwd_from: fwd_from.to_string(),
                        fwd_anon,
                        // kind заполняем ЯВНО: пустая строка — не «текст», а «неизвестно»,
                        // и UI рисовал такое сообщение дважды (как содержимое и как
                        // подпись к вложению).
                        kind: "text".into(),
                        ..Default::default()
                    },
                );
                // В офлайн-очередь сообщение положил сам мессенджер — целиком, с
                // цитатой и пометкой пересылки.
                Some(id as i64)
            }
            Err(_) => None,
        }
    }

    /// Исправить СВОЁ сообщение: заменить текст (у вложения — подпись).
    ///
    /// Чужое править нельзя — это была бы подмена чужих слов; проверяем здесь, а не
    /// надеемся на клиент. История правок не хранится, но пометка «изменено»
    /// остаётся навсегда: молча подменённый текст хуже отсутствия правки.
    pub fn edit_message(&self, chat: ChatKey, id: u64, text: &str) -> bool {
        self.edit_message_with(chat, id, text, &[])
    }

    /// То же с упоминаниями. Они бывают только в группе: в личном чате упоминать
    /// некого, и там список молча отбрасывается.
    pub fn edit_message_with(
        &self,
        chat: ChatKey,
        id: u64,
        text: &str,
        mentions: &[Mention],
    ) -> bool {
        let me = self.me_hex();
        match &chat {
            ChatKey::Dm(peer) => {
                let store = self.messages.lock_safe();
                if !store.contains(chat.as_chat(), &me, id) {
                    return false; // не наше сообщение (или его уже нет)
                }
                drop(store);
                if self.messenger.send_edit(peer, id, text).is_err() {
                    return false;
                }
                let _ =
                    self.messages
                        .lock_safe()
                        .edit_text(chat.as_chat(), &me, id, text, &[], now());
                push_event(
                    self,
                    dto::UiEvent::MessageEdited {
                        chat: hex::encode(chat.id_bytes()),
                        id,
                        sender: me,
                        text: text.to_string(),
                        mentions: Vec::new(),
                    },
                );
                true
            }
            ChatKey::Group(gid) => self
                .jobs
                .send(Job::Cmd(GroupCmd::Edit {
                    gid: gid.clone(),
                    id,
                    text: text.to_string(),
                    mentions: mentions.to_vec(),
                }))
                .is_ok(),
        }
    }

    /// Закрепить или открепить сообщение в чате. Закреп общий для всех участников:
    /// он живёт у чата, а не у того, кто его поставил, — иначе «закрепил важное»
    /// означало бы «закрепил у себя», и остальные об этом не узнали бы.
    ///
    /// В группе закрепляет только админ (ADR-004): иначе любой участник вешал бы
    /// своё сообщение в шапку всем.
    pub fn pin_message(&self, chat: ChatKey, id: u64, sender: &str, on: bool) -> bool {
        match &chat {
            ChatKey::Dm(peer) => {
                let mine = sender == self.me_hex();
                if self.messenger.send_pin(peer, id, mine, on).is_err() {
                    return false;
                }
                self.apply_pin(chat.as_chat(), id, sender, on, &self.me_hex());
                push_event(
                    self,
                    dto::UiEvent::PinsChanged {
                        chat: hex::encode(chat.id_bytes()),
                    },
                );
                true
            }
            ChatKey::Group(gid) => self
                .jobs
                .send(Job::Cmd(GroupCmd::Pin {
                    gid: gid.clone(),
                    id,
                    sender: sender.to_string(),
                    on,
                }))
                .is_ok(),
        }
    }

    /// Поставить, заменить или снять (пустой `emoji`) свою реакцию на сообщение
    /// `(id, sender)`. Одна реакция от человека: что повтор снимает, решает клиент.
    /// Кривой эмодзи отклоняется здесь, на входе.
    pub fn react(&self, chat: ChatKey, id: u64, sender: &str, emoji: &str) -> bool {
        if !emoji.is_empty() && !peerage_core::protocol::reaction_ok(emoji) {
            return false;
        }
        let at = react_clock();
        match &chat {
            ChatKey::Dm(peer) => {
                let mine = sender == self.me_hex();
                if self
                    .messenger
                    .send_react(peer, id, mine, emoji, at)
                    .is_err()
                {
                    return false;
                }
                let changed = self
                    .messages
                    .lock_safe()
                    .react(chat.as_chat(), id, sender, &self.me_hex(), emoji, at)
                    .unwrap_or(false);
                if changed {
                    push_reactions(self, chat.as_chat(), id, sender);
                }
                true
            }
            ChatKey::Group(gid) => self
                .jobs
                .send(Job::Cmd(GroupCmd::React {
                    gid: gid.clone(),
                    id,
                    sender: sender.to_string(),
                    emoji: emoji.to_string(),
                    at,
                }))
                .is_ok(),
        }
    }

    /// Поиск по переписке. `chat = None` — по всем чатам сразу; свежие первыми.
    /// Каждая находка помнит, ГДЕ она: результат по всем чатам без этого бесполезен.
    pub fn search_json(&self, query: &str, chat: Option<ChatKey>, limit: usize) -> String {
        let store = self.messages.lock_safe();
        let found = store.search(query, chat.as_ref().map(|c| c.as_chat()), limit);
        let hits: Vec<SearchHit> = found
            .into_iter()
            .map(|(chat, scope, msg)| SearchHit {
                chat: hex::encode(chat),
                group: scope == 1,
                msg,
            })
            .collect();
        serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into())
    }

    /// Поиск в одном чате (2.9): слова по отдельности, имя файла, отправитель
    /// (`sender` hex, пустой — все), листание `offset`/`limit` и общее число находок.
    /// JSON `{total, hits: [{chat, group, ...сообщение}]}`, свежие первыми.
    pub fn search_page_json(
        &self,
        chat: &ChatKey,
        query: &str,
        sender: &str,
        offset: usize,
        limit: usize,
    ) -> String {
        let page = self
            .messages
            .lock_safe()
            .search_page(&peerage_core::store::SearchQuery {
                text: query,
                chat: Some(chat.as_chat()),
                sender: Some(sender.trim()).filter(|s| !s.is_empty()),
                offset,
                limit,
            });
        let json = SearchPageJson {
            total: page.total,
            hits: page
                .hits
                .into_iter()
                .map(|(chat, scope, msg)| SearchHit {
                    chat: hex::encode(chat),
                    group: scope == 1,
                    msg,
                })
                .collect(),
        };
        serde_json::to_string(&json).unwrap_or_else(|_| r#"{"total":0,"hits":[]}"#.into())
    }

    /// Окно истории вокруг места `(time, row)` — переход к находке или к дате
    /// (`row = 0` — первое сообщение не раньше `time`; `(0, 0)` — начало). Старые сверху.
    pub fn history_around_json(&self, chat: &ChatKey, time: i64, row: i64) -> String {
        let page =
            self.messages
                .lock_safe()
                .history_around(chat.as_chat(), (time, row), HISTORY_PAGE);
        serde_json::to_string(&page).unwrap_or_else(|_| "[]".into())
    }

    /// Страница НИЖЕ показанной, когда лента открыта не с хвоста: курсор — `time`/`row`
    /// самого нижнего сообщения. Пустая — дошли до хвоста.
    pub fn history_after_json(&self, chat: &ChatKey, time: i64, row: i64) -> String {
        let page =
            self.messages
                .lock_safe()
                .history_after(chat.as_chat(), (time, row), HISTORY_PAGE);
        serde_json::to_string(&page).unwrap_or_else(|_| "[]".into())
    }

    /// Дни с сообщениями в `[from, to)` (секунды) по местному времени (`offset` секунд
    /// от UTC): JSON `["ГГГГ-ММ-ДД", …]` — календарь «Перейти к дате».
    pub fn message_days_json(&self, chat: &ChatKey, from: i64, to: i64, offset: i64) -> String {
        let days = self
            .messages
            .lock_safe()
            .message_days(chat.as_chat(), from, to, offset);
        serde_json::to_string(&days).unwrap_or_else(|_| "[]".into())
    }

    /// Сводка по всем чатам для списка на главной: последнее сообщение, время,
    /// закреп и «без звука». Одним вызовом, а не по чату на строку: главная
    /// перерисовывается на каждое входящее сообщение.
    ///
    /// Непрочитанные и присутствие сюда не входят — они не в истории; клиент
    /// подмешивает их из своих источников.
    pub fn chat_summaries_json(&self) -> String {
        let store = self.messages.lock_safe();
        let mut rows: Vec<ChatSummaryJson> = store
            .summaries()
            .into_iter()
            .map(|s| ChatSummaryJson {
                chat: hex::encode(&s.chat),
                group: s.scope == 1,
                pinned_at: s.pinned_at,
                muted: s.muted,
                last: Some(s.last),
            })
            .collect();
        // Чат без единого сообщения тоже может быть закреплён или заглушен —
        // такие строки в `summaries` не попадают, добавляем их отдельно.
        let seen: HashSet<(String, bool)> =
            rows.iter().map(|r| (r.chat.clone(), r.group)).collect();
        for (chat, scope, pinned_at, muted) in store.chat_flags() {
            let row = (hex::encode(&chat), scope == 1);
            if !seen.contains(&row) {
                rows.push(ChatSummaryJson {
                    chat: row.0,
                    group: row.1,
                    pinned_at,
                    muted,
                    last: None,
                });
            }
        }
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
    }

    /// Закрепить чат в списке или снять закреп. Пометка ЛОКАЛЬНАЯ: собеседник о ней
    /// не узнаёт, в отличие от закрепа сообщения (тот общий для чата).
    pub fn set_chat_pinned(&self, chat: ChatKey, on: bool) {
        let store = self.messages.lock_safe();
        let _ = store.set_chat_pinned(chat.as_chat(), if on { now() } else { 0 });
    }

    /// Выключить/включить звук чата (тоже локально).
    pub fn set_chat_muted(&self, chat: ChatKey, muted: bool) {
        let store = self.messages.lock_safe();
        let _ = store.set_chat_muted(chat.as_chat(), muted);
    }

    /// Закреплённые сообщения чата (свежие первыми) — как обычные строки истории.
    pub fn pinned_json(&self, chat: ChatKey) -> String {
        let list = self.messages.lock_safe().pinned(chat.as_chat());
        serde_json::to_string(&list).unwrap_or_else(|_| "[]".into())
    }

    /// Положить закреп в хранилище (общий путь для своих действий и входящих).
    pub(crate) fn apply_pin(&self, chat: Chat, id: u64, sender: &str, on: bool, by: &str) {
        let store = self.messages.lock_safe();
        let _ = if on {
            store.pin(chat, id, sender, now(), by)
        } else {
            store.unpin(chat, id, sender).map(|_| ())
        };
    }

    /// Переслать сообщения в другой чат. `src`/`dst` — «личный чат или группа» плюс
    /// идентификатор; `refs` — пары «id + автор» (ADR-006). Возвращает, сколько
    /// сообщений ушло.
    ///
    /// Пересылка сохраняет АВТОРСТВО: получатель видит, от кого сообщение изначально.
    /// Это осознанный размен — без источника пересылка превращается в анонимный
    /// пересказ, которому нельзя верить. Автор передаётся ключом, а не готовой
    /// строкой «переслано от Ани»: имя рисует клиент по своим контактам, и подделать
    /// чужую подпись пересылкой нельзя.
    ///
    /// Вложение пересылается, только если его байты уже есть у нас: иначе получателю
    /// было бы некуда идти за содержимым — у ОТПРАВИТЕЛЯ его нет.
    pub fn forward_messages(
        &self,
        src: ChatKey,
        refs: &[(u64, String)],
        dst: ChatKey,
        hide_author: bool,
    ) -> usize {
        let items = self.messages.lock_safe().get_many(src.as_chat(), refs);
        let mut sent = 0;
        for m in items {
            // Пересылка пересланного сохраняет ПЕРВОГО автора, а не посредника.
            let origin = if m.fwd_from.is_empty() {
                m.sender.clone()
            } else {
                m.fwd_from.clone()
            };
            if m.system {
                continue; // системные строки — не сообщения
            }
            // Скрыли автора — ключ не уходит ВООБЩЕ: иначе «скрыл» значило бы
            // «спрятал в интерфейсе, но отдал по проводу».
            let origin = if hide_author { String::new() } else { origin };
            // Ссылки упоминаний в новый чат не едут (упомянутый там не обязан
            // состоять), поэтому участки становятся текстом — с теми именами, которые
            // видел пересылающий, а не с метками автора.
            let mut m = m;
            m.text = peerage_core::mention::plain(&m.text, &m.mentions, |k| self.mention_name(k));
            let ok = if m.blob_id.is_empty() {
                self.forward_text(&dst, &m.text, &origin, hide_author)
            } else {
                self.forward_media(&dst, &m, &origin, hide_author)
            };
            if ok {
                sent += 1;
            }
        }
        sent
    }

    /// Имя для «@» в тексте, который уходит без ссылки: своё — моё самоназвание,
    /// контакт — по тому же приоритету, что и в клиенте (локальное → самоназвание →
    /// короткий hex). Незнакомый ключ — короткий hex: другого имени у нас нет.
    fn mention_name(&self, key_hex: &str) -> String {
        if key_hex.eq_ignore_ascii_case(&self.me_hex()) {
            let own = self.self_name();
            if !own.is_empty() {
                return own;
            }
        }
        let short = || key_hex.chars().take(8).collect::<String>();
        let Some(key) = hex::decode(key_hex)
            .ok()
            .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
        else {
            return short();
        };
        match self.contacts.lock_safe().get(&key) {
            Some(c) if !c.name.is_empty() => c.name.clone(),
            Some(c) if !c.profile_name.is_empty() => c.profile_name.clone(),
            _ => short(),
        }
    }

    fn forward_text(&self, dst: &ChatKey, text: &str, origin: &str, anon: bool) -> bool {
        match dst {
            ChatKey::Dm(peer) => self
                .send_text_full(peer, text, 0, false, origin, anon)
                .is_some(),
            ChatKey::Group(gid) => {
                self.jobs
                    .send(Job::Cmd(GroupCmd::SendText {
                        gid: gid.clone(),
                        text: text.to_string(),
                        reply_to: 0,
                        reply_sender: String::new(),
                        fwd_from: origin.to_string(),
                        fwd_anon: anon,
                        // Пересылка несёт текст, а не ссылки: упомянутый в исходном
                        // чате не обязан состоять в этом.
                        mentions: Vec::new(),
                    }))
                    .is_ok()
            }
        }
    }

    fn forward_media(&self, dst: &ChatKey, m: &StoredMessage, origin: &str, anon: bool) -> bool {
        // Пересылаем ССЫЛКУ на блоб, который уже у нас: получатель запросит байты
        // сам. Читать и перешифровывать файл ради пересылки незачем — у видео это
        // гигабайт в память. Блоба у нас нет — ссылка вела бы получателя в тупик.
        if !self.blob_exists(&m.blob_id) {
            return false;
        }
        let as_hash = |h: &str| {
            hex::decode(h)
                .ok()
                .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
        };
        let Some(hash) = as_hash(&m.blob_id) else {
            return false;
        };
        let thumb = as_hash(&m.thumb_id).filter(|_| self.blob_exists(&m.thumb_id));
        match dst {
            ChatKey::Dm(peer) => {
                let info = MediaInfo {
                    duration_ms: m.duration_ms,
                    width: m.width,
                    height: m.height,
                    thumb: None,
                    caption: m.text.clone(),
                    fwd_from: origin.to_string(),
                    fwd_anon: anon,
                    // Пересылка не тащит цитату: ответ имеет смысл только в том чате,
                    // где лежит оригинал, а в новом эта ссылка указывала бы в пустоту.
                    reply_to: 0,
                    reply_sender: String::new(),
                };
                self.send_media_ref(peer, hash, m.size, &m.mime, &m.name, thumb, &info)
                    .is_some()
            }
            ChatKey::Group(gid) => {
                let media = MediaBody {
                    id: 0,
                    fwd_from: origin.to_string(),
                    fwd_anon: anon,
                    reply_to: 0,
                    reply_sender: String::new(),
                    kind: Self::media_kind(&m.mime).into(),
                    hash: m.blob_id.clone(),
                    mime: m.mime.clone(),
                    name: m.name.clone(),
                    caption: m.text.clone(),
                    size: m.size,
                    duration_ms: m.duration_ms,
                    width: m.width,
                    height: m.height,
                    thumb: thumb.map(hex::encode).unwrap_or_default(),
                };
                self.jobs
                    .send(Job::GroupMediaRef {
                        gid: gid.clone(),
                        media,
                    })
                    .is_ok()
            }
        }
    }

    /// Удалить сообщения ТОЛЬКО У СЕБЯ. `refs` — пары `(id, mine)`; ключ истории —
    /// именно пара, потому что id посеян временем отправителя и у обеих сторон свой.
    /// Возвращает, сколько строк удалено.
    pub fn delete_messages(&self, key: &[u8; 32], refs: &[(u64, bool)]) -> usize {
        let refs: Vec<(u64, String)> = refs
            .iter()
            .map(|(id, mine)| (*id, self.author_hex(key, *mine)))
            .collect();
        self.messages
            .lock_safe()
            .delete_many(Chat::Dm(key), &refs)
            .unwrap_or(0)
    }

    /// hex автора сообщения в личном чате: своё — мы, чужое — собеседник.
    fn author_hex(&self, peer: &[u8; 32], mine: bool) -> String {
        if mine {
            self.me_hex()
        } else {
            hex::encode(peer)
        }
    }

    /// hex автора ЦИТИРУЕМОГО сообщения; пусто, если это не ответ.
    fn reply_author(&self, peer: &[u8; 32], reply_to: u64, reply_mine: bool) -> String {
        if reply_to == 0 {
            String::new()
        } else {
            self.author_hex(peer, reply_mine)
        }
    }

    /// hex своего ключа — автор всех наших сообщений.
    pub(crate) fn me_hex(&self) -> String {
        hex::encode(self.me_pub)
    }

    /// Отозвать СВОИ сообщения у собеседника («удалить у всех»): просьба уходит
    /// надёжно (доедет и после офлайна), локально сообщение сразу становится строкой
    /// «Сообщение удалено» ([`MessageStore::retract_many`]).
    ///
    /// Возвращает число отозванных. Чужие сообщения молча пропускаются: отзывать
    /// чужое нельзя — это не «удалить», а «переписать чужую историю».
    pub fn retract_messages(&self, key: &[u8; 32], ids: &[u64]) -> usize {
        let mut sent = 0;
        for id in ids {
            if self.messenger.send_retract(key, *id).is_ok() {
                sent += 1;
            }
        }
        // Из очереди тоже убираем: иначе досылка доставила бы уже отозванное.
        for id in ids {
            self.messenger.forget(key, *id);
        }
        let me = self.me_hex();
        let refs: Vec<(u64, String)> = ids.iter().map(|id| (*id, me.clone())).collect();
        let _ = self.messages.lock_safe().retract_many(Chat::Dm(key), &refs);
        sent
    }

    /// Тип медиа-сообщения по MIME (M2): картинка → превью, видео → кадр с кнопкой
    /// воспроизведения, аудио → голосовое-плеер, остальное → файл-карточка.
    fn media_kind(mime: &str) -> &'static str {
        if mime.starts_with("image/") {
            "image"
        } else if mime.starts_with("video/") {
            "video"
        } else if mime.starts_with("audio/") {
            "voice"
        } else {
            "file"
        }
    }

    /// Отправить медиа-вложение (M2): scrub (для картинок) → блоб пиру (push) →
    /// медиа-сообщение со ссылкой на блоб по хэшу (тот же надёжный путь, что текст).
    /// Возвращает id сообщения или None. `name` — исходное имя файла (для не-картинок).
    pub fn send_media(
        &self,
        key: &[u8; 32],
        content: Vec<u8>,
        mime: &str,
        name: &str,
    ) -> Option<i64> {
        self.send_media_with(key, content, mime, name, MediaInfo::default())
    }

    /// То же с метаданными для карточки: длительность, размеры кадра, миниатюра.
    /// Получатель видит «видео · 12:05 · 800 МБ» с превью ДО скачивания.
    pub fn send_media_with(
        &self,
        key: &[u8; 32],
        content: Vec<u8>,
        mime: &str,
        name: &str,
        info: MediaInfo,
    ) -> Option<i64> {
        self.send_media_from(key, MediaSource::Bytes(content), mime, name, info)
    }

    /// Отправить вложение, лежащее ФАЙЛОМ, не поднимая его в память.
    pub fn send_media_file(
        &self,
        key: &[u8; 32],
        path: &std::path::Path,
        mime: &str,
        name: &str,
        info: MediaInfo,
    ) -> Option<i64> {
        self.send_media_from(key, MediaSource::File(path.to_path_buf()), mime, name, info)
    }

    fn send_media_from(
        &self,
        key: &[u8; 32],
        source: MediaSource,
        mime: &str,
        name: &str,
        info: MediaInfo,
    ) -> Option<i64> {
        let (hash, size) = self.store_media(source, mime)?;
        // Ни миниатюру, ни сам файл не толкаем: получатель запросит по хэшу из
        // сообщения — миниатюру и мелкое сразу, крупное по кнопке.
        let thumb = self.store_thumb(info.thumb.as_deref());
        self.send_media_ref(key, hash, size, mime, name, thumb, &info)
    }

    /// Положить своё вложение в хранилище блобов (чтобы показывать его сразу и
    /// после рестарта) и вернуть его контент-адрес и размер. Общая подготовка
    /// личного и группового вложения.
    ///
    /// Байты в памяти: картинку сперва чистим от метаданных (EXIF/GPS), как аватар.
    /// Файл: манифест считается потоково, копия пишется тем же потоком чанками —
    /// в память он не поднимается. Картинки сюда файлом не приходят: scrub-у нужен
    /// весь файл, а весят они немного. `None` — файл не читается или больше предела
    /// приёма (его не примет ни один получатель).
    fn store_media(&self, source: MediaSource, mime: &str) -> Option<(peerage_blob::Hash, u64)> {
        match source {
            MediaSource::Bytes(content) => {
                let bytes = if mime.starts_with("image/") {
                    peerage_blob::scrub::scrub(mime, &content)
                } else {
                    content
                };
                let hash = peerage_blob::blob_id(&bytes);
                self.store_blob(hash, &bytes);
                Some((hash, bytes.len() as u64))
            }
            MediaSource::File(path) => {
                let manifest = peerage_blob::manifest_of_file(&path, mime.to_string()).ok()?;
                if manifest.size > peerage_blob::Limits::default().max_size {
                    return None;
                }
                if !self.blob_exists(&hex::encode(manifest.id)) {
                    self.store_blob_streamed(&path, &manifest).ok()?;
                }
                Some((manifest.id, manifest.size))
            }
        }
    }

    /// Сохранить миниатюру вложения отдельным блобом. Её получатель запрашивает
    /// сразу, без кнопки: без картинки предложение «скачать 800 МБ» слепое.
    fn store_thumb(&self, thumb: Option<&[u8]>) -> Option<peerage_blob::Hash> {
        let t = thumb.filter(|t| !t.is_empty())?;
        let h = peerage_blob::blob_id(t);
        self.store_blob(h, t);
        Some(h)
    }

    /// Отправить медиа-сообщение со ссылкой на блоб, который УЖЕ лежит у нас, и
    /// записать его в историю. Общий хвост отправки вложения и пересылки: байты
    /// получатель запросит сам по хэшу.
    #[allow(clippy::too_many_arguments)]
    fn send_media_ref(
        &self,
        key: &[u8; 32],
        hash: peerage_blob::Hash,
        size: u64,
        mime: &str,
        name: &str,
        thumb: Option<peerage_blob::Hash>,
        info: &MediaInfo,
    ) -> Option<i64> {
        let meta = peerage_messenger::MediaMeta {
            fwd_from: info.fwd_from.clone(),
            fwd_anon: info.fwd_anon,
            size,
            duration_ms: info.duration_ms,
            width: info.width,
            height: info.height,
            thumb: thumb.map(|h| h.to_vec()).unwrap_or_default(),
            caption: info.caption.clone(),
            reply_to: info.reply_to,
            // На проводе «чьё» едет с точки зрения отправителя: наш ответ на своё
            // сообщение для получателя — ответ на наше.
            reply_mine: info.reply_sender == self.me_hex(),
        };
        let id = self
            .messenger
            .send_media_message(key, &hash, mime, name, meta.clone())
            .ok()?;
        let _ = self.messages.lock_safe().append(
            Chat::Dm(key),
            &StoredMessage {
                // "sent" — как у текста: отдано в сеть, доставка НЕ подтверждена
                // (✓✓ появится только по Ack, ☁ — если ретраи выдохлись).
                id,
                mine: true,
                time: now(),
                status: "sent".into(),
                sender: self.me_hex(),
                fwd_from: info.fwd_from.clone(),
                fwd_anon: info.fwd_anon,
                // Подпись живёт в `text`: у медиа-сообщения это поле свободно,
                // и тогда поиск и история работают с ней как с обычным текстом.
                text: info.caption.clone(),
                kind: Self::media_kind(mime).into(),
                blob_id: hex::encode(hash),
                mime: mime.into(),
                name: name.into(),
                size,
                duration_ms: info.duration_ms,
                width: info.width,
                height: info.height,
                thumb_id: hex::encode(&meta.thumb),
                downloaded: true, // своё вложение уже у нас
                reply_to: info.reply_to,
                reply_sender: info.reply_sender.clone(),
                ..Default::default()
            },
        );
        Some(id as i64)
    }

    /// Записать свою копию блоба на диск потоково, в том же чанкованном формате, что
    /// и принятые крупные блобы (каждый чанк зашифрован отдельно).
    fn store_blob_streamed(
        &self,
        src: &std::path::Path,
        manifest: &peerage_blob::Manifest,
    ) -> Result<(), peerage_blob::BlobError> {
        let _ = std::fs::create_dir_all(&self.blob_dir);
        let dst = self.blob_dir.join(hex::encode(manifest.id));
        let mut asm = peerage_blob::disk::DiskAssembler::create(manifest.clone(), &dst)?;
        for i in 0..manifest.chunks.len() {
            let chunk =
                peerage_blob::read_file_chunk(src, i).map_err(|_| peerage_blob::BlobError::Io)?;
            asm.add(i, &chunk, |i, b| {
                vault::seal(&vault::chunk_purpose(&manifest.id, i), b)
            })?;
        }
        asm.finish()?;
        Ok(())
    }

    /// Скачать вложение по кнопке (крупное не тянется автоматически). Запрашиваем
    /// манифест по хэшу — дальше обычная докачка чанков; готовность придёт событием
    /// `blob_received`. Идемпотентно: если блоб уже есть, ничего не делаем.
    pub fn download_media(&self, peer: &[u8; 32], blob_id_hex: &str) -> bool {
        if self.blob_exists(blob_id_hex) {
            return true; // уже скачано
        }
        let Ok(hash) = hex::decode(blob_id_hex.trim()) else {
            return false;
        };
        // Намерение ПЕРСИСТЕНТНО: пир может быть офлайн или уйти посреди докачки —
        // продолжим сами, когда он вернётся, и переживём перезапуск. Пользователь не
        // должен сторожить, когда собеседник появится в сети.
        let _ = self.wanted.lock_safe().add(blob_id_hex, peer, now());
        // Раз пользователь СAM попросил файл — этот трафик ожидаемый, и резать его
        // своим же анти-флуд бюджетом бессмысленно (иначе 64 МБ ехали бы полторы
        // минуты). Поднимаем лимит адресно: только этому пиру и только пока качаем.
        self.messenger.boost_peer(peer, BLOB_BOOST_FOR);
        self.request_blob(peer, &hash);
        true
    }

    /// hex-id вложений, докачка которых заказана пользователем и ещё не закончена.
    /// UI по ним рисует прогресс вместо кнопки «Скачать» — в том числе после
    /// перезапуска приложения.
    pub fn wanted_json(&self) -> String {
        let ids: Vec<String> = self
            .wanted
            .lock_safe()
            .all()
            .into_iter()
            .map(|w| w.blob_id)
            .collect();
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into())
    }

    /// Продолжить всё, что пользователь просил скачать у этого пира (он в сети).
    /// То же на старте процесса: намерение «скачать» переживает перезапуск.
    fn resume_all_wanted(&self) {
        let mut peers: Vec<[u8; 32]> = self
            .wanted
            .lock_safe()
            .all()
            .into_iter()
            .map(|w| w.peer)
            .collect();
        peers.sort();
        peers.dedup();
        for p in peers {
            self.resume_wanted(&p);
        }
    }

    fn resume_wanted(&self, peer: &[u8; 32]) {
        for w in self.wanted.lock_safe().for_peer(peer) {
            if self.blob_exists(&w.blob_id) {
                let _ = self.wanted.lock_safe().remove(&w.blob_id);
                continue;
            }
            if let Ok(hash) = hex::decode(&w.blob_id) {
                self.messenger.boost_peer(peer, BLOB_BOOST_FOR);
                self.request_blob(peer, &hash);
            }
        }
    }

    /// Попросить у пира манифест блоба и запомнить, что ждём его именно от него.
    /// Единственный путь, которым в нас попадает чужой блоб.
    pub(crate) fn request_blob(&self, peer: &[u8; 32], hash: &[u8]) {
        let Ok(id) = <[u8; 32]>::try_from(hash) else {
            return;
        };
        self.blob_expect
            .lock_safe()
            .insert(id, (*peer, Instant::now()));
        let _ = self.messenger.request_blob_manifest(peer, hash);
    }

    /// Ждём ли блоб `id` от `peer`.
    fn blob_expected(&self, peer: &[u8; 32], id: &[u8]) -> bool {
        let Ok(id) = <[u8; 32]>::try_from(id) else {
            return false;
        };
        let mut ex = self.blob_expect.lock_safe();
        ex.retain(|_, (_, at)| at.elapsed() < BLOB_EXPECT_TTL);
        ex.get(&id).is_some_and(|(p, _)| p == peer)
    }

    /// Достучаться до пира прямо сейчас (открыли чат с ним), не дожидаясь
    /// расписания проб. Держать связь постоянно — забота мессенджера по набору,
    /// который задаёт [`Self::refresh_targets`].
    pub fn connect(&self, key: [u8; 32]) {
        self.messenger.poke(&key);
    }

    // — Контакты (TOFU) —
    /// Добавить контакт. Возвращает "added" | "known" | "error".
    /// Строка обмена контактом (QR / «скопировать ключ»).
    ///
    /// Пока адрес не менялся — это просто hex личности, ровно как раньше: старые
    /// коды и сканеры продолжают работать. После смены адреса к личности
    /// добавляется ПОДПИСАННАЯ привязка к текущему адресу — иначе тот, кто добавит
    /// нас по такому коду, будет слать на покинутый ключ и не достучится.
    ///
    /// Подпись здесь не формальность: код часто пересылают через третьи руки
    /// (переписка, форум), и без неё посредник мог бы подменить адрес, оставив
    /// личность.
    ///
    /// Если задано самоназвание, оно едет хвостом `#<hex UTF-8>`: добавивший сразу
    /// видит имя, а не обрывок ключа. Хвост не подписан и подписи не требует — это
    /// то же самоназвание, что придёт в профиле, и профиль его перезапишет.
    pub fn my_contact_code(&self) -> String {
        let epoch = self.transport_epoch();
        let mut code = if epoch == 0 {
            hex::encode(self.me_pub)
        } else {
            let tk = self.identity.transport_public(epoch);
            let sig = self.identity.sign_transport_rotate(epoch, &tk);
            format!(
                "{}:{}:{}:{}",
                hex::encode(self.me_pub),
                epoch,
                hex::encode(tk),
                hex::encode(sig)
            )
        };
        let name = self.self_name();
        if !name.trim().is_empty() {
            code.push('#');
            code.push_str(&hex::encode(name.trim()));
        }
        code
    }

    /// Добавить контакт по коду обмена: либо голый hex личности (как раньше), либо
    /// расширенная форма `личность:эпоха:адрес:подпись`. Привязку к адресу
    /// принимаем только с верной подписью — тем же путём, что и объявление о смене
    /// адреса.
    pub fn add_contact_code(&self, code: &str, name: &str) -> &'static str {
        let (code, self_name) = match code.trim().split_once('#') {
            Some((c, tail)) => (c, code_name(tail)),
            None => (code.trim(), None),
        };
        let parts: Vec<&str> = code.split(':').collect();
        let Some(ik) = parse_key(parts[0]) else {
            return "error";
        };
        let outcome = self.add_contact(&ik, name);
        // Самоназвание из кода — только новому контакту: у знакомого оно уже пришло
        // профилем, а тот достовернее (пришёл от самого пира, не через третьи руки).
        if let (Some(n), "added") = (self_name, outcome) {
            let _ = self.contacts.lock_safe().set_profile(&ik, &n, "");
        }
        if parts.len() == 4 {
            let epoch = parts[1].parse::<u32>().unwrap_or(0);
            let tk = parse_key(parts[2]);
            let sig = hex::decode(parts[3])
                .ok()
                .and_then(|s| <[u8; 64]>::try_from(s.as_slice()).ok());
            if let (Some(tk), Some(sig)) = (tk, sig) {
                let applied = self
                    .contacts
                    .lock_safe()
                    .apply_transport_rotate(&ik, epoch, &tk, &sig)
                    .unwrap_or(false);
                if applied {
                    self.refresh_transports();
                }
            }
        }
        outcome
    }

    pub fn add_contact(&self, key: &[u8; 32], name: &str) -> &'static str {
        // Пустое имя остаётся пустым: локальное имя — заметка пользователя и
        // перекрывает самоназвание. Заглушка «ab12cd34…» здесь навсегда прятала бы
        // имя, которое пир пришлёт профилем (показ: локальное → самоназвание → hex).
        let name = name.trim();
        // ВАЖНО: гард лока из scrutinee `match` живёт до конца всех арм → refresh_budgets
        // (лочит contacts) внутри арма = повторный лок того же мьютекса = дедлок. Поэтому
        // снимаем результат в переменную (гард освобождается на `;`), затем match.
        let outcome = self.contacts.lock_safe().add(key, name, now());
        match outcome {
            Ok(peerage_core::store::AddOutcome::Added) => {
                self.refresh_budgets(); // M3: контакт → щедрый ingress-бюджет; и в набор связи
                self.messenger.poke(key);
                "added"
            }
            Ok(peerage_core::store::AddOutcome::AlreadyKnown) => "known",
            Err(_) => "error",
        }
    }

    // ── Смена своего адреса (ADR-007) ───────────────────────────────────────

    /// Подменить способ поднятия узла (тесты гоняют ротацию на loopback-транспорте,
    /// без настоящей сети).
    pub fn set_transport_factory(&self, f: TransportFactory) {
        *self.transport_factory.lock_safe() = Some(f);
    }

    /// Текущая эпоха нашего адреса. 0 — адрес совпадает с ключом личности.
    pub fn transport_epoch(&self) -> u32 {
        load_transport_epoch(&self.data_dir)
    }

    /// Сменить свой адрес: погасить узел, **выждать паузу офлайн**, подняться на
    /// новом транспортном ключе и разослать подписанное объявление ВЫБРАННЫМ
    /// контактам.
    ///
    /// Пауза — не осторожность, а условие работоспособности: при мгновенном
    /// перезаходе новый узел наследует маршрутную позицию старого ключа, и поток
    /// флудера продолжает приходить (замерено на публичном меше, 88 % от базы).
    /// После ~30 с офлайна старый ключ выпадает из дерева, и слать на него
    /// становится некуда.
    ///
    /// `recipients` — кому сообщить новый адрес; кого в списке нет, тот останется
    /// со старым и потеряет с нами связь. Это и есть ответ на «контакт слил мой
    /// адрес»: ротируем и не говорим ему.
    ///
    /// Долгая (пауза!) и однопоточная операция — уходит в отдельный поток,
    /// вызывающий не блокируется. Личность, история, группы и верификации не
    /// затрагиваются вовсе.
    pub fn rotate_transport(self: &Arc<Self>, recipients: Vec<[u8; 32]>, gap_secs: u64) {
        let engine = self.clone();
        std::thread::spawn(move || engine.rotate_transport_blocking(recipients, gap_secs));
    }

    fn rotate_transport_blocking(self: &Arc<Self>, recipients: Vec<[u8; 32]>, gap_secs: u64) {
        let epoch = self.transport_epoch().saturating_add(1);
        let tk = self.identity.transport_public(epoch);
        log::info!("смена адреса: эпоха {epoch}, пауза офлайн {gap_secs}с");

        // 1. Гасим текущий узел: с этого момента нас нет в сети — ни для флудера,
        //    ни для контактов.
        self.offline.store(true, Ordering::Relaxed);
        self.messenger.stop_transport();
        // 2. Пауза, за которую старый ключ выпадает из дерева маршрутизации.
        std::thread::sleep(Duration::from_secs(gap_secs));
        // 3. Поднимаемся на новом ключе и возвращаемся в меш.
        let Some(node) = self.spawn_node(epoch) else {
            // Новый узел не поднялся. Молча остаться офлайн значило бы потерять сеть
            // до перезапуска, а клиент так и показывал бы ход смены адреса (R-032).
            // Возвращаемся на прежний адрес — уйти от флуда не вышло, но связь есть.
            log::error!(
                "смена адреса: не удалось поднять узел эпохи {epoch}, возвращаюсь на прежний"
            );
            let online = self.go_online();
            push_event(self, dto::UiEvent::TransportRotateFailed { online });
            return;
        };
        let address = node.address();
        self.messenger.swap_transport(node);
        self.messenger.reset_probes(); // новый адрес — пусть контакты увидят нас сразу
                                       // Эпоху фиксируем ТОЛЬКО после успешного подъёма: иначе перезапуск после
                                       // сбоя искал бы нас по адресу, на котором никто не слушает.
        save_transport_epoch(&self.data_dir, epoch);

        // 4. Рассылаем объявление. Оно надёжное (retry-until-Ack), а тех, кто сейчас
        //    офлайн, догоняем в `announce_rotation_to` при их возвращении.
        self.offline.store(false, Ordering::Relaxed);
        let sig = self.identity.sign_transport_rotate(epoch, &tk);
        // Список «кому ещё не сказали» переживает перезапуск: контакт может
        // отсутствовать неделями, а не узнав нового адреса, он теряет с нами связь
        // навсегда. (Грейс-период старого узла отвергнут — он держал бы старый ключ
        // в сети и отменял весь смысл ротации, см. ADR-007.)
        self.set_rotate_pending(&recipients);
        for peer in &recipients {
            let _ = self.messenger.send_transport_rotate(peer, epoch, &tk, &sig);
        }
        log::info!(
            "смена адреса завершена: {address} (оповещено {})",
            recipients.len()
        );
        push_event(self, dto::UiEvent::TransportRotated { address, epoch });
    }

    /// Поднять узел на транспортном ключе указанной эпохи. Одна точка на две
    /// операции — смену адреса и возврат из офлайна.
    fn spawn_node(&self, epoch: u32) -> Option<Arc<dyn peerage_messenger::Transport>> {
        let seed = self.identity.transport_seed(epoch);
        let factory = self.transport_factory.lock_safe().clone();
        match factory {
            Some(f) => f(&seed),
            None => match Node::start(&seed) {
                Ok(n) => {
                    for uri in public_peer_uris() {
                        let _ = n.add_peer(uri);
                    }
                    Some(Arc::new(n))
                }
                Err(e) => {
                    log::error!("не удалось поднять узел: {e}");
                    None
                }
            },
        }
    }

    /// Мы сейчас намеренно вне сети (пользователь ушёл в офлайн или идёт пауза
    /// смены адреса).
    pub fn is_offline(&self) -> bool {
        self.offline.load(Ordering::Relaxed)
    }

    /// Уйти в офлайн: разорвать соединения. Единственный способ ПРЯМО СЕЙЧАС
    /// перестать платить за чужой поток — байты перестают доходить до радио, потому
    /// что доходить больше некуда. Сообщения при этом тоже не приходят: честный
    /// размен, и он должен быть виден пользователю.
    pub fn go_offline(&self) {
        self.offline.store(true, Ordering::Relaxed);
        self.messenger.stop_transport();
        log::info!("офлайн: соединения разорваны по запросу пользователя");
    }

    /// Вернуться в сеть на текущем адресе.
    pub fn go_online(&self) -> bool {
        let epoch = self.transport_epoch();
        match self.spawn_node(epoch) {
            Some(node) => {
                self.messenger.swap_transport(node);
                self.offline.store(false, Ordering::Relaxed);
                // Пока были офлайн, паузы проб выросли до минут — сеть вернулась,
                // спрашиваем всех заново сразу.
                self.messenger.reset_probes();
                log::info!("онлайн: узел поднят заново (эпоха {epoch})");
                true
            }
            None => false,
        }
    }

    /// Догнать объявлением контакта, который был офлайн в момент смены адреса.
    /// Зовётся по presence: пока он не появился, слать некуда, а как появился —
    /// он обязан узнать новый адрес, иначе связь с ним оборвётся навсегда.
    fn announce_rotation_to(&self, peer: &[u8; 32]) {
        if !self.rotate_pending.lock_safe().contains(peer) {
            return;
        }
        let epoch = self.transport_epoch();
        if epoch == 0 {
            return;
        }
        let tk = self.identity.transport_public(epoch);
        let sig = self.identity.sign_transport_rotate(epoch, &tk);
        if self
            .messenger
            .send_transport_rotate(peer, epoch, &tk, &sig)
            .is_ok()
        {
            // Ушло в надёжную отправку — из списка убираем: повторы дальше ведёт
            // сам мессенджер (retry-until-Ack).
            let rest: Vec<[u8; 32]> = {
                let mut p = self.rotate_pending.lock_safe();
                p.remove(peer);
                p.iter().copied().collect()
            };
            self.set_rotate_pending(&rest);
        }
    }

    /// Сохранить список «кому ещё не сообщили новый адрес» (память + диск).
    fn set_rotate_pending(&self, peers: &[[u8; 32]]) {
        *self.rotate_pending.lock_safe() = peers.iter().copied().collect();
        let hexes: Vec<String> = peers.iter().map(hex::encode).collect();
        if let Ok(v) = serde_json::to_vec(&hexes) {
            let _ =
                peerage_core::fsutil::write_atomic(&self.data_dir.join("rotate_pending.json"), v);
        }
    }

    /// Прочитать список с диска (после перезапуска).
    fn load_rotate_pending(&self) {
        let list: Vec<String> = std::fs::read(self.data_dir.join("rotate_pending.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        *self.rotate_pending.lock_safe() = list.iter().filter_map(|h| parse_key(h)).collect();
    }

    /// Переслать транспорту актуальную карту адресов контактов (ADR-007):
    /// личность → её текущий транспортный ключ. Зовётся при старте и после каждого
    /// принятого объявления о смене адреса — без этого сообщения продолжали бы
    /// уходить на покинутый ключ.
    fn refresh_transports(&self) {
        let map: Vec<([u8; 32], [u8; 32])> = self
            .contacts
            .lock_safe()
            .list()
            .iter()
            .filter(|c| !c.transport_key.is_empty())
            .filter_map(|c| {
                let ik = parse_key(&c.key)?;
                let tk = parse_key(&c.transport_key)?;
                Some((ik, tk))
            })
            .collect();
        self.messenger.set_peer_transports(&map);
    }

    /// Пере-залить тиры доверия в мессенджер — единый бюджет ресурсов (M3).
    /// Источник истины: контакты (сверенный fingerprint → `Verified`) и блок-лист
    /// гейта (`Blocked` — режется на ingress, ДО расшифровки). Кого нет — `Stranger`
    /// (общая корзина класса, анти-Sybil). Зовётся при любой смене того или другого.
    fn refresh_budgets(&self) {
        // ВАЖНО: не держим оба лока разом (contacts+gate) — иначе гонка порядка
        // захвата с API гейта = дедлок. Снимаем контакты, отпускаем, затем блоки.
        let mut tiers: Vec<([u8; 32], Tier, u32)> = self
            .contacts
            .lock_safe()
            .list()
            .iter()
            .filter_map(|c| {
                parse_key(&c.key).map(|k| {
                    (
                        k,
                        if c.verified {
                            Tier::Verified
                        } else {
                            Tier::Contact
                        },
                        c.helpful,
                    )
                })
            })
            .collect();
        let blocked: Vec<[u8; 32]> = self
            .gate
            .lock_safe()
            .list_blocked()
            .iter()
            .filter_map(|h| parse_key(h))
            .collect();
        // Блок сильнее контакта: если пир и там и там — он заблокирован.
        tiers.retain(|(k, _, _)| !blocked.contains(k));
        tiers.extend(blocked.into_iter().map(|k| (k, Tier::Blocked, 0)));
        self.messenger.set_peer_tiers(&tiers);
        // Тот же бюджет — офлайн-очереди (cap/TTL хранения).
        *self.tier_cache.lock_safe() = tiers.into_iter().map(|(k, t, _)| (k, t)).collect();
        self.refresh_targets();
    }

    /// С кем мессенджер держит связь: контакты и участники наших групп, без
    /// заблокированных и без себя. Зовётся на любую смену контактов, блок-листа и
    /// состава групп — выпавшему из набора больше не уходят ни пробы, ни Pong.
    fn refresh_targets(&self) {
        let blocked: std::collections::HashSet<[u8; 32]> = self
            .gate
            .lock_safe()
            .list_blocked()
            .iter()
            .filter_map(|h| parse_key(h))
            .collect();
        let mut targets: std::collections::HashSet<[u8; 32]> = self
            .contacts
            .lock_safe()
            .list()
            .iter()
            .filter_map(|c| parse_key(&c.key))
            .collect();
        for roster in self.group_cache.lock_safe().members.values() {
            let keys = serde_json::from_str::<Vec<String>>(roster).unwrap_or_default();
            targets.extend(keys.iter().filter_map(|h| parse_key(h)));
        }
        targets.remove(&self.me_pub);
        targets.retain(|k| !blocked.contains(k));
        self.messenger.set_targets(targets);
    }

    pub fn contacts_json(&self) -> String {
        serde_json::to_string(&self.contacts.lock_safe().list()).unwrap_or_else(|_| "[]".into())
    }

    /// Хвост переписки (последние [`HISTORY_PAGE`] сообщений), старые сверху.
    pub fn history_json(&self, key: &[u8; 32]) -> String {
        self.chat_history_json(Chat::Dm(key), None)
    }

    /// Страница ВЫШЕ уже показанной: курсор — `(время, строка)` верхнего сообщения
    /// (поля `time` и `row` первого элемента предыдущей страницы).
    pub fn history_page_json(&self, key: &[u8; 32], before_time: i64, before_row: i64) -> String {
        self.chat_history_json(Chat::Dm(key), Some((before_time, before_row)))
    }

    /// Общий путь для личных чатов, «Сохранённого» и групп — хранилище у них одно.
    pub(crate) fn chat_history_json(&self, chat: Chat, before: Option<(i64, i64)>) -> String {
        let page = self
            .messages
            .lock_safe()
            .history(chat, before, HISTORY_PAGE);
        serde_json::to_string(&page).unwrap_or_else(|_| "[]".into())
    }

    pub fn set_verified(&self, key: &[u8; 32], verified: bool) {
        let _ = self.contacts.lock_safe().set_verified(key, verified);
        // Сверка — смена доверия: тир Verified щедрее и по приёму, и по очереди.
        self.refresh_budgets();
    }

    pub fn rename_contact(&self, key: &[u8; 32], name: &str) {
        let _ = self.contacts.lock_safe().rename(key, name);
    }

    /// Удалить контакт вместе с перепиской у себя — так обещает подтверждение в UI.
    /// Раньше уходил только контакт, а история оставалась в базе.
    pub fn remove_contact(&self, key: &[u8; 32]) {
        let _ = self.contacts.lock_safe().remove(key);
        self.clear_dm_history(key);
        self.refresh_budgets();
    }

    // — Гейт контактов (замена TOFU-автодобавления) —
    /// Входящие запросы от незнакомцев как JSON-массив (ключ, fingerprint, превью, время).
    pub fn contact_requests_json(&self) -> String {
        serde_json::to_string(&self.gate.lock_safe().list_requests())
            .unwrap_or_else(|_| "[]".into())
    }

    /// Одобрить запрос: незнакомец становится контактом, снимаем запрос, шлём ему свой
    /// профиль и держим коннект. Его первое сообщение ложится в чат — отправитель
    /// получил на него Ack и второй раз не пришлёт. Дальше всё идёт как обычно.
    pub fn accept_contact_request(&self, key: &[u8; 32]) -> bool {
        let Some(req) = self.gate.lock_safe().take_request(key) else {
            return false;
        };
        // Пустое локальное имя → покажется самоназвание пира или hex (см. Contact.displayName).
        let _ = self.contacts.lock_safe().add(key, "", now());
        if let Some(m) = req.first {
            let _ = self.messages.lock_safe().append(
                Chat::Dm(key),
                &StoredMessage {
                    id: m.id,
                    text: m.text,
                    mine: false,
                    time: m.time,
                    status: String::new(),
                    sender: hex::encode(key),
                    kind: "text".into(),
                    ..Default::default()
                },
            );
        }
        self.refresh_budgets();
        self.messenger.poke(key);
        // Свой профиль — и просьба прислать его: профиль, пришедший до одобрения, мы
        // выбросили (от незнакомцев не принимаем), а сам пир повторил бы его лишь на
        // следующем коннекте — и до тех пор оставался бы у нас безымянным.
        self.send_profile_to(key, true);
        true
    }

    /// Отклонить запрос: тихо убрать из очереди (пир об этом не узнаёт).
    pub fn decline_contact_request(&self, key: &[u8; 32]) -> bool {
        self.gate.lock_safe().take_request(key).is_some()
    }

    /// Заблокировать пира: будущие сообщения тихо дропаются, запрос снят, из контактов
    /// удалён (перестаёт быть доверенным и получать наш профиль).
    pub fn block_contact(&self, key: &[u8; 32]) -> bool {
        let _ = self.gate.lock_safe().block(key, now());
        let _ = self.contacts.lock_safe().remove(key);
        self.refresh_budgets();
        true
    }

    pub fn unblock_contact(&self, key: &[u8; 32]) -> bool {
        let ok = self.gate.lock_safe().unblock(key).unwrap_or(false);
        // ОБЯЗАТЕЛЬНО: снять Tier::Blocked в ingress-лимитере, иначе пир остаётся
        // отрезанным ДО расшифровки и разблокировка ничего не меняет.
        if ok {
            self.refresh_budgets();
        }
        ok
    }

    /// Кто сейчас флудит: `[{peer, dropped, pps, blocked}]`, по убыванию темпа.
    /// Пустой массив — флуда нет.
    ///
    /// Остановить чужой поток на устройстве нельзя (threat-model N8): байты уже
    /// оплачены радио. Но пользователь имеет право знать, почему тает трафик, и
    /// решить — уйти в офлайн или сменить адрес. Отсюда и снимок.
    pub fn flood_json(&self) -> String {
        let tiers = self.tier_cache.lock_safe();
        let list: Vec<dto::FloodPeer> = self
            .messenger
            .flood_stats()
            .into_iter()
            .map(|f| dto::FloodPeer {
                peer: hex::encode(f.peer),
                dropped: f.dropped,
                pps: (f.pps * 10.0).round() / 10.0, // десятых хватает, UI не нужен шум
                blocked: tiers.get(&f.peer).map(|t| t.is_blocked()).unwrap_or(false),
            })
            .collect();
        serde_json::to_string(&list).unwrap_or_else(|_| "[]".into())
    }

    /// Снимок офлайн-очереди для UI: `{pending, bytes, peers, warn}`. Ничего не
    /// удаляем автоматически — по `warn` клиент показывает предупреждение, а решение
    /// (подождать или сбросить очередь) остаётся за пользователем.
    pub fn outbox_stats_json(&self) -> String {
        let s = self.outbox.lock_safe().stats();
        let dto = dto::OutboxStats {
            pending: s.pending,
            bytes: s.bytes,
            peers: s.peers,
            warn: s.should_warn(),
        };
        serde_json::to_string(&dto).unwrap_or_else(|_| "{}".into())
    }

    /// Очередь по получателям: `[{peer, count, bytes, oldest}]`. Сумма «ждёт 1248»
    /// ничего не говорит, пока не видно, что 1202 из них — одному человеку.
    pub fn outbox_by_peer_json(&self) -> String {
        let rows: Vec<dto::OutboxPeer> = self
            .outbox
            .lock_safe()
            .per_peer()
            .into_iter()
            .map(|(peer, count, bytes, oldest)| dto::OutboxPeer {
                peer: hex::encode(peer),
                count,
                bytes,
                oldest,
            })
            .collect();
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
    }

    /// Сбросить очередь ВСЕМ (двойное подтверждение — на стороне UI).
    pub fn clear_outbox_all(&self) -> usize {
        let entries = self.outbox.lock_safe().waiting(None);
        self.give_up(entries)
    }

    /// Сбросить очередь недоставленного конкретному пиру (явное действие пользователя).
    pub fn clear_outbox_for(&self, key: &[u8; 32]) -> usize {
        let entries = self.outbox.lock_safe().waiting(Some(key));
        self.give_up(entries)
    }

    /// Выбросить записи из досылки. Своё сообщение переписки становится «не
    /// доставлено» (запись остаётся для «Повторить»), служебный пакет — стирается:
    /// раньше выброшенное навсегда оставалось в чате «в очереди». Возвращает число.
    fn give_up(&self, entries: Vec<peerage_core::outbox::OutboxEntry>) -> usize {
        let n = entries.len();
        for e in entries {
            self.messenger.hold(&e.peer, e.msg_id);
            let failed = self
                .messages
                .lock_safe()
                .set_status(Chat::Dm(&e.peer), e.msg_id, "failed")
                .unwrap_or(false);
            if failed {
                self.outbox.lock_safe().fail(e.id);
                push_event(
                    self,
                    dto::UiEvent::Failed {
                        peer: hex::encode(e.peer),
                        id: e.msg_id,
                    },
                );
            } else {
                self.outbox.lock_safe().remove(e.id);
            }
        }
        n
    }

    /// «Повторить» не доставленное: запись возвращается в очередь с тем же id —
    /// получатель отбросит дубль, если первая копия всё же дошла. false — повторять
    /// нечего (уже доставлено или стёрто).
    pub fn retry_message(&self, key: &[u8; 32], id: u64) -> bool {
        if !self.outbox.lock_safe().retry(key, id, now()) {
            return false;
        }
        let _ = self
            .messages
            .lock_safe()
            .set_status(Chat::Dm(key), id, "pending");
        self.messenger.resume(key);
        true
    }

    /// Очистить историю личного чата У СЕБЯ: сообщения, закрепы и то, что ещё ждёт
    /// отправки этому собеседнику (иначе очередь дошла бы позже и «вернула» в чат
    /// часть стёртого). Собеседник ничего не узнаёт, его копия остаётся. Контакт,
    /// флаги чата («без звука», закреп в списке) и скачанные вложения — не трогаем:
    /// файлы чистятся в «Хранилище», где видно, сколько они занимают.
    pub fn clear_dm_history(&self, key: &[u8; 32]) {
        let msgs = self.messages.lock_safe();
        let _ = msgs.clear(Chat::Dm(key));
        let _ = msgs.clear_pins(Chat::Dm(key));
        drop(msgs);
        self.outbox.lock_safe().clear_peer(key);
    }

    /// Что занимает место: вложения (блобы) и очередь недоставленного.
    /// JSON `{blobs, blob_bytes, outbox_pending, outbox_bytes}`.
    pub fn storage_stats_json(&self) -> String {
        let (mut blobs, mut blob_bytes) = (0u64, 0u64);
        if let Ok(rd) = std::fs::read_dir(&self.blob_dir) {
            for e in rd.flatten() {
                if let Ok(meta) = e.metadata() {
                    if meta.is_file() {
                        blobs += 1;
                        blob_bytes += meta.len();
                    }
                }
            }
        }
        let ob = self.outbox.lock_safe().stats();
        // Разбивка «что съело место»: по типу вложения и по чату. Считаем по РЕАЛЬНЫМ
        // файлам на диске (сообщение может ссылаться на ещё не скачанный блоб), а один
        // и тот же блоб в разных чатах не дублируем — он контент-адресный.
        let mut by_kind: HashMap<String, u64> = HashMap::new();
        let mut by_peer: HashMap<[u8; 32], (u64, u64)> = HashMap::new();
        let mut counted: std::collections::HashSet<String> = std::collections::HashSet::new();
        for a in self.messages.lock_safe().attachments() {
            let (blob_id, kind) = (a.blob_id, a.kind);
            let peer = <[u8; 32]>::try_from(a.chat.as_slice()).unwrap_or([0u8; 32]);
            let path = self.blob_dir.join(&blob_id);
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let len = meta.len();
            if counted.insert(blob_id.clone()) {
                *by_kind.entry(kind).or_insert(0) += len;
            }
            let e = by_peer.entry(peer).or_insert((0, 0));
            e.0 += 1;
            e.1 += len;
        }
        let mut chats: Vec<dto::ChatStorage> = by_peer
            .into_iter()
            .map(|(p, (files, bytes))| dto::ChatStorage {
                peer: hex::encode(p),
                files,
                bytes,
            })
            .collect();
        // Самый тяжёлый чат первым — с него и начнут чистить.
        chats.sort_by_key(|c| std::cmp::Reverse(c.bytes));
        let dto = dto::StorageStats {
            blobs,
            blob_bytes,
            outbox_pending: ob.pending,
            outbox_bytes: ob.bytes,
            by_kind,
            chats,
        };
        serde_json::to_string(&dto).unwrap_or_else(|_| "{}".into())
    }

    /// Удалить ВСЕ сохранённые вложения (явное действие пользователя). Не трогает
    /// историю сообщений: ссылки остаются, и байты можно скачать у собеседника
    /// заново — блобы контент-адресные. Возвращает освобождённые байты.
    /// Удалить сохранённые вложения (R-030). Остаются аватары (свой — его у нас
    /// запрашивают контакты; их — иначе пропали бы до следующей смены) и миниатюры
    /// (без них карточки пусты). Идущие докачки отменяются явно, пометки «скачано»
    /// снимаются — карточки снова предлагают «Скачать». Возвращает освобождённые байты.
    pub fn clear_blobs(&self) -> u64 {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        if self.jobs.send(Job::CancelDownloads(done_tx)).is_ok() {
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
        }
        let wanted = self.wanted.lock_safe().all();
        for w in wanted {
            let _ = self.wanted.lock_safe().remove(&w.blob_id);
        }
        self.blob_expect.lock_safe().clear();

        let mut keep: std::collections::HashSet<String> =
            self.messages.lock_safe().thumb_ids().into_iter().collect();
        keep.extend(self.self_avatar_id());
        keep.extend(
            self.contacts
                .lock_safe()
                .list()
                .iter()
                .map(|c| c.avatar_id.clone())
                .filter(|a| !a.is_empty()),
        );

        let mut freed = 0u64;
        if let Ok(rd) = std::fs::read_dir(&self.blob_dir) {
            for e in rd.flatten() {
                let Ok(meta) = e.metadata() else { continue };
                let name = e.file_name().to_string_lossy().to_string();
                if !meta.is_file() || keep.contains(&name) {
                    continue;
                }
                if std::fs::remove_file(e.path()).is_ok() {
                    freed += meta.len();
                }
            }
        }
        let _ = self.messages.lock_safe().reset_downloaded(&keep);
        freed
    }

    pub fn is_online(&self, key: &[u8; 32]) -> bool {
        self.messenger.is_online(key)
    }

    // — Сеть —
    pub fn net_peers(&self) -> i32 {
        self.messenger.net_peers() as i32
    }

    pub fn net_tree(&self) -> i32 {
        self.messenger.net_tree()
    }

    /// Снапшот стата сети: пиры + агрегаты. Агрегаты считает движок (см. net.rs).
    pub fn net_stats(&self) -> NetStats {
        let peers: Vec<PeerStat> =
            serde_json::from_str(&self.messenger.net_peers_json()).unwrap_or_default();
        NetStats::aggregate(peers, self.net_tree())
    }

    /// Принудительно переподключить публичных пиров (обход backoff).
    pub fn reconnect_peers(&self) {
        for uri in public_peer_uris() {
            self.messenger.add_peer(uri);
        }
        log::info!("reconnect_peers: re-added public peers");
    }

    // — Группы (MLS) — мутации fire-and-forget через актора, чтение из кэша —
    /// Создать группу. Возвращает true, если команда принята актором.
    pub fn group_create(&self, name: &str) -> bool {
        self.jobs
            .send(Job::Cmd(GroupCmd::Create {
                name: name.to_string(),
            }))
            .is_ok()
    }

    pub fn group_invite(&self, gid: Vec<u8>, peer: [u8; 32]) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::Invite { gid, peer }));
    }

    /// Входящие приглашения в группы как JSON [{gid, name, inviter}] (из кэша).
    pub fn group_invites_json(&self) -> String {
        let s = self.group_cache.lock_safe().invites.clone();
        if s.is_empty() {
            "[]".into()
        } else {
            s
        }
    }

    /// Принять приглашение в группу (карточка). Fire-and-forget через актора.
    pub fn accept_group_invite(&self, gid: Vec<u8>) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::AcceptInvite { gid }));
    }

    /// Отклонить приглашение в группу. Fire-and-forget.
    pub fn decline_group_invite(&self, gid: Vec<u8>) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::DeclineInvite { gid }));
    }

    pub fn group_send_text(&self, gid: Vec<u8>, text: String) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::SendText {
            gid,
            text,
            reply_to: 0,
            reply_sender: String::new(),
            fwd_from: String::new(),
            fwd_anon: false,
            mentions: Vec::new(),
        }));
    }

    /// Текст ответом на сообщение `reply_to` автора `reply_sender` (hex; `0` — не
    /// ответ), с упоминаниями поверх участков текста.
    pub fn group_send_text_reply(
        &self,
        gid: Vec<u8>,
        text: String,
        reply_to: u64,
        reply_sender: String,
        mentions: Vec<Mention>,
    ) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::SendText {
            gid,
            text,
            reply_to,
            reply_sender,
            fwd_from: String::new(),
            fwd_anon: false,
            mentions,
        }));
    }

    /// Отозвать СВОИ сообщения в группе у всех участников.
    pub fn group_retract(&self, gid: Vec<u8>, ids: Vec<u64>) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::Retract { gid, ids }));
    }

    /// Удалить групповые сообщения только у себя.
    pub fn group_delete_local(&self, gid: Vec<u8>, refs: Vec<(u64, String)>) {
        let _ = self
            .jobs
            .send(Job::Cmd(GroupCmd::DeleteLocal { gid, refs }));
    }

    pub fn group_send_media(
        &self,
        gid: Vec<u8>,
        content: Vec<u8>,
        mime: String,
        name: String,
        info: MediaInfo,
    ) {
        let _ = self.jobs.send(Job::GroupMedia {
            gid,
            source: MediaSource::Bytes(content),
            mime,
            name,
            info,
        });
    }

    /// Отправить в группу вложение, лежащее ФАЙЛОМ, не поднимая его в память.
    /// Fire-and-forget: манифест и своя копия считаются в потоке подготовки.
    pub fn group_send_media_file(
        &self,
        gid: Vec<u8>,
        path: PathBuf,
        mime: String,
        name: String,
        info: MediaInfo,
    ) {
        let _ = self.jobs.send(Job::GroupMedia {
            gid,
            source: MediaSource::File(path),
            mime,
            name,
            info,
        });
    }

    /// Отозвать участника из группы (ADR-004, только админ). Fire-and-forget.
    pub fn group_remove_member(&self, gid: Vec<u8>, peer: [u8; 32]) {
        let _ = self
            .jobs
            .send(Job::Cmd(GroupCmd::RemoveMember { gid, peer }));
    }

    /// Ротация ключа группы (PCS, ADR-004). Fire-and-forget.
    pub fn group_rekey(&self, gid: Vec<u8>) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::Rekey { gid }));
    }

    /// Выйти из группы (ADR-004). Fire-and-forget.
    pub fn group_leave(&self, gid: Vec<u8>) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::Leave { gid }));
    }

    // — Блобы (M2) —
    /// Контент-адрес (hex) содержимого — для ссылки на блоб из сообщения/аватарки.
    pub fn blob_id_hex(&self, content: &[u8]) -> String {
        hex::encode(peerage_blob::blob_id(content))
    }

    /// Прочитать сохранённый блоб по hex-id (принятый или свой отправленный).
    /// Расшифровывает at-rest (`vault::open`).
    pub fn blob_read(&self, id_hex: &str) -> Option<Vec<u8>> {
        read_blob_at(&self.blob_dir, id_hex)
    }

    /// Расшифровать блоб в файл `dst` потоково, по одному чанку: так открывают и
    /// проигрывают крупные вложения, не поднимая их в память. Пишем через `.part`
    /// и переименование — читатель не увидит недописанный файл.
    pub fn blob_export(&self, id_hex: &str, dst: &Path) -> bool {
        use std::io::Write;
        let Ok(raw) = hex::decode(id_hex.trim()) else {
            return false;
        };
        let src = self.blob_dir.join(hex::encode(&raw));
        let part = dst.with_extension("part");
        let ok = (|| -> Option<()> {
            let mut out = std::io::BufWriter::new(std::fs::File::create(&part).ok()?);
            match peerage_blob::disk::read_header(&src) {
                Some((_cs, n, _size)) => {
                    for i in 0..n {
                        let c = peerage_blob::disk::read_chunk(&src, i, |i, b| {
                            vault::open(&vault::chunk_purpose(&raw, i), b).ok()
                        })?;
                        out.write_all(&c).ok()?;
                    }
                }
                // Мелкий блоб лежит одним запечатанным буфером — он и так невелик.
                None => {
                    let bytes =
                        vault::open(&vault::blob_purpose(&raw), &std::fs::read(&src).ok()?).ok()?;
                    out.write_all(&bytes).ok()?
                }
            }
            out.flush().ok()?;
            drop(out);
            std::fs::rename(&part, dst).ok()
        })();
        if ok.is_none() {
            let _ = std::fs::remove_file(&part);
        }
        ok.is_some()
    }

    /// Есть ли блоб локально. Отдельно от [`blob_read`], потому что «скачано ли
    /// вложение» нельзя выяснять чтением: у крупного это гигабайт в память ради
    /// одного булева.
    pub fn blob_exists(&self, id_hex: &str) -> bool {
        let Ok(raw) = hex::decode(id_hex.trim()) else {
            return false;
        };
        self.blob_dir.join(hex::encode(raw)).exists()
    }

    // — Свой профиль (M2): имя + аватар (контент-адресный блоб). Хранится в
    // profile_meta.json; рассылается контактам при смене и на коннекте.
    fn store_blob(&self, id: peerage_blob::Hash, content: &[u8]) {
        write_blob_file(&self.blob_dir, id, content);
    }

    fn profile_meta_path(&self) -> PathBuf {
        self.blob_dir
            .parent()
            .unwrap_or(&self.blob_dir)
            .join("profile_meta.json")
    }

    fn load_profile_meta(&self) -> ProfileMeta {
        std::fs::read(self.profile_meta_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_profile_meta(&self, m: &ProfileMeta) {
        if let Ok(v) = serde_json::to_vec(m) {
            let _ = peerage_core::fsutil::write_atomic(&self.profile_meta_path(), v);
        }
    }

    /// Своё самоназвание.
    pub fn self_name(&self) -> String {
        self.load_profile_meta().name
    }

    /// Установить своё имя → сохранить и разослать контактам.
    pub fn set_self_name(&self, name: &str) {
        let mut m = self.load_profile_meta();
        m.name = name.to_string();
        self.save_profile_meta(&m);
        self.broadcast_profile();
    }

    /// Установить свой аватар: scrub (PET) → контент-адресный блоб → запись id+mime.
    /// Разослать контактам. Возвращает hex-id.
    pub fn set_self_avatar(&self, content: Vec<u8>, mime: &str) -> String {
        let scrubbed = peerage_blob::scrub::scrub(mime, &content);
        let id = peerage_blob::blob_id(&scrubbed);
        self.store_blob(id, &scrubbed);
        let id_hex = hex::encode(id);
        let mut m = self.load_profile_meta();
        m.avatar_id = id_hex.clone();
        m.avatar_mime = mime.to_string();
        self.save_profile_meta(&m);
        self.broadcast_profile();
        id_hex
    }

    /// hex-id своего аватара, если задан.
    pub fn self_avatar_id(&self) -> Option<String> {
        let id = self.load_profile_meta().avatar_id;
        (!id.is_empty()).then_some(id)
    }

    /// Байты своего аватара (расшифрованные at-rest), если задан.
    pub fn self_avatar(&self) -> Option<Vec<u8>> {
        self.blob_read(&self.self_avatar_id()?)
    }

    /// Послать свой профиль одному пиру; `ask_back` — попросить его профиль в ответ.
    fn send_profile_to(&self, peer: &[u8; 32], ask_back: bool) {
        let m = self.load_profile_meta();
        let hash = hex::decode(&m.avatar_id).unwrap_or_default();
        let _ = self
            .messenger
            .send_profile_update(peer, &m.name, &hash, ask_back);
    }

    /// Разослать свой профиль (имя + хэш аватара) всем контактам (M2). Fire-and-forget.
    pub fn broadcast_profile(&self) {
        let keys: Vec<[u8; 32]> = self
            .contacts
            .lock_safe()
            .list()
            .iter()
            .filter_map(|c| parse_key(&c.key))
            .collect();
        for k in keys {
            self.send_profile_to(&k, false);
        }
    }

    /// Список групп для клиента: живые (мы в них состоим) плюс АРХИВНЫЕ — те, чья
    /// переписка есть в базе, а членства нет. Архив появляется после восстановления
    /// копии: сообщения в неё входят, MLS-состояние — нет (ADR-004), поэтому читать
    /// такую группу можно, а писать в неё нельзя, пока не позовут заново.
    pub fn group_list_json(&self) -> String {
        let cached = self.group_cache.lock_safe().list.clone();
        let mut rows: Vec<GroupRowJson> = serde_json::from_str(&cached).unwrap_or_default();
        let live: std::collections::HashSet<String> = rows.iter().map(|r| r.gid.clone()).collect();
        for (gid, name) in self.messages.lock_safe().chat_titles(1) {
            let hex = hex::encode(&gid);
            if !live.contains(&hex) {
                rows.push(GroupRowJson {
                    gid: hex,
                    name,
                    members: 0,
                    admin: false,
                    admin_key: String::new(),
                    archived: true,
                });
            }
        }
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
    }

    /// Хвост истории группы — из общего хранилища, без кэша: раньше кэш жил в
    /// акторе и UI отставал на одно действие.
    pub fn group_history_json(&self, gid: &[u8]) -> String {
        self.chat_history_json(Chat::Group(gid), None)
    }

    /// Страница истории группы ВЫШЕ показанной; курсор — `(время, строка)`.
    pub fn group_history_page_json(&self, gid: &[u8], before_time: i64, before_row: i64) -> String {
        self.chat_history_json(Chat::Group(gid), Some((before_time, before_row)))
    }

    pub fn group_members_json(&self, gid: &[u8]) -> String {
        self.group_cache
            .lock_safe()
            .members
            .get(gid)
            .cloned()
            .unwrap_or_else(|| "[]".into())
    }

    /// Снапшот присутствия группы «N уч · M в сети» (ростер ∩ online). Презенс
    /// динамичен → считаем на момент чтения. Кэш `members` — ДРУГИЕ (без себя).
    pub fn group_presence(&self, gid: &[u8]) -> GroupPresence {
        let json = self.group_members_json(gid);
        let others: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
        let online_others = others
            .iter()
            .filter_map(|h| parse_key(h))
            .filter(|k| self.messenger.is_online(k))
            .count();
        GroupPresence::compute(others.len(), online_others)
    }

    // — Комнаты-звонки (присутствие, Discord-style) —
    pub(crate) fn room_engine(&self) -> Option<Arc<RoomCallEngine>> {
        self.room_call
            .lock_safe()
            .as_ref()
            .map(|r| r.engine.clone())
    }

    pub(crate) fn live_participants(&self, gid: &[u8]) -> Vec<[u8; 32]> {
        self.rooms
            .lock_safe()
            .live_with_flags(gid, Instant::now(), PRESENCE_TTL)
            .into_iter()
            .map(|(p, _)| p)
            .collect()
    }

    fn sync_room_members(&self) {
        let guard = self.room_call.lock_safe();
        if let Some(rc) = guard.as_ref() {
            let mut live = self.live_participants(&rc.gid);
            // `members` — адресаты нашего голоса и видео, поэтому маячок присутствия
            // сам по себе не пропуск. В DM-комнате бывает ровно один собеседник; в
            // групповой — только текущий MLS-ростер (SEC-2), и пустой ростер значит
            // «никого», а не «всех».
            let dm_peer = self.rooms.lock_safe().dm_peers.get(&rc.gid).copied();
            match dm_peer {
                Some(peer) => live.retain(|p| *p == peer),
                None => {
                    let roster = self.group_roster_set(&rc.gid);
                    live.retain(|p| roster.contains(p));
                }
            }
            *rc.members.lock_safe() = live;
        }
    }

    /// Множество ключей текущего MLS-ростера группы (без себя) из кэша — для SEC-2.
    fn group_roster_set(&self, gid: &[u8]) -> std::collections::HashSet<[u8; 32]> {
        serde_json::from_str::<Vec<String>>(&self.group_members_json(gid))
            .unwrap_or_default()
            .iter()
            .filter_map(|h| parse_key(h))
            .collect()
    }

    /// DM-звонок: создать комнату, послать RoomInvite, начать слать маячок.
    /// Возвращает room_id.
    pub fn room_invite_dm(&self, peer: [u8; 32], audio: bool, video: bool) -> Vec<u8> {
        let room_id = peerage_core::dm_room_id(&self.me_pub, &peer);
        // Пишем сразу как «не дозвонился»: исход поднимется до answered, когда
        // собеседник появится в комнате. Иначе оборванная попытка не попала бы в
        // журнал вовсе — а именно её чаще всего и ищут.
        self.begin_call(&peer, &room_id, false, video, "unanswered");
        if let Ok(id) = self
            .messenger
            .send_room_invite(&peer, &room_id, audio, video)
        {
            self.rings
                .lock_safe()
                .entry(room_id.clone())
                .or_default()
                .push(id);
        }
        let flags = av_flags(audio, video);
        {
            let mut rs = self.rooms.lock_safe();
            rs.joined.insert(room_id.clone(), flags);
            rs.dm_peers.insert(room_id.clone(), peer);
        }
        let _ = self.jobs.send(Job::Beacon);
        room_id
    }

    pub fn room_decline_dm(&self, peer: [u8; 32]) {
        let room_id = peerage_core::dm_room_id(&self.me_pub, &peer);
        let _ = self.messenger.send_room_decline(&peer, &room_id);
        // «Отклонён» — исход только ВХОДЯЩЕГО: отклонить может лишь принимающая
        // сторона. Свой же исходящий, который я сам и завершил, отклонённым не был —
        // он остаётся «не дозвонился».
        self.decline_incoming();
    }

    /// Пометить текущий ВХОДЯЩИЙ звонок отклонённым и закрыть его.
    fn decline_incoming(&self) {
        let id = {
            let mut ac = self.active_call.lock_safe();
            let id = ac.as_ref().filter(|c| c.incoming).map(|c| c.id);
            if id.is_some() {
                *ac = None;
            }
            id
        };
        if let Some(id) = id {
            self.call_outcome(id, "declined");
        }
    }

    /// Принять DM-звонок: войти в комнату с регистрацией dm_peer. Возвращает room_id.
    pub fn room_join_dm(&self, peer: [u8; 32], audio: bool, video: bool) -> Vec<u8> {
        let room_id = peerage_core::dm_room_id(&self.me_pub, &peer);
        // Входящий, который мы приняли: запись уже заведена на RoomInvite. Комнату
        // сверяем — активным может быть и другой звонок (R-034).
        if !self.answer_call(&room_id) {
            self.begin_call(&peer, &room_id, true, video, "answered");
        }
        let flags = av_flags(audio, video);
        {
            let mut rs = self.rooms.lock_safe();
            rs.joined.insert(room_id.clone(), flags);
            rs.dm_peers.insert(room_id.clone(), peer);
        }
        let _ = self.jobs.send(Job::Beacon);
        room_id
    }

    /// Позвать участников группы в её комнату: каждому RoomInvite с room_id = gid.
    /// Ключи вне ростера отбрасываются — звать в группу можно только её членов.
    /// Возвращает, скольким ушло приглашение.
    pub fn room_ring(&self, gid: &[u8], peers: &[[u8; 32]], audio: bool, video: bool) -> usize {
        let roster = self.group_roster_set(gid);
        peers
            .iter()
            .filter(|p| roster.contains(*p))
            .filter_map(|p| self.messenger.send_room_invite(p, gid, audio, video).ok())
            .map(|id| {
                self.rings
                    .lock_safe()
                    .entry(gid.to_vec())
                    .or_default()
                    .push(id)
            })
            .count()
    }

    /// Вправе ли `peer` звать нас в комнату `room_id` и находиться в ней: это либо
    /// наша DM-комната с ним (и он — контакт), либо комната группы, в ростере которой
    /// он есть. Ростер ещё не прогрет — не пускаем никого: присутствие в комнате —
    /// это адрес, куда уходят наш голос и видео.
    ///
    /// Сверять DM-комнату именно с `dm_room_id(мы, он)` обязательно: id комнаты
    /// вычисляется из двух публичных ключей, и без этой сверки любой, кто знает
    /// ключи двух собеседников, заявил бы себя участником их разговора.
    fn room_peer_allowed(&self, peer: &[u8; 32], room_id: &[u8]) -> bool {
        if room_id == peerage_core::dm_room_id(&self.me_pub, peer).as_slice() {
            return self.contacts.lock_safe().contains(peer);
        }
        self.group_roster_set(room_id).contains(peer)
    }

    /// Отклонить приглашение в комнату группы. Звонящему ничего не шлём: он уже в
    /// комнате и сидит там дальше, «сбросить» ему нечего. Только журнал.
    pub fn room_decline_group(&self) {
        self.decline_incoming();
    }

    pub fn room_join(&self, gid: Vec<u8>, audio: bool, video: bool) {
        // Вход по приглашению: запись заведена на RoomInvite как пропущенная.
        self.answer_call(&gid);
        self.rooms
            .lock_safe()
            .joined
            .insert(gid, av_flags(audio, video));
        let _ = self.jobs.send(Job::Beacon);
    }

    pub fn room_leave(&self, gid: Vec<u8>) {
        self.end_call();
        self.missed_calls.lock_safe().remove(&gid);
        // Ушли — недоставленные приглашения в эту комнату снимаем: иначе они
        // зазвонили бы у собеседника, появившегося позже (R-007).
        for id in self.rings.lock_safe().remove(&gid).unwrap_or_default() {
            self.messenger.cancel(id);
        }
        {
            let mut rs = self.rooms.lock_safe();
            rs.joined.remove(&gid);
            rs.remote.remove(&gid);
        }
        let _ = self.jobs.send(Job::Leave(gid));
    }

    /// Начать запись о звонке (одна активная за раз — звонков параллельно не бывает).
    fn begin_call(&self, peer: &[u8; 32], gid: &[u8], incoming: bool, video: bool, outcome: &str) {
        let id = self
            .calls
            .lock_safe()
            .start(peer, gid, incoming, video, outcome, now());
        if id >= 0 {
            *self.active_call.lock_safe() = Some(ActiveCall {
                id,
                since: Instant::now(),
                incoming,
                room: gid.to_vec(),
            });
            self.sync_call_row(id);
        }
    }

    /// Сменить исход звонка — и его строку в ленте.
    fn call_outcome(&self, id: i64, outcome: &str) {
        self.calls.lock_safe().set_outcome(id, outcome);
        self.sync_call_row(id);
    }

    /// Закрыть звонок длительностью — и его строку в ленте.
    fn call_finish(&self, id: i64, secs: i64) {
        self.calls.lock_safe().finish(id, secs);
        self.sync_call_row(id);
    }

    /// Строка личного звонка в ленте (журнал движка, 2.8б): отражение записи журнала
    /// в чате с этим человеком. Звонки комнаты группы в ленту не идут. Каждый вызов
    /// заменяет строку на месте — у клиента и в базе она одна на звонок.
    fn sync_call_row(&self, id: i64) {
        let Some(c) = self.calls.lock_safe().get(id) else {
            return;
        };
        if c.gid != peerage_core::dm_room_id(&self.me_pub, &c.peer) {
            return;
        }
        let msg = StoredMessage {
            id: id as u64,
            sender: if c.incoming {
                hex::encode(c.peer)
            } else {
                self.me_hex()
            },
            mine: !c.incoming,
            text: c.outcome,
            time: c.started,
            kind: if c.video { "video_call" } else { "call" }.into(),
            duration_ms: (c.duration.max(0) * 1000).min(u32::MAX as i64) as u32,
            ..Default::default()
        };
        let row = self
            .messages
            .lock_safe()
            .upsert_call(Chat::Dm(&c.peer), &msg);
        if let Ok(row) = row {
            push_event(
                self,
                dto::UiEvent::Message {
                    chat: hex::encode(c.peer),
                    group: false,
                    msg: StoredMessage { row, ..msg },
                },
            );
        }
    }

    /// Пропущенный входящий, пока идёт другой разговор: отдельная строка журнала,
    /// активная запись не трогается.
    fn record_missed(&self, peer: &[u8; 32], gid: &[u8], video: bool) {
        let id = self
            .calls
            .lock_safe()
            .start(peer, gid, true, video, "missed", now());
        if id >= 0 {
            self.missed_calls.lock_safe().insert(gid.to_vec(), id);
            self.sync_call_row(id);
        }
    }

    /// Мы вошли в комнату `room` по входящему звонку: его строка журнала становится
    /// отвеченной и активной. Строка — либо активный входящий именно этой комнаты,
    /// либо звонок, пропущенный посреди другого разговора (тот разговор тогда
    /// закрывается: звонков параллельно не бывает). `false` — звонка в эту комнату
    /// не было.
    fn answer_call(&self, room: &[u8]) -> bool {
        let mut ac = self.active_call.lock_safe();
        if let Some(id) = ac
            .as_ref()
            .filter(|c| c.incoming && c.room == room)
            .map(|c| c.id)
        {
            drop(ac);
            self.call_outcome(id, "answered");
            return true;
        }
        let Some(id) = self.missed_calls.lock_safe().remove(room) else {
            return false;
        };
        let prev = ac.replace(ActiveCall {
            id,
            since: Instant::now(),
            incoming: true,
            room: room.to_vec(),
        });
        drop(ac);
        if let Some(prev) = prev {
            self.call_finish(prev.id, prev.since.elapsed().as_secs() as i64);
        }
        self.call_outcome(id, "answered");
        true
    }

    /// Закрыть активную запись: проставить длительность.
    fn end_call(&self) {
        let Some(c) = self.active_call.lock_safe().take() else {
            return;
        };
        self.call_finish(c.id, c.since.elapsed().as_secs() as i64);
    }

    /// Заметка себе («Сохранённое»). Хранится как обычное сообщение в чате с самим
    /// собой: никуда не отправляется, статусов доставки у неё нет — отправлять некому.
    /// Отдельное хранилище заводить незачем, история уже умеет всё нужное.
    pub fn save_note(&self, text: &str) -> Option<i64> {
        let t = now();
        let me = self.me_pub;
        let mut note = StoredMessage {
            // id — миллисекунды: в секундах вторая заметка за ту же секунду
            // получала id первой (R-040). Совпало и так — берём следующий.
            id: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            sender: self.me_hex(),
            text: text.to_string(),
            mine: true,
            time: t,
            kind: "text".into(),
            ..Default::default()
        };
        let store = self.messages.lock_safe();
        loop {
            match store.append(Chat::Dm(&me), &note) {
                Ok(_) => return Some(t),
                Err(peerage_core::store::StoreError::Duplicate) => note.id += 1,
                Err(_) => return None,
            }
        }
    }

    /// Заметки себе, старые первыми (как в чате).
    pub fn notes_json(&self) -> String {
        let me = self.me_pub;
        self.history_json(&me)
    }

    /// Удалить заметку по её id.
    pub fn delete_note(&self, id: u64) {
        let me = self.me_pub;
        let _ = self
            .messages
            .lock_safe()
            .delete_one(Chat::Dm(&me), &self.me_hex(), id);
    }

    /// Журнал звонков для UI: `[{peer, gid, incoming, outcome, started, duration}]`.
    pub fn calls_json(&self, limit: usize) -> String {
        let rows: Vec<dto::CallEntry> = self
            .calls
            .lock_safe()
            .list(limit)
            .into_iter()
            .map(|c| dto::CallEntry {
                peer: hex::encode(c.peer),
                gid: hex::encode(&c.gid),
                incoming: c.incoming,
                outcome: c.outcome,
                started: c.started,
                duration: c.duration,
                video: c.video,
            })
            .collect();
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
    }

    /// Очистить журнал (явное действие пользователя).
    pub fn clear_calls(&self) {
        self.calls.lock_safe().clear();
    }

    /// Живые участники любой комнаты по маячкам: JSON [{peer,audio,video}].
    pub fn room_presence_peers_json(&self, gid: &[u8]) -> String {
        let live = self
            .rooms
            .lock_safe()
            .live_with_flags(gid, Instant::now(), PRESENCE_TTL);
        room_members_json(live)
    }

    /// Текущие участники комнаты (то же, что presence_peers — по маячкам).
    pub fn room_participants_json(&self, gid: &[u8]) -> String {
        self.room_presence_peers_json(gid)
    }

    /// Запустить мультипати аудио комнаты. `bitrate` (бит/с, >0) и `music` (Audio-режим
    /// vs Voip) — из настроек звука; 0 = дефолт. Возвращает JSON {sample_rate,channels,frame_len}.
    pub fn room_audio_start(&self, gid: Vec<u8>, bitrate: i32, music: bool) -> String {
        let mut audio = AudioSettings::default();
        audio.jitter_depth = 5;
        if bitrate > 0 {
            audio.bitrate = bitrate;
            audio.mode = if music {
                peerage_core::settings::OpusMode::Audio
            } else {
                peerage_core::settings::OpusMode::Voip
            };
        }
        // Адресаты — пусто до сверки: список собирает только sync_room_members, по
        // тем же правилам, что и дальше по ходу звонка (DM-собеседник / ростер).
        let members = Arc::new(Mutex::new(Vec::new()));
        let engine = RoomCallEngine::start(self.messenger.clone(), audio, members.clone());
        let old = self.room_call.lock_safe().replace(RoomCall {
            gid,
            engine,
            members,
        });
        if let Some(old) = old {
            old.engine.halt();
            std::thread::spawn(move || old.engine.stop());
        }
        self.sync_room_members();
        call_params(&audio)
    }

    /// Остановить звук комнаты. Отправка гаснет до возврата (`halt`), потоки
    /// дожидаются в фоне.
    pub fn room_audio_stop(&self) {
        let rc = self.room_call.lock_safe().take();
        if let Some(rc) = rc {
            rc.engine.halt();
            std::thread::spawn(move || rc.engine.stop());
        }
    }

    /// PCM с микрофона; последний сэмпл записан `age` назад.
    pub fn room_push_audio(&self, samples: &[i16], age: std::time::Duration) {
        if let Some(engine) = self.room_engine() {
            engine.push_audio(samples, age);
        }
    }

    /// Смешанный PCM; от `out` до динамика звук идёт `output`.
    pub fn room_pull_audio(&self, out: &mut [i16], output: std::time::Duration) -> usize {
        self.room_engine()
            .map(|e| e.pull_audio(out, output))
            .unwrap_or(0)
    }

    /// Hex-ключи говорящих сейчас участников (active-speaker). "[]" вне звонка.
    pub fn room_speaking_json(&self) -> String {
        self.room_engine()
            .map(|e| e.speaking_json())
            .unwrap_or_else(|| "[]".into())
    }

    /// Сменить Opus-битрейт текущей аудио-сессии на лету (настройки звука без перезвона).
    pub fn room_set_audio_bitrate(&self, bitrate: i32) {
        if let Some(e) = self.room_engine() {
            e.set_bitrate(bitrate);
        }
    }

    /// Накопленные байты трафика по участникам {hex: rx, "self": tx}. "{}" вне звонка.
    pub fn room_traffic_json(&self) -> String {
        self.room_engine()
            .map(|e| e.traffic_json())
            .unwrap_or_else(|| "{}".into())
    }

    pub fn room_set_muted(&self, muted: bool) {
        if let Some(engine) = self.room_engine() {
            engine.set_muted(muted);
        }
    }

    /// H264 access-unit, снятый `age` назад. `true` — участник просит ключевой кадр.
    pub fn room_push_video(
        &self,
        frame: &[u8],
        info: litertc::VideoInfo,
        age: std::time::Duration,
    ) -> bool {
        self.room_engine()
            .is_some_and(|e| e.push_video(frame, info, age))
    }

    /// Задержка нашего видео-декодера для участника (срок кадра → экран).
    pub fn room_set_video_render_delay(&self, peer: &[u8; 32], delay: std::time::Duration) {
        if let Some(engine) = self.room_engine() {
            engine.set_video_render_delay(peer, delay);
        }
    }

    /// Потолок битрейта видео из настроек качества.
    pub fn room_set_video_max_bitrate(&self, bps: u32) {
        if let Some(engine) = self.room_engine() {
            engine.set_video_max_bitrate(bps);
        }
    }

    /// Сколько сейчас отдать видео-кодеру, бит/с (0 — звонка нет).
    pub fn room_video_target_bitrate(&self) -> u32 {
        self.room_engine().map_or(0, |e| e.video_target_bitrate())
    }

    /// Декодер видео участника начат заново — попросить у него ключевой кадр.
    pub fn room_request_keyframe(&self, peer: &[u8; 32]) {
        if let Some(engine) = self.room_engine() {
            engine.request_keyframe(peer);
        }
    }

    /// Следующий видео-кадр участника, как только его пора декодировать (`play_at` —
    /// срок); ждёт не дольше `wait`.
    pub fn room_pull_video(
        &self,
        peer: &[u8; 32],
        wait: std::time::Duration,
    ) -> Option<litertc::VideoFrame> {
        self.room_engine()
            .and_then(|e| e.pull_video_for(peer, wait))
    }

    // — Диагностика: тест пропускной способности меша —
    pub fn bench_recv_start(&self) {
        // Медиа-канал у мессенджера один: забрать его посреди звонка значит оглушить
        // звонок (R-033). Диагностика подождёт.
        if self.room_call.lock_safe().is_some() {
            log::warn!("bench: идёт звонок — замер не запускаю");
            return;
        }
        let rx = self.messenger.media_channel(8192);
        let st = Arc::new(BenchRecv {
            bytes: AtomicU64::new(0),
            pkts: AtomicU64::new(0),
            running: AtomicBool::new(true),
            start: Instant::now(),
        });
        *self.bench.lock_safe() = Some(st.clone());
        std::thread::spawn(move || {
            while st.running.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(p) if p.frame().is_some_and(|f| f.header.track == BENCH_TRACK) => {
                        st.bytes.fetch_add(
                            (p.data.len() - litertc::MEDIA_HEADER_LEN) as u64,
                            Ordering::Relaxed,
                        );
                        st.pkts.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return,
                }
            }
        });
    }

    pub fn bench_recv_stats_json(&self) -> String {
        let g = self.bench.lock_safe();
        match g.as_ref() {
            Some(st) => {
                let bytes = st.bytes.load(Ordering::Relaxed);
                let pkts = st.pkts.load(Ordering::Relaxed);
                let secs = st.start.elapsed().as_secs_f64().max(0.001);
                let dto = dto::BenchRecv {
                    pkts,
                    bytes,
                    secs,
                    mbps: bytes as f64 * 8.0 / 1e6 / secs,
                };
                serde_json::to_string(&dto).unwrap_or_else(|_| "{}".into())
            }
            None => "{}".into(),
        }
    }

    pub fn bench_blast(&self, peer: &[u8; 32], total_kb: i32) -> String {
        let payload = vec![0u8; 1200];
        let target = (total_kb.max(0) as u64) * 1024;
        let mut sent_bytes = 0u64;
        let mut sent_pkts = 0u64;
        let mut seq = 0u16;
        let t0 = Instant::now();
        while sent_bytes < target && t0.elapsed() < Duration::from_secs(10) {
            if self
                .messenger
                .send_media(peer, BENCH_TRACK, seq, 0, &payload)
                .is_ok()
            {
                sent_bytes += payload.len() as u64;
                sent_pkts += 1;
                seq = seq.wrapping_add(1);
            }
        }
        let secs = t0.elapsed().as_secs_f64().max(0.001);
        let dto = dto::BenchResult {
            sent_pkts,
            sent_bytes,
            secs,
            mbps: sent_bytes as f64 * 8.0 / 1e6 / secs,
        };
        serde_json::to_string(&dto).unwrap_or_else(|_| "{}".into())
    }
}

// ── Хелперы маршалинга/презенса ─────────────────────────────────────────────

fn av_flags(audio: bool, video: bool) -> u8 {
    let mut f = 0u8;
    if audio {
        f |= 0b01;
    }
    if video {
        f |= 0b10;
    }
    f
}

fn room_members_json(live: Vec<([u8; 32], u8)>) -> String {
    let arr: Vec<dto::RoomMember> = live
        .into_iter()
        .map(|(p, f)| dto::RoomMember {
            peer: hex::encode(p),
            audio: f & 0b01 != 0,
            video: f & 0b10 != 0,
        })
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
}

fn call_params(audio: &AudioSettings) -> String {
    let dto = dto::CallParams {
        call_id: 0,
        sample_rate: audio.sample_rate,
        channels: audio.channels,
        frame_len: audio.interleaved_len(),
    };
    serde_json::to_string(&dto).unwrap_or_else(|_| "{}".into())
}

/// Своя эпоха адреса (ADR-007) — простой JSON рядом с профилем. Отдельно от
/// `profile_meta.json`: тот уходит контактам, а это чисто локальная величина.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct TransportMeta {
    #[serde(default)]
    epoch: u32,
}

fn transport_meta_path(dir: &Path) -> PathBuf {
    dir.join("transport.json")
}

/// Текущая эпоха адреса. Нет файла = 0, то есть адрес совпадает с ключом личности.
fn load_transport_epoch(dir: &Path) -> u32 {
    std::fs::read(transport_meta_path(dir))
        .ok()
        .and_then(|b| serde_json::from_slice::<TransportMeta>(&b).ok())
        .map(|m| m.epoch)
        .unwrap_or(0)
}

fn save_transport_epoch(dir: &Path, epoch: u32) {
    if let Ok(v) = serde_json::to_vec(&TransportMeta { epoch }) {
        let _ = peerage_core::fsutil::write_atomic(&transport_meta_path(dir), v);
    }
}

fn public_peer_uris() -> impl Iterator<Item = &'static str> {
    PUBLIC_PEERS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

// ── Внутренняя оркестрация: event_pump + обработчики ────────────────────────

/// Гейт входящего сообщения (замена TOFU-автодобавления).
enum Gate {
    /// Известный контакт — сообщение идёт в чат как обычно.
    Contact,
    /// Заблокирован — тихо дропаем.
    Blocked,
    /// Незнакомец — первое сообщение уходит в очередь запросов, не в чат.
    Stranger,
}

/// Классифицировать отправителя. ВАЖНО: не держим оба лока одновременно, чтобы
/// не поймать дедлок с APIs, что лочат contacts+gate последовательно.
fn gate_of(engine: &Engine, peer: &[u8; 32]) -> Gate {
    if engine.gate.lock_safe().is_blocked(peer) {
        Gate::Blocked
    } else if engine.contacts.lock_safe().contains(peer) {
        Gate::Contact
    } else {
        Gate::Stranger
    }
}

/// Записать входящий запрос от незнакомца и (если новый) уведомить UI. Превью —
/// короткая выжимка первого сообщения для осознанного решения. Повторные до
/// одобрения не копим (анти-DoS).
fn record_request(
    engine: &Arc<Engine>,
    peer: [u8; 32],
    preview: &str,
    first: Option<FirstMessage>,
) {
    let preview: String = preview.chars().take(200).collect();
    let is_new = engine
        .gate
        .lock_safe()
        .upsert_request(&peer, &preview, first, now())
        .unwrap_or(false);
    push_event(
        engine,
        dto::UiEvent::ContactRequest {
            peer: hex::encode(peer),
            new: is_new,
        },
    );
}

/// Новый набор реакций одного сообщения — клиенту, чтобы поправить его на месте.
fn push_reactions(engine: &Engine, chat: Chat, id: u64, sender: &str) {
    let reactions = engine.messages.lock_safe().reactions(chat, id, sender);
    push_event(
        engine,
        dto::UiEvent::Reactions {
            chat: hex::encode(chat.id()),
            group: chat.scope() == 1,
            id,
            sender: sender.to_string(),
            reactions,
        },
    );
}

/// Положить событие в очередь для клиента. Принимает ТИП, а не свободный JSON:
/// форма события фиксируется `dto::UiEvent` и проверяется компилятором.
fn push_event(engine: &Engine, ev: dto::UiEvent) {
    engine.events.lock_safe().push_back(ev.to_json());
}

/// Записать входящее личное сообщение и отдать его клиенту той же строкой.
fn push_dm_message(engine: &Engine, peer: &[u8; 32], mut msg: StoredMessage) {
    if let Ok(row) = engine.messages.lock_safe().append(Chat::Dm(peer), &msg) {
        msg.row = row;
        push_event(
            engine,
            dto::UiEvent::Message {
                chat: hex::encode(peer),
                group: false,
                msg,
            },
        );
    }
}

/// Отдать клиенту строки групп, записанные за этот виток. Флаг «скачано» уточняем
/// по факту: у входящего вложения слой групп его не знает.
fn push_group_messages(engine: &Engine, groups: &mut GroupManager) {
    for (gid, mut msg) in groups.take_fresh() {
        if !msg.downloaded && !msg.blob_id.is_empty() && engine.blob_exists(&msg.blob_id) {
            msg.downloaded = true;
        }
        push_event(
            engine,
            dto::UiEvent::Message {
                chat: hex::encode(&gid),
                group: true,
                msg,
            },
        );
    }
}

/// Поток event_pump = групповой актор: владеет [`GroupManager`] (GroupChat !Send),
/// обрабатывает события мессенджера и групповые команды — всё в одном потоке.
fn event_pump(
    engine: Arc<Engine>,
    job_rx: Receiver<Job>,
    identity: Identity,
    messenger: Arc<Messenger>,
    data_dir: PathBuf,
    group_meta: peerage_core::store::GroupStore,
) {
    let blob_dir = data_dir.join("blobs");
    let mut groups = GroupManager::new(
        identity,
        messenger.clone(),
        data_dir,
        engine.messages.clone(),
        group_meta,
    );
    let mut blobs =
        blobs::BlobManager::new(messenger.clone(), peerage_blob::Limits::default(), blob_dir);
    engine.resume_all_wanted(); // докачки, начатые до перезапуска
    refresh_group_cache(&engine, &groups); // прайм кэша
                                           // Клиент читает оба списка до прайма — объявляем оба. Без второго сохранённое
                                           // приглашение (в т. ч. принятое, ждущее пригласившего) не появлялось после перезапуска.
    push_event(&engine, dto::UiEvent::GroupsChanged);
    push_event(&engine, dto::UiEvent::GroupInvitesChanged);
    let mut last_beacon = Instant::now();
    // Один поток подготовки — вложения уходят в группу в том порядке, в каком их
    // отправили.
    let (prep_tx, prep_rx) = std::sync::mpsc::channel::<Job>();
    {
        let engine = engine.clone();
        std::thread::spawn(move || {
            while let Ok(job) = prep_rx.recv() {
                let Some((gid, media)) = prepare_group_media(&engine, job) else {
                    continue;
                };
                if engine.jobs.send(Job::GroupMediaRef { gid, media }).is_err() {
                    break;
                }
            }
        });
    }
    loop {
        match job_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Job::Ev(ev)) => handle_event(&engine, &mut groups, &mut blobs, ev),
            Ok(Job::Cmd(cmd)) => handle_cmd(&engine, &mut groups, cmd),
            Ok(Job::CancelDownloads(done)) => {
                blobs.cancel_downloads();
                let _ = done.send(());
            }
            // Подготовка (scrub, хэш, своя зашифрованная копия) — в своём потоке:
            // файл на сотни мегабайт держал бы актор, а с ним приём сообщений и
            // маячки звонка (R-028). Готовое вернётся сюда как GroupMediaRef.
            Ok(job @ Job::GroupMedia { .. }) => {
                let _ = prep_tx.send(job);
            }
            Ok(Job::GroupMediaRef { gid, media }) => {
                groups.send_media(&gid, &media);
            }
            Ok(Job::Beacon) => {
                last_beacon = Instant::now();
                beacon_tick(&engine, &mut groups, &messenger);
            }
            Ok(Job::Leave(gid)) => {
                // Стабильная комната: если мы УЖЕ перезашли — отложенный Leave устарел.
                if engine.rooms.lock_safe().joined.contains_key(&gid) {
                    // перезашли — ничего не делаем
                } else {
                    let dm_peer = engine.rooms.lock_safe().dm_peers.remove(&gid);
                    if let Some(peer) = dm_peer {
                        let _ = messenger.send_room_presence(&peer, &gid, PRESENCE_LEFT);
                    } else {
                        for m in groups.members(&gid) {
                            if m == engine.me_pub {
                                continue;
                            }
                            let _ = messenger.send_room_presence(&m, &gid, PRESENCE_LEFT);
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if last_beacon.elapsed() >= BEACON_INTERVAL {
            last_beacon = Instant::now();
            beacon_tick(&engine, &mut groups, &messenger);
        }
        push_group_messages(&engine, &mut groups);
        blobs.tick(); // переспросить потерянные чанки докачки (M2)
                      // Прогресс докачки наверх: UI рисует кружок вместо бесконечного спиннера.
        for (id, done, total) in blobs.take_progress() {
            push_event(
                &engine,
                dto::UiEvent::BlobProgress {
                    id: hex::encode(id),
                    done,
                    total,
                },
            );
        }
        // Не началась (велик, нет места) — снимаем заказ, чтобы он не крутился
        // вечно, и говорим человеку почему.
        for (id, why) in blobs.take_failures() {
            let id_hex = hex::encode(id);
            let _ = engine.wanted.lock_safe().remove(&id_hex);
            engine.blob_expect.lock_safe().remove(&id);
            push_event(
                &engine,
                dto::UiEvent::BlobFailed {
                    id: id_hex,
                    reason: why.as_str().into(),
                },
            );
        }
    }
}

/// Прочитать блоб из каталога по hex-id. Свободная функция — нужна и офлайн-очереди
/// (её колбэк живёт вне `Engine`), и самому движку.
fn read_blob_at(dir: &Path, id_hex: &str) -> Option<Vec<u8>> {
    // Санитизация: только валидный hex → чистое имя файла (без traversal).
    let id = hex::decode(id_hex.trim()).ok()?;
    let path = dir.join(hex::encode(&id));
    // Крупные блобы лежат в чанкованном формате (собраны потоково на диск), мелкие —
    // одним запечатанным буфером. Различаем по магии в заголовке.
    if peerage_blob::disk::read_header(&path).is_some() {
        return peerage_blob::disk::read_all(&path, |i, b| {
            vault::open(&vault::chunk_purpose(&id, i), b).ok()
        });
    }
    let raw = std::fs::read(path).ok()?;
    vault::open(&vault::blob_purpose(&id), &raw).ok()
}

/// Сохранить блоб на диск по hex-id (принятый или свой отправленный).
/// Шифрует at-rest (`vault::seal`) — как сообщения/группы.
fn save_blob(engine: &Arc<Engine>, id: peerage_blob::Hash, content: &[u8]) {
    write_blob_file(&engine.blob_dir, id, content);
}

/// Записать блоб одним буфером — атомарно (`fsutil::write_atomic`). Голый
/// `fs::write` сначала создаёт пустой файл и лишь потом пишет, поэтому читатель
/// (UI опрашивает `blob_read`/`blob_exists`) ловил пустое или обрезанное вложение,
/// а падение посреди записи оставляло огрызок, который считается «скачанным» и
/// больше никогда не перекачивается. Крупные блобы так же собираются в `.part` и
/// переименовываются (`peerage_blob::disk`).
fn write_blob_file(dir: &Path, id: peerage_blob::Hash, content: &[u8]) {
    let _ = std::fs::create_dir_all(dir);
    let _ = peerage_core::fsutil::write_atomic(
        &dir.join(hex::encode(id)),
        &vault::seal(&vault::blob_purpose(&id), content),
    );
}

/// Подготовить вложение для группы (см. [`Engine::store_media`]) и собрать
/// MLS-ссылку на него. Байты участники запросят сами — мелкое сразу, крупное по
/// кнопке. Работает в потоке подготовки, не в акторе.
fn prepare_group_media(engine: &Arc<Engine>, job: Job) -> Option<(Vec<u8>, MediaBody)> {
    let Job::GroupMedia {
        gid,
        source,
        mime,
        name,
        info,
    } = job
    else {
        return None;
    };
    let (hash, size) = engine.store_media(source, &mime)?;
    let thumb = engine
        .store_thumb(info.thumb.as_deref())
        .map(hex::encode)
        .unwrap_or_default();
    let media = MediaBody {
        // id проставит слой групп при отправке (он же кладёт его в историю).
        id: 0,
        fwd_from: info.fwd_from,
        fwd_anon: info.fwd_anon,
        reply_to: info.reply_to,
        reply_sender: info.reply_sender,
        kind: Engine::media_kind(&mime).into(),
        hash: hex::encode(hash),
        mime,
        name,
        caption: info.caption,
        size,
        duration_ms: info.duration_ms,
        width: info.width,
        height: info.height,
        thumb,
    };
    Some((gid, media))
}

/// Сохранить принятый блоб и уведомить UI (`blob_received`).
fn save_and_notify_blob(
    engine: &Arc<Engine>,
    groups: &mut GroupManager,
    peer: [u8; 32],
    done: blobs::Completed,
) {
    // M3, репутация: пир потратил на нас свой трафик, отдав блоб целиком → растим
    // его бюджет. Не-контакту счётчик не двигается (анти-Sybil, см. budget.rs).
    // ДО сохранения: блоб становится виден читателям сразу после записи, и кто
    // увидел блоб, должен видеть и засчитанную заслугу (иначе гонка в media-тесте).
    let bumped = engine.contacts.lock_safe().bump_helpful(&peer).unwrap_or(0);
    if bumped > 0 {
        engine.refresh_budgets();
    }
    match &done.data {
        // Мелкий блоб собран в памяти — сохраняем как раньше (один запечатанный файл).
        blobs::CompletedData::InMemory(content) => save_blob(engine, done.id, content),
        // Крупный уже лежит на диске в чанкованном виде — писать нечего.
        blobs::CompletedData::OnDisk(_) => {}
    }
    // Отмечаем вложение скачанным во ВСЕХ чатах: блоб контент-адресный, один и тот
    // же файл в разных чатах скачан разом. Иначе карточка так и предлагала бы «скачать».
    let id_hex = hex::encode(done.id);
    let _ = engine.messages.lock_safe().mark_downloaded(&id_hex);
    // То же для групп: их история живёт отдельным стором, но блоб — один и тот же.
    // Клиент отмечает строки сам по `blob_received` — перечитывать ленту незачем.
    groups.mark_downloaded(&id_hex);
    let _ = engine.wanted.lock_safe().remove(&id_hex); // намерение исполнено
    engine.blob_expect.lock_safe().remove(&done.id);
    push_event(
        engine,
        dto::UiEvent::BlobReceived {
            id: id_hex,
            mime: done.manifest.mime,
            size: done.manifest.size,
        },
    );
}

/// Разослать маячок присутствия членам всех наших комнат и подчистить протухших.
fn beacon_tick(engine: &Arc<Engine>, groups: &mut GroupManager, messenger: &Messenger) {
    // Незавершённое вступление в группу: пригласивший мог быть офлайн, когда мы
    // нажали «Вступить». Повтор живёт здесь, а не на presence, потому что
    // пригласивший может не быть контактом — presence о нём не приходит вовсе.
    groups.retry_pending_joins();
    let joined: Vec<(Vec<u8>, u8)> = {
        let rs = engine.rooms.lock_safe();
        rs.joined.iter().map(|(g, f)| (g.clone(), *f)).collect()
    };
    for (gid, flags) in joined {
        let dm_peer = engine.rooms.lock_safe().dm_peers.get(&gid).copied();
        if let Some(peer) = dm_peer {
            let _ = messenger.send_room_presence(&peer, &gid, flags);
        } else {
            for m in groups.members(&gid) {
                if m == engine.me_pub {
                    continue;
                }
                let _ = messenger.send_room_presence(&m, &gid, flags);
            }
        }
    }
    engine.rooms.lock_safe().prune(Instant::now(), PRESENCE_TTL);
    engine.sync_room_members();
}

/// Обработать одно событие мессенджера: персист + JSON в очередь + маршрутизация групп.
fn handle_event(
    engine: &Arc<Engine>,
    groups: &mut GroupManager,
    blobs: &mut blobs::BlobManager,
    ev: Event,
) {
    match ev {
        Event::Presence { peer, online } => {
            if online {
                // Профиль — ТОЛЬКО контактам (не светим профиль незнакомцам,
                // presence сам по себе больше не добавляет в контакты — нужен явный запрос).
                let is_contact = engine.contacts.lock_safe().contains(&peer);
                if is_contact {
                    // M2: на коннекте шлём контакту свой профиль (догоняет смену, что была офлайн).
                    engine.send_profile_to(&peer, false);
                    // Продолжаем то, что пользователь просил скачать у этого пира.
                    engine.resume_wanted(&peer);
                }
            }
            if online {
                // Вступление в группу могло застрять: мы нажали «Вступить», пока
                // пригласивший был офлайн. Он вернулся — повторяем KeyPackage.
                let _ = engine.jobs.send(Job::Cmd(GroupCmd::PeerOnline { peer }));
            }
            push_event(
                engine,
                dto::UiEvent::Presence {
                    peer: hex::encode(peer),
                    online,
                },
            );
            // ADR-007: контакт вернулся, а объявление о смене нашего адреса до него
            // не доехало — досылаем сейчас.
            if online {
                engine.announce_rotation_to(&peer);
                engine.resend_read_mark(&peer);
            }
        }
        Event::Text {
            peer,
            id,
            text,
            reply_to,
            reply_mine,
            fwd_from,
            fwd_anon,
        } => {
            match gate_of(engine, &peer) {
                Gate::Blocked => return,
                Gate::Stranger => {
                    // Откладываем сам текст. Ответ незнакомцу ссылаться не на что, а
                    // пометку «переслано» первого сообщения не храним — не стоит
                    // отдельной схемы.
                    let first = FirstMessage {
                        id,
                        text: text.clone(),
                        time: now(),
                    };
                    record_request(engine, peer, &text, Some(first));
                    return;
                }
                Gate::Contact => {}
            }
            // Дедуп повторной доставки: офлайн-досылка со стабильным id приходит дважды.
            let peer_hex = hex::encode(peer);
            let dup = engine
                .messages
                .lock_safe()
                .contains(Chat::Dm(&peer), &peer_hex, id);
            if !dup {
                // reply_mine прислан С ТОЧКИ ЗРЕНИЯ ОТПРАВИТЕЛЯ и уже перевёрнут выше,
                // поэтому «моё» здесь — действительно наше сообщение.
                let msg = StoredMessage {
                    id,
                    text,
                    mine: false,
                    time: now(),
                    status: String::new(),
                    sender: peer_hex,
                    reply_to,
                    reply_sender: engine.reply_author(&peer, reply_to, reply_mine),
                    fwd_from,
                    fwd_anon,
                    kind: "text".into(),
                    ..Default::default()
                };
                push_dm_message(engine, &peer, msg);
            }
        }
        Event::Edit { peer, target, text } => {
            // Правим строку АВТОРА правки — чужую он переписать не может.
            let author = hex::encode(peer);
            let changed = engine
                .messages
                .lock_safe()
                .edit_text(Chat::Dm(&peer), &author, target, &text, &[], now())
                .unwrap_or(false);
            if changed {
                push_event(
                    engine,
                    dto::UiEvent::MessageEdited {
                        chat: author.clone(),
                        id: target,
                        sender: author,
                        text,
                        mentions: Vec::new(),
                    },
                );
            }
        }
        Event::Pin {
            peer,
            target,
            target_mine,
            on,
        } => {
            // Ссылка на сообщение — «автор + id»: чьё именно, решает перевёрнутый
            // флаг (его перевернул мессенджер на приёме).
            let author = engine.author_hex(&peer, target_mine);
            engine.apply_pin(Chat::Dm(&peer), target, &author, on, &hex::encode(peer));
            push_event(
                engine,
                dto::UiEvent::PinsChanged {
                    chat: hex::encode(peer),
                },
            );
        }
        Event::React {
            peer,
            target,
            target_mine,
            emoji,
            at,
        } => {
            // Ссылка на сообщение — «автор + id», как у закрепа; ставит — собеседник.
            let author = engine.author_hex(&peer, target_mine);
            let changed = engine
                .messages
                .lock_safe()
                .react(
                    Chat::Dm(&peer),
                    target,
                    &author,
                    &hex::encode(peer),
                    &emoji,
                    at,
                )
                .unwrap_or(false);
            if changed {
                push_reactions(engine, Chat::Dm(&peer), target, &author);
            }
        }
        Event::Read { peer, upto } => engine.on_read(&peer, upto),
        Event::Typing { peer } => engine.on_typing(&peer),
        Event::Retract { peer, target } => {
            // Отзываем ЕГО сообщение (автор — он): отозвать наше собственное он не может.
            let n = engine
                .messages
                .lock_safe()
                .retract_many(Chat::Dm(&peer), &[(target, hex::encode(peer))])
                .unwrap_or(0);
            if n > 0 {
                push_event(
                    engine,
                    dto::UiEvent::Retracted {
                        peer: hex::encode(peer),
                        id: target,
                    },
                );
            }
        }
        Event::Media {
            peer,
            id,
            hash,
            mime,
            name,
            size,
            duration_ms,
            width,
            height,
            caption,
            thumb,
            fwd_from,
            fwd_anon,
            reply_to,
            reply_mine,
        } => {
            match gate_of(engine, &peer) {
                Gate::Blocked => return,
                Gate::Stranger => {
                    record_request(
                        engine,
                        peer,
                        &format!("[{}]", Engine::media_kind(&mime)),
                        None,
                    );
                    return;
                }
                Gate::Contact => {}
            }
            let dup = engine
                .messages
                .lock_safe()
                .contains(Chat::Dm(&peer), &hex::encode(peer), id);
            if !dup {
                let t = now();
                let id_hex = hex::encode(&hash);
                let thumb_hex = hex::encode(&thumb);
                let kind = Engine::media_kind(&mime);
                let have = engine.blob_exists(&id_hex);
                // Крупное НЕ тянем молча: показываем карточку (имя/размер/превью), а
                // байты качаем по кнопке — иначе чужое видео на 800 МБ приедет само.
                let auto = have || size < AUTO_DOWNLOAD_LIMIT;
                let msg = StoredMessage {
                    id,
                    mine: false,
                    time: t,
                    status: String::new(),
                    sender: hex::encode(peer),
                    fwd_from: fwd_from.clone(),
                    fwd_anon,
                    text: caption.clone(),
                    kind: kind.into(),
                    blob_id: id_hex.clone(),
                    mime: mime.clone(),
                    name: name.clone(),
                    size,
                    duration_ms,
                    width,
                    height,
                    thumb_id: thumb_hex.clone(),
                    downloaded: have,
                    reply_to,
                    // Флаг с провода уже перевёрнут мессенджером: «моё» здесь —
                    // действительно наше сообщение (ADR-006 хранит явного автора).
                    reply_sender: if reply_to == 0 {
                        String::new()
                    } else if reply_mine {
                        engine.me_hex()
                    } else {
                        hex::encode(peer)
                    },
                    ..Default::default()
                };
                // Миниатюру тянем всегда — она мелкая, ради неё и заведена.
                if !thumb.is_empty() && !engine.blob_exists(&thumb_hex) {
                    engine.request_blob(&peer, &thumb);
                }
                if auto && !have {
                    engine.request_blob(&peer, &hash);
                }
                push_dm_message(engine, &peer, msg);
            }
        }
        Event::Ack { peer, id } => {
            let _ = engine
                .messages
                .lock_safe()
                .set_status(Chat::Dm(&peer), id, "delivered");
            push_event(
                engine,
                dto::UiEvent::Ack {
                    peer: hex::encode(peer),
                    id,
                },
            );
        }
        Event::Undelivered { peer, id } => {
            // "queued", а НЕ "failed": ретраи в памяти кончились, но сообщение лежит в
            // персистентной офлайн-очереди и уйдёт, когда получатель появится. Раньше
            // в БД писалось "failed" (в UI при этом рисовался часами "queued"), и после
            // перезапуска ждущее доставки сообщение показывалось красным «!».
            let _ = engine
                .messages
                .lock_safe()
                .set_status(Chat::Dm(&peer), id, "queued");
            push_event(
                engine,
                dto::UiEvent::Undelivered {
                    peer: hex::encode(peer),
                    id,
                },
            );
        }
        Event::RoomInvite {
            peer,
            room_id,
            audio,
            video,
        } => {
            // Звать можно либо в нашу общую DM-комнату (только контакт: незнакомец
            // идёт через запрос в контакты, а не через звонок), либо в комнату группы,
            // где звонящий состоит.
            // Мы уже в этой комнате — звать некуда, а запись о звонке затёрла бы текущую.
            if !engine.room_peer_allowed(&peer, &room_id)
                || engine.rooms.lock_safe().joined.contains_key(&room_id)
            {
                return;
            }
            // Идёт другой разговор: второй звонок клиент не показывает, и активную
            // запись он не заменяет — иначе длительность разговора досталась бы
            // пропущенному, а у самого разговора остался бы ноль (R-049).
            if !engine.rooms.lock_safe().joined.is_empty() {
                engine.record_missed(&peer, &room_id, video);
            } else {
                // Заводим сразу как пропущенный: если пользователь ответит, исход
                // поднимется в room_join(_dm), если откажется — в room_decline_*.
                engine.begin_call(&peer, &room_id, true, video, "missed");
            }
            push_event(
                engine,
                dto::UiEvent::RoomInvite {
                    peer: hex::encode(peer),
                    room_id: hex::encode(&room_id),
                    audio,
                    video,
                },
            );
        }
        Event::RoomDecline { peer, room_id } => push_event(
            engine,
            dto::UiEvent::RoomDecline {
                peer: hex::encode(peer),
                room_id: hex::encode(&room_id),
            },
        ),
        Event::Group { peer, data } => {
            let out = groups.on_group(peer, &data);
            // Саму строку клиенту отдаст push_group_messages. Здесь — только докачка:
            // крупное НЕ тянем молча, как и в DM, — показываем карточку, а байты
            // качаем по кнопке, иначе чужое видео приезжает всей группе само.
            if let Some(m) = out.media.as_ref() {
                let have = engine.blob_exists(&m.hash);
                let auto = have || m.size < AUTO_DOWNLOAD_LIMIT;
                // Миниатюру тянем всегда — она мелкая, ради неё карточка и есть.
                if !m.thumb.is_empty() && !engine.blob_exists(&m.thumb) {
                    if let Ok(h) = hex::decode(&m.thumb) {
                        engine.request_blob(&peer, &h);
                    }
                }
                if auto && !have {
                    if let Ok(h) = hex::decode(&m.hash) {
                        engine.request_blob(&peer, &h);
                    }
                }
                if have {
                    groups.mark_downloaded(&m.hash);
                }
            }
            if out.retracted {
                // Строка стала «Сообщение удалено» — UI обязан перечитать историю, иначе
                // отозванное останется на экране до перезахода в чат.
                if let Some(gid) = &out.gid {
                    push_event(
                        engine,
                        dto::UiEvent::GroupRetracted {
                            gid: hex::encode(gid),
                        },
                    );
                }
            }
            if let Some((sender, id, text, mentions)) = out.edited.clone() {
                if let Some(gid) = &out.gid {
                    push_event(
                        engine,
                        dto::UiEvent::MessageEdited {
                            chat: hex::encode(gid),
                            id,
                            sender,
                            text,
                            mentions,
                        },
                    );
                }
            }
            if let (Some(gid), Some((id, sender))) = (&out.gid, &out.reacted) {
                push_reactions(engine, Chat::Group(gid), *id, sender);
            }
            if out.pinned {
                // Шапка чата живёт отдельно от ленты — событие своё.
                if let Some(gid) = &out.gid {
                    push_event(
                        engine,
                        dto::UiEvent::PinsChanged {
                            chat: hex::encode(gid),
                        },
                    );
                }
            }
            if out.joined {
                refresh_group_cache(engine, groups);
                // Карточку приглашения движок уже снял (Welcome пришёл) — но UI об этом
                // узнаёт только из GroupInvitesChanged. Без него «ждём ответа» висит
                // рядом с уже вступившей группой до перезапуска.
                push_event(engine, dto::UiEvent::GroupInvitesChanged);
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
            if out.renamed {
                // Имя группы пришло от админа — обновляем кэш списка и уведомляем UI.
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
            if out.removed {
                // ADR-004: нас отозвали — обновляем кэш и уведомляем UI отдельным
                // событием (чтобы показать «вы удалены» и закрыть экран группы).
                refresh_group_cache(engine, groups);
                if let Some(gid) = &out.gid {
                    push_event(
                        engine,
                        dto::UiEvent::GroupRemoved {
                            gid: hex::encode(gid),
                        },
                    );
                }
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
        }
        Event::GroupKeyRequest { peer, gid } => groups.on_key_request(peer, &gid),
        Event::GroupInvite { peer, gid, name } => {
            // Звать в группу может только контакт: иначе незнакомец обходил бы гейт
            // карточками с любым названием, сколько позволит общая корзина чужих.
            if !matches!(gate_of(engine, &peer), Gate::Contact) {
                return;
            }
            // Приглашение-карточка: не вступаем молча — кладём в очередь, уведомляем UI.
            if groups.on_invite(peer, gid, name) {
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupInvitesChanged);
            }
        }
        Event::GroupResyncRequest { peer, gid } => groups.on_resync_request(peer, &gid),
        Event::GroupLeaveRequest { peer, gid } => {
            if groups.on_leave_request(peer, &gid) {
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
        }
        // M2 блобы: докачка получателем + отдача чанков. Событие blob_received →
        // UI (сохранён на диск, доступен через blob_read).
        // Манифест принимаем только на свой запрос и только от того, у кого просили:
        // иначе любой залил бы нам файл без спроса, в обход гейта и кнопки.
        Event::BlobManifest { peer, manifest } => {
            let Ok(m) = peerage_blob::Manifest::from_json(&manifest) else {
                return;
            };
            if !engine.blob_expected(&peer, &m.id) {
                return;
            }
            if let Some(done) = blobs.on_manifest(peer, m) {
                save_and_notify_blob(engine, groups, peer, done);
            }
        }
        Event::BlobManifestPart {
            peer,
            id,
            mime,
            size,
            part,
            total,
            hashes,
        } => {
            if !engine.blob_expected(&peer, &id) {
                return;
            }
            if let Some(done) = blobs.on_manifest_part(peer, &id, &mime, size, part, total, &hashes)
            {
                save_and_notify_blob(engine, groups, peer, done);
            }
        }
        Event::BlobRequest { peer, id, index } => blobs.on_request(&id, index, peer),
        Event::BlobChunk {
            peer,
            id,
            index,
            data,
        } => {
            if let Some(done) = blobs.on_chunk(peer, &id, index, &data) {
                save_and_notify_blob(engine, groups, peer, done);
            }
        }
        // M2 профиль: контакт прислал самоназвание + хэш аватара.
        Event::ProfileUpdate {
            peer,
            name,
            avatar_hash,
            ask_back,
        } => {
            // Только от контактов (профиль не принимаем от посторонних). ВАЖНО: гард лока
            // в условии `if` живёт до конца тела → повторный лок внутри = дедлок; поэтому
            // читаем `contains` в переменную (гард снимается на `;`).
            let is_contact = engine.contacts.lock_safe().contains(&peer);
            if is_contact {
                let avatar_id = if avatar_hash.is_empty() {
                    String::new()
                } else {
                    hex::encode(&avatar_hash)
                };
                let _ = engine
                    .contacts
                    .lock_safe()
                    .set_profile(&peer, &name, &avatar_id);
                // Аватар тянем по хэшу ТОЛЬКО если новый и блоба ещё нет (контент-адрес → дедуп).
                if !avatar_id.is_empty() && !engine.blob_exists(&avatar_id) {
                    engine.request_blob(&peer, &avatar_hash);
                }
                // Нас только что приняли в контакты и просят наш профиль. Ответ — без
                // флага, иначе стороны перебрасывались бы профилями бесконечно.
                if ask_back {
                    engine.send_profile_to(&peer, false);
                }
                push_event(engine, dto::UiEvent::ContactsChanged);
            }
        }
        // ADR-007: контакт сменил адрес. Применяем ТОЛЬКО через стор — там и
        // проверка подписи ключом личности, и монотонность эпохи. Приняли —
        // немедленно обновляем карту адресов, иначе следующее сообщение уйдёт на
        // покинутый ключ.
        Event::TransportRotate {
            peer,
            epoch,
            transport_pub,
            sig,
        } => {
            let (Ok(tp), Ok(sg)) = (
                <[u8; 32]>::try_from(transport_pub.as_slice()),
                <[u8; 64]>::try_from(sig.as_slice()),
            ) else {
                return;
            };
            // Заблокированному новый адрес не записываем: мы не хотим ни слать ему,
            // ни следовать за ним. Механизм это и так вытянет (тир ищется по
            // личности), но лишний шаг за тем, кого отрезали, тут ни к чему.
            if matches!(gate_of(engine, &peer), Gate::Blocked) {
                return;
            }
            let applied = engine
                .contacts
                .lock_safe()
                .apply_transport_rotate(&peer, epoch, &tp, &sg)
                .unwrap_or(false);
            if applied {
                log::info!("контакт {:02x?}… сменил адрес (эпоха {epoch})", &peer[..4],);
                engine.refresh_transports();
                push_event(engine, dto::UiEvent::ContactsChanged);
            }
        }
        // Пир просит манифест нашего блоба (аватар, вложение чата или группы).
        Event::BlobManifestRequest { peer, id } => {
            let Ok(hash) = <[u8; 32]>::try_from(id.as_slice()) else {
                return;
            };
            // Отдаём только своим: контакту или участнику общей группы. Хэш — не
            // секрет (он ходит в пересылках и группах, у публичного файла его считает
            // кто угодно), и без этой проверки любой узнавал бы, есть ли у нас файл.
            let ours = matches!(gate_of(engine, &peer), Gate::Contact)
                || groups
                    .group_ids()
                    .iter()
                    .any(|g| groups.members(g).contains(&peer));
            if !ours {
                return;
            }
            // Уже отдаётся — переслать манифест, не трогая диск.
            if blobs.reoffer(peer, &hash) {
                return;
            }
            let id_hex = hex::encode(hash);
            let path = engine.blob_dir.join(&id_hex);
            // Mime для аватара точный; у вложения его знает клиент из ссылки.
            let meta = engine.load_profile_meta();
            let mime = if meta.avatar_id == id_hex {
                meta.avatar_mime
            } else {
                "application/octet-stream".into()
            };
            if peerage_blob::disk::read_header(&path).is_some() {
                // Крупный блоб не поднимаем в память: манифест считаем потоково,
                // чанки отдаём с диска по одному.
                let open =
                    |i: usize, b: &[u8]| vault::open(&vault::chunk_purpose(&hash, i), b).ok();
                if let Some(m) = peerage_blob::disk::manifest_of_stored(&path, mime, open) {
                    if m.id == hash {
                        blobs.offer_stored(peer, m, path);
                    }
                }
            } else if let Some(bytes) = engine.blob_read(&id_hex) {
                blobs.offer(peer, &bytes, &mime);
            }
        }
        Event::GroupKeyReply {
            peer,
            gid,
            key_package,
        } => {
            if groups.on_key_reply(peer, &gid, &key_package) {
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
        }
        Event::RoomPresence { peer, gid, flags } => {
            // Чужой маячок в комнату, где ему не место, не записываем вовсе: из
            // присутствия собирается список адресатов нашего голоса и видео.
            if !engine.room_peer_allowed(&peer, &gid) {
                return;
            }
            let left = flags & PRESENCE_LEFT != 0;
            {
                let mut rs = engine.rooms.lock_safe();
                if left {
                    if let Some(m) = rs.remote.get_mut(&gid) {
                        m.remove(&peer);
                    }
                } else {
                    rs.remote
                        .entry(gid.clone())
                        .or_default()
                        .insert(peer, (Instant::now(), flags));
                }
            }
            // Собеседник появился в комнате — значит наш исходящий звонок
            // состоялся. Без этого «позвонил и поговорил» осталось бы в журнале
            // как «не дозвонился».
            if !left {
                let id = engine
                    .active_call
                    .lock_safe()
                    .as_ref()
                    // Только СВОЙ исходящий: у входящего звонящий в комнате с самого
                    // начала, и его маячок не значит, что мы ответили.
                    .filter(|c| c.room == gid && !c.incoming)
                    .map(|c| c.id);
                if let Some(id) = id {
                    engine.call_outcome(id, "answered");
                }
            }
            push_event(
                engine,
                dto::UiEvent::RoomPresence {
                    gid: hex::encode(&gid),
                    peer: hex::encode(peer),
                    flags,
                    left,
                },
            );
            engine.sync_room_members();
        }
    }
}

/// Пересчитать кэш групповых read-данных (зовётся после каждой мутации).
fn refresh_group_cache(engine: &Arc<Engine>, groups: &GroupManager) {
    {
        let mut cache = engine.group_cache.lock_safe();
        cache.list = groups.list_json();
        cache.invites = groups.invites_json();
        // Пересобираем целиком: вышедшая группа не должна оставлять ростер в кэше
        // (а с ним — участников в наборе связи).
        cache.members = groups
            .group_ids()
            .into_iter()
            .map(|g| {
                let m = groups.members_json(&g);
                (g, m)
            })
            .collect();
    }
    engine.refresh_targets(); // состав групп мог смениться
}

/// Исполнить групповую команду (в потоке event_pump). Fire-and-forget.
fn handle_cmd(engine: &Arc<Engine>, groups: &mut GroupManager, cmd: GroupCmd) {
    match cmd {
        GroupCmd::Create { name } => {
            if groups.create(&name).is_some() {
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
        }
        GroupCmd::Invite { gid, peer } => groups.invite(&gid, peer),
        GroupCmd::AcceptInvite { gid } => {
            // KeyPackage ушёл пригласившему; вступление завершится по Welcome (joined).
            // Карточка остаётся до этого момента в состоянии «ждём ответа»: пригласивший
            // мог быть офлайн, и тогда мы повторим KeyPackage на его presence.
            groups.accept_invite(&gid);
            refresh_group_cache(engine, groups);
            push_event(engine, dto::UiEvent::GroupInvitesChanged);
        }
        GroupCmd::DeclineInvite { gid } => {
            if groups.decline_invite(&gid) {
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupInvitesChanged);
            }
        }
        GroupCmd::SendText {
            gid,
            text,
            reply_to,
            reply_sender,
            fwd_from,
            fwd_anon,
            mentions,
        } => {
            // Своё сообщение уйдёт в UI событием `message` (take_fresh) — с id,
            // который присвоил слой групп: на него сразу можно ответить и отозвать.
            let sent = groups.send_text_full(
                &gid,
                &text,
                reply_to,
                &reply_sender,
                &fwd_from,
                fwd_anon,
                mentions,
            );
            if sent {
                refresh_group_cache(engine, groups);
            }
        }
        GroupCmd::Edit {
            gid,
            id,
            text,
            mentions,
        } => {
            if let Some((sender, text, mentions)) = groups.edit(&gid, id, &text, &mentions) {
                push_event(
                    engine,
                    dto::UiEvent::MessageEdited {
                        chat: hex::encode(&gid),
                        id,
                        sender,
                        text,
                        mentions,
                    },
                );
            }
        }
        GroupCmd::React {
            gid,
            id,
            sender,
            emoji,
            at,
        } => {
            if groups.react(&gid, id, &sender, &emoji, at) {
                push_reactions(engine, Chat::Group(&gid), id, &sender);
            }
        }
        GroupCmd::Pin {
            gid,
            id,
            sender,
            on,
        } => {
            if groups.pin(&gid, id, &sender, on) {
                push_event(
                    engine,
                    dto::UiEvent::PinsChanged {
                        chat: hex::encode(&gid),
                    },
                );
            }
        }
        GroupCmd::Retract { gid, ids } => {
            if groups.retract(&gid, &ids) > 0 {
                refresh_group_cache(engine, groups);
                push_event(
                    engine,
                    dto::UiEvent::GroupRetracted {
                        gid: hex::encode(&gid),
                    },
                );
            }
        }
        GroupCmd::DeleteLocal { gid, refs } => {
            if groups.delete_local(&gid, &refs) > 0 {
                refresh_group_cache(engine, groups);
                push_event(
                    engine,
                    dto::UiEvent::GroupRetracted {
                        gid: hex::encode(&gid),
                    },
                );
            }
        }
        GroupCmd::PeerOnline { peer } => {
            groups.on_peer_online(peer);
        }
        // (событие о завершении вступления придёт по Welcome — GroupsChanged)
        GroupCmd::RemoveMember { gid, peer } => {
            if groups.remove_member(&gid, peer) {
                refresh_group_cache(engine, groups);
                engine.sync_room_members(); // SEC-2: отозванного убрать и из звонка
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
        }
        GroupCmd::Rekey { gid } => {
            if groups.rekey(&gid) {
                refresh_group_cache(engine, groups);
            }
        }
        GroupCmd::Leave { gid } => {
            // Добровольный выход: группа забыта локально. Навигацию делает UI сам
            // (без «вас удалили»-тоста — это не отзыв). Обновляем список.
            if groups.leave(&gid) {
                refresh_group_cache(engine, groups);
                engine.sync_room_members();
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
        }
    }
}

/// Удалить зашифрованные пользовательские данные при потере vault-ключа.
/// Ошибка старта: данные на диске зашифрованы другим ключом хранилища (ключ
/// потерян или подменён). Клиент показывает экран восстановления: решать, что
/// делать с данными, должен человек, а не движок.
pub const START_VAULT_MISMATCH: &str = "vault_mismatch";

/// Каталог, куда откладываются нечитаемые данные (внутри каталога данных).
pub const QUARANTINE_DIR: &str = "quarantine";

/// Что остаётся на месте при карантине: обёртка ключа хранилища (её разбирает
/// клиент) и сам карантин.
const QUARANTINE_KEEP: &[&str] = &[QUARANTINE_DIR, "vault.key.enc", "vault.meta"];

/// Загрузить личность из `profile.json` (или создать новую, если её нет). Если
/// профиль есть, но не расшифровывается, — [`START_VAULT_MISMATCH`]: ничего не
/// стирается и новая личность не создаётся.
pub fn load_identity(dir: &Path) -> Result<Identity, String> {
    match Profile::load_or_create(&dir.join("profile.json")) {
        Ok(p) => Ok(p.identity),
        Err(peerage_core::store::StoreError::Vault(peerage_core::vault::VaultError::Decrypt)) => {
            log::warn!(
                "профиль не расшифровывается этим ключом хранилища — нужен выбор пользователя"
            );
            Err(START_VAULT_MISMATCH.into())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Отложить данные, которые нельзя прочитать: всё содержимое каталога, кроме
/// [`QUARANTINE_KEEP`], переезжает в `quarantine/<время>/`. Не удаление — если
/// старый ключ найдётся, данные можно вернуть руками. Возвращает путь карантина.
pub fn quarantine_user_data(dir: &Path) -> std::io::Result<PathBuf> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let root = dir.join(QUARANTINE_DIR);
    let mut dst = root.join(secs.to_string());
    let mut n = 1;
    while dst.exists() {
        dst = root.join(format!("{secs}-{n}"));
        n += 1;
    }
    std::fs::create_dir_all(&dst)?;
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name();
        if QUARANTINE_KEEP.iter().any(|k| name == *k) {
            continue;
        }
        // rename в пределах одного каталога данных — без копирования.
        std::fs::rename(e.path(), dst.join(&name))?;
    }
    Ok(dst)
}

impl Engine {
    /// Инициализация. `dir` — приватный каталог данных. `vault_key` — 32-байтовый
    /// ключ at-rest (None → at-rest отключено). Поднимает транспорт, хранилища,
    /// поток event_pump. Возвращает движок + публичную идентичность.
    pub fn start(dir: PathBuf, vault_key: Option<[u8; 32]>) -> Result<StartOk, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        if let Some(key) = vault_key {
            peerage_core::vault::init_with_key(key);
        }

        // Не подошёл ключ — не стираем и не заводим новую личность молча: это
        // решает человек на экране восстановления (R-029).
        let identity = load_identity(&dir)?;
        // ADR-007: узел поднимается на ТРАНСПОРТНОМ ключе текущей эпохи. Эпоха 0 —
        // это сам ключ личности, поэтому у тех, кто адрес не менял, ничего не
        // меняется.
        let epoch = load_transport_epoch(&dir);
        let node = Node::start(&identity.transport_seed(epoch)).map_err(|e| e.to_string())?;
        let address = node.address();
        for uri in public_peer_uris() {
            let _ = node.add_peer(uri);
        }
        Self::start_inner(
            dir,
            identity,
            Arc::new(node),
            address,
            peerage_messenger::Config::default(),
        )
    }

    /// Поднять движок поверх ПРОИЗВОЛЬНОГО транспорта (loopback в интеграционных
    /// тестах): идентичность задаёт вызывающий, реальный меш-узел НЕ создаётся —
    /// можно гонять два движка в одном процессе (доставка/офлайн/рестарт/дедуп).
    pub fn start_with_transport(
        dir: PathBuf,
        identity: Identity,
        transport: Arc<dyn peerage_messenger::Transport>,
    ) -> Result<StartOk, String> {
        Self::start_with_transport_cfg(
            dir,
            identity,
            transport,
            peerage_messenger::Config::default(),
        )
    }

    /// То же, но с явным [`peerage_messenger::Config`] — нужен тестам, которые проверяют
    /// поведение по таймаутам (штатный `message_ttl` = 5 минут, столько не подождать).
    pub fn start_with_transport_cfg(
        dir: PathBuf,
        identity: Identity,
        transport: Arc<dyn peerage_messenger::Transport>,
        cfg: peerage_messenger::Config,
    ) -> Result<StartOk, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let address = transport.address();
        Self::start_inner(dir, identity, transport, address, cfg)
    }

    /// Общая сборка движка поверх готового транспорта (реального или loopback).
    fn start_inner(
        dir: PathBuf,
        identity: Identity,
        transport: Arc<dyn peerage_messenger::Transport>,
        address: String,
        cfg: peerage_messenger::Config,
    ) -> Result<StartOk, String> {
        let me_pub = identity.public_key_bytes();
        // Обрывки атомарной записи от убитого процесса (R-042). Сейчас, пока никто
        // не пишет: позже временный файл мог бы оказаться чьей-то записью в процессе.
        for d in [dir.clone(), dir.join("groups"), dir.join("blobs")] {
            let n = peerage_core::fsutil::sweep_temp(&d);
            if n > 0 {
                log::info!("{}: убрано {n} временных файлов", d.display());
            }
        }
        let db = peerage_core::db::open(&dir).map_err(|e| format!("db: {e}"))?;
        let contacts = ContactStore::open(db.clone()).map_err(|e| format!("contacts: {e}"))?;
        let gate = GateStore::open(db.clone()).map_err(|e| format!("gate: {e}"))?;
        let messages = MessageStore::open(db.clone());
        let identity_for_groups = identity.clone();
        let identity_for_engine = identity.clone();
        let (messenger, rx) = Messenger::start_with_transport(identity, transport, cfg);
        let messenger = Arc::new(messenger);
        // Тиры и набор связи зальём ниже, после создания Engine (refresh_budgets).

        // Актор-канал: события мессенджера и групповые команды идут в один поток.
        let (job_tx, job_rx) = std::sync::mpsc::channel::<Job>();
        {
            let job_tx = job_tx.clone();
            std::thread::spawn(move || {
                while let Ok(ev) = rx.recv() {
                    if job_tx.send(Job::Ev(ev)).is_err() {
                        break;
                    }
                }
            });
        }

        // Кэш тиров — общий для ingress-лимитера и офлайн-очереди (единый бюджет M3).
        // Нужен здесь, потому что очередь создаётся раньше Engine и не может на него
        // ссылаться; `refresh_budgets` держит кэш в актуальном состоянии.
        let tier_cache: Arc<Mutex<HashMap<[u8; 32], Tier>>> = Arc::new(Mutex::new(HashMap::new()));

        // Офлайн-очередь (M3): ставит и досылает мессенджер; движку — сроки по тиру,
        // статистика и ручной сброс.
        let outbox = {
            let tc = tier_cache.clone();
            peerage_core::outbox::OutboxManager::new(
                peerage_core::outbox::OutboxStore::new(db.clone()),
                // Cap/TTL очереди — из того же бюджета, что и ingress (единая точка).
                Box::new(move |peer| {
                    tc.lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .get(peer)
                        .copied()
                        .unwrap_or_default()
                }),
            )
        };

        let engine = Arc::new(Engine {
            messenger: messenger.clone(),
            contacts: Mutex::new(contacts),
            gate: Mutex::new(gate),
            wanted: Mutex::new(peerage_core::store::WantedStore::new(db.clone())),
            calls: Mutex::new(peerage_core::store::CallStore::new(db.clone())),
            active_call: Mutex::new(None),
            missed_calls: Mutex::new(HashMap::new()),
            messages: Arc::new(Mutex::new(messages)),
            outbox: Mutex::new(outbox),
            me_pub,
            identity: identity_for_engine,
            events: Mutex::new(VecDeque::new()),
            group_cache: Mutex::new(GroupCache::default()),
            jobs: job_tx,
            room_call: Mutex::new(None),
            rooms: Mutex::new(RoomState::default()),
            rings: Mutex::new(HashMap::new()),
            blob_expect: Mutex::new(HashMap::new()),
            bench: Mutex::new(None),
            blob_dir: dir.join("blobs"),
            tier_cache,
            data_dir: dir.clone(),
            rotate_pending: Mutex::new(std::collections::HashSet::new()),
            transport_factory: Mutex::new(None),
            offline: AtomicBool::new(false),
            receipts: Mutex::new(receipts::Receipts::load(&dir)),
        });
        engine.load_rotate_pending();

        // Единый бюджет (M3): тиры контактов/блок-листа в ingress-лимитер. Делаем
        // ДО запуска приёмного цикла — иначе первые пакеты заблокированного пира
        // успели бы пройти расшифровку.
        engine.refresh_budgets();
        // ADR-007: адреса контактов, сменивших ключ. Тоже до приёмного цикла —
        // иначе первые исходящие ушли бы на покинутые адреса.
        engine.refresh_transports();
        // Офлайн-очередь: сперва выбросить просроченное по тиру (тиры уже залиты),
        // потом отдать её мессенджеру — он и дошлёт оставшееся.
        let expired = engine.outbox.lock_safe().expired(now());
        let dropped = engine.give_up(expired);
        log::info!(
            "outbox: выброшено просроченных {dropped}, ждёт {}",
            engine.outbox.lock_safe().pending_count()
        );
        engine
            .messenger
            .set_outbox(peerage_core::outbox::OutboxStore::new(db.clone()));

        let pump_engine = engine.clone();
        let groups_dir = dir.clone();
        let group_meta = peerage_core::store::GroupStore::new(db.clone());
        std::thread::spawn(move || {
            event_pump(
                pump_engine,
                job_rx,
                identity_for_groups,
                messenger,
                groups_dir,
                group_meta,
            )
        });

        Ok(StartOk {
            engine,
            pubkey: me_pub,
            address,
        })
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;

    fn k(b: u8) -> [u8; 32] {
        [b; 32]
    }

    #[test]
    fn live_filters_stale_beacons() {
        let mut rs = RoomState::default();
        let now = Instant::now();
        let ttl = Duration::from_secs(12);
        let gid = vec![1u8, 2, 3];
        let fresh = now;
        let stale = now - Duration::from_secs(20);
        let m = rs.remote.entry(gid.clone()).or_default();
        m.insert(k(1), (fresh, 0b01));
        m.insert(k(2), (stale, 0b11));
        let live = rs.live_with_flags(&gid, now, ttl);
        assert_eq!(live.len(), 1, "протухший маячок должен быть отфильтрован");
        assert_eq!(live[0], (k(1), 0b01));
    }

    #[test]
    fn prune_drops_stale() {
        let mut rs = RoomState::default();
        let now = Instant::now();
        let ttl = Duration::from_secs(12);
        let gid = vec![9u8];
        let m = rs.remote.entry(gid.clone()).or_default();
        m.insert(k(1), (now, 0));
        m.insert(k(2), (now - Duration::from_secs(30), 0));
        rs.prune(now, ttl);
        assert_eq!(rs.remote.get(&gid).unwrap().len(), 1);
    }

    #[test]
    fn empty_room_has_no_participants() {
        let rs = RoomState::default();
        assert!(rs
            .live_with_flags(&[7u8], Instant::now(), Duration::from_secs(12))
            .is_empty());
    }
}
