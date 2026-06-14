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

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jni::objects::{JByteArray, JClass, JShortArray, JString};
use jni::sys::{jboolean, jbyteArray, jint, jlong, jsize, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use p2p_core::settings::{AudioSettings, VideoSettings};
use p2p_core::store::{ContactStore, MessageStore, Profile, StoredMessage};
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

use call::CallEngine;

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
}

static APP: OnceLock<Arc<App>> = OnceLock::new();

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn parse_key(s: &str) -> Option<[u8; 32]> {
    hex::decode(s.trim()).ok()?.as_slice().try_into().ok()
}

/// Фоновый коннект к пиру (как на десктопе): держим попытки, пока процесс жив.
fn spawn_connect(m: Arc<Messenger>, key: [u8; 32]) {
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

/// Поток-насос событий мессенджера: персистит входящее/статусы и кладёт JSON в очередь.
fn event_pump(app: Arc<App>, rx: std::sync::mpsc::Receiver<Event>) {
    while let Ok(ev) = rx.recv() {
        match ev {
            Event::Presence { peer, online } => {
                if online {
                    ensure_contact(&app, peer);
                }
                push_event(&app, serde_json::json!({
                    "type": "presence", "peer": hex::encode(peer), "online": online,
                }));
            }
            Event::Text { peer, id, text } => {
                ensure_contact(&app, peer);
                let t = now();
                let _ = app.messages.lock().unwrap().append(&peer, StoredMessage {
                    id, text: text.clone(), mine: false, time: t, status: String::new(),
                });
                push_event(&app, serde_json::json!({
                    "type": "text", "peer": hex::encode(peer), "id": id, "text": text, "time": t,
                }));
            }
            Event::Ack { peer, id } => {
                let _ = app.messages.lock().unwrap().set_status(&peer, id, "delivered");
                push_event(&app, serde_json::json!({
                    "type": "ack", "peer": hex::encode(peer), "id": id,
                }));
            }
            Event::Undelivered { peer, id } => {
                let _ = app.messages.lock().unwrap().set_status(&peer, id, "failed");
                push_event(&app, serde_json::json!({
                    "type": "undelivered", "peer": hex::encode(peer), "id": id,
                }));
            }
            Event::CallInvite { peer, call_id, audio, video } => push_event(&app, serde_json::json!({
                "type": "call_invite", "peer": hex::encode(peer), "call_id": call_id,
                "audio": audio, "video": video,
            })),
            Event::CallAccept { peer, call_id } => push_event(&app, serde_json::json!({
                "type": "call_accept", "peer": hex::encode(peer), "call_id": call_id,
            })),
            Event::CallEnd { peer, call_id } => push_event(&app, serde_json::json!({
                "type": "call_end", "peer": hex::encode(peer), "call_id": call_id,
            })),
            Event::Group { peer, data } => push_event(&app, serde_json::json!({
                "type": "group", "peer": hex::encode(peer), "data": hex::encode(data),
            })),
            Event::GroupKeyRequest { peer } => push_event(&app, serde_json::json!({
                "type": "group_key_request", "peer": hex::encode(peer),
            })),
            Event::GroupKeyReply { peer, key_package } => push_event(&app, serde_json::json!({
                "type": "group_key_reply", "peer": hex::encode(peer),
                "key_package": hex::encode(key_package),
            })),
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

    // At-rest: ключ из Android Keystore придёт позже (G4); пока — graceful fallback.
    if let Err(e) = p2p_core::vault::init() {
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

    let (messenger, rx) = Messenger::start(identity, node);
    let messenger = Arc::new(messenger);
    for k in &contact_keys {
        spawn_connect(messenger.clone(), *k);
    }

    let app = Arc::new(App {
        messenger,
        contacts: Mutex::new(contacts),
        messages: Mutex::new(messages),
        me_pub,
        events: Mutex::new(VecDeque::new()),
    });
    let _ = APP.set(app.clone());
    std::thread::spawn(move || event_pump(app, rx));

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

fn video_settings(w: jint, h: jint) -> VideoSettings {
    VideoSettings { width: w.max(0) as u32, height: h.max(0) as u32, fps: 30, bitrate: 2_000_000 }
}

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

fn begin_call(peer: [u8; 32], video: bool, w: jint, h: jint) -> Option<AudioSettings> {
    let app = app()?;
    let audio = AudioSettings::default();
    let vcfg = if video { Some(video_settings(w, h)) } else { None };
    let engine = CallEngine::start(app.messenger.clone(), peer, audio, vcfg);
    *CALL.lock().unwrap() = Some(engine);
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
    let Some(audio) = begin_call(peer, video, vw, vh) else { return ret(&env, "{}") };
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
    let Some(audio) = begin_call(peer, video != JNI_FALSE, vw, vh) else { return ret(&env, "{}") };
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
    if let Some(engine) = CALL.lock().unwrap().take() {
        engine.stop();
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

/// RGB8-кадр с камеры/экрана → движку (для отправки видео).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callPushVideo<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    rgb: JByteArray<'local>,
) {
    let len = env.get_array_length(&rgb).unwrap_or(0).max(0) as usize;
    if len == 0 {
        return;
    }
    let mut buf = vec![0i8; len];
    if env.get_byte_array_region(&rgb, 0, &mut buf).is_err() {
        return;
    }
    let bytes: Vec<u8> = buf.into_iter().map(|b| b as u8).collect();
    if let Some(engine) = CALL.lock().unwrap().as_ref() {
        engine.push_video(bytes);
    }
}

/// Забрать декодированный входящий видео-кадр. Формат: [w:4 LE][h:4 LE][rgb…];
/// пустой массив — кадра нет.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_callPullVideo<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jbyteArray {
    let frame = CALL.lock().unwrap().as_ref().and_then(|e| e.pull_frame());
    let Some(f) = frame else {
        return env.new_byte_array(0).map(|a| a.into_raw()).unwrap_or(std::ptr::null_mut());
    };
    let mut out = Vec::with_capacity(8 + f.rgb.len());
    out.extend_from_slice(&f.width.to_le_bytes());
    out.extend_from_slice(&f.height.to_le_bytes());
    out.extend_from_slice(&f.rgb);
    let signed: Vec<i8> = out.into_iter().map(|b| b as i8).collect();
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
