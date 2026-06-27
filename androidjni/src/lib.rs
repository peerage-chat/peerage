//! JNI-мост Android ↔ ядро. Грузится из Kotlin: `System.loadLibrary("p2p_android")`.
//!
//! Тот же `p2p-core`/`p2p-messenger`/`p2p-ygg`, что и десктоп — UI-независимая
//! логика переиспользуется как есть. Android поставляет лишь UI (Compose) и
//! платформенный ввод/вывод (позже — медиа через MediaCodec/Camera2).
//!
//! Модель событий — поллинг: Kotlin периодически зовёт `pollEvent()` и получает
//! JSON следующего события (или пустую строку). Просто и устойчиво через JNI;
//! состояние и история живут в Rust (как на десктопе), UI — отражение.

mod bench_jni;
mod call_jni;
mod group_jni;
mod groups;
mod room_call;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use p2p_core::store::{ContactStore, MessageStore, Profile, StoredMessage};
use p2p_core::Identity;
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

use groups::GroupManager;
use room_call::RoomCallEngine;

// Ловим Rust-паники на JNI-границе: паника сквозь FFI = UB (на Android — SIGABRT
// без стека). catch_unwind превращает её в ошибку с логом и безопасным дефолтом.
#[macro_export]
macro_rules! jni_safe {
    ($default:expr, $body:block) => {
        match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| $body)) {
            Ok(v) => v,
            Err(e) => {
                let msg = e.downcast_ref::<&str>().copied()
                    .or_else(|| e.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("unknown panic");
                ::log::error!("JNI panic: {msg}");
                $default
            }
        }
    };
}

// Отказоустойчивый захват мьютекса: если другой поток паниковал держа его,
// стандартный .unwrap() тоже паникует → cascade crash. Забираем внутреннее
// значение, игнорируя poison — данные остаются консистентными.
pub(crate) trait LockRecover<T> {
    fn lock_safe(&self) -> ::std::sync::MutexGuard<'_, T>;
}
impl<T> LockRecover<T> for Mutex<T> {
    fn lock_safe(&self) -> ::std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Команда группового актора (исполняется в потоке event_pump, т.к. GroupChat !Send).
/// Все команды — fire-and-forget: результат не ждётся на вызывающем потоке (раньше
/// `group_query` блокировал `ops` до ответа event_pump → serialization stall).
/// Read-данные (список/история/участники) UI читает из кэша [`Session::group_cache`],
/// который event_pump поддерживает актуальным после каждой мутации. Структурные
/// изменения (create/join) уведомляют UI событием `groups_changed`.
pub(crate) enum GroupCmd {
    Create { name: String },
    Invite { gid: Vec<u8>, peer: [u8; 32] },
    SendText { gid: Vec<u8>, text: String },
}

/// Работа для потока event_pump: событие мессенджера, групповая команда либо
/// немедленная рассылка маячка присутствия (при входе в комнату).
pub(crate) enum Job {
    Ev(Event),
    Cmd(GroupCmd),
    Beacon,
    /// Рассылка «я вышел» членам комнаты `gid` (мгновенное исчезновение).
    Leave(Vec<u8>),
}

/// Период рассылки маячка присутствия в комнатах (пакеты крошечные).
const BEACON_INTERVAL: Duration = Duration::from_secs(2);
/// Не видели маячок дольше — участник считается вышедшим из комнаты (страховка
/// на случай вылета/потери связи; явный выход шлёт мгновенный сигнал ниже).
/// 12с = 6 интервалов маячка: переживаем многосекундные провалы доставки на рваной
/// сети, не выкидывая собеседника (иначе флап «вы одни» + пересоздание видео-тайла
/// → серое). Явный выход (PRESENCE_LEFT) убирает участника мгновенно, не дожидаясь TTL.
pub(crate) const PRESENCE_TTL: Duration = Duration::from_secs(12);
/// Бит в `flags`: «я выхожу из комнаты» — получатель убирает участника сразу.
const PRESENCE_LEFT: u8 = 0x80;

pub(crate) const BENCH_TRACK: u8 = 200;

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

/// Активный мультипати аудио-звонок комнаты (один на процесс).
pub(crate) struct RoomCall {
    pub(crate) gid: Vec<u8>,
    pub(crate) engine: Arc<RoomCallEngine>,
    /// Цели мультикаста — синхронизируется с живыми участниками (см. sync).
    pub(crate) members: Arc<Mutex<Vec<[u8; 32]>>>,
}

const PUBLIC_PEERS: &str = include_str!("../../peers/public-peers.txt");

pub(crate) struct BenchRecv {
    pub(crate) bytes: AtomicU64,
    pub(crate) pkts: AtomicU64,
    pub(crate) running: AtomicBool,
    pub(crate) start: Instant,
}

/// Кэш групповых read-данных для UI. event_pump поддерживает его актуальным
/// после каждой мутации. JNI `groupList/History/Members` читают кэш мгновенно
/// (без блокировки event_pump — раньше `group_query` сериализовал весь `ops`).
#[derive(Default)]
struct GroupCache {
    list: String,
    history: HashMap<Vec<u8>, String>,
    members: HashMap<Vec<u8>, String>,
}

/// Состояние сессии приложения. Поля `pub(crate)` — JNI-обёртки вынесены в
/// доменные модули (`call_jni` и др.) и обращаются к ним напрямую.
pub struct Session {
    pub(crate) messenger: Arc<Messenger>,
    pub(crate) contacts: Mutex<ContactStore>,
    pub(crate) messages: Mutex<MessageStore>,
    pub(crate) me_pub: [u8; 32],
    pub(crate) events: Mutex<VecDeque<String>>,
    /// Кэш групповых read-данных (см. [`GroupCache`]).
    pub(crate) group_cache: Mutex<GroupCache>,
    /// Канал к групповому актору (потоку event_pump).
    pub(crate) jobs: Sender<Job>,
    pub(crate) room_call: Mutex<Option<RoomCall>>,
    pub(crate) rooms: Mutex<RoomState>,
    pub(crate) bench: Mutex<Option<Arc<BenchRecv>>>,
}

impl Session {
    pub(crate) fn room_engine(&self) -> Option<Arc<RoomCallEngine>> {
        self.room_call.lock_safe().as_ref().map(|r| r.engine.clone())
    }

    pub(crate) fn live_participants(&self, gid: &[u8]) -> Vec<[u8; 32]> {
        self.live_participants_with_flags(gid).into_iter().map(|(p, _)| p).collect()
    }

    pub(crate) fn live_participants_with_flags(&self, gid: &[u8]) -> Vec<([u8; 32], u8)> {
        let now = Instant::now();
        self.rooms.lock_safe()
            .remote
            .get(gid)
            .map(|m| {
                m.iter()
                    .filter(|(_, (t, _))| now.saturating_duration_since(*t) < PRESENCE_TTL)
                    .map(|(p, (_, f))| (*p, *f))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn sync_room_members(&self) {
        let guard = self.room_call.lock_safe();
        if let Some(rc) = guard.as_ref() {
            let live = self.live_participants(&rc.gid);
            *rc.members.lock_safe() = live;
        }
    }

}

pub(crate) fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub(crate) fn parse_key(s: &str) -> Option<[u8; 32]> {
    hex::decode(s.trim()).ok()?.as_slice().try_into().ok()
}

/// Фоновый коннект к пиру (как на десктопе): держим попытки, пока процесс жив.
pub(crate) fn spawn_connect(m: Arc<Messenger>, key: [u8; 32]) {
    std::thread::spawn(move || loop {
        let _ = m.connect(&key, Duration::from_secs(20));
        std::thread::sleep(Duration::from_secs(5));
    });
}

/// TOFU: добавить неизвестного пира в контакты (имя по умолчанию — префикс ключа).
fn ensure_contact(session: &Session, peer: [u8; 32]) {
    let mut store = session.contacts.lock_safe();
    if !store.contains(&peer) {
        let hex = hex::encode(peer);
        let name = format!("{}…", &hex[..8]);
        let _ = store.add(&peer, &name, now());
    }
}

fn push_event(session: &Session, json: serde_json::Value) {
    session.events.lock_safe().push_back(json.to_string());
}

/// Поток event_pump = групповой актор: владеет [`GroupManager`] (GroupChat !Send),
/// обрабатывает события мессенджера и групповые команды из JNI — всё в одном потоке.
fn event_pump(
    session: Arc<Session>,
    job_rx: Receiver<Job>,
    identity: Identity,
    messenger: Arc<Messenger>,
    data_dir: PathBuf,
) {
    let mut groups = GroupManager::new(identity, messenger.clone(), data_dir);
    refresh_group_cache(&session, &groups); // прайм кэша
    // start() в Kotlin читает groupList до прайминга (гонка) → шлём событие, чтобы
    // UI перечитал группы с диска, когда кэш готов.
    push_event(&session, serde_json::json!({"type": "groups_changed"}));
    let mut last_beacon = Instant::now();
    loop {
        match job_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Job::Ev(ev)) => handle_event(&session, &mut groups, ev),
            Ok(Job::Cmd(cmd)) => handle_cmd(&session, &mut groups, cmd),
            Ok(Job::Beacon) => {
                // Вход в комнату: сразу шлём маячок, не ждём интервала.
                last_beacon = Instant::now();
                beacon_tick(&session, &groups, &messenger);
            }
            Ok(Job::Leave(gid)) => {
                // Стабильная комната: если мы УЖЕ перезашли (joined содержит gid), этот
                // отложенный Leave относится к прошлой сессии — игнорируем. Иначе он
                // снёс бы свежий dm_peer и послал ложный LEFT по тому же room_id.
                if session.rooms.lock_safe().joined.contains_key(&gid) {
                    // перезашли — выход устарел, ничего не делаем
                } else {
                    // Явный выход: шлём LEFT-маячок. DM — только пиру, группа — всем членам.
                    let dm_peer = session.rooms.lock_safe().dm_peers.remove(&gid);
                    if let Some(peer) = dm_peer {
                        let _ = messenger.send_room_presence(&peer, &gid, PRESENCE_LEFT);
                    } else {
                        for m in groups.members(&gid) {
                            if m == session.me_pub { continue; }
                            let _ = messenger.send_room_presence(&m, &gid, PRESENCE_LEFT);
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        // Маячок присутствия ~раз в BEACON_INTERVAL (проверяем после каждой работы,
        // чтобы под нагрузкой событий он не голодал).
        if last_beacon.elapsed() >= BEACON_INTERVAL {
            last_beacon = Instant::now();
            beacon_tick(&session, &groups, &messenger);
        }
    }
}

/// Разослать маячок присутствия членам всех комнат, где мы сейчас находимся,
/// и подчистить протухшее присутствие других.
fn beacon_tick(session: &Arc<Session>, groups: &GroupManager, messenger: &Messenger) {
    let joined: Vec<(Vec<u8>, u8)> = {
        let rs = session.rooms.lock_safe();
        rs.joined.iter().map(|(g, f)| (g.clone(), *f)).collect()
    };
    for (gid, flags) in joined {
        let dm_peer = session.rooms.lock_safe().dm_peers.get(&gid).copied();
        if let Some(peer) = dm_peer {
            // DM-комната: маячок только одному пиру
            let _ = messenger.send_room_presence(&peer, &gid, flags);
        } else {
            // Групповая комната: маячок всем членам MLS-группы
            for m in groups.members(&gid) {
                if m == session.me_pub { continue; }
                let _ = messenger.send_room_presence(&m, &gid, flags);
            }
        }
    }
    {
        let now = Instant::now();
        let mut rs = session.rooms.lock_safe();
        for peers in rs.remote.values_mut() {
            peers.retain(|_, (t, _)| now.saturating_duration_since(*t) < PRESENCE_TTL);
        }
    }
    session.sync_room_members(); // цели мультикаста = живые участники
}

/// Обработать одно событие мессенджера: персист + JSON в очередь поллинга +
/// маршрутизация групповых событий в [`GroupManager`].
fn handle_event(session: &Arc<Session>, groups: &mut GroupManager, ev: Event) {
    match ev {
        Event::Presence { peer, online } => {
            if online {
                ensure_contact(session, peer);
            }
            push_event(session, serde_json::json!({
                "type": "presence", "peer": hex::encode(peer), "online": online,
            }));
        }
        Event::Text { peer, id, text } => {
            ensure_contact(session, peer);
            let t = now();
            let _ = session.messages.lock_safe().append(&peer, StoredMessage {
                id, text: text.clone(), mine: false, time: t, status: String::new(),
            });
            push_event(session, serde_json::json!({
                "type": "text", "peer": hex::encode(peer), "id": id, "text": text, "time": t,
            }));
        }
        Event::Ack { peer, id } => {
            let _ = session.messages.lock_safe().set_status(&peer, id, "delivered");
            push_event(session, serde_json::json!({"type": "ack", "peer": hex::encode(peer), "id": id}));
        }
        Event::Undelivered { peer, id } => {
            let _ = session.messages.lock_safe().set_status(&peer, id, "failed");
            push_event(session, serde_json::json!({"type": "undelivered", "peer": hex::encode(peer), "id": id}));
        }
        Event::RoomInvite { peer, room_id, audio, video } => push_event(session, serde_json::json!({
            "type": "room_invite", "peer": hex::encode(peer),
            "room_id": hex::encode(&room_id), "audio": audio, "video": video,
        })),
        Event::RoomDecline { peer, room_id } => push_event(session, serde_json::json!({
            "type": "room_decline", "peer": hex::encode(peer), "room_id": hex::encode(&room_id),
        })),
        Event::Group { data, .. } => {
            let out = groups.on_group(&data);
            if let Some(gid) = &out.gid {
                // Прикладное сообщение или системная строка (смена состава).
                if out.text.is_some() || out.system {
                    push_event(session, serde_json::json!({
                        "type": "group_msg",
                        "gid": hex::encode(gid),
                        "text": out.text.clone().unwrap_or_default(),
                        "sender": out.sender.clone().unwrap_or_default(),
                        "system": out.system,
                    }));
                }
            }
            if out.joined {
                refresh_group_cache(session, groups);
                push_event(session, serde_json::json!({"type": "groups_changed"}));
            }
        }
        Event::GroupKeyRequest { peer } => groups.on_key_request(peer),
        Event::GroupKeyReply { peer, key_package } => {
            if groups.on_key_reply(peer, &key_package) {
                refresh_group_cache(session, groups);
                push_event(session, serde_json::json!({"type": "groups_changed"}));
            }
        }
        Event::RoomPresence { peer, gid, flags } => {
            let left = flags & PRESENCE_LEFT != 0;
            {
                let mut rs = session.rooms.lock_safe();
                if left {
                    if let Some(m) = rs.remote.get_mut(&gid) {
                        m.remove(&peer);
                    }
                } else {
                    rs.remote.entry(gid.clone()).or_default().insert(peer, (Instant::now(), flags));
                }
            }
            push_event(session, serde_json::json!({
                "type": "room_presence", "gid": hex::encode(&gid),
                "peer": hex::encode(peer), "flags": flags, "left": left,
            }));
            session.sync_room_members(); // обновить цели мультикаста активного звонка
        }
    }
}

/// Пересчитать кэш групповых read-данных. Зовётся event_pump'ом после каждой
/// мутации (create/send/join/key_reply/on_group) — кэш всегда свежий.
fn refresh_group_cache(session: &Arc<Session>, groups: &GroupManager) {
    let mut cache = session.group_cache.lock_safe();
    cache.list = groups.list_json();
    for gid in groups.group_ids() {
        cache.history.insert(gid.clone(), groups.history_json(&gid));
        cache.members.insert(gid.clone(), groups.members_json(&gid));
    }
}

/// Исполнить групповую команду (в потоке event_pump). Fire-and-forget:
/// результат приходит в UI через `groups_changed` + обновлённый кэш.
fn handle_cmd(session: &Arc<Session>, groups: &mut GroupManager, cmd: GroupCmd) {
    match cmd {
        GroupCmd::Create { name } => {
            if groups.create(&name).is_some() {
                refresh_group_cache(session, groups);
                push_event(session, serde_json::json!({"type": "groups_changed"}));
            }
        }
        GroupCmd::Invite { gid, peer } => groups.invite(&gid, peer),
        GroupCmd::SendText { gid, text } => {
            if groups.send_text(&gid, &text) {
                refresh_group_cache(session, groups);
            }
        }
    }
}

// ── JNI-обвязка ────────────────────────────────────────────────────────────
// Хелперы строк: читаем JString в Rust String, отдаём Rust String как jstring.

/// Удалить зашифрованные пользовательские данные при потере vault-ключа.
/// vault.key.enc оставляем: ключ в Android Keystore жив, его можно использовать снова.
fn wipe_user_data(dir: &std::path::Path) {
    for name in &["profile.json", "contacts.json", "messages.json", "messenger.db",
                  "messenger.db-shm", "messenger.db-wal"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("groups")) {
        for e in entries.flatten() { let _ = std::fs::remove_file(e.path()); }
    }
}

pub(crate) fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(|v| v.into()).unwrap_or_default()
}
pub(crate) fn ret(env: &JNIEnv, s: &str) -> jstring {
    env.new_string(s).map(|v| v.into_raw()).unwrap_or(std::ptr::null_mut())
}

fn identity_json(session_ptr: jlong, me_pub: &[u8; 32], address: &str) -> serde_json::Value {
    serde_json::json!({
        "session_ptr": session_ptr,
        "pubkey": hex::encode(me_pub),
        "address": address,
        "fingerprint": p2p_core::identity::fingerprint_words(me_pub).join(" "),
    })
}

/// Инициализация. dataDir — приватный каталог приложения (filesDir).
/// Возвращает JSON `{session_ptr, pubkey, address, fingerprint}` или `{"error": "..."}`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_start<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
    vault_key: JByteArray<'local>,
) -> jstring {
    let _ = android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("p2p"),
    );

    let dir = PathBuf::from(jstr(&mut env, &data_dir));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return ret(&env, &serde_json::json!({"error": e.to_string()}).to_string());
    }

    // Ключ получен из Android Keystore через VaultKeyManager (Kotlin).
    // Keystore держит AES-256-GCM ключ в hardware-backed хранилище; Kotlin
    // разворачивает им 32-байтовый vault key и передаёт сюда уже plaintext.
    match env.convert_byte_array(&vault_key) {
        Ok(raw) if raw.len() == 32 => {
            let key: [u8; 32] = std::array::from_fn(|i| raw[i] as u8);
            p2p_core::vault::init_with_key(key);
        }
        Ok(raw) => log::warn!("vault key: неверный размер {} (ожидается 32); at-rest отключено", raw.len()),
        Err(e) => log::warn!("vault key: ошибка чтения JByteArray ({e}); at-rest отключено"),
    }

    let identity = match Profile::load_or_create(&dir.join("profile.json")) {
        Ok(p) => p.identity,
        Err(p2p_core::store::StoreError::Vault(p2p_core::vault::VaultError::Decrypt)) => {
            // Vault key lost (factory reset, new install over old data).
            // Wipe user data and start fresh — unrecoverable without the key.
            log::warn!("vault key mismatch: wiping user data and regenerating identity");
            wipe_user_data(&dir);
            match Profile::load_or_create(&dir.join("profile.json")) {
                Ok(p) => p.identity,
                Err(e) => return ret(&env, &serde_json::json!({"error": e.to_string()}).to_string()),
            }
        }
        Err(e) => return ret(&env, &serde_json::json!({"error": e.to_string()}).to_string()),
    };
    let me_pub = identity.public_key_bytes();

    let node = match Node::start(&identity.seed()) {
        Ok(n) => n,
        Err(e) => return ret(&env, &serde_json::json!({"error": e.to_string()}).to_string()),
    };
    let address = node.address();
    for uri in PUBLIC_PEERS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let _ = node.add_peer(uri);
    }

    let db = match p2p_core::db::open(&dir) {
        Ok(db) => db,
        Err(e) => return ret(&env, &serde_json::json!({"error": format!("db: {e}")}).to_string()),
    };
    let contacts = ContactStore::open(db.clone(), Some(&dir.join("contacts.json")))
        .expect("contacts store");
    let messages = MessageStore::open(db.clone(), Some(&dir.join("messages.json")))
        .expect("messages store");
    let contact_keys: Vec<[u8; 32]> =
        contacts.list().iter().filter_map(|c| parse_key(&c.key)).collect();

    let identity_for_groups = identity.clone(); // для GroupManager (нужна та же identity)
    let (messenger, rx) = Messenger::start(identity, node);
    let messenger = Arc::new(messenger);
    for k in &contact_keys {
        spawn_connect(messenger.clone(), *k);
    }

    // Актор-канал: события мессенджера и групповые команды JNI идут в один поток.
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

    let session = Arc::new(Session {
        messenger: messenger.clone(),
        contacts: Mutex::new(contacts),
        messages: Mutex::new(messages),
        me_pub,
        events: Mutex::new(VecDeque::new()),
        group_cache: Mutex::new(GroupCache::default()),
        jobs: job_tx,
        room_call: Mutex::new(None),
        rooms: Mutex::new(RoomState::default()),
        bench: Mutex::new(None),
    });
    
    let session_ptr = Box::into_raw(Box::new(session.clone())) as jlong;

    let groups_dir = dir.clone();
    std::thread::spawn(move || {
        event_pump(session, job_rx, identity_for_groups, messenger, groups_dir)
    });

    ret(&env, &identity_json(session_ptr, &me_pub, &address).to_string())
}

/// Добавить деструктор сессии.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_destroySession<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    if session_ptr != 0 {
        let _boxed = unsafe { Box::from_raw(session_ptr as *mut Arc<Session>) };
        // Выгрузит Arc и освободит память.
    }
}

/// Следующее событие как JSON-строка, либо "" если очередь пуста.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_pollEvent<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        if session_ptr == 0 { return ret(&env, ""); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let s = session.events.lock_safe().pop_front().unwrap_or_default();
        ret(&env, &s)
    })
}

/// Отправить текст. Возвращает id сообщения (>=0) либо -1 при ошибке.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_sendText<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    text: JString<'local>,
) -> jlong {
    jni_safe!(-1, {
        if session_ptr == 0 { return -1; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return -1 };
        let text = jstr(&mut env, &text);
        match session.messenger.send_text(&key, &text) {
            Ok(id) => {
                let _ = session.messages.lock_safe().append(&key, StoredMessage {
                    id, text: text.clone(), mine: true, time: now(), status: "sent".into(),
                });
                id as jlong
            }
            Err(_) => -1,
        }
    })
}

/// Инициировать/держать соединение с пиром (фоновый коннект).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_connect<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        if let Some(key) = parse_key(&jstr(&mut env, &key_hex)) {
            spawn_connect(session.messenger.clone(), key);
        }
    });
}

/// Добавить контакт (TOFU). Возвращает "added" | "known" | "error".
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_addContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    name: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "error"), {
        if session_ptr == 0 { return ret(&env, "error"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "error") };
        let name = jstr(&mut env, &name);
        let name = if name.trim().is_empty() { format!("{}…", &hex::encode(key)[..8]) } else { name };
        match session.contacts.lock_safe().add(&key, &name, now()) {
            Ok(p2p_core::store::AddOutcome::Added) => { spawn_connect(session.messenger.clone(), key); ret(&env, "added") }
            Ok(p2p_core::store::AddOutcome::AlreadyKnown) => ret(&env, "known"),
            Err(_) => ret(&env, "error"),
        }
    })
}

/// Список контактов как JSON-массив.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_listContacts<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        if session_ptr == 0 { return ret(&env, "[]"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let json = serde_json::to_string(&session.contacts.lock_safe().list()).unwrap_or_default();
        ret(&env, &json)
    })
}

/// История переписки с пиром как JSON-массив.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_history<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        if session_ptr == 0 { return ret(&env, "[]"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "[]") };
        let json = serde_json::to_string(&session.messages.lock_safe().history(&key)).unwrap_or_default();
        ret(&env, &json)
    })
}

/// Пометить контакт проверенным/непроверенным.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_setVerified<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    verified: jboolean,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        if let Some(key) = parse_key(&jstr(&mut env, &key_hex)) {
            let _ = session.contacts.lock_safe().set_verified(&key, verified != JNI_FALSE);
        }
    });
}

/// Переименовать контакт.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_renameContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    name: JString<'local>,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        if let Some(key) = parse_key(&jstr(&mut env, &key_hex)) {
            let name = jstr(&mut env, &name);
            let _ = session.contacts.lock_safe().rename(&key, &name);
        }
    });
}

/// Удалить контакт.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_removeContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        if let Some(key) = parse_key(&jstr(&mut env, &key_hex)) {
            let _ = session.contacts.lock_safe().remove(&key);
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_isOnline<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        if session_ptr == 0 { return JNI_FALSE; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let online = parse_key(&jstr(&mut env, &key_hex))
            .map(|k| session.messenger.is_online(&k))
            .unwrap_or(false);
        if online { JNI_TRUE } else { JNI_FALSE }
    })
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_netPeers<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jint {
    jni_safe!(0, {
        if session_ptr == 0 { return 0; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        session.messenger.net_peers() as jint
    })
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_netTree<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jint {
    jni_safe!(0, {
        if session_ptr == 0 { return 0; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        session.messenger.net_tree() as jint
    })
}

// ── Контракт движка (зерно ADR-003): типизированный снапшот стата сети ──────
// Домейн-логику (агрегаты) считает движок, не клиент — Kotlin лишь рендерит.
#[derive(serde::Serialize, serde::Deserialize)]
struct PeerStat {
    key: String,
    inbound: bool,
    up: bool,
    latency_ms: f64,
    rx_rate: u64,
    tx_rate: u64,
    uptime_s: f64,
}

#[derive(serde::Serialize)]
struct NetStats {
    peer_count: usize,
    tree: i32,
    ping_ms: f64, // лучший (минимальный) пинг среди up-пиров; 0 если неизвестен
    rx_rate: u64, // суммарный входящий байт/с
    tx_rate: u64, // суммарный исходящий байт/с
    peers: Vec<PeerStat>,
}

/// Снапшот стата сети: пиры + агрегаты (число/пинг/трафик). JSON NetStats.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_netStats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        if session_ptr == 0 { return ret(&env, "{}"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let peers: Vec<PeerStat> =
            serde_json::from_str(&session.messenger.net_peers_json()).unwrap_or_default();
        let best_ping = peers
            .iter()
            .filter(|p| p.up && p.latency_ms > 0.0)
            .map(|p| p.latency_ms)
            .fold(f64::INFINITY, f64::min);
        let stats = NetStats {
            peer_count: peers.len(),
            tree: session.messenger.net_tree(),
            ping_ms: if best_ping.is_finite() { best_ping } else { 0.0 },
            rx_rate: peers.iter().map(|p| p.rx_rate).sum(),
            tx_rate: peers.iter().map(|p| p.tx_rate).sum(),
            peers,
        };
        ret(&env, &serde_json::to_string(&stats).unwrap_or_else(|_| "{}".into()))
    })
}

/// Принудительно переподключить всех публичных Yggdrasil-пиров:
/// обходит внутренний exponential backoff и форсирует немедленную попытку.
/// Вызывается при смене сети или обнаружении peers=0 из Kotlin-тикера.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_reconnectPeers<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        for uri in PUBLIC_PEERS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            session.messenger.add_peer(uri);
        }
        log::info!("reconnectPeers: re-added {} public peers",
            PUBLIC_PEERS.lines().filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#')).count());
    });
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_stop<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        session.messenger.shutdown();
    });
}

// JNI-обёртки по доменам вынесены в модули: звонки/комнаты → call_jni,
// группы → group_jni, диагностика пропускной способности → bench_jni.


