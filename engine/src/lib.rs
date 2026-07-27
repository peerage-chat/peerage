//! Движок мессенджера — device-independent ядро (ADR-003). Владеет состоянием
//! сессии (контакты, сообщения, группы, комнаты-звонки), исполняет команды и
//! отдаёт события. НЕ зависит от JNI/Android — `androidjni` лишь маршалит вызовы.
//! Десктоп может использовать тот же крейт.
//!
//! Контракт: клиент шлёт команды (методы `Engine`) и читает снапшоты; движок
//! асинхронно копит события (`poll_event`). Домейн-логику (агрегаты, презенс,
//! маршрутизацию) считает движок, не клиент. Поэтому ядро юнит-тестируется без
//! устройства (см. `net.rs`, тесты презенса ниже).

pub mod blobs;
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
use p2p_core::store::{ContactStore, GateStore, MessageStore, Profile, StoredMessage};
use p2p_core::Identity;
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
    SendText { gid: Vec<u8>, text: String },
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
    /// Отправить медиа-вложение в группу (M2): scrub → сохранить → MLS-ссылка всем +
    /// push блоба каждому участнику. В одном Job (нужны и группы, и BlobManager).
    GroupMedia { gid: Vec<u8>, content: Vec<u8>, mime: String, name: String },
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
    history: HashMap<Vec<u8>, String>,
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
    messages: Mutex<MessageStore>,
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
    /// Единый бюджет (M3): тир каждого известного пира. Читают ingress-лимитер
    /// (через messenger) и офлайн-очередь (через замыкание) — одна точка истины.
    tier_cache: Arc<Mutex<HashMap<[u8; 32], Tier>>>,
}

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

    /// Секретная сид-фраза (24 слова BIP39) — по запросу пользователя (онбординг M4
    /// «Показать фразу»). СЕКРЕТ: показывать только на FLAG_SECURE-экране.
    pub fn seed_phrase(&self) -> String {
        self.identity.seed_phrase()
    }

    // — Сообщения 1:1 —
    /// Отправить текст. Возвращает id (≥0) или None при ошибке.
    pub fn send_text(&self, key: &[u8; 32], text: &str) -> Option<i64> {
        match self.messenger.send_text(key, text) {
            Ok(id) => {
                let t = now();
                let _ = self.messages.lock_safe().append(key, StoredMessage {
                    id, text: text.to_string(), mine: true, time: t, status: "sent".into(),
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
        // Картинки чистим от метаданных (EXIF/GPS) перед отправкой, как аватар.
        let bytes = if mime.starts_with("image/") {
            p2p_blob::scrub::scrub(mime, &content)
        } else {
            content
        };
        let hash = p2p_blob::blob_id(&bytes);
        let kind = Self::media_kind(mime);
        // Блоб получателю: push-анонс + отдача чанков (если промахнётся — pull по хэшу).
        self.blob_offer(*key, bytes.clone(), mime.to_string());
        // Сохраняем локально, чтобы показывать своё вложение сразу и после рестарта.
        self.store_blob(hash, &bytes);
        let id_hex = hex::encode(hash);
        match self.messenger.send_media_message(key, &hash, mime, name) {
            Ok(id) => {
                let t = now();
                let _ = self.messages.lock_safe().append(key, StoredMessage {
                    id, mine: true, time: t, status: "sent".into(),
                    kind: kind.into(), blob_id: id_hex, mime: mime.into(), name: name.into(),
                    ..Default::default()
                });
                Some(id as i64)
            }
            Err(_) => None,
        }
    }

    pub fn connect(&self, key: [u8; 32]) {
        spawn_connect(self.messenger.clone(), key);
    }

    // — Контакты (TOFU) —
    /// Добавить контакт. Возвращает "added" | "known" | "error".
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
    fn refresh_budgets(&self) {
        // ВАЖНО: не держим оба лока разом (contacts+gate) — иначе гонка порядка
        // захвата с API гейта = дедлок. Снимаем контакты, отпускаем, затем блоки.
        let mut tiers: Vec<([u8; 32], Tier)> = self
            .contacts
            .lock_safe()
            .list()
            .iter()
            .filter_map(|c| {
                parse_key(&c.key).map(|k| (k, if c.verified { Tier::Verified } else { Tier::Contact }))
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
        tiers.retain(|(k, _)| !blocked.contains(k));
        tiers.extend(blocked.into_iter().map(|k| (k, Tier::Blocked)));
        self.messenger.set_peer_tiers(&tiers);
        // Тот же бюджет — офлайн-очереди (cap/TTL хранения).
        *self.tier_cache.lock_safe() = tiers.into_iter().collect();
    }

    pub fn contacts_json(&self) -> String {
        serde_json::to_string(&self.contacts.lock_safe().list()).unwrap_or_else(|_| "[]".into())
    }

    pub fn history_json(&self, key: &[u8; 32]) -> String {
        serde_json::to_string(&self.messages.lock_safe().history(key)).unwrap_or_else(|_| "[]".into())
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
        let _ = self.jobs.send(Job::Cmd(GroupCmd::SendText { gid, text }));
    }

    /// Отправить медиа-вложение в группу (M2). Fire-and-forget: движок чистит
    /// картинку, сохраняет блоб, шлёт MLS-ссылку всем и пушит байты каждому.
    pub fn group_send_media(&self, gid: Vec<u8>, content: Vec<u8>, mime: String, name: String) {
        let _ = self.jobs.send(Job::GroupMedia { gid, content, mime, name });
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
        // Санитизация: только валидный hex → чистое имя файла (без traversal).
        let clean = hex::encode(hex::decode(id_hex.trim()).ok()?);
        let raw = std::fs::read(self.blob_dir.join(clean)).ok()?;
        p2p_core::vault::open(&raw).ok()
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

    pub fn group_list_json(&self) -> String {
        let s = self.group_cache.lock_safe().list.clone();
        if s.is_empty() { "[]".into() } else { s }
    }

    pub fn group_history_json(&self, gid: &[u8]) -> String {
        self.group_cache.lock_safe().history.get(gid).cloned().unwrap_or_else(|| "[]".into())
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
    }

    /// Принять DM-звонок: войти в комнату с регистрацией dm_peer. Возвращает room_id.
    pub fn room_join_dm(&self, peer: [u8; 32], audio: bool, video: bool) -> Vec<u8> {
        let room_id = p2p_core::dm_room_id(&self.me_pub, &peer);
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
        {
            let mut rs = self.rooms.lock_safe();
            rs.joined.remove(&gid);
            rs.remote.remove(&gid);
        }
        let _ = self.jobs.send(Job::Leave(gid));
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
                serde_json::json!({
                    "pkts": pkts, "bytes": bytes, "secs": secs,
                    "mbps": bytes as f64 * 8.0 / 1e6 / secs,
                }).to_string()
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
        serde_json::json!({
            "sent_pkts": sent_pkts, "sent_bytes": sent_bytes, "secs": secs,
            "mbps": sent_bytes as f64 * 8.0 / 1e6 / secs,
        }).to_string()
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
    let arr: Vec<serde_json::Value> = live
        .into_iter()
        .map(|(p, f)| serde_json::json!({
            "peer": hex::encode(p), "audio": f & 0b01 != 0, "video": f & 0b10 != 0,
        }))
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
}

fn call_params(audio: &AudioSettings) -> String {
    serde_json::json!({
        "call_id": 0,
        "sample_rate": audio.sample_rate,
        "channels": audio.channels,
        "frame_len": audio.interleaved_len(),
    })
    .to_string()
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
    push_event(engine, serde_json::json!({
        "type": "contact_request", "peer": hex::encode(peer), "new": is_new,
    }));
}

fn push_event(engine: &Engine, json: serde_json::Value) {
    engine.events.lock_safe().push_back(json.to_string());
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
    let mut groups = GroupManager::new(identity, messenger.clone(), data_dir);
    let mut blobs = blobs::BlobManager::new(messenger.clone(), p2p_blob::Limits::default());
    {
        let ob = engine.outbox.lock_safe();
        log::info!("outbox on_start: pending={} (до prune+досылки)", ob.pending_count());
    }
    engine.outbox.lock_safe().on_start(now()); // M3: prune TTL + дослать недоставленное
    refresh_group_cache(&engine, &groups); // прайм кэша
    push_event(&engine, serde_json::json!({"type": "groups_changed"}));
    let mut last_beacon = Instant::now();
    loop {
        match job_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Job::Ev(ev)) => handle_event(&engine, &mut groups, &mut blobs, ev),
            Ok(Job::Cmd(cmd)) => handle_cmd(&engine, &mut groups, cmd),
            Ok(Job::BlobOffer { peer, content, mime }) => {
                // Сохраняем свой блоб локально (чтобы отправитель тоже его показывал)
                // и анонсируем манифест — докачку ведёт получатель.
                save_blob(&engine, p2p_blob::blob_id(&content), &content);
                blobs.offer(peer, &content, &mime);
            }
            Ok(Job::GroupMedia { gid, content, mime, name }) => {
                // Картинки чистим (scrub), считаем контент-адрес, сохраняем локально.
                let bytes = if mime.starts_with("image/") {
                    p2p_blob::scrub::scrub(&mime, &content)
                } else {
                    content
                };
                let hash = p2p_blob::blob_id(&bytes);
                let id_hex = hex::encode(hash);
                let kind = Engine::media_kind(&mime);
                save_blob(&engine, hash, &bytes);
                // MLS-сообщение со ссылкой всем + push байтов каждому участнику.
                if groups.send_media(&gid, kind, &id_hex, &mime, &name) {
                    for m in groups.members(&gid) {
                        if m != engine.me_pub {
                            blobs.offer(m, &bytes, &mime);
                        }
                    }
                    engine.group_cache.lock_safe().history.insert(gid.clone(), groups.history_json(&gid));
                    push_event(&engine, serde_json::json!({"type": "group_msg", "gid": hex::encode(&gid), "mine": true}));
                }
            }
            Ok(Job::Beacon) => {
                last_beacon = Instant::now();
                beacon_tick(&engine, &groups, &messenger);
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
            beacon_tick(&engine, &groups, &messenger);
        }
        blobs.tick(); // переспросить потерянные чанки докачки (M2)
    }
}

/// Сохранить блоб на диск по hex-id (принятый или свой отправленный).
/// Шифрует at-rest (`vault::seal`) — как сообщения/группы.
fn save_blob(engine: &Arc<Engine>, id: p2p_blob::Hash, content: &[u8]) {
    let _ = std::fs::create_dir_all(&engine.blob_dir);
    let _ = std::fs::write(engine.blob_dir.join(hex::encode(id)), p2p_core::vault::seal(content));
}

/// Сохранить принятый блоб и уведомить UI (`blob_received`).
fn save_and_notify_blob(engine: &Arc<Engine>, done: blobs::Completed) {
    save_blob(engine, done.id, &done.content);
    push_event(engine, serde_json::json!({
        "type": "blob_received",
        "id": hex::encode(done.id),
        "mime": done.manifest.mime,
        "size": done.manifest.size,
    }));
}

/// Разослать маячок присутствия членам всех наших комнат и подчистить протухших.
fn beacon_tick(engine: &Arc<Engine>, groups: &GroupManager, messenger: &Messenger) {
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
                    let mut ob = engine.outbox.lock_safe();
                    let c = ob.pending_count();
                    ob.on_online(&peer); // M3: дослать ожидающее пиру
                    if c > 0 {
                        log::info!("outbox on_online peer={:02x?}: досылка (в очереди {})", &peer[..4], c);
                    }
                }
            }
            push_event(engine, serde_json::json!({
                "type": "presence", "peer": hex::encode(peer), "online": online,
            }));
        }
        Event::Text { peer, id, text } => {
            match gate_of(engine, &peer) {
                Gate::Blocked => return,
                Gate::Stranger => {
                    record_request(engine, peer, &text);
                    return;
                }
                Gate::Contact => {}
            }
            // Дедуп повторной доставки: офлайн-досылка со стабильным id приходит дважды.
            let dup = engine.messages.lock_safe().contains(&peer, id, false);
            if !dup {
                let t = now();
                let _ = engine.messages.lock_safe().append(&peer, StoredMessage {
                    id, text: text.clone(), mine: false, time: t, status: String::new(),
                    ..Default::default()
                });
                push_event(engine, serde_json::json!({
                    "type": "text", "peer": hex::encode(peer), "id": id, "text": text, "time": t,
                }));
            }
        }
        Event::Media { peer, id, hash, mime, name } => {
            match gate_of(engine, &peer) {
                Gate::Blocked => return,
                Gate::Stranger => {
                    record_request(engine, peer, &format!("[{}]", Engine::media_kind(&mime)));
                    return;
                }
                Gate::Contact => {}
            }
            let dup = engine.messages.lock_safe().contains(&peer, id, false);
            if !dup {
                let t = now();
                let id_hex = hex::encode(&hash);
                let kind = Engine::media_kind(&mime);
                let _ = engine.messages.lock_safe().append(&peer, StoredMessage {
                    id, mine: false, time: t, status: String::new(),
                    kind: kind.into(), blob_id: id_hex.clone(), mime: mime.clone(), name: name.clone(),
                    ..Default::default()
                });
                // Байты вложения: отправитель их push-нул, но если чанк промахнулся —
                // тянем по хэшу (контент-адрес → авто-дедуп; blob_received перерисует).
                if engine.blob_read(&id_hex).is_none() {
                    let _ = engine.messenger.request_blob_manifest(&peer, &hash);
                }
                push_event(engine, serde_json::json!({
                    "type": "media", "peer": hex::encode(peer), "id": id, "time": t,
                    "kind": kind, "blob_id": id_hex, "mime": mime, "name": name,
                }));
            }
        }
        Event::Ack { peer, id } => {
            let _ = engine.messages.lock_safe().set_status(&peer, id, "delivered");
            engine.outbox.lock_safe().on_ack(&peer, id); // M3: снять из очереди
            push_event(engine, serde_json::json!({"type": "ack", "peer": hex::encode(peer), "id": id}));
        }
        Event::Undelivered { peer, id } => {
            let _ = engine.messages.lock_safe().set_status(&peer, id, "failed");
            push_event(engine, serde_json::json!({"type": "undelivered", "peer": hex::encode(peer), "id": id}));
        }
        Event::RoomInvite { peer, room_id, audio, video } => push_event(engine, serde_json::json!({
            "type": "room_invite", "peer": hex::encode(peer),
            "room_id": hex::encode(&room_id), "audio": audio, "video": video,
        })),
        Event::RoomDecline { peer, room_id } => push_event(engine, serde_json::json!({
            "type": "room_decline", "peer": hex::encode(peer), "room_id": hex::encode(&room_id),
        })),
        Event::Group { peer, data } => {
            let out = groups.on_group(&data);
            if let Some(gid) = &out.gid {
                if out.text.is_some() || out.system || out.media.is_some() {
                    // Обновляем кэш истории этого gid: group_history_json читает кэш,
                    // а не live-messages. Без этого полученное сообщение пропадает при
                    // переоткрытии чата (отображается только live-событием).
                    engine.group_cache.lock_safe().history.insert(gid.clone(), groups.history_json(gid));
                    let mut ev = serde_json::json!({
                        "type": "group_msg",
                        "gid": hex::encode(gid),
                        "text": out.text.clone().unwrap_or_default(),
                        "sender": out.sender.clone().unwrap_or_default(),
                        "system": out.system,
                    });
                    // M2: медиа-поля + докачка блоба по хэшу от отправителя, если промах.
                    if let Some(m) = &out.media {
                        ev["kind"] = m.kind.clone().into();
                        ev["blob_id"] = m.blob_id.clone().into();
                        ev["mime"] = m.mime.clone().into();
                        ev["name"] = m.name.clone().into();
                        if engine.blob_read(&m.blob_id).is_none() {
                            if let Ok(h) = hex::decode(&m.blob_id) {
                                let _ = engine.messenger.request_blob_manifest(&peer, &h);
                            }
                        }
                    }
                    push_event(engine, ev);
                }
            }
            if out.joined {
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
            if out.renamed {
                // Имя группы пришло от админа — обновляем кэш списка и уведомляем UI.
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
            if out.removed {
                // ADR-004: нас отозвали — обновляем кэш и уведомляем UI отдельным
                // событием (чтобы показать «вы удалены» и закрыть экран группы).
                refresh_group_cache(engine, groups);
                if let Some(gid) = &out.gid {
                    push_event(engine, serde_json::json!({
                        "type": "group_removed", "gid": hex::encode(gid),
                    }));
                }
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
        }
        Event::GroupKeyRequest { peer } => groups.on_key_request(peer),
        Event::GroupInvite { peer, gid, name } => {
            // Приглашение-карточка: не вступаем молча — кладём в очередь, уведомляем UI.
            if groups.on_invite(peer, gid, name) {
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "group_invites_changed"}));
            }
        }
        Event::GroupResyncRequest { peer, gid } => groups.on_resync_request(peer, &gid),
        Event::GroupLeaveRequest { peer, gid } => {
            if groups.on_leave_request(peer, &gid) {
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
        }
        // M2 блобы: докачка получателем + отдача чанков. Событие blob_received →
        // UI (сохранён на диск, доступен через blob_read).
        Event::BlobManifest { peer, manifest } => {
            if let Some(done) = blobs.on_manifest(peer, &manifest) {
                save_and_notify_blob(engine, done);
            }
        }
        Event::BlobRequest { peer, id, index } => blobs.on_request(&id, index, peer),
        Event::BlobChunk { id, index, data, .. } => {
            if let Some(done) = blobs.on_chunk(&id, index, &data) {
                save_and_notify_blob(engine, done);
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
                push_event(engine, serde_json::json!({"type": "contacts_changed"}));
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
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
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
            push_event(engine, serde_json::json!({
                "type": "room_presence", "gid": hex::encode(&gid),
                "peer": hex::encode(peer), "flags": flags, "left": left,
            }));
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
        cache.history.insert(gid.clone(), groups.history_json(&gid));
        cache.members.insert(gid.clone(), groups.members_json(&gid));
    }
}

/// Исполнить групповую команду (в потоке event_pump). Fire-and-forget.
fn handle_cmd(engine: &Arc<Engine>, groups: &mut GroupManager, cmd: GroupCmd) {
    match cmd {
        GroupCmd::Create { name } => {
            if groups.create(&name).is_some() {
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
        }
        GroupCmd::Invite { gid, peer } => groups.invite(&gid, peer),
        GroupCmd::AcceptInvite { gid } => {
            // KeyPackage ушёл пригласившему; вступление завершится по Welcome (joined).
            // Карточку убираем из кэша сразу.
            groups.accept_invite(&gid);
            refresh_group_cache(engine, groups);
            push_event(engine, serde_json::json!({"type": "group_invites_changed"}));
        }
        GroupCmd::DeclineInvite { gid } => {
            if groups.decline_invite(&gid) {
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "group_invites_changed"}));
            }
        }
        GroupCmd::SendText { gid, text } => {
            if groups.send_text(&gid, &text) {
                refresh_group_cache(engine, groups);
            }
        }
        GroupCmd::RemoveMember { gid, peer } => {
            if groups.remove_member(&gid, peer) {
                refresh_group_cache(engine, groups);
                engine.sync_room_members(); // SEC-2: отозванного убрать и из звонка
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
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
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
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
        let node = Node::start(&identity.seed()).map_err(|e| e.to_string())?;
        let address = node.address();
        for uri in public_peer_uris() {
            let _ = node.add_peer(uri);
        }
        Self::start_inner(dir, identity, Arc::new(node), address)
    }

    /// Поднять движок поверх ПРОИЗВОЛЬНОГО транспорта (loopback в интеграционных
    /// тестах): идентичность задаёт вызывающий, реальный меш-узел НЕ создаётся —
    /// можно гонять два движка в одном процессе (доставка/офлайн/рестарт/дедуп).
    pub fn start_with_transport(
        dir: PathBuf,
        identity: Identity,
        transport: Arc<dyn p2p_messenger::Transport>,
    ) -> Result<StartOk, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let address = transport.address();
        Self::start_inner(dir, identity, transport, address)
    }

    /// Общая сборка движка поверх готового транспорта (реального или loopback).
    fn start_inner(
        dir: PathBuf,
        identity: Identity,
        transport: Arc<dyn p2p_messenger::Transport>,
        address: String,
    ) -> Result<StartOk, String> {
        let me_pub = identity.public_key_bytes();
        let db = p2p_core::db::open(&dir).map_err(|e| format!("db: {e}"))?;
        let contacts = ContactStore::open(db.clone(), Some(&dir.join("contacts.json")))
            .map_err(|e| format!("contacts: {e}"))?;
        let gate = GateStore::open(db.clone()).map_err(|e| format!("gate: {e}"))?;
        let messages = MessageStore::open(db.clone(), Some(&dir.join("messages.json")))
            .map_err(|e| format!("messages: {e}"))?;
        let contact_keys: Vec<[u8; 32]> =
            contacts.list().iter().filter_map(|c| parse_key(&c.key)).collect();

        let identity_for_groups = identity.clone();
        let identity_for_engine = identity.clone();
        let (messenger, rx) =
            Messenger::start_with_transport(identity, transport, p2p_messenger::Config::default());
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

        // Офлайн-очередь (M3): досылку делает messenger.resend_text (тот же id → дедуп).
        let outbox = {
            let m = messenger.clone();
            let tc = tier_cache.clone();
            p2p_core::outbox::OutboxManager::new(
                p2p_core::outbox::OutboxStore::new(db.clone()),
                Box::new(move |peer, id, text| {
                    let _ = m.resend_text(peer, id, text);
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
            messages: Mutex::new(messages),
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
        });

        // Единый бюджет (M3): тиры контактов/блок-листа в ingress-лимитер. Делаем
        // ДО запуска приёмного цикла — иначе первые пакеты заблокированного пира
        // успели бы пройти расшифровку.
        engine.refresh_budgets();

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
