//! JNI-мост Android ↔ ядро. Грузится из Kotlin: `System.loadLibrary("p2p_android")`.
//!
//! Тот же `p2p-core`/`p2p-messenger`/`p2p-ygg`, что и десктоп — UI-независимая
//! логика переиспользуется как есть. Android поставляет лишь UI (Compose) и
//! платформенный ввод/вывод (позже — медиа через MediaCodec/Camera2).
//!
//! Модель событий — поллинг: Kotlin периодически зовёт `pollEvent()` и получает
//! JSON следующего события (или пустую строку). Просто и устойчиво через JNI;
//! состояние и история живут в Rust (как на десктопе), UI — отражение.

mod call;
mod groups;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jni::objects::{JByteArray, JClass, JShortArray, JString};
use jni::sys::{jboolean, jbyteArray, jint, jlong, jsize, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use p2p_core::settings::AudioSettings;
use p2p_core::store::{ContactStore, MessageStore, Profile, StoredMessage};
use p2p_core::Identity;
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

use call::CallEngine;
use groups::GroupManager;

/// Команда группового актора (исполняется в потоке event_pump, т.к. GroupChat !Send).
enum GroupCmd {
    Create { name: String, resp: Sender<String> },
    Invite { gid: Vec<u8>, peer: [u8; 32] },
    SendText { gid: Vec<u8>, text: String, resp: Sender<bool> },
    List { resp: Sender<String> },
    History { gid: Vec<u8>, resp: Sender<String> },
    Members { gid: Vec<u8>, resp: Sender<String> },
}

/// Работа для потока event_pump: событие мессенджера, групповая команда либо
/// немедленная рассылка маячка присутствия (при входе в комнату).
enum Job {
    Ev(Event),
    Cmd(GroupCmd),
    Beacon,
    /// Рассылка «я вышел» членам комнаты `gid` (мгновенное исчезновение).
    Leave(Vec<u8>),
}

/// Текущий активный звонок (1:1 на процесс).
static CALL: Mutex<Option<Arc<CallEngine>>> = Mutex::new(None);
/// Счётчик id звонков (для исходящих приглашений).
static CALL_ID: AtomicU64 = AtomicU64::new(1);

/// Период рассылки маячка присутствия в комнатах (пакеты крошечные).
const BEACON_INTERVAL: Duration = Duration::from_secs(2);
/// Не видели маячок дольше — участник считается вышедшим из комнаты (страховка
/// на случай вылета/потери связи; явный выход шлёт мгновенный сигнал ниже).
const PRESENCE_TTL: Duration = Duration::from_secs(5);
/// Бит в `flags`: «я выхожу из комнаты» — получатель убирает участника сразу.
const PRESENCE_LEFT: u8 = 0x80;

/// Присутствие в комнатах-звонках (Discord-style). Глобально: дёргается из JNI
/// (join/leave/participants) и из event_pump (приём маячков + рассылка).
#[derive(Default)]
struct RoomState {
    /// Комнаты, в которых МЫ сейчас находимся: gid → наши A/V-флаги.
    joined: HashMap<Vec<u8>, u8>,
    /// Присутствие других: gid → (peer → (время последнего маячка, флаги)).
    remote: HashMap<Vec<u8>, HashMap<[u8; 32], (Instant, u8)>>,
}

fn rooms() -> &'static Mutex<RoomState> {
    static ROOMS: OnceLock<Mutex<RoomState>> = OnceLock::new();
    ROOMS.get_or_init(|| Mutex::new(RoomState::default()))
}

const PUBLIC_PEERS: &str = include_str!("../../peers/public-peers.txt");

/// Глобальное состояние приложения (один мессенджер на процесс).
struct App {
    messenger: Arc<Messenger>,
    contacts: Mutex<ContactStore>,
    messages: Mutex<MessageStore>,
    me_pub: [u8; 32],
    events: Mutex<VecDeque<String>>,
    /// Канал к групповому актору (потоку event_pump).
    jobs: Sender<Job>,
}

static APP: OnceLock<Arc<App>> = OnceLock::new();

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn parse_key(s: &str) -> Option<[u8; 32]> {
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
fn ensure_contact(app: &App, peer: [u8; 32]) {
    let mut store = app.contacts.lock().unwrap();
    if !store.contains(&peer) {
        let hex = hex::encode(peer);
        let name = format!("{}…", &hex[..8]);
        let _ = store.add(&peer, &name, now());
    }
}

fn push_event(app: &App, json: serde_json::Value) {
    app.events.lock().unwrap().push_back(json.to_string());
}

/// Поток event_pump = групповой актор: владеет [`GroupManager`] (GroupChat !Send),
/// обрабатывает события мессенджера и групповые команды из JNI — всё в одном потоке.
fn event_pump(
    app: Arc<App>,
    job_rx: Receiver<Job>,
    identity: Identity,
    messenger: Arc<Messenger>,
    data_dir: PathBuf,
) {
    let mut groups = GroupManager::new(identity, messenger.clone(), data_dir);
    let mut last_beacon = Instant::now();
    loop {
        match job_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Job::Ev(ev)) => handle_event(&app, &mut groups, ev),
            Ok(Job::Cmd(cmd)) => handle_cmd(&app, &mut groups, cmd),
            Ok(Job::Beacon) => {
                // Вход в комнату: сразу шлём маячок, не ждём интервала.
                last_beacon = Instant::now();
                beacon_tick(&app, &groups, &messenger);
            }
            Ok(Job::Leave(gid)) => {
                // Явный выход: шлём членам маячок с флагом LEFT (мгновенно убрать).
                for m in groups.members(&gid) {
                    if m == app.me_pub {
                        continue;
                    }
                    let _ = messenger.send_room_presence(&m, &gid, PRESENCE_LEFT);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        // Маячок присутствия ~раз в BEACON_INTERVAL (проверяем после каждой работы,
        // чтобы под нагрузкой событий он не голодал).
        if last_beacon.elapsed() >= BEACON_INTERVAL {
            last_beacon = Instant::now();
            beacon_tick(&app, &groups, &messenger);
        }
    }
}

/// Разослать маячок присутствия членам всех комнат, где мы сейчас находимся,
/// и подчистить протухшее присутствие других.
fn beacon_tick(app: &Arc<App>, groups: &GroupManager, messenger: &Messenger) {
    let joined: Vec<(Vec<u8>, u8)> = {
        let rs = rooms().lock().unwrap();
        rs.joined.iter().map(|(g, f)| (g.clone(), *f)).collect()
    };
    for (gid, flags) in joined {
        for m in groups.members(&gid) {
            if m == app.me_pub {
                continue; // себе слать не нужно
            }
            let _ = messenger.send_room_presence(&m, &gid, flags);
        }
    }
    let now = Instant::now();
    let mut rs = rooms().lock().unwrap();
    for peers in rs.remote.values_mut() {
        peers.retain(|_, (t, _)| now.duration_since(*t) < PRESENCE_TTL);
    }
}

/// Обработать одно событие мессенджера: персист + JSON в очередь поллинга +
/// маршрутизация групповых событий в [`GroupManager`].
fn handle_event(app: &Arc<App>, groups: &mut GroupManager, ev: Event) {
    match ev {
        Event::Presence { peer, online } => {
            if online {
                ensure_contact(app, peer);
            }
            push_event(app, serde_json::json!({
                "type": "presence", "peer": hex::encode(peer), "online": online,
            }));
        }
        Event::Text { peer, id, text } => {
            ensure_contact(app, peer);
            let t = now();
            let _ = app.messages.lock().unwrap().append(&peer, StoredMessage {
                id, text: text.clone(), mine: false, time: t, status: String::new(),
            });
            push_event(app, serde_json::json!({
                "type": "text", "peer": hex::encode(peer), "id": id, "text": text, "time": t,
            }));
        }
        Event::Ack { peer, id } => {
            let _ = app.messages.lock().unwrap().set_status(&peer, id, "delivered");
            push_event(app, serde_json::json!({"type": "ack", "peer": hex::encode(peer), "id": id}));
        }
        Event::Undelivered { peer, id } => {
            let _ = app.messages.lock().unwrap().set_status(&peer, id, "failed");
            push_event(app, serde_json::json!({"type": "undelivered", "peer": hex::encode(peer), "id": id}));
        }
        Event::CallInvite { peer, call_id, audio, video } => push_event(app, serde_json::json!({
            "type": "call_invite", "peer": hex::encode(peer), "call_id": call_id,
            "audio": audio, "video": video,
        })),
        Event::CallAccept { peer, call_id } => push_event(app, serde_json::json!({
            "type": "call_accept", "peer": hex::encode(peer), "call_id": call_id,
        })),
        Event::CallEnd { peer, call_id } => push_event(app, serde_json::json!({
            "type": "call_end", "peer": hex::encode(peer), "call_id": call_id,
        })),
        Event::Group { data, .. } => {
            let out = groups.on_group(&data);
            if let (Some(gid), Some(text)) = (&out.gid, &out.text) {
                push_event(app, serde_json::json!({
                    "type": "group_msg", "gid": hex::encode(gid), "text": text,
                }));
            }
            if out.joined {
                push_event(app, serde_json::json!({"type": "groups_changed"}));
            }
        }
        Event::GroupKeyRequest { peer } => groups.on_key_request(peer),
        Event::GroupKeyReply { peer, key_package } => {
            if groups.on_key_reply(peer, &key_package) {
                push_event(app, serde_json::json!({"type": "groups_changed"}));
            }
        }
        Event::RoomPresence { peer, gid, flags } => {
            let left = flags & PRESENCE_LEFT != 0;
            {
                let mut rs = rooms().lock().unwrap();
                if left {
                    if let Some(m) = rs.remote.get_mut(&gid) {
                        m.remove(&peer);
                    }
                } else {
                    rs.remote.entry(gid.clone()).or_default().insert(peer, (Instant::now(), flags));
                }
            }
            push_event(app, serde_json::json!({
                "type": "room_presence", "gid": hex::encode(&gid),
                "peer": hex::encode(peer), "flags": flags, "left": left,
            }));
        }
    }
}

/// Исполнить групповую команду из JNI (в потоке актора) и вернуть результат.
fn handle_cmd(app: &Arc<App>, groups: &mut GroupManager, cmd: GroupCmd) {
    match cmd {
        GroupCmd::Create { name, resp } => {
            let gid = groups.create(&name).map(|g| hex::encode(g)).unwrap_or_default();
            if !gid.is_empty() {
                push_event(app, serde_json::json!({"type": "groups_changed"}));
            }
            let _ = resp.send(gid);
        }
        GroupCmd::Invite { gid, peer } => groups.invite(&gid, peer),
        GroupCmd::SendText { gid, text, resp } => {
            let ok = groups.send_text(&gid, &text);
            let _ = resp.send(ok);
        }
        GroupCmd::List { resp } => {
            let _ = resp.send(groups.list_json());
        }
        GroupCmd::History { gid, resp } => {
            let _ = resp.send(groups.history_json(&gid));
        }
        GroupCmd::Members { gid, resp } => {
            let _ = resp.send(groups.members_json(&gid));
        }
    }
}

fn app() -> Option<&'static Arc<App>> {
    APP.get()
}

// ── JNI-обвязка ────────────────────────────────────────────────────────────
// Хелперы строк: читаем JString в Rust String, отдаём Rust String как jstring.

fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(|v| v.into()).unwrap_or_default()
}
fn ret(env: &JNIEnv, s: &str) -> jstring {
    env.new_string(s).map(|v| v.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Инициализация. dataDir — приватный каталог приложения (filesDir).
/// Возвращает JSON `{pubkey, address, fingerprint}` или `{"error": "..."}`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_start<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
) -> jstring {
    let _ = android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("p2p"),
    );

    if let Some(app) = APP.get() {
        let id = identity_json(&app.me_pub, &app.messenger.address());
        return ret(&env, &id.to_string());
    }

    let dir = PathBuf::from(jstr(&mut env, &data_dir));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return ret(&env, &serde_json::json!({"error": e.to_string()}).to_string());
    }

    // At-rest: ключ в файле приватного каталога (keyring на Android ненадёжен —
    // разные ключи между перезапусками). Android Keystore — дальнейшее усиление.
    if let Err(e) = p2p_core::vault::init_with_keyfile(&dir.join("vault.key")) {
        log::warn!("at-rest шифрование недоступно ({e}); данные открытым текстом");
    }

    let identity = match Profile::load_or_create(&dir.join("profile.json")) {
        Ok(p) => p.identity,
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

    let contacts = ContactStore::load(&dir.join("contacts.json")).expect("contacts");
    let messages = MessageStore::load(&dir.join("messages.json")).expect("messages");
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

    let app = Arc::new(App {
        messenger: messenger.clone(),
        contacts: Mutex::new(contacts),
        messages: Mutex::new(messages),
        me_pub,
        events: Mutex::new(VecDeque::new()),
        jobs: job_tx,
    });
    let _ = APP.set(app.clone());
    let groups_dir = dir.clone();
    std::thread::spawn(move || {
        event_pump(app, job_rx, identity_for_groups, messenger, groups_dir)
    });

    ret(&env, &identity_json(&me_pub, &address).to_string())
}

fn identity_json(me_pub: &[u8; 32], address: &str) -> serde_json::Value {
    serde_json::json!({
        "pubkey": hex::encode(me_pub),
        "address": address,
        "fingerprint": p2p_core::identity::fingerprint_words(me_pub).join(" "),
    })
}

/// Следующее событие как JSON-строка, либо "" если очередь пуста.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_pollEvent<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let s = app()
        .and_then(|a| a.events.lock().unwrap().pop_front())
        .unwrap_or_default();
    ret(&env, &s)
}

/// Отправить текст. Возвращает id сообщения (>=0) либо -1 при ошибке.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_sendText<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
    text: JString<'local>,
) -> jlong {
    let Some(app) = app() else { return -1 };
    let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return -1 };
    let text = jstr(&mut env, &text);
    match app.messenger.send_text(&key, &text) {
        Ok(id) => {
            let _ = app.messages.lock().unwrap().append(&key, StoredMessage {
                id, text, mine: true, time: now(), status: "sent".into(),
            });
            id as jlong
        }
        Err(_) => -1,
    }
}

/// Инициировать/держать соединение с пиром (фоновый коннект).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_connect<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
) {
    if let (Some(app), Some(key)) = (app(), parse_key(&jstr(&mut env, &key_hex))) {
        spawn_connect(app.messenger.clone(), key);
    }
}

/// Добавить контакт (TOFU). Возвращает "added" | "known" | "error".
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_addContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
    name: JString<'local>,
) -> jstring {
    let Some(app) = app() else { return ret(&env, "error") };
    let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "error") };
    let name = jstr(&mut env, &name);
    let name = if name.trim().is_empty() { format!("{}…", &hex::encode(key)[..8]) } else { name };
    let outcome = app.contacts.lock().unwrap().add(&key, &name, now());
    match outcome {
        Ok(p2p_core::store::AddOutcome::Added) => {
            spawn_connect(app.messenger.clone(), key);
            ret(&env, "added")
        }
        Ok(p2p_core::store::AddOutcome::AlreadyKnown) => ret(&env, "known"),
        Err(_) => ret(&env, "error"),
    }
}

/// Список контактов как JSON-массив.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_listContacts<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let json = app()
        .map(|a| serde_json::to_string(&a.contacts.lock().unwrap().list()).unwrap_or_default())
        .unwrap_or_else(|| "[]".into());
    ret(&env, &json)
}

/// История переписки с пиром как JSON-массив.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_history<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
) -> jstring {
    let Some(app) = app() else { return ret(&env, "[]") };
    let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "[]") };
    let json = serde_json::to_string(app.messages.lock().unwrap().history(&key)).unwrap_or_default();
    ret(&env, &json)
}

/// Пометить контакт проверенным/непроверенным.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_setVerified<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
    verified: jboolean,
) {
    if let (Some(app), Some(key)) = (app(), parse_key(&jstr(&mut env, &key_hex))) {
        let _ = app.contacts.lock().unwrap().set_verified(&key, verified != JNI_FALSE);
    }
}

/// Переименовать контакт.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_renameContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
    name: JString<'local>,
) {
    if let (Some(app), Some(key)) = (app(), parse_key(&jstr(&mut env, &key_hex))) {
        let name = jstr(&mut env, &name);
        let _ = app.contacts.lock().unwrap().rename(&key, &name);
    }
}

/// Удалить контакт.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_removeContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
) {
    if let (Some(app), Some(key)) = (app(), parse_key(&jstr(&mut env, &key_hex))) {
        let _ = app.contacts.lock().unwrap().remove(&key);
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_isOnline<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
) -> jboolean {
    let online = app()
        .zip(parse_key(&jstr(&mut env, &key_hex)))
        .map(|(a, k)| a.messenger.is_online(&k))
        .unwrap_or(false);
    if online { JNI_TRUE } else { JNI_FALSE }
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_netPeers<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    app().map(|a| a.messenger.net_peers()).unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_netTree<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    app().map(|a| a.messenger.net_tree()).unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_stop<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    if let Some(app) = app() {
        app.messenger.shutdown();
    }
}

// ── Звонки (1:1) ───────────────────────────────────────────────────────────

/// JSON-параметры для настройки аудио на стороне Kotlin (AudioRecord/Track).
fn call_params(call_id: u64, audio: &AudioSettings) -> String {
    serde_json::json!({
        "call_id": call_id,
        "sample_rate": audio.sample_rate,
        "channels": audio.channels,
        "frame_len": audio.interleaved_len(),
    })
    .to_string()
}

/// Текущий движок звонка как Arc-клон. Глобальный мьютекс [`CALL`] держим ТОЛЬКО
/// на время клонирования Arc — НИКОГДА во время работы движка (сеть/кольца),
/// иначе блокирующий `send_media`/`ygg_write` (backpressure Yggdrasil) удержит
/// мьютекс и заморозит весь медиа-конвейер (видео+аудио).
fn current_call() -> Option<Arc<CallEngine>> {
    CALL.lock().unwrap().clone()
}

fn begin_call(peer: [u8; 32]) -> Option<AudioSettings> {
    let app = app()?;
    let mut audio = AudioSettings::default();
    // Чуть глубже джиттер-буфер на мобиле: сглаживает сетевой джиттер (чёткость
    // речи ценой ~+40мс задержки). 5 кадров × 20мс = 100мс.
    audio.jitter_depth = 5;
    // Видео-кодек теперь аппаратный (MediaCodec/Kotlin); движок только
    // фрагментирует/собирает H.264 — параметры видео в Rust не нужны.
    let engine = CallEngine::start(app.messenger.clone(), peer, audio);
    let old = CALL.lock().unwrap().replace(engine); // лок освобождается сразу
    if let Some(old) = old {
        std::thread::spawn(move || old.stop()); // не джойнить под локом/на UI
    }
    Some(audio)
}

/// Начать исходящий звонок: шлём invite и запускаем движок.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callStart<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
    video: jboolean,
    vw: jint,
    vh: jint,
) -> jstring {
    let Some(app) = app() else { return ret(&env, "{}") };
    let Some(peer) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "{}") };
    let video = video != JNI_FALSE;
    let call_id = CALL_ID.fetch_add(1, Ordering::Relaxed);
    let _ = app.messenger.send_call_invite(&peer, call_id, true, video);
    let _ = (vw, vh); // видео-разрешение теперь задаёт Kotlin/MediaCodec
    let Some(audio) = begin_call(peer) else { return ret(&env, "{}") };
    ret(&env, &call_params(call_id, &audio))
}

/// Принять входящий звонок: шлём accept и запускаем движок.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callAccept<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
    call_id: jlong,
    video: jboolean,
    vw: jint,
    vh: jint,
) -> jstring {
    let Some(app) = app() else { return ret(&env, "{}") };
    let Some(peer) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "{}") };
    let _ = app.messenger.send_call_accept(&peer, call_id as u64);
    let _ = (video, vw, vh); // видео задаёт Kotlin/MediaCodec
    let Some(audio) = begin_call(peer) else { return ret(&env, "{}") };
    ret(&env, &call_params(call_id as u64, &audio))
}

/// Завершить звонок: шлём end и останавливаем движок.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callEnd<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
    call_id: jlong,
) {
    if let (Some(app), Some(peer)) = (app(), parse_key(&jstr(&mut env, &key_hex))) {
        let _ = app.messenger.send_call_end(&peer, call_id as u64);
    }
    // Забираем движок (мьютекс освобождается сразу) и останавливаем его в ФОНЕ —
    // stop() джойнит потоки, нельзя блокировать вызывающий (часто UI) поток.
    let engine = CALL.lock().unwrap().take();
    if let Some(engine) = engine {
        std::thread::spawn(move || engine.stop());
    }
}

// ── Комнаты-звонки (присутствие, Discord-style) ──────────────────────────────

/// Войти в комнату `gid` (начать слать маячок присутствия). `audio`/`video` —
/// наши текущие A/V-флаги (для H1.0 медиа ещё нет, флаги информативны).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomJoin<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    gid_hex: JString<'local>,
    audio: jboolean,
    video: jboolean,
) {
    let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
    let mut flags = 0u8;
    if audio != JNI_FALSE {
        flags |= 0b01;
    }
    if video != JNI_FALSE {
        flags |= 0b10;
    }
    rooms().lock().unwrap().joined.insert(gid, flags);
    // Сразу разослать маячок (не ждать интервала) — участники увидят нас быстро.
    if let Some(a) = app() {
        let _ = a.jobs.send(Job::Beacon);
    }
}

/// Выйти из комнаты `gid` (перестать слать маячок, забыть её присутствие).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomLeave<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    gid_hex: JString<'local>,
) {
    if let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) {
        {
            let mut rs = rooms().lock().unwrap();
            rs.joined.remove(&gid);
            rs.remote.remove(&gid);
        }
        // Сообщаем членам комнаты, что вышли — чтобы убрали нас мгновенно.
        if let Some(a) = app() {
            let _ = a.jobs.send(Job::Leave(gid));
        }
    }
}

/// Текущие участники комнаты `gid` (чей маячок свежий): JSON
/// `[{peer, audio, video}]`. Себя сюда не включаем — UI добавит сам.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomParticipants<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    gid_hex: JString<'local>,
) -> jstring {
    let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "[]") };
    let now = Instant::now();
    let rs = rooms().lock().unwrap();
    let arr: Vec<serde_json::Value> = rs
        .remote
        .get(&gid)
        .map(|peers| {
            peers
                .iter()
                .filter(|(_, (t, _))| now.duration_since(*t) < PRESENCE_TTL)
                .map(|(p, (_, f))| serde_json::json!({
                    "peer": hex::encode(p),
                    "audio": f & 0b01 != 0,
                    "video": f & 0b10 != 0,
                }))
                .collect()
        })
        .unwrap_or_default();
    ret(&env, &serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into()))
}

/// Захваченный PCM с микрофона (первые `n` сэмплов массива) → движку.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callPushAudio<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    data: JShortArray<'local>,
    n: jint,
) {
    let n = n.max(0) as usize;
    if n == 0 {
        return;
    }
    let mut buf = vec![0i16; n];
    if env.get_short_array_region(&data, 0, &mut buf).is_err() {
        return;
    }
    if let Some(engine) = current_call() {
        engine.push_audio(&buf);
    }
}

/// Заполнить массив PCM для воспроизведения. Возвращает число реальных сэмплов.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callPullAudio<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    out: JShortArray<'local>,
) -> jint {
    let len = env.get_array_length(&out).unwrap_or(0).max(0) as usize;
    if len == 0 {
        return 0;
    }
    let mut buf = vec![0i16; len];
    let n = current_call().map(|e| e.pull_audio(&mut buf)).unwrap_or(0);
    let _ = env.set_short_array_region(&out, 0, &buf);
    n as jint
}

/// H.264 access-unit от аппаратного энкодера (Kotlin/MediaCodec) → фрагментация
/// и отправка по сети.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callSendVideo<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    nal: JByteArray<'local>,
    keyframe: jboolean,
) {
    let len = env.get_array_length(&nal).unwrap_or(0).max(0) as usize;
    if len == 0 {
        return;
    }
    let mut buf = vec![0i8; len];
    if env.get_byte_array_region(&nal, 0, &mut buf).is_err() {
        return;
    }
    let bytes: Vec<u8> = buf.into_iter().map(|b| b as u8).collect();
    if let Some(engine) = current_call() {
        engine.send_video(&bytes, keyframe != JNI_FALSE);
    }
}

/// Забрать собранный входящий H.264 access-unit для аппаратного декодера
/// (Kotlin/MediaCodec). Пустой массив — кадра нет.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callPullVideoPacket<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jbyteArray {
    let pkt = current_call().and_then(|e| e.pull_video_packet());
    let Some(data) = pkt else {
        return env.new_byte_array(0).map(|a| a.into_raw()).unwrap_or(std::ptr::null_mut());
    };
    let signed: Vec<i8> = data.into_iter().map(|b| b as i8).collect();
    match env.new_byte_array(signed.len() as jsize) {
        Ok(arr) => {
            let _ = env.set_byte_array_region(&arr, 0, &signed);
            arr.into_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callSetMuted<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    muted: jboolean,
) {
    if let Some(engine) = current_call() {
        engine.set_muted(muted != JNI_FALSE);
    }
}

// ── Группы (MLS) ───────────────────────────────────────────────────────────
// Команды уходят групповому актору (поток event_pump) и, для запросов, блокируют
// до ответа — JNI-вызовы могут блокировать.

fn group_query<T>(make: impl FnOnce(Sender<T>) -> GroupCmd) -> Option<T> {
    let app = app()?;
    let (tx, rx) = std::sync::mpsc::channel::<T>();
    app.jobs.send(Job::Cmd(make(tx))).ok()?;
    rx.recv().ok()
}

/// Создать группу. Возвращает group_id (hex) или "".
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupCreate<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    name: JString<'local>,
) -> jstring {
    let name = jstr(&mut env, &name);
    let gid = group_query(|resp| GroupCmd::Create { name, resp }).unwrap_or_default();
    ret(&env, &gid)
}

/// Пригласить контакт в группу (просим его KeyPackage).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupInvite<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    gid_hex: JString<'local>,
    peer_hex: JString<'local>,
) {
    let (Some(app), Some(gid), Some(peer)) = (
        app(),
        hex::decode(jstr(&mut env, &gid_hex)).ok(),
        parse_key(&jstr(&mut env, &peer_hex)),
    ) else {
        return;
    };
    let _ = app.jobs.send(Job::Cmd(GroupCmd::Invite { gid, peer }));
}

/// Отправить текст в группу. Возвращает true при успехе.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupSendText<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    gid_hex: JString<'local>,
    text: JString<'local>,
) -> jboolean {
    let Some(gid) = hex::decode(jstr(&mut env, &gid_hex)).ok() else { return JNI_FALSE };
    let text = jstr(&mut env, &text);
    let ok = group_query(|resp| GroupCmd::SendText { gid, text, resp }).unwrap_or(false);
    if ok { JNI_TRUE } else { JNI_FALSE }
}

/// Список групп как JSON-массив [{gid, name, members}].
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupList<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let json = group_query(|resp| GroupCmd::List { resp }).unwrap_or_else(|| "[]".into());
    ret(&env, &json)
}

/// История группового чата как JSON-массив [{text, mine, time}].
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupHistory<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    gid_hex: JString<'local>,
) -> jstring {
    let Some(gid) = hex::decode(jstr(&mut env, &gid_hex)).ok() else { return ret(&env, "[]") };
    let json = group_query(|resp| GroupCmd::History { gid, resp }).unwrap_or_else(|| "[]".into());
    ret(&env, &json)
}

/// Ключи участников группы (hex) как JSON-массив — для группового звонка.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupMembers<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    gid_hex: JString<'local>,
) -> jstring {
    let Some(gid) = hex::decode(jstr(&mut env, &gid_hex)).ok() else { return ret(&env, "[]") };
    let json = group_query(|resp| GroupCmd::Members { gid, resp }).unwrap_or_else(|| "[]".into());
    ret(&env, &json)
}

// ── Диагностика: тест пропускной способности меша ──────────────────────────
// Реальный замер throughput между двумя устройствами по живой сети Yggdrasil.
// Один зовёт benchRecvStart() (приёмник считает), другой benchBlast() (бластит).

/// Трек-маркер бенч-датаграмм (не пересекается с медиа-треками звонка).
const BENCH_TRACK: u8 = 200;

struct BenchRecv {
    bytes: AtomicU64,
    pkts: AtomicU64,
    running: AtomicBool,
    start: Instant,
}

static BENCH: Mutex<Option<Arc<BenchRecv>>> = Mutex::new(None);

/// Включить приём бенча: считаем входящие BENCH_TRACK-датаграммы.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_benchRecvStart<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    let Some(app) = app() else { return };
    let rx = app.messenger.media_channel(8192);
    let st = Arc::new(BenchRecv {
        bytes: AtomicU64::new(0),
        pkts: AtomicU64::new(0),
        running: AtomicBool::new(true),
        start: Instant::now(),
    });
    *BENCH.lock().unwrap() = Some(st.clone());
    std::thread::spawn(move || {
        while st.running.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(p) if p.track == BENCH_TRACK => {
                    st.bytes.fetch_add(p.payload.len() as u64, Ordering::Relaxed);
                    st.pkts.fetch_add(1, Ordering::Relaxed);
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return,
            }
        }
    });
}

/// Статистика приёмника: JSON {pkts, bytes, secs, mbps}.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_benchRecvStats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let json = {
        let g = BENCH.lock().unwrap();
        match g.as_ref() {
            Some(st) => {
                let bytes = st.bytes.load(Ordering::Relaxed);
                let pkts = st.pkts.load(Ordering::Relaxed);
                let secs = st.start.elapsed().as_secs_f64().max(0.001);
                serde_json::json!({
                    "pkts": pkts, "bytes": bytes, "secs": secs,
                    "mbps": bytes as f64 * 8.0 / 1e6 / secs,
                })
                .to_string()
            }
            None => "{}".into(),
        }
    };
    ret(&env, &json)
}

/// Бластить `total_kb` КБ датаграммами по 1200Б на пира `key_hex` (макс 10с).
/// Возвращает JSON {sent_pkts, sent_bytes, secs, mbps} — скорость ОТПРАВКИ.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_benchBlast<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    key_hex: JString<'local>,
    total_kb: jint,
) -> jstring {
    let Some(app) = app() else { return ret(&env, "{}") };
    let Some(peer) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "{}") };
    let payload = vec![0u8; 1200];
    let target = (total_kb.max(0) as u64) * 1024;
    let mut sent_bytes = 0u64;
    let mut sent_pkts = 0u64;
    let mut seq = 0u16;
    let t0 = Instant::now();
    while sent_bytes < target && t0.elapsed() < Duration::from_secs(10) {
        if app.messenger.send_media(&peer, BENCH_TRACK, seq, 0, &payload).is_ok() {
            sent_bytes += payload.len() as u64;
            sent_pkts += 1;
            seq = seq.wrapping_add(1);
        }
    }
    let secs = t0.elapsed().as_secs_f64().max(0.001);
    ret(&env, &serde_json::json!({
        "sent_pkts": sent_pkts, "sent_bytes": sent_bytes, "secs": secs,
        "mbps": sent_bytes as f64 * 8.0 / 1e6 / secs,
    }).to_string())
}
