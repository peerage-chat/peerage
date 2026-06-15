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

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// Работа для потока event_pump: событие мессенджера либо групповая команда.
enum Job {
    Ev(Event),
    Cmd(GroupCmd),
}

/// Текущий активный звонок (1:1 на процесс).
static CALL: Mutex<Option<Arc<CallEngine>>> = Mutex::new(None);
/// Счётчик id звонков (для исходящих приглашений).
static CALL_ID: AtomicU64 = AtomicU64::new(1);

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
    let mut groups = GroupManager::new(identity, messenger, data_dir);
    while let Ok(job) = job_rx.recv() {
        match job {
            Job::Ev(ev) => handle_event(&app, &mut groups, ev),
            Job::Cmd(cmd) => handle_cmd(&app, &mut groups, cmd),
        }
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
    if let Some(engine) = CALL.lock().unwrap().as_ref() {
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
    let n = CALL.lock().unwrap().as_ref().map(|e| e.pull_audio(&mut buf)).unwrap_or(0);
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
    if let Some(engine) = CALL.lock().unwrap().as_ref() {
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
    let pkt = CALL.lock().unwrap().as_ref().and_then(|e| e.pull_video_packet());
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
    if let Some(engine) = CALL.lock().unwrap().as_ref() {
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
