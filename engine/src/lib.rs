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
pub mod room_call;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::budget::Tier;
use p2p_core::settings::AudioSettings;
use p2p_core::store::{Chat, ContactStore, GateStore, MessageStore, Profile, StoredMessage};
use p2p_core::Identity;
/// Реэкспорт: медиа-вложение группового сообщения — часть контракта движка
/// (клиенты и тесты собирают его, чтобы отправить вложение в группу).
pub use p2p_groupchat::MediaBody;
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

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
    Create { name: String },
    Invite { gid: Vec<u8>, peer: [u8; 32] },
    /// Принять входящее приглашение в группу (карточка).
    AcceptInvite { gid: Vec<u8> },
    /// Отклонить входящее приглашение.
    DeclineInvite { gid: Vec<u8> },
    SendText {
        gid: Vec<u8>,
        text: String,
        reply_to: u64,
        reply_sender: String,
        /// Пересылка: hex автора исходного сообщения (пусто — своё).
        fwd_from: String,
        /// «Переслано, автора не называем».
        fwd_anon: bool,
    },
    /// Отозвать свои сообщения у участников («удалить у всех»).
    Retract { gid: Vec<u8>, ids: Vec<u64> },
    /// Удалить сообщения только у себя: пары `(id, автор-hex)`.
    DeleteLocal { gid: Vec<u8>, refs: Vec<(u64, String)> },
    /// Пир появился в сети — проверить незавершённые вступления в группы.
    PeerOnline { peer: [u8; 32] },
    /// Отозвать участника (ADR-004, только админ).
    RemoveMember { gid: Vec<u8>, peer: [u8; 32] },
    /// Ротация ключа группы (PCS, ADR-004).
    Rekey { gid: Vec<u8> },
    /// Выйти из группы (ADR-004).
    Leave { gid: Vec<u8> },
}

/// Работа для потока event_pump: событие мессенджера, групповая команда либо
/// рассылка маячка присутствия.
pub(crate) enum Job {
    Ev(Event),
    Cmd(GroupCmd),
    Beacon,
    /// Рассылка «я вышел» членам комнаты `gid` (мгновенное исчезновение).
    Leave(Vec<u8>),
    /// Предложить блоб пиру (M2): сохранить локально + анонсировать манифест.
    BlobOffer { peer: [u8; 32], content: Vec<u8>, mime: String },
    /// Отдать блоб ФАЙЛОМ: манифест посчитан потоково, чанки читаются с диска.
    BlobOfferFile { peer: [u8; 32], path: PathBuf, manifest: p2p_blob::Manifest },
    /// Отправить медиа-вложение в группу (M2): scrub → сохранить → MLS-ссылка всем +
    /// push блоба каждому участнику. В одном Job (нужны и группы, и BlobManager).
    GroupMedia { gid: Vec<u8>, content: Vec<u8>, mime: String, name: String, info: MediaInfo },
    /// То же ФАЙЛОМ: манифест считается потоково, чанки читаются с диска.
    GroupMediaFile { gid: Vec<u8>, path: PathBuf, mime: String, name: String, info: MediaInfo },
}

/// До этого размера вложение скачивается автоматически (фото, голосовое — их ждут
/// сразу). Крупнее — сначала карточка с именем/размером/превью, байты по кнопке:
/// решение «тянуть ли 800 МБ» принимает получатель, а не отправитель.
const AUTO_DOWNLOAD_LIMIT: u64 = 2 * 1024 * 1024;

/// На сколько поднимаем ingress-бюджет пира после явного «скачать». Продлевается
/// каждым новым запросом, поэтому длинная докачка не обрывается на середине.
const BLOB_BOOST_FOR: Duration = Duration::from_secs(120);

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
            hex::decode(id_hex.trim()).ok().filter(|v| !v.is_empty()).map(ChatKey::Group)
        } else {
            parse_key(id_hex).map(ChatKey::Dm)
        }
    }

    fn as_chat(&self) -> Chat<'_> {
        match self {
            ChatKey::Dm(k) => Chat::Dm(k),
            ChatKey::Group(g) => Chat::Group(g),
        }
    }
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
    pub(crate) fn live_with_flags(&self, gid: &[u8], now: Instant, ttl: Duration) -> Vec<([u8; 32], u8)> {
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
    bench: Mutex<Option<Arc<BenchRecv>>>,
    /// Каталог хранения блобов (контент по hex-id). M2.
    blob_dir: PathBuf,
    /// Персистентная офлайн-очередь исходящих (M3): досылка после офлайна/рестарта.
    outbox: Mutex<p2p_core::outbox::OutboxManager>,
    /// Журнал звонков (локальный) + id текущего активного звонка и время старта.
    calls: Mutex<p2p_core::store::CallStore>,
    /// (id записи, время старта, входящий ли) — активных звонков не бывает больше одного.
    active_call: Mutex<Option<(i64, i64, bool)>>,
    /// Запрошенные пользователем вложения — докачиваем сами, когда пир в сети.
    wanted: Mutex<p2p_core::store::WantedStore>,
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
    /// Чем поднимать новый узел при смене адреса. По умолчанию — настоящий
    /// Yggdrasil; тесты подставляют loopback, иначе ротацию нельзя прогнать без
    /// реальной сети.
    transport_factory: Mutex<Option<TransportFactory>>,
}

/// Создатель транспорта из seed узла: `None` — не удалось поднять.
pub type TransportFactory =
    Arc<dyn Fn(&[u8; 32]) -> Option<Arc<dyn p2p_messenger::Transport>> + Send + Sync>;

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
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// hex(32 байта) → ключ. Кодек-хелпер (строка↔ключ); экспортируется для androidjni.
pub fn parse_key(s: &str) -> Option<[u8; 32]> {
    hex::decode(s.trim()).ok()?.as_slice().try_into().ok()
}

/// Фоновый коннект к пиру (как на десктопе): держим попытки, пока процесс жив.
pub(crate) fn spawn_connect(m: Arc<Messenger>, key: [u8; 32]) {
    std::thread::spawn(move || loop {
        let _ = m.connect(&key, Duration::from_secs(20));
        std::thread::sleep(Duration::from_secs(5));
    });
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
        hex::encode(p2p_core::identity::visual_fingerprint(key))
    }

    pub fn visual_fingerprint_self(&self) -> String {
        hex::encode(p2p_core::identity::visual_fingerprint(&self.me_pub))
    }

    /// Секретная сид-фраза (24 слова BIP39) — по запросу пользователя (онбординг M4
    /// «Показать фразу»). СЕКРЕТ: показывать только на FLAG_SECURE-экране.
    pub fn seed_phrase(&self) -> String {
        self.identity.seed_phrase()
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
        match self.messenger.send_text_reply(key, text, reply_to, reply_mine, fwd_from, fwd_anon) {
            Ok(id) => {
                let t = now();
                let _ = self.messages.lock_safe().append(Chat::Dm(key), &StoredMessage {
                    id, text: text.to_string(), mine: true, time: t, status: "sent".into(),
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
                });
                // M3: в офлайн-очередь — если не дойдёт, дошлём при онлайне/рестарте.
                let ob = self.outbox.lock_safe();
                ob.on_sent(key, id, text, t);
                log::info!("outbox: enqueued id={} peer={:02x?} count={}", id, &key[..4], ob.pending_count());
                Some(id as i64)
            }
            Err(_) => None,
        }
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
            let origin = if m.fwd_from.is_empty() { m.sender.clone() } else { m.fwd_from.clone() };
            if m.system {
                continue; // системные строки — не сообщения
            }
            // Скрыли автора — ключ не уходит ВООБЩЕ: иначе «скрыл» значило бы
            // «спрятал в интерфейсе, но отдал по проводу».
            let origin = if hide_author { String::new() } else { origin };
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

    fn forward_text(&self, dst: &ChatKey, text: &str, origin: &str, anon: bool) -> bool {
        match dst {
            ChatKey::Dm(peer) => self.send_text_full(peer, text, 0, false, origin, anon).is_some(),
            ChatKey::Group(gid) => {
                self.jobs
                    .send(Job::Cmd(GroupCmd::SendText {
                        gid: gid.clone(),
                        text: text.to_string(),
                        reply_to: 0,
                        reply_sender: String::new(),
                        fwd_from: origin.to_string(),
                        fwd_anon: anon,
                    }))
                    .is_ok()
            }
        }
    }

    fn forward_media(&self, dst: &ChatKey, m: &StoredMessage, origin: &str, anon: bool) -> bool {
        // Байты берём из своего хранилища: пересылать ссылку, за которой у нас
        // ничего нет, значит отправить получателю тупик.
        let Some(bytes) = self.blob_read(&m.blob_id) else { return false };
        let thumb = if m.thumb_id.is_empty() { None } else { self.blob_read(&m.thumb_id) };
        let info = MediaInfo {
            duration_ms: m.duration_ms,
            width: m.width,
            height: m.height,
            thumb,
            caption: m.text.clone(),
            fwd_from: origin.to_string(),
            fwd_anon: anon,
        };
        match dst {
            ChatKey::Dm(peer) => {
                self.send_media_with(peer, bytes, &m.mime, &m.name, info).is_some()
            }
            ChatKey::Group(gid) => {
                self.group_send_media(gid.clone(), bytes, m.mime.clone(), m.name.clone(), info);
                true
            }
        }
    }

    /// Удалить сообщения ТОЛЬКО У СЕБЯ. `refs` — пары `(id, mine)`; ключ истории —
    /// именно пара, потому что id посеян временем отправителя и у обеих сторон свой.
    /// Возвращает, сколько строк удалено.
    pub fn delete_messages(&self, key: &[u8; 32], refs: &[(u64, bool)]) -> usize {
        let refs: Vec<(u64, String)> =
            refs.iter().map(|(id, mine)| (*id, self.author_hex(key, *mine))).collect();
        self.messages.lock_safe().delete_many(Chat::Dm(key), &refs).unwrap_or(0)
    }

    /// hex автора сообщения в личном чате: своё — мы, чужое — собеседник.
    fn author_hex(&self, peer: &[u8; 32], mine: bool) -> String {
        if mine { self.me_hex() } else { hex::encode(peer) }
    }

    /// hex автора ЦИТИРУЕМОГО сообщения; пусто, если это не ответ.
    fn reply_author(&self, peer: &[u8; 32], reply_to: u64, reply_mine: bool) -> String {
        if reply_to == 0 { String::new() } else { self.author_hex(peer, reply_mine) }
    }

    /// hex своего ключа — автор всех наших сообщений.
    pub(crate) fn me_hex(&self) -> String {
        hex::encode(self.me_pub)
    }

    /// Отозвать СВОИ сообщения у собеседника («удалить у всех»): просьба уходит
    /// надёжно (доедет и после офлайна), локально сообщения удаляются сразу.
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
        // Из своей очереди тоже убираем: иначе ретрай досылал бы уже отозванное.
        let ob = self.outbox.lock_safe();
        for id in ids {
            ob.on_ack(key, *id);
        }
        drop(ob);
        let refs: Vec<(u64, bool)> = ids.iter().map(|id| (*id, true)).collect();
        self.delete_messages(key, &refs);
        sent
    }

    /// Тип медиа-сообщения по MIME (M2): картинка → превью, аудио → голосовое-плеер,
    /// остальное → файл-карточка.
    fn media_kind(mime: &str) -> &'static str {
        if mime.starts_with("image/") {
            "image"
        } else if mime.starts_with("audio/") {
            "voice"
        } else {
            "file"
        }
    }

    /// Отправить медиа-вложение (M2): scrub (для картинок) → блоб пиру (push) →
    /// медиа-сообщение со ссылкой на блоб по хэшу (тот же надёжный путь, что текст).
    /// Возвращает id сообщения или None. `name` — исходное имя файла (для не-картинок).
    pub fn send_media(&self, key: &[u8; 32], content: Vec<u8>, mime: &str, name: &str) -> Option<i64> {
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
        // Картинки чистим от метаданных (EXIF/GPS) перед отправкой, как аватар.
        let bytes = if mime.starts_with("image/") {
            p2p_blob::scrub::scrub(mime, &content)
        } else {
            content
        };
        let hash = p2p_blob::blob_id(&bytes);
        let kind = Self::media_kind(mime);
        let size = bytes.len() as u64;
        // Миниатюру (если есть) сохраняем и пушим всегда — она мелкая, ради неё карточка.
        let thumb_hash = info.thumb.as_ref().map(|t| {
            let h = p2p_blob::blob_id(t);
            self.store_blob(h, t);
            self.blob_offer(*key, t.clone(), "image/jpeg".to_string());
            h
        });
        // Крупный блоб НЕ пушим: получатель сам запросит его по хэшу, когда решит
        // скачать. Мелкое (фото/голосовое) отдаём сразу — его ждут немедленно.
        if size < AUTO_DOWNLOAD_LIMIT {
            self.blob_offer(*key, bytes.clone(), mime.to_string());
        }
        // Сохраняем локально, чтобы показывать своё вложение сразу и после рестарта.
        self.store_blob(hash, &bytes);
        let id_hex = hex::encode(hash);
        let meta = p2p_messenger::MediaMeta {
            fwd_from: info.fwd_from.clone(),
            fwd_anon: info.fwd_anon,
            size,
            duration_ms: info.duration_ms,
            width: info.width,
            height: info.height,
            thumb: thumb_hash.map(|h| h.to_vec()).unwrap_or_default(),
            caption: info.caption.clone(),
        };
        match self.messenger.send_media_message(key, &hash, mime, name, meta.clone()) {
            Ok(id) => {
                let t = now();
                let _ = self.messages.lock_safe().append(Chat::Dm(key), &StoredMessage {
                    // "sent" — как у текста: отдано в сеть, доставка НЕ подтверждена
                    // (✓✓ появится только по Ack, ☁ — если ретраи выдохлись).
                    id, mine: true, time: t, status: "sent".into(), sender: self.me_hex(),
                    fwd_from: info.fwd_from.clone(),
                    // Подпись живёт в `text`: у медиа-сообщения это поле свободно,
                    // и тогда поиск и история работают с ней как с обычным текстом.
                    text: info.caption.clone(),
                    kind: kind.into(), blob_id: id_hex.clone(), mime: mime.into(), name: name.into(),
                    size, duration_ms: info.duration_ms, width: info.width, height: info.height,
                    thumb_id: hex::encode(&meta.thumb),
                    downloaded: true, // своё вложение уже у нас
                    ..Default::default()
                });
                // В офлайн-очередь — как текст: если получатель сейчас недоступен,
                // дошлём при его возвращении/после рестарта. Байты уже сохранены
                // локально, в очереди лежит только ссылка на них.
                self.outbox.lock_safe().on_sent_media(
                    key, id, &id_hex, mime, name,
                    &p2p_core::outbox::MediaRef {
                        size,
                        duration_ms: info.duration_ms,
                        width: info.width,
                        height: info.height,
                        thumb_hex: hex::encode(&meta.thumb),
                        caption: info.caption.clone(),
                    },
                    t,
                );
                Some(id as i64)
            }
            Err(_) => None,
        }
    }

    /// Отправить вложение, лежащее ФАЙЛОМ, не поднимая его в память.
    ///
    /// Манифест считается потоково, чанки отдаются чтением с диска по запросу, а своя
    /// копия кладётся в хранилище блобов тем же потоком. Картинки при этом идут через
    /// обычный путь в памяти: их надо scrub-ить (EXIF/GPS), а это требует всего файла,
    /// и весят они немного.
    pub fn send_media_file(
        &self,
        key: &[u8; 32],
        path: &std::path::Path,
        mime: &str,
        name: &str,
        info: MediaInfo,
    ) -> Option<i64> {
        let manifest = p2p_blob::manifest_of_file(path, mime.to_string()).ok()?;
        let hash = manifest.id;
        let size = manifest.size;
        let id_hex = hex::encode(hash);
        let kind = Self::media_kind(mime);

        // Своя копия — тем же потоком, чанками и зашифрованной at-rest. Без неё
        // отправитель не сможет ни показать вложение, ни дослать его после рестарта.
        if self.blob_read(&id_hex).is_none() {
            self.store_blob_streamed(path, &manifest).ok()?;
        }

        let thumb_hash = info.thumb.as_ref().map(|t| {
            let h = p2p_blob::blob_id(t);
            self.store_blob(h, t);
            self.blob_offer(*key, t.clone(), "image/jpeg".to_string());
            h
        });
        // Крупное не пушим — получатель запросит сам; мелкое отдаём сразу.
        if size < AUTO_DOWNLOAD_LIMIT {
            let _ = self.jobs.send(Job::BlobOfferFile {
                peer: *key,
                path: path.to_path_buf(),
                manifest: manifest.clone(),
            });
        }
        let meta = p2p_messenger::MediaMeta {
            fwd_from: String::new(),
            fwd_anon: false,
            size,
            duration_ms: info.duration_ms,
            width: info.width,
            height: info.height,
            thumb: thumb_hash.map(|h| h.to_vec()).unwrap_or_default(),
            caption: info.caption.clone(),
        };
        let id = self.messenger.send_media_message(key, &hash, mime, name, meta.clone()).ok()?;
        let t = now();
        let _ = self.messages.lock_safe().append(Chat::Dm(key), &StoredMessage {
            id, mine: true, time: t, status: "sent".into(), sender: self.me_hex(),
            text: info.caption.clone(),
            kind: kind.into(), blob_id: id_hex.clone(), mime: mime.into(), name: name.into(),
            size, duration_ms: info.duration_ms, width: info.width, height: info.height,
            thumb_id: hex::encode(&meta.thumb),
            downloaded: true,
            ..Default::default()
        });
        self.outbox.lock_safe().on_sent_media(
            key, id, &id_hex, mime, name,
            &p2p_core::outbox::MediaRef {
                size,
                duration_ms: info.duration_ms,
                width: info.width,
                height: info.height,
                thumb_hex: hex::encode(&meta.thumb),
                caption: info.caption.clone(),
            },
            t,
        );
        Some(id as i64)
    }

    /// Записать свою копию блоба на диск потоково, в том же чанкованном формате, что
    /// и принятые крупные блобы (каждый чанк зашифрован отдельно).
    fn store_blob_streamed(
        &self,
        src: &std::path::Path,
        manifest: &p2p_blob::Manifest,
    ) -> Result<(), p2p_blob::BlobError> {
        let _ = std::fs::create_dir_all(&self.blob_dir);
        let dst = self.blob_dir.join(hex::encode(manifest.id));
        let mut asm = p2p_blob::disk::DiskAssembler::create(manifest.clone(), &dst)?;
        for i in 0..manifest.chunks.len() {
            let chunk = p2p_blob::read_file_chunk(src, i).map_err(|_| p2p_blob::BlobError::Io)?;
            asm.add(i, &chunk, p2p_core::vault::seal)?;
        }
        asm.finish()?;
        Ok(())
    }

    /// Скачать вложение по кнопке (крупное не тянется автоматически). Запрашиваем
    /// манифест по хэшу — дальше обычная докачка чанков; готовность придёт событием
    /// `blob_received`. Идемпотентно: если блоб уже есть, ничего не делаем.
    pub fn download_media(&self, peer: &[u8; 32], blob_id_hex: &str) -> bool {
        if self.blob_read(blob_id_hex).is_some() {
            return true; // уже скачано
        }
        let Ok(hash) = hex::decode(blob_id_hex.trim()) else { return false };
        // Намерение ПЕРСИСТЕНТНО: пир может быть офлайн или уйти посреди докачки —
        // продолжим сами, когда он вернётся, и переживём перезапуск. Пользователь не
        // должен сторожить, когда собеседник появится в сети.
        let _ = self.wanted.lock_safe().add(blob_id_hex, peer, now());
        // Раз пользователь СAM попросил файл — этот трафик ожидаемый, и резать его
        // своим же анти-флуд бюджетом бессмысленно (иначе 64 МБ ехали бы полторы
        // минуты). Поднимаем лимит адресно: только этому пиру и только пока качаем.
        self.messenger.boost_peer(peer, BLOB_BOOST_FOR);
        self.messenger.request_blob_manifest(peer, &hash).is_ok()
    }

    /// hex-id вложений, докачка которых заказана пользователем и ещё не закончена.
    /// UI по ним рисует прогресс вместо кнопки «Скачать» — в том числе после
    /// перезапуска приложения.
    pub fn wanted_json(&self) -> String {
        let ids: Vec<String> = self.wanted.lock_safe().all().into_iter().map(|w| w.blob_id).collect();
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into())
    }

    /// Продолжить всё, что пользователь просил скачать у этого пира (он в сети).
    /// То же на старте процесса: намерение «скачать» переживает перезапуск.
    fn resume_all_wanted(&self) {
        let mut peers: Vec<[u8; 32]> = self.wanted.lock_safe().all().into_iter().map(|w| w.peer).collect();
        peers.sort();
        peers.dedup();
        for p in peers {
            self.resume_wanted(&p);
        }
    }

    fn resume_wanted(&self, peer: &[u8; 32]) {
        for w in self.wanted.lock_safe().for_peer(peer) {
            if self.blob_read(&w.blob_id).is_some() {
                let _ = self.wanted.lock_safe().remove(&w.blob_id);
                continue;
            }
            if let Ok(hash) = hex::decode(&w.blob_id) {
                self.messenger.boost_peer(peer, BLOB_BOOST_FOR);
                let _ = self.messenger.request_blob_manifest(peer, &hash);
            }
        }
    }

    pub fn connect(&self, key: [u8; 32]) {
        spawn_connect(self.messenger.clone(), key);
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
    pub fn my_contact_code(&self) -> String {
        let epoch = self.transport_epoch();
        if epoch == 0 {
            return hex::encode(self.me_pub);
        }
        let tk = self.identity.transport_public(epoch);
        let sig = self.identity.sign_transport_rotate(epoch, &tk);
        format!("{}:{}:{}:{}", hex::encode(self.me_pub), epoch, hex::encode(tk), hex::encode(sig))
    }

    /// Добавить контакт по коду обмена: либо голый hex личности (как раньше), либо
    /// расширенная форма `личность:эпоха:адрес:подпись`. Привязку к адресу
    /// принимаем только с верной подписью — тем же путём, что и объявление о смене
    /// адреса.
    pub fn add_contact_code(&self, code: &str, name: &str) -> &'static str {
        let parts: Vec<&str> = code.trim().split(':').collect();
        let Some(ik) = parse_key(parts[0]) else { return "error" };
        let outcome = self.add_contact(&ik, name);
        if parts.len() == 4 {
            let epoch = parts[1].parse::<u32>().unwrap_or(0);
            let tk = parse_key(parts[2]);
            let sig = hex::decode(parts[3]).ok().and_then(|s| <[u8; 64]>::try_from(s.as_slice()).ok());
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
        let name = if name.trim().is_empty() {
            format!("{}…", &hex::encode(key)[..8])
        } else {
            name.to_string()
        };
        // ВАЖНО: гард лока из scrutinee `match` живёт до конца всех арм → refresh_budgets
        // (лочит contacts) внутри арма = повторный лок того же мьютекса = дедлок. Поэтому
        // снимаем результат в переменную (гард освобождается на `;`), затем match.
        let outcome = self.contacts.lock_safe().add(key, &name, now());
        match outcome {
            Ok(p2p_core::store::AddOutcome::Added) => {
                spawn_connect(self.messenger.clone(), *key);
                self.refresh_budgets(); // M3: контакт → щедрый ingress-бюджет
                "added"
            }
            Ok(p2p_core::store::AddOutcome::AlreadyKnown) => "known",
            Err(_) => "error",
        }
    }

    /// Пере-залить тиры доверия в мессенджер — единый бюджет ресурсов (M3).
    /// Источник истины: контакты (сверенный fingerprint → `Verified`) и блок-лист
    /// гейта (`Blocked` — режется на ingress, ДО расшифровки). Кого нет — `Stranger`
    /// (общая корзина класса, анти-Sybil). Зовётся при любой смене того или другого.
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
            log::error!("смена адреса: не удалось поднять узел");
            return;
        };
        let address = node.address();
        self.messenger.swap_transport(node);
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
        log::info!("смена адреса завершена: {address} (оповещено {})", recipients.len());
        push_event(self, dto::UiEvent::TransportRotated { address, epoch });
    }

    /// Поднять узел на транспортном ключе указанной эпохи. Одна точка на две
    /// операции — смену адреса и возврат из офлайна.
    fn spawn_node(&self, epoch: u32) -> Option<Arc<dyn p2p_messenger::Transport>> {
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
        if self.messenger.send_transport_rotate(peer, epoch, &tk, &sig).is_ok() {
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
            let _ = std::fs::write(self.data_dir.join("rotate_pending.json"), v);
        }
    }

    /// Прочитать список с диска (после перезапуска).
    fn load_rotate_pending(&self) {
        let list: Vec<String> = std::fs::read(self.data_dir.join("rotate_pending.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        *self.rotate_pending.lock_safe() =
            list.iter().filter_map(|h| parse_key(h)).collect();
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
                    (k, if c.verified { Tier::Verified } else { Tier::Contact }, c.helpful)
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
        let page = self.messages.lock_safe().history(chat, before, HISTORY_PAGE);
        serde_json::to_string(&page).unwrap_or_else(|_| "[]".into())
    }

    pub fn set_verified(&self, key: &[u8; 32], verified: bool) {
        let _ = self.contacts.lock_safe().set_verified(key, verified);
    }

    pub fn rename_contact(&self, key: &[u8; 32], name: &str) {
        let _ = self.contacts.lock_safe().rename(key, name);
    }

    pub fn remove_contact(&self, key: &[u8; 32]) {
        let _ = self.contacts.lock_safe().remove(key);
        self.refresh_budgets();
    }

    // — Гейт контактов (замена TOFU-автодобавления) —
    /// Входящие запросы от незнакомцев как JSON-массив (ключ, fingerprint, превью, время).
    pub fn contact_requests_json(&self) -> String {
        serde_json::to_string(&self.gate.lock_safe().list_requests()).unwrap_or_else(|_| "[]".into())
    }

    /// Одобрить запрос: незнакомец становится контактом, снимаем запрос, шлём ему свой
    /// профиль и держим коннект. Дальнейшие его сообщения идут в чат как обычно.
    /// (Первое сообщение-превью в историю не заливаем — это было только для решения.)
    pub fn accept_contact_request(&self, key: &[u8; 32]) -> bool {
        if self.gate.lock_safe().take_request(key).is_none() {
            return false;
        }
        // Пустое локальное имя → покажется самоназвание пира или hex (см. Contact.displayName).
        let _ = self.contacts.lock_safe().add(key, "", now());
        self.refresh_budgets();
        spawn_connect(self.messenger.clone(), *key);
        let m = self.load_profile_meta();
        let hash = hex::decode(&m.avatar_id).unwrap_or_default();
        let _ = self.messenger.send_profile_update(key, &m.name, &hash);
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

    /// Снимок офлайн-очереди для UI: `{pending, bytes, peers, warn}`. Ничего не
    /// удаляем автоматически — по `warn` клиент показывает предупреждение, а решение
    /// (подождать или сбросить очередь) остаётся за пользователем.
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
        self.outbox.lock_safe().clear_all()
    }

    /// Сбросить очередь недоставленного конкретному пиру (явное действие пользователя).
    pub fn clear_outbox_for(&self, key: &[u8; 32]) -> usize {
        self.outbox.lock_safe().clear_peer(key)
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
            let Ok(meta) = std::fs::metadata(&path) else { continue };
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
            .map(|(p, (files, bytes))| dto::ChatStorage { peer: hex::encode(p), files, bytes })
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
    pub fn clear_blobs(&self) -> u64 {
        let mut freed = 0u64;
        if let Ok(rd) = std::fs::read_dir(&self.blob_dir) {
            for e in rd.flatten() {
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_file() && std::fs::remove_file(e.path()).is_ok() {
                    freed += meta.len();
                }
            }
        }
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
        self.jobs.send(Job::Cmd(GroupCmd::Create { name: name.to_string() })).is_ok()
    }

    pub fn group_invite(&self, gid: Vec<u8>, peer: [u8; 32]) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::Invite { gid, peer }));
    }

    /// Входящие приглашения в группы как JSON [{gid, name, inviter}] (из кэша).
    pub fn group_invites_json(&self) -> String {
        let s = self.group_cache.lock_safe().invites.clone();
        if s.is_empty() { "[]".into() } else { s }
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
        }));
    }

    /// Отправить медиа-вложение в группу (M2). Fire-and-forget: движок чистит
    /// картинку, сохраняет блоб, шлёт MLS-ссылку всем и пушит байты каждому.
    /// Текст ответом на сообщение `reply_to` автора `reply_sender` (hex).
    pub fn group_send_text_reply(&self, gid: Vec<u8>, text: String, reply_to: u64, reply_sender: String) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::SendText {
            gid,
            text,
            reply_to,
            reply_sender,
            fwd_from: String::new(),
            fwd_anon: false,
        }));
    }

    /// Отозвать СВОИ сообщения в группе у всех участников.
    pub fn group_retract(&self, gid: Vec<u8>, ids: Vec<u64>) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::Retract { gid, ids }));
    }

    /// Удалить групповые сообщения только у себя.
    pub fn group_delete_local(&self, gid: Vec<u8>, refs: Vec<(u64, String)>) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::DeleteLocal { gid, refs }));
    }

    pub fn group_send_media(
        &self,
        gid: Vec<u8>,
        content: Vec<u8>,
        mime: String,
        name: String,
        info: MediaInfo,
    ) {
        let _ = self.jobs.send(Job::GroupMedia { gid, content, mime, name, info });
    }

    /// Отправить в группу вложение, лежащее ФАЙЛОМ, не поднимая его в память.
    /// Fire-and-forget: манифест и своя копия считаются на акторе.
    pub fn group_send_media_file(
        &self,
        gid: Vec<u8>,
        path: PathBuf,
        mime: String,
        name: String,
        info: MediaInfo,
    ) {
        let _ = self.jobs.send(Job::GroupMediaFile { gid, path, mime, name, info });
    }

    /// Отозвать участника из группы (ADR-004, только админ). Fire-and-forget.
    pub fn group_remove_member(&self, gid: Vec<u8>, peer: [u8; 32]) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::RemoveMember { gid, peer }));
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
    /// Предложить пиру блоб (картинку/файл/голосовое). Fire-and-forget: движок
    /// сохранит локально и анонсирует манифест; докачку ведёт получатель. id блоба
    /// детерминирован — `blob_id` (BLAKE3), UI может вычислить его сам для ссылки.
    pub fn blob_offer(&self, peer: [u8; 32], content: Vec<u8>, mime: String) {
        let _ = self.jobs.send(Job::BlobOffer { peer, content, mime });
    }

    /// Контент-адрес (hex) содержимого — для ссылки на блоб из сообщения/аватарки.
    pub fn blob_id_hex(&self, content: &[u8]) -> String {
        hex::encode(p2p_blob::blob_id(content))
    }

    /// Прочитать сохранённый блоб по hex-id (принятый или свой отправленный).
    /// Расшифровывает at-rest (`vault::open`).
    pub fn blob_read(&self, id_hex: &str) -> Option<Vec<u8>> {
        read_blob_at(&self.blob_dir, id_hex)
    }

    /// Есть ли блоб локально. Отдельно от [`blob_read`], потому что «скачано ли
    /// вложение» нельзя выяснять чтением: у крупного это гигабайт в память ради
    /// одного булева.
    pub fn blob_exists(&self, id_hex: &str) -> bool {
        let Ok(raw) = hex::decode(id_hex.trim()) else { return false };
        self.blob_dir.join(hex::encode(raw)).exists()
    }

    // — Свой профиль (M2): имя + аватар (контент-адресный блоб). Хранится в
    // profile_meta.json; рассылается контактам при смене и на коннекте.
    fn store_blob(&self, id: p2p_blob::Hash, content: &[u8]) {
        let _ = std::fs::create_dir_all(&self.blob_dir);
        let _ = std::fs::write(self.blob_dir.join(hex::encode(id)), p2p_core::vault::seal(content));
    }

    fn profile_meta_path(&self) -> PathBuf {
        self.blob_dir.parent().unwrap_or(&self.blob_dir).join("profile_meta.json")
    }

    fn load_profile_meta(&self) -> ProfileMeta {
        std::fs::read(self.profile_meta_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_profile_meta(&self, m: &ProfileMeta) {
        if let Ok(v) = serde_json::to_vec(m) {
            let _ = std::fs::write(self.profile_meta_path(), v);
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
        let scrubbed = p2p_blob::scrub::scrub(mime, &content);
        let id = p2p_blob::blob_id(&scrubbed);
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

    /// Разослать свой профиль (имя + хэш аватара) всем контактам (M2). Fire-and-forget.
    pub fn broadcast_profile(&self) {
        let m = self.load_profile_meta();
        let hash = hex::decode(&m.avatar_id).unwrap_or_default();
        let keys: Vec<[u8; 32]> =
            self.contacts.lock_safe().list().iter().filter_map(|c| parse_key(&c.key)).collect();
        for k in keys {
            let _ = self.messenger.send_profile_update(&k, &m.name, &hash);
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
                rows.push(GroupRowJson { gid: hex, name, members: 0, admin: false, archived: true });
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
        self.group_cache.lock_safe().members.get(gid).cloned().unwrap_or_else(|| "[]".into())
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
        self.room_call.lock_safe().as_ref().map(|r| r.engine.clone())
    }

    pub(crate) fn live_participants(&self, gid: &[u8]) -> Vec<[u8; 32]> {
        self.rooms.lock_safe().live_with_flags(gid, Instant::now(), PRESENCE_TTL)
            .into_iter().map(|(p, _)| p).collect()
    }

    fn sync_room_members(&self) {
        let guard = self.room_call.lock_safe();
        if let Some(rc) = guard.as_ref() {
            let mut live = self.live_participants(&rc.gid);
            // SEC-2 (ADR-004): для ГРУППОВОЙ комнаты в состав звонка допускаем только
            // пиров из текущего MLS-ростера — маячок присутствия по gid сам по себе не
            // пропуск. DM-комнаты (room_id в dm_peers) не фильтруем: там ростера нет.
            // Fail-open, если ростер ещё не в кэше (не рвём легитимный звонок): гейт
            // обретает зубы после отзыва (SEC-1), когда ростер реально сужается.
            let is_dm = self.rooms.lock_safe().dm_peers.contains_key(&rc.gid);
            if !is_dm {
                let roster = self.group_roster_set(&rc.gid);
                if !roster.is_empty() {
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
        let room_id = p2p_core::dm_room_id(&self.me_pub, &peer);
        // Пишем сразу как «не дозвонился»: исход поднимется до answered, когда
        // собеседник появится в комнате. Иначе оборванная попытка не попала бы в
        // журнал вовсе — а именно её чаще всего и ищут.
        self.begin_call(&peer, &room_id, false, "unanswered");
        let _ = self.messenger.send_room_invite(&peer, &room_id, audio, video);
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
        let room_id = p2p_core::dm_room_id(&self.me_pub, &peer);
        let _ = self.messenger.send_room_decline(&peer, &room_id);
        // «Отклонён» — исход только ВХОДЯЩЕГО: отклонить может лишь принимающая
        // сторона. Свой же исходящий, который я сам и завершил, отклонённым не был —
        // он остаётся «не дозвонился».
        if let Some((id, _, true)) = *self.active_call.lock_safe() {
            self.calls.lock_safe().set_outcome(id, "declined");
            *self.active_call.lock_safe() = None;
        }
    }

    /// Принять DM-звонок: войти в комнату с регистрацией dm_peer. Возвращает room_id.
    pub fn room_join_dm(&self, peer: [u8; 32], audio: bool, video: bool) -> Vec<u8> {
        let room_id = p2p_core::dm_room_id(&self.me_pub, &peer);
        match *self.active_call.lock_safe() {
            // Входящий, который мы приняли: запись уже заведена на RoomInvite.
            Some((id, _, _)) => self.calls.lock_safe().set_outcome(id, "answered"),
            None => self.begin_call(&peer, &room_id, true, "answered"),
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

    pub fn room_join(&self, gid: Vec<u8>, audio: bool, video: bool) {
        self.rooms.lock_safe().joined.insert(gid, av_flags(audio, video));
        let _ = self.jobs.send(Job::Beacon);
    }

    pub fn room_leave(&self, gid: Vec<u8>) {
        self.end_call();
        {
            let mut rs = self.rooms.lock_safe();
            rs.joined.remove(&gid);
            rs.remote.remove(&gid);
        }
        let _ = self.jobs.send(Job::Leave(gid));
    }

    /// Начать запись о звонке (одна активная за раз — звонков параллельно не бывает).
    fn begin_call(&self, peer: &[u8; 32], gid: &[u8], incoming: bool, outcome: &str) {
        let now = now();
        let id = self.calls.lock_safe().start(peer, gid, incoming, outcome, now);
        if id >= 0 {
            *self.active_call.lock_safe() = Some((id, now, incoming));
        }
    }

    /// Закрыть активную запись: проставить длительность.
    fn end_call(&self) {
        let Some((id, started, _)) = self.active_call.lock_safe().take() else { return };
        self.calls.lock_safe().finish(id, now() - started);
    }

    /// Заметка себе («Сохранённое»). Хранится как обычное сообщение в чате с самим
    /// собой: никуда не отправляется, статусов доставки у неё нет — отправлять некому.
    /// Отдельное хранилище заводить незачем, история уже умеет всё нужное.
    pub fn save_note(&self, text: &str) -> Option<i64> {
        let t = now();
        let id = t as u64;
        let me = self.me_pub;
        self.messages
            .lock_safe()
            .append(Chat::Dm(&me), &StoredMessage {
                id,
                sender: self.me_hex(),
                text: text.to_string(),
                mine: true,
                time: t,
                status: String::new(),
                kind: "text".into(),
                ..Default::default()
            })
            .ok()?;
        Some(t)
    }

    /// Заметки себе, старые первыми (как в чате).
    pub fn notes_json(&self) -> String {
        let me = self.me_pub;
        self.history_json(&me)
    }

    /// Удалить заметку по её id.
    pub fn delete_note(&self, id: u64) {
        let me = self.me_pub;
        let _ = self.messages.lock_safe().delete_one(Chat::Dm(&me), &self.me_hex(), id);
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
        let live = self.rooms.lock_safe().live_with_flags(gid, Instant::now(), PRESENCE_TTL);
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
                p2p_core::settings::OpusMode::Audio
            } else {
                p2p_core::settings::OpusMode::Voip
            };
        }
        let members = Arc::new(Mutex::new(self.live_participants(&gid)));
        let engine = RoomCallEngine::start(self.messenger.clone(), audio, members.clone());
        let old = self.room_call.lock_safe().replace(RoomCall { gid, engine, members });
        if let Some(old) = old {
            std::thread::spawn(move || old.engine.stop());
        }
        call_params(&audio)
    }

    pub fn room_audio_stop(&self) {
        let rc = self.room_call.lock_safe().take();
        if let Some(rc) = rc {
            std::thread::spawn(move || rc.engine.stop());
        }
    }

    pub fn room_push_audio(&self, samples: &[i16]) {
        if let Some(engine) = self.room_engine() {
            engine.push_audio(samples);
        }
    }

    pub fn room_pull_audio(&self, out: &mut [i16]) -> usize {
        self.room_engine().map(|e| e.pull_audio(out)).unwrap_or(0)
    }

    /// Hex-ключи говорящих сейчас участников (active-speaker). "[]" вне звонка.
    pub fn room_speaking_json(&self) -> String {
        self.room_engine().map(|e| e.speaking_json()).unwrap_or_else(|| "[]".into())
    }

    /// Сменить Opus-битрейт текущей аудио-сессии на лету (настройки звука без перезвона).
    pub fn room_set_audio_bitrate(&self, bitrate: i32) {
        if let Some(e) = self.room_engine() {
            e.set_bitrate(bitrate);
        }
    }

    /// Накопленные байты трафика по участникам {hex: rx, "self": tx}. "{}" вне звонка.
    pub fn room_traffic_json(&self) -> String {
        self.room_engine().map(|e| e.traffic_json()).unwrap_or_else(|| "{}".into())
    }

    pub fn room_set_muted(&self, muted: bool) {
        if let Some(engine) = self.room_engine() {
            engine.set_muted(muted);
        }
    }

    pub fn room_push_video(&self, frame: &[u8], keyframe: bool) {
        if let Some(engine) = self.room_engine() {
            engine.push_video(frame, keyframe);
        }
    }

    pub fn room_set_video_cap(&self, cap: usize) {
        if let Some(engine) = self.room_engine() {
            engine.set_video_cap(cap);
        }
    }

    pub fn room_pull_video_packet(&self, peer: &[u8; 32]) -> Option<Vec<u8>> {
        self.room_engine().and_then(|e| e.pull_video_for(peer))
    }

    // — Диагностика: тест пропускной способности меша —
    pub fn bench_recv_start(&self) {
        let rx = self.messenger.media_channel(8192);
        let st = Arc::new(BenchRecv {
            bytes: AtomicU64::new(0), pkts: AtomicU64::new(0),
            running: AtomicBool::new(true), start: Instant::now(),
        });
        *self.bench.lock_safe() = Some(st.clone());
        std::thread::spawn(move || {
            while st.running.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(p) if p.track == BENCH_TRACK => {
                        st.bytes.fetch_add(p.payload.len() as u64, Ordering::Relaxed);
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
            if self.messenger.send_media(peer, BENCH_TRACK, seq, 0, &payload).is_ok() {
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
    if audio { f |= 0b01; }
    if video { f |= 0b10; }
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
        let _ = std::fs::write(transport_meta_path(dir), v);
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
fn record_request(engine: &Arc<Engine>, peer: [u8; 32], preview: &str) {
    let preview: String = preview.chars().take(200).collect();
    let is_new = engine.gate.lock_safe().upsert_request(&peer, &preview, now()).unwrap_or(false);
    push_event(engine, dto::UiEvent::ContactRequest { peer: hex::encode(peer), new: is_new });
}

/// Положить событие в очередь для клиента. Принимает ТИП, а не свободный JSON:
/// форма события фиксируется `dto::UiEvent` и проверяется компилятором.
fn push_event(engine: &Engine, ev: dto::UiEvent) {
    engine.events.lock_safe().push_back(ev.to_json());
}

/// Поток event_pump = групповой актор: владеет [`GroupManager`] (GroupChat !Send),
/// обрабатывает события мессенджера и групповые команды — всё в одном потоке.
fn event_pump(
    engine: Arc<Engine>,
    job_rx: Receiver<Job>,
    identity: Identity,
    messenger: Arc<Messenger>,
    data_dir: PathBuf,
) {
    let blob_dir = data_dir.join("blobs");
    let mut groups = GroupManager::new(identity, messenger.clone(), data_dir, engine.messages.clone());
    let mut blobs = blobs::BlobManager::new(messenger.clone(), p2p_blob::Limits::default(), blob_dir);
    {
        let ob = engine.outbox.lock_safe();
        log::info!("outbox on_start: pending={} (до prune+досылки)", ob.pending_count());
    }
    engine.outbox.lock_safe().on_start(now()); // M3: prune TTL + дослать недоставленное
    engine.resume_all_wanted(); // докачки, начатые до перезапуска
    refresh_group_cache(&engine, &groups); // прайм кэша
    push_event(&engine, dto::UiEvent::GroupsChanged);
    let mut last_beacon = Instant::now();
    loop {
        match job_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Job::Ev(ev)) => handle_event(&engine, &mut groups, &mut blobs, ev),
            Ok(Job::Cmd(cmd)) => handle_cmd(&engine, &mut groups, cmd),
            Ok(Job::BlobOfferFile { peer, path, manifest }) => {
                blobs.offer_file(peer, manifest, path);
            }
            Ok(Job::BlobOffer { peer, content, mime }) => {
                // Сохраняем свой блоб локально (чтобы отправитель тоже его показывал)
                // и анонсируем манифест — докачку ведёт получатель.
                save_blob(&engine, p2p_blob::blob_id(&content), &content);
                blobs.offer(peer, &content, &mime);
            }
            Ok(Job::GroupMedia { gid, content, mime, name, info }) => {
                // Картинки чистим (scrub), считаем контент-адрес, сохраняем локально.
                let bytes = if mime.starts_with("image/") {
                    p2p_blob::scrub::scrub(&mime, &content)
                } else {
                    content
                };
                let hash = p2p_blob::blob_id(&bytes);
                let id_hex = hex::encode(hash);
                let size = bytes.len() as u64;
                let members: Vec<[u8; 32]> =
                    groups.members(&gid).into_iter().filter(|m| *m != engine.me_pub).collect();
                save_blob(&engine, hash, &bytes);
                let thumb_hex = push_group_thumb(&engine, &mut blobs, &members, info.thumb.as_deref());
                let media = MediaBody {
                    // id проставит слой групп при отправке (он же кладёт его в историю).
                    id: 0,
                    fwd_from: info.fwd_from.clone(),
                    fwd_anon: info.fwd_anon,
                    kind: Engine::media_kind(&mime).into(),
                    hash: id_hex,
                    mime: mime.clone(),
                    name,
                    caption: info.caption,
                    size,
                    duration_ms: info.duration_ms,
                    width: info.width,
                    height: info.height,
                    thumb: thumb_hex,
                };
                // MLS-ссылка всем; байты — только мелкие. Крупное участник запросит
                // сам по кнопке: пуш «всем сразу» означал бы, что отправитель решает
                // за пятерых, тратить ли им трафик на видео (в DM так уже нельзя).
                if groups.send_media(&gid, &media) {
                    if size < AUTO_DOWNLOAD_LIMIT {
                        for m in &members {
                            blobs.offer(*m, &bytes, &mime);
                        }
                    }
                    push_event(&engine, dto::UiEvent::group_msg_mine(hex::encode(&gid)));
                }
            }
            Ok(Job::GroupMediaFile { gid, path, mime, name, info }) => {
                // Потоковый путь: ни манифест, ни отдача чанков не поднимают файл в
                // память. Картинки сюда не приходят — им нужен scrub целиком.
                let Ok(manifest) = p2p_blob::manifest_of_file(&path, mime.clone()) else { continue };
                let id_hex = hex::encode(manifest.id);
                let size = manifest.size;
                let members: Vec<[u8; 32]> =
                    groups.members(&gid).into_iter().filter(|m| *m != engine.me_pub).collect();
                if engine.blob_read(&id_hex).is_none() {
                    let _ = engine.store_blob_streamed(&path, &manifest);
                }
                let thumb_hex = push_group_thumb(&engine, &mut blobs, &members, info.thumb.as_deref());
                let media = MediaBody {
                    // id проставит слой групп при отправке (он же кладёт его в историю).
                    id: 0,
                    fwd_from: info.fwd_from.clone(),
                    fwd_anon: info.fwd_anon,
                    kind: Engine::media_kind(&mime).into(),
                    hash: id_hex,
                    mime: mime.clone(),
                    name,
                    caption: info.caption,
                    size,
                    duration_ms: info.duration_ms,
                    width: info.width,
                    height: info.height,
                    thumb: thumb_hex,
                };
                if groups.send_media(&gid, &media) {
                    // Мелкое отдаём сразу тем же файлом (чанки читаются с диска);
                    // крупное ждёт запроса получателя.
                    if size < AUTO_DOWNLOAD_LIMIT {
                        for m in &members {
                            blobs.offer_file(*m, manifest.clone(), path.clone());
                        }
                    }
                    push_event(&engine, dto::UiEvent::group_msg_mine(hex::encode(&gid)));
                }
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
                            if m == engine.me_pub { continue; }
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
        blobs.tick(); // переспросить потерянные чанки докачки (M2)
        // Прогресс докачки наверх: UI рисует кружок вместо бесконечного спиннера.
        for (id, done, total) in blobs.take_progress() {
            push_event(&engine, dto::UiEvent::BlobProgress {
                id: hex::encode(id),
                done,
                total,
            });
        }
    }
}

/// Прочитать блоб из каталога по hex-id. Свободная функция — нужна и офлайн-очереди
/// (её колбэк живёт вне `Engine`), и самому движку.
fn read_blob_at(dir: &Path, id_hex: &str) -> Option<Vec<u8>> {
    // Санитизация: только валидный hex → чистое имя файла (без traversal).
    let clean = hex::encode(hex::decode(id_hex.trim()).ok()?);
    let path = dir.join(clean);
    // Крупные блобы лежат в чанкованном формате (собраны потоково на диск), мелкие —
    // одним запечатанным буфером. Различаем по магии в заголовке.
    if p2p_blob::disk::read_header(&path).is_some() {
        return p2p_blob::disk::read_all(&path, |b| p2p_core::vault::open(b).ok());
    }
    let raw = std::fs::read(path).ok()?;
    p2p_core::vault::open(&raw).ok()
}

/// Сохранить блоб на диск по hex-id (принятый или свой отправленный).
/// Шифрует at-rest (`vault::seal`) — как сообщения/группы.
fn save_blob(engine: &Arc<Engine>, id: p2p_blob::Hash, content: &[u8]) {
    let _ = std::fs::create_dir_all(&engine.blob_dir);
    let _ = std::fs::write(engine.blob_dir.join(hex::encode(id)), p2p_core::vault::seal(content));
}

/// Сохранить миниатюру группового вложения и раздать её всем участникам, вернув
/// её hex-id (пусто, если миниатюры нет).
///
/// Превью пушим ВСЕГДА, в отличие от самого вложения: оно мелкое, и ради него
/// карточка и существует — без картинки предложение «скачать 800 МБ» слепое.
fn push_group_thumb(
    engine: &Arc<Engine>,
    blobs: &mut blobs::BlobManager,
    members: &[[u8; 32]],
    thumb: Option<&[u8]>,
) -> String {
    let Some(t) = thumb.filter(|t| !t.is_empty()) else { return String::new() };
    let h = p2p_blob::blob_id(t);
    save_blob(engine, h, t);
    for m in members {
        blobs.offer(*m, t, "image/jpeg");
    }
    hex::encode(h)
}

/// Сохранить принятый блоб и уведомить UI (`blob_received`).
fn save_and_notify_blob(
    engine: &Arc<Engine>,
    groups: &mut GroupManager,
    peer: [u8; 32],
    done: blobs::Completed,
) {
    match &done.data {
        // Мелкий блоб собран в памяти — сохраняем как раньше (один запечатанный файл).
        blobs::CompletedData::InMemory(content) => save_blob(engine, done.id, content),
        // Крупный уже лежит на диске в чанкованном виде — писать нечего.
        blobs::CompletedData::OnDisk(_) => {}
    }
    // M3, репутация: пир потратил на нас свой трафик, отдав блоб целиком → растим
    // его бюджет. Не-контакту счётчик не двигается (анти-Sybil, см. budget.rs).
    let bumped = engine.contacts.lock_safe().bump_helpful(&peer).unwrap_or(0);
    if bumped > 0 {
        engine.refresh_budgets();
    }
    // Отмечаем вложение скачанным во ВСЕХ чатах: блоб контент-адресный, один и тот
    // же файл в разных чатах скачан разом. Иначе карточка так и предлагала бы «скачать».
    let id_hex = hex::encode(done.id);
    let _ = engine.messages.lock_safe().mark_downloaded(&id_hex);
    // То же для групп: их история живёт отдельным стором, но блоб — один и тот же.
    for gid in groups.mark_downloaded(&id_hex) {
        push_event(engine, dto::UiEvent::GroupMsg {
            gid: hex::encode(&gid),
            mine: false,
            text: None,
            sender: None,
            system: None,
            media: None,
        });
    }
    let _ = engine.wanted.lock_safe().remove(&id_hex); // намерение исполнено
    push_event(engine, dto::UiEvent::BlobReceived {
        id: id_hex,
        mime: done.manifest.mime,
        size: done.manifest.size,
    });
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
                if m == engine.me_pub { continue; }
                let _ = messenger.send_room_presence(&m, &gid, flags);
            }
        }
    }
    engine.rooms.lock_safe().prune(Instant::now(), PRESENCE_TTL);
    engine.sync_room_members();
}

/// Обработать одно событие мессенджера: персист + JSON в очередь + маршрутизация групп.
fn handle_event(engine: &Arc<Engine>, groups: &mut GroupManager, blobs: &mut blobs::BlobManager, ev: Event) {
    match ev {
        Event::Presence { peer, online } => {
            if online {
                // Профиль и досылку — ТОЛЬКО контактам (не светим профиль незнакомцам,
                // presence сам по себе больше не добавляет в контакты — нужен явный запрос).
                let is_contact = engine.contacts.lock_safe().contains(&peer);
                if is_contact {
                    // M2: на коннекте шлём контакту свой профиль (догоняет смену, что была офлайн).
                    let m = engine.load_profile_meta();
                    let hash = hex::decode(&m.avatar_id).unwrap_or_default();
                    let _ = engine.messenger.send_profile_update(&peer, &m.name, &hash);
                    // Продолжаем то, что пользователь просил скачать у этого пира.
                    engine.resume_wanted(&peer);
                    let mut ob = engine.outbox.lock_safe();
                    let c = ob.pending_count();
                    ob.on_online(&peer); // M3: дослать ожидающее пиру
                    if c > 0 {
                        log::info!("outbox on_online peer={:02x?}: досылка (в очереди {})", &peer[..4], c);
                    }
                }
            }
            if online {
                // Вступление в группу могло застрять: мы нажали «Вступить», пока
                // пригласивший был офлайн. Он вернулся — повторяем KeyPackage.
                let _ = engine.jobs.send(Job::Cmd(GroupCmd::PeerOnline { peer }));
            }
            push_event(engine, dto::UiEvent::Presence { peer: hex::encode(peer), online });
            // ADR-007: контакт вернулся, а объявление о смене нашего адреса до него
            // не доехало — досылаем сейчас.
            if online {
                engine.announce_rotation_to(&peer);
            }
        }
        Event::Text { peer, id, text, reply_to, reply_mine, fwd_from, fwd_anon } => {
            match gate_of(engine, &peer) {
                Gate::Blocked => return,
                Gate::Stranger => {
                    record_request(engine, peer, &text);
                    return;
                }
                Gate::Contact => {}
            }
            // Дедуп повторной доставки: офлайн-досылка со стабильным id приходит дважды.
            let peer_hex = hex::encode(peer);
            let dup = engine.messages.lock_safe().contains(Chat::Dm(&peer), &peer_hex, id);
            if !dup {
                let t = now();
                // reply_mine прислан С ТОЧКИ ЗРЕНИЯ ОТПРАВИТЕЛЯ и уже перевёрнут выше,
                // поэтому «моё» здесь — действительно наше сообщение.
                let _ = engine.messages.lock_safe().append(Chat::Dm(&peer), &StoredMessage {
                    id, text: text.clone(), mine: false, time: t, status: String::new(),
                    sender: peer_hex.clone(),
                    reply_to,
                    reply_sender: engine.reply_author(&peer, reply_to, reply_mine),
                    fwd_from: fwd_from.clone(),
                    fwd_anon,
                    kind: "text".into(),
                    ..Default::default()
                });
                push_event(engine, dto::UiEvent::Text {
                    peer: hex::encode(peer),
                    id,
                    text,
                    time: t,
                    reply_to,
                    reply_mine,
                });
            }
        }
        Event::Retract { peer, target } => {
            // Удаляем ЕГО сообщение (mine=false): отозвать наше собственное он не может.
            let n = engine
                .messages
                .lock_safe()
                .delete_many(Chat::Dm(&peer), &[(target, hex::encode(peer))])
                .unwrap_or(0);
            if n > 0 {
                push_event(engine, dto::UiEvent::Retracted { peer: hex::encode(peer), id: target });
            }
        }
        Event::Media {
            peer, id, hash, mime, name, size, duration_ms, width, height, caption, thumb, fwd_from,
            fwd_anon,
        } => {
            match gate_of(engine, &peer) {
                Gate::Blocked => return,
                Gate::Stranger => {
                    record_request(engine, peer, &format!("[{}]", Engine::media_kind(&mime)));
                    return;
                }
                Gate::Contact => {}
            }
            let dup = engine.messages.lock_safe().contains(Chat::Dm(&peer), &hex::encode(peer), id);
            if !dup {
                let t = now();
                let id_hex = hex::encode(&hash);
                let thumb_hex = hex::encode(&thumb);
                let kind = Engine::media_kind(&mime);
                let have = engine.blob_read(&id_hex).is_some();
                // Крупное НЕ тянем молча: показываем карточку (имя/размер/превью), а
                // байты качаем по кнопке — иначе чужое видео на 800 МБ приедет само.
                let auto = have || size < AUTO_DOWNLOAD_LIMIT;
                let _ = engine.messages.lock_safe().append(Chat::Dm(&peer), &StoredMessage {
                    id, mine: false, time: t, status: String::new(),
                    sender: hex::encode(peer),
                    fwd_from: fwd_from.clone(),
                    fwd_anon,
                    text: caption.clone(),
                    kind: kind.into(), blob_id: id_hex.clone(), mime: mime.clone(), name: name.clone(),
                    size, duration_ms, width, height, thumb_id: thumb_hex.clone(),
                    downloaded: have,
                    ..Default::default()
                });
                // Миниатюру тянем всегда — она мелкая, ради неё и заведена.
                if !thumb.is_empty() && engine.blob_read(&thumb_hex).is_none() {
                    let _ = engine.messenger.request_blob_manifest(&peer, &thumb);
                }
                if auto && !have {
                    let _ = engine.messenger.request_blob_manifest(&peer, &hash);
                }
                push_event(engine, dto::UiEvent::Media {
                    peer: hex::encode(peer),
                    id,
                    time: t,
                    kind: kind.to_string(),
                    blob_id: id_hex,
                    mime,
                    name,
                    size,
                    duration_ms,
                    width,
                    height,
                    thumb_id: thumb_hex,
                    downloaded: have,
                    auto,
                });
            }
        }
        Event::Ack { peer, id } => {
            let _ = engine.messages.lock_safe().set_status(Chat::Dm(&peer), id, "delivered");
            engine.outbox.lock_safe().on_ack(&peer, id); // M3: снять из очереди
            push_event(engine, dto::UiEvent::Ack { peer: hex::encode(peer), id });
        }
        Event::Undelivered { peer, id } => {
            // "queued", а НЕ "failed": ретраи в памяти кончились, но сообщение лежит в
            // персистентной офлайн-очереди и уйдёт, когда получатель появится. Раньше
            // в БД писалось "failed" (в UI при этом рисовался часами "queued"), и после
            // перезапуска ждущее доставки сообщение показывалось красным «!».
            let _ = engine.messages.lock_safe().set_status(Chat::Dm(&peer), id, "queued");
            push_event(engine, dto::UiEvent::Undelivered { peer: hex::encode(peer), id });
        }
        Event::RoomInvite { peer, room_id, audio, video } => {
            // Заводим сразу как пропущенный: если пользователь ответит, исход
            // поднимется в room_join_dm, если откажется — в room_decline_dm.
            engine.begin_call(&peer, &room_id, true, "missed");
            push_event(engine, dto::UiEvent::RoomInvite {
                peer: hex::encode(peer),
                room_id: hex::encode(&room_id),
                audio,
                video,
            });
        }
        Event::RoomDecline { peer, room_id } => push_event(engine, dto::UiEvent::RoomDecline {
            peer: hex::encode(peer),
            room_id: hex::encode(&room_id),
        }),
        Event::Group { peer, data } => {
            let out = groups.on_group(&data);
            if let Some(gid) = &out.gid {
                if out.text.is_some() || out.system || out.media.is_some() {
                    // M2: докачка блоба по хэшу от отправителя, если его у нас нет.
                    // Крупное НЕ тянем молча — как в DM, показываем карточку, а байты
                    // качаем по кнопке: иначе чужое видео приезжает всей группе само.
                    let media = out.media.as_ref().map(|m| {
                        let have = engine.blob_read(&m.hash).is_some();
                        let auto = have || m.size < AUTO_DOWNLOAD_LIMIT;
                        // Миниатюру тянем всегда — она мелкая, ради неё карточка и есть.
                        if !m.thumb.is_empty() && engine.blob_read(&m.thumb).is_none() {
                            if let Ok(h) = hex::decode(&m.thumb) {
                                let _ = engine.messenger.request_blob_manifest(&peer, &h);
                            }
                        }
                        if auto && !have {
                            if let Ok(h) = hex::decode(&m.hash) {
                                let _ = engine.messenger.request_blob_manifest(&peer, &h);
                            }
                        }
                        if have {
                            groups.mark_downloaded(&m.hash);
                        }
                        dto::GroupMediaEv {
                            kind: m.kind.clone(),
                            blob_id: m.hash.clone(),
                            mime: m.mime.clone(),
                            name: m.name.clone(),
                            size: m.size,
                            duration_ms: m.duration_ms,
                            width: m.width,
                            height: m.height,
                            thumb_id: m.thumb.clone(),
                            downloaded: have,
                            auto,
                        }
                    });
                    // Обновляем кэш истории этого gid: group_history_json читает кэш,
                    // а не live-messages. Без этого полученное сообщение пропадает при
                    // переоткрытии чата (отображается только live-событием).
                    push_event(engine, dto::UiEvent::GroupMsg {
                        gid: hex::encode(gid),
                        mine: false,
                        text: Some(out.text.clone().unwrap_or_default()),
                        sender: Some(out.sender.clone().unwrap_or_default()),
                        system: Some(out.system),
                        media,
                    });
                }
            }
            if out.retracted {
                // Строка исчезла из истории — UI обязан перечитать её целиком, иначе
                // отозванное останется на экране до перезахода в чат.
                if let Some(gid) = &out.gid {
                    push_event(engine, dto::UiEvent::GroupRetracted { gid: hex::encode(gid) });
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
                    push_event(engine, dto::UiEvent::GroupRemoved { gid: hex::encode(gid) });
                }
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
        }
        Event::GroupKeyRequest { peer } => groups.on_key_request(peer),
        Event::GroupInvite { peer, gid, name } => {
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
        Event::BlobManifest { peer, manifest } => {
            if let Some(done) = blobs.on_manifest(peer, &manifest) {
                save_and_notify_blob(engine, groups, peer, done);
            }
        }
        Event::BlobManifestPart { peer, id, mime, size, part, total, hashes } => {
            if let Some(done) =
                blobs.on_manifest_part(peer, &id, &mime, size, part, total, &hashes)
            {
                save_and_notify_blob(engine, groups, peer, done);
            }
        }
        Event::BlobRequest { peer, id, index } => blobs.on_request(&id, index, peer),
        Event::BlobChunk { peer, id, index, data } => {
            if let Some(done) = blobs.on_chunk(&id, index, &data) {
                save_and_notify_blob(engine, groups, peer, done);
            }
        }
        // M2 профиль: контакт прислал самоназвание + хэш аватара.
        Event::ProfileUpdate { peer, name, avatar_hash } => {
            // Только от контактов (профиль не принимаем от посторонних). ВАЖНО: гард лока
            // в условии `if` живёт до конца тела → повторный лок внутри = дедлок; поэтому
            // читаем `contains` в переменную (гард снимается на `;`).
            let is_contact = engine.contacts.lock_safe().contains(&peer);
            if is_contact {
                let avatar_id = if avatar_hash.is_empty() { String::new() } else { hex::encode(&avatar_hash) };
                let _ = engine.contacts.lock_safe().set_profile(&peer, &name, &avatar_id);
                // Аватар тянем по хэшу ТОЛЬКО если новый и блоба ещё нет (контент-адрес → дедуп).
                if !avatar_id.is_empty() && engine.blob_read(&avatar_id).is_none() {
                    let _ = engine.messenger.request_blob_manifest(&peer, &avatar_hash);
                }
                push_event(engine, dto::UiEvent::ContactsChanged);
            }
        }
        // ADR-007: контакт сменил адрес. Применяем ТОЛЬКО через стор — там и
        // проверка подписи ключом личности, и монотонность эпохи. Приняли —
        // немедленно обновляем карту адресов, иначе следующее сообщение уйдёт на
        // покинутый ключ.
        Event::TransportRotate { peer, epoch, transport_pub, sig } => {
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
                log::info!(
                    "контакт {:02x?}… сменил адрес (эпоха {epoch})",
                    &peer[..4],
                );
                engine.refresh_transports();
                push_event(engine, dto::UiEvent::ContactsChanged);
            }
        }
        // M2 pull-по-хэшу: пир просит манифест нашего блоба (пока — аватар).
        Event::BlobManifestRequest { peer, id } => {
            // Отдаём ЛЮБОЙ сохранённый у нас блоб по хэшу (аватар / вложение чата или
            // группы). Безопасно: контент-адрес узнаёт лишь тот, кому мы уже прислали
            // ссылку (профиль/сообщение); чужой хэш не угадать. Mime для аватара точный,
            // для вложений неизвестен на этом слое → generic (клиент знает mime из ссылки).
            let id_hex = hex::encode(&id);
            if let Some(bytes) = engine.blob_read(&id_hex) {
                let meta = engine.load_profile_meta();
                let mime = if meta.avatar_id == id_hex { meta.avatar_mime } else { "application/octet-stream".into() };
                blobs.offer(peer, &bytes, &mime);
            }
        }
        Event::GroupKeyReply { peer, key_package } => {
            if groups.on_key_reply(peer, &key_package) {
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupsChanged);
            }
        }
        Event::RoomPresence { peer, gid, flags } => {
            let left = flags & PRESENCE_LEFT != 0;
            {
                let mut rs = engine.rooms.lock_safe();
                if left {
                    if let Some(m) = rs.remote.get_mut(&gid) {
                        m.remove(&peer);
                    }
                } else {
                    rs.remote.entry(gid.clone()).or_default().insert(peer, (Instant::now(), flags));
                }
            }
            // Собеседник появился в комнате — значит наш исходящий звонок
            // состоялся. Без этого «позвонил и поговорил» осталось бы в журнале
            // как «не дозвонился».
            if !left {
                if let Some((id, _, _)) = *engine.active_call.lock_safe() {
                    engine.calls.lock_safe().set_outcome(id, "answered");
                }
            }
            push_event(engine, dto::UiEvent::RoomPresence {
                gid: hex::encode(&gid),
                peer: hex::encode(peer),
                flags,
                left,
            });
            engine.sync_room_members();
        }
    }
}

/// Пересчитать кэш групповых read-данных (зовётся после каждой мутации).
fn refresh_group_cache(engine: &Arc<Engine>, groups: &GroupManager) {
    let mut cache = engine.group_cache.lock_safe();
    cache.list = groups.list_json();
    cache.invites = groups.invites_json();
    for gid in groups.group_ids() {
        cache.members.insert(gid.clone(), groups.members_json(&gid));
    }
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
        GroupCmd::SendText { gid, text, reply_to, reply_sender, fwd_from, fwd_anon } => {
            if groups.send_text_full(&gid, &text, reply_to, &reply_sender, &fwd_from, fwd_anon) {
                refresh_group_cache(engine, groups);
                // Своё сообщение — событием, а не «клиент дорисует сам»: id ему
                // присваивает этот слой, и без перечитывания истории на него нельзя
                // было бы ни ответить, ни отозвать.
                push_event(engine, dto::UiEvent::GroupRetracted { gid: hex::encode(&gid) });
            }
        }
        GroupCmd::Retract { gid, ids } => {
            if groups.retract(&gid, &ids) > 0 {
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupRetracted { gid: hex::encode(&gid) });
            }
        }
        GroupCmd::DeleteLocal { gid, refs } => {
            if groups.delete_local(&gid, &refs) > 0 {
                refresh_group_cache(engine, groups);
                push_event(engine, dto::UiEvent::GroupRetracted { gid: hex::encode(&gid) });
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
fn wipe_user_data(dir: &Path) {
    for name in &["profile.json", "contacts.json", "messages.json", "messenger.db",
                  "messenger.db-shm", "messenger.db-wal"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("groups")) {
        for e in entries.flatten() { let _ = std::fs::remove_file(e.path()); }
    }
}

impl Engine {
    /// Инициализация. `dir` — приватный каталог данных. `vault_key` — 32-байтовый
    /// ключ at-rest (None → at-rest отключено). Поднимает транспорт, хранилища,
    /// поток event_pump. Возвращает движок + публичную идентичность.
    pub fn start(dir: PathBuf, vault_key: Option<[u8; 32]>) -> Result<StartOk, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        if let Some(key) = vault_key {
            p2p_core::vault::init_with_key(key);
        }

        let identity = match Profile::load_or_create(&dir.join("profile.json")) {
            Ok(p) => p.identity,
            Err(p2p_core::store::StoreError::Vault(p2p_core::vault::VaultError::Decrypt)) => {
                // Vault key lost (factory reset / переустановка поверх старых данных).
                log::warn!("vault key mismatch: wiping user data and regenerating identity");
                wipe_user_data(&dir);
                Profile::load_or_create(&dir.join("profile.json"))
                    .map_err(|e| e.to_string())?
                    .identity
            }
            Err(e) => return Err(e.to_string()),
        };
        // ADR-007: узел поднимается на ТРАНСПОРТНОМ ключе текущей эпохи. Эпоха 0 —
        // это сам ключ личности, поэтому у тех, кто адрес не менял, ничего не
        // меняется.
        let epoch = load_transport_epoch(&dir);
        let node = Node::start(&identity.transport_seed(epoch)).map_err(|e| e.to_string())?;
        let address = node.address();
        for uri in public_peer_uris() {
            let _ = node.add_peer(uri);
        }
        Self::start_inner(dir, identity, Arc::new(node), address, p2p_messenger::Config::default())
    }

    /// Поднять движок поверх ПРОИЗВОЛЬНОГО транспорта (loopback в интеграционных
    /// тестах): идентичность задаёт вызывающий, реальный меш-узел НЕ создаётся —
    /// можно гонять два движка в одном процессе (доставка/офлайн/рестарт/дедуп).
    pub fn start_with_transport(
        dir: PathBuf,
        identity: Identity,
        transport: Arc<dyn p2p_messenger::Transport>,
    ) -> Result<StartOk, String> {
        Self::start_with_transport_cfg(dir, identity, transport, p2p_messenger::Config::default())
    }

    /// То же, но с явным [`p2p_messenger::Config`] — нужен тестам, которые проверяют
    /// поведение по таймаутам (штатный `message_ttl` = 5 минут, столько не подождать).
    pub fn start_with_transport_cfg(
        dir: PathBuf,
        identity: Identity,
        transport: Arc<dyn p2p_messenger::Transport>,
        cfg: p2p_messenger::Config,
    ) -> Result<StartOk, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let address = transport.address();
        Self::start_inner(dir, identity, transport, address, cfg)
    }

    /// Общая сборка движка поверх готового транспорта (реального или loopback).
    fn start_inner(
        dir: PathBuf,
        identity: Identity,
        transport: Arc<dyn p2p_messenger::Transport>,
        address: String,
        cfg: p2p_messenger::Config,
    ) -> Result<StartOk, String> {
        let me_pub = identity.public_key_bytes();
        let db = p2p_core::db::open(&dir).map_err(|e| format!("db: {e}"))?;
        let contacts = ContactStore::open(db.clone(), Some(&dir.join("contacts.json")))
            .map_err(|e| format!("contacts: {e}"))?;
        let gate = GateStore::open(db.clone()).map_err(|e| format!("gate: {e}"))?;
        let messages = MessageStore::open(db.clone(), Some(&dir.join("messages.json")), &me_pub)
            .map_err(|e| format!("messages: {e}"))?;
        let contact_keys: Vec<[u8; 32]> =
            contacts.list().iter().filter_map(|c| parse_key(&c.key)).collect();

        let identity_for_groups = identity.clone();
        let identity_for_engine = identity.clone();
        let (messenger, rx) =
            Messenger::start_with_transport(identity, transport, cfg);
        let messenger = Arc::new(messenger);
        // Тиры зальём ниже, после создания Engine (единая логика — refresh_budgets).
        for k in &contact_keys {
            spawn_connect(messenger.clone(), *k);
        }

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

        // Офлайн-очередь (M3): досылка с ТЕМ ЖЕ id → получатель дедуплицирует.
        let outbox = {
            let m = messenger.clone();
            let tc = tier_cache.clone();
            let blobs_dir = dir.join("blobs");
            let resend_jobs = job_tx.clone();
            p2p_core::outbox::OutboxManager::new(
                p2p_core::outbox::OutboxStore::new(db.clone()),
                Box::new(move |peer, id, kind, payload| match kind {
                    "text" => {
                        if let Ok(text) = std::str::from_utf8(payload) {
                            let _ = m.resend_text(peer, id, text);
                        }
                    }
                    // Вложение: заново предлагаем блоб (байты лежат у нас) и повторяем
                    // ссылку. Если блоба уже нет на диске — досылать нечего, пропускаем.
                    "media" => {
                        let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) else { return };
                        let (Some(blob_hex), Some(mime), Some(name)) = (
                            v["blob"].as_str(),
                            v["mime"].as_str(),
                            v["name"].as_str(),
                        ) else { return };
                        let Ok(hash) = hex::decode(blob_hex) else { return };
                        // Блоб предлагаем через актора (BlobManager живёт в его потоке).
                        if let Some(bytes) = read_blob_at(&blobs_dir, blob_hex) {
                            let _ = resend_jobs.send(Job::BlobOffer {
                                peer: *peer,
                                content: bytes,
                                mime: mime.to_string(),
                            });
                        }
                        let meta = p2p_messenger::MediaMeta {
                            // Досылка из очереди: пересылка (если была) уже записана
                            // в истории, а очередь хранит только сам файл и подпись.
                            fwd_from: String::new(),
                            fwd_anon: false,
                            size: v["size"].as_u64().unwrap_or(0),
                            duration_ms: v["duration_ms"].as_u64().unwrap_or(0) as u32,
                            width: v["width"].as_u64().unwrap_or(0) as u32,
                            height: v["height"].as_u64().unwrap_or(0) as u32,
                            thumb: v["thumb"].as_str().and_then(|t| hex::decode(t).ok()).unwrap_or_default(),
                            caption: v["caption"].as_str().unwrap_or_default().to_string(),
                        };
                        let _ = m.resend_media_message(peer, id, &hash, mime, name, meta);
                    }
                    _ => {}
                }),
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
            wanted: Mutex::new(p2p_core::store::WantedStore::new(db.clone())),
            calls: Mutex::new(p2p_core::store::CallStore::new(db.clone())),
            active_call: Mutex::new(None),
            messages: Arc::new(Mutex::new(messages)),
            outbox: Mutex::new(outbox),
            me_pub,
            identity: identity_for_engine,
            events: Mutex::new(VecDeque::new()),
            group_cache: Mutex::new(GroupCache::default()),
            jobs: job_tx,
            room_call: Mutex::new(None),
            rooms: Mutex::new(RoomState::default()),
            bench: Mutex::new(None),
            blob_dir: dir.join("blobs"),
            tier_cache,
            data_dir: dir.clone(),
            rotate_pending: Mutex::new(std::collections::HashSet::new()),
            transport_factory: Mutex::new(None),
            offline: AtomicBool::new(false),
        });
        engine.load_rotate_pending();

        // Единый бюджет (M3): тиры контактов/блок-листа в ingress-лимитер. Делаем
        // ДО запуска приёмного цикла — иначе первые пакеты заблокированного пира
        // успели бы пройти расшифровку.
        engine.refresh_budgets();
        // ADR-007: адреса контактов, сменивших ключ. Тоже до приёмного цикла —
        // иначе первые исходящие ушли бы на покинутые адреса.
        engine.refresh_transports();

        let pump_engine = engine.clone();
        let groups_dir = dir.clone();
        std::thread::spawn(move || {
            event_pump(pump_engine, job_rx, identity_for_groups, messenger, groups_dir)
        });

        Ok(StartOk { engine, pubkey: me_pub, address })
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;

    fn k(b: u8) -> [u8; 32] { [b; 32] }

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
        assert!(rs.live_with_flags(&[7u8], Instant::now(), Duration::from_secs(12)).is_empty());
    }
}
