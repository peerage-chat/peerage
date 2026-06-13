//! Десктоп-приложение (Tauri v2): Holo-интерфейс поверх движка p2p-messenger.
//!
//! Бэкенд — источник истины: хранит идентичность, контакты и историю сообщений
//! на диске, отдаёт команды фронтенду и пробрасывает события движка в UI.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use p2p_core::store::{ContactStore, MessageStore, Profile, StoredMessage};
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

/// Встроенный список публичных пиров (входим в глобальную сеть при старте).
const PUBLIC_PEERS: &str = include_str!("../../peers/public-peers.txt");

type Contacts = Arc<Mutex<ContactStore>>;
type Messages = Arc<Mutex<MessageStore>>;

struct AppState {
    messenger: Arc<Messenger>,
    contacts: Contacts,
    messages: Messages,
    me_pub: [u8; 32],
    me_address: String,
    me_fingerprint: Vec<String>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn parse_key(hex_str: &str) -> Option<[u8; 32]> {
    let b = hex::decode(hex_str.trim()).ok()?;
    b.as_slice().try_into().ok()
}

fn spawn_connect(messenger: Arc<Messenger>, key: [u8; 32]) {
    std::thread::spawn(move || loop {
        let _ = messenger.connect(&key, Duration::from_secs(20));
        std::thread::sleep(Duration::from_secs(5));
    });
}

// ---- Команды ----

#[derive(Serialize)]
struct Me {
    address: String,
    pubkey: String,
    fingerprint: Vec<String>,
}

#[tauri::command]
fn my_identity(state: State<AppState>) -> Me {
    Me {
        address: state.me_address.clone(),
        pubkey: hex::encode(state.me_pub),
        fingerprint: state.me_fingerprint.clone(),
    }
}

#[derive(Serialize)]
struct Net {
    peers: i32,
    tree: i32,
    online: bool,
}

#[tauri::command]
fn net_status(state: State<AppState>) -> Net {
    let peers = state.messenger.net_peers();
    let tree = state.messenger.net_tree();
    Net { peers, tree, online: peers >= 1 && tree > 2 }
}

#[derive(Serialize)]
struct ContactView {
    key: String,
    name: String,
    fingerprint: String,
    online: bool,
    added: i64,
}

fn view_of(c: &p2p_core::store::Contact, messenger: &Messenger) -> ContactView {
    let online = parse_key(&c.key)
        .map(|k| messenger.is_online(&k))
        .unwrap_or(false);
    ContactView {
        key: c.key.clone(),
        name: c.name.clone(),
        fingerprint: c.fingerprint.clone(),
        online,
        added: c.added,
    }
}

#[tauri::command]
fn list_contacts(state: State<AppState>) -> Vec<ContactView> {
    let store = state.contacts.lock().unwrap();
    let mut v: Vec<ContactView> = store.list().iter().map(|c| view_of(c, &state.messenger)).collect();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

#[tauri::command]
fn contact_info(state: State<AppState>, key_hex: String) -> Result<ContactView, String> {
    let key = parse_key(&key_hex).ok_or("неверный ключ")?;
    let store = state.contacts.lock().unwrap();
    store
        .get(&key)
        .map(|c| view_of(c, &state.messenger))
        .ok_or_else(|| "контакт не найден".into())
}

#[tauri::command]
fn add_contact(state: State<AppState>, key_hex: String, name: String) -> Result<String, String> {
    let key = parse_key(&key_hex).ok_or("неверный ключ (нужно 64 hex-символа)")?;
    if key == state.me_pub {
        return Err("это ваш собственный ключ".into());
    }
    let name = if name.trim().is_empty() { "Без имени".into() } else { name.trim().to_string() };
    state
        .contacts
        .lock()
        .unwrap()
        .add(&key, &name, now())
        .map_err(|e| e.to_string())?;
    spawn_connect(state.messenger.clone(), key);
    Ok(hex::encode(key))
}

#[tauri::command]
fn rename_contact(state: State<AppState>, key_hex: String, name: String) -> Result<(), String> {
    let key = parse_key(&key_hex).ok_or("неверный ключ")?;
    if name.trim().is_empty() {
        return Err("имя не может быть пустым".into());
    }
    state
        .contacts
        .lock()
        .unwrap()
        .rename(&key, name.trim())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_contact(state: State<AppState>, key_hex: String) -> Result<(), String> {
    let key = parse_key(&key_hex).ok_or("неверный ключ")?;
    state.contacts.lock().unwrap().remove(&key).map_err(|e| e.to_string())?;
    state.messages.lock().unwrap().clear(&key).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_history(state: State<AppState>, key_hex: String) -> Result<Vec<StoredMessage>, String> {
    let key = parse_key(&key_hex).ok_or("неверный ключ")?;
    Ok(state.messages.lock().unwrap().history(&key).to_vec())
}

#[tauri::command]
fn send_text(state: State<AppState>, key_hex: String, text: String) -> Result<u64, String> {
    let key = parse_key(&key_hex).ok_or("неверный ключ")?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("пустое сообщение".into());
    }
    let id = state.messenger.send_text(&key, &text).map_err(|e| e.to_string())?;
    let _ = state.messages.lock().unwrap().append(
        &key,
        StoredMessage { id, text, mine: true, time: now(), status: "sent".into() },
    );
    Ok(id)
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = match std::env::var_os("P2P_DATA_DIR") {
                Some(d) => std::path::PathBuf::from(d),
                None => app
                    .path()
                    .app_data_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from(".")),
            };
            std::fs::create_dir_all(&dir).ok();

            let profile = Profile::load_or_create(&dir.join("profile.json"))
                .expect("не удалось загрузить профиль");
            let identity = profile.identity;
            let me_pub = identity.public_key_bytes();
            let me_fingerprint = identity
                .fingerprint_words()
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>();

            let node = Node::start(&identity.seed()).expect("не удалось запустить узел");
            let me_address = node.address();
            for uri in PUBLIC_PEERS
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
            {
                let _ = node.add_peer(uri);
            }

            let contacts: Contacts = Arc::new(Mutex::new(
                ContactStore::load(&dir.join("contacts.json")).expect("контакты"),
            ));
            let messages: Messages = Arc::new(Mutex::new(
                MessageStore::load(&dir.join("messages.json")).expect("сообщения"),
            ));
            let contact_keys: Vec<[u8; 32]> = contacts
                .lock()
                .unwrap()
                .list()
                .iter()
                .filter_map(|c| parse_key(&c.key))
                .collect();

            let (messenger, rx) = Messenger::start(identity, node);
            let messenger = Arc::new(messenger);

            // Проброс событий движка во фронтенд + персистентность.
            let handle = app.handle().clone();
            let ev_contacts = contacts.clone();
            let ev_messages = messages.clone();
            let ev_messenger = messenger.clone();
            std::thread::spawn(move || {
                for ev in rx.iter() {
                    match ev {
                        Event::Presence { peer, online } => {
                            ensure_contact(&ev_contacts, &ev_messenger, &handle, peer);
                            let _ = handle.emit(
                                "presence",
                                serde_json::json!({ "peer": hex::encode(peer), "online": online }),
                            );
                        }
                        Event::Text { peer, id, text } => {
                            ensure_contact(&ev_contacts, &ev_messenger, &handle, peer);
                            let _ = ev_messages.lock().unwrap().append(
                                &peer,
                                StoredMessage {
                                    id,
                                    text: text.clone(),
                                    mine: false,
                                    time: now(),
                                    status: String::new(),
                                },
                            );
                            let _ = handle.emit(
                                "message",
                                serde_json::json!({ "peer": hex::encode(peer), "id": id, "text": text }),
                            );
                        }
                        Event::Ack { peer, id } => {
                            let _ = ev_messages.lock().unwrap().set_status(&peer, id, "delivered");
                            let _ = handle.emit(
                                "ack",
                                serde_json::json!({ "peer": hex::encode(peer), "id": id }),
                            );
                        }
                        Event::Undelivered { peer, id } => {
                            let _ = ev_messages.lock().unwrap().set_status(&peer, id, "failed");
                            let _ = handle.emit(
                                "undelivered",
                                serde_json::json!({ "peer": hex::encode(peer), "id": id }),
                            );
                        }
                    }
                }
            });

            for key in contact_keys {
                spawn_connect(messenger.clone(), key);
            }

            app.manage(AppState {
                messenger,
                contacts,
                messages,
                me_pub,
                me_address,
                me_fingerprint,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            my_identity,
            net_status,
            list_contacts,
            contact_info,
            add_contact,
            rename_contact,
            delete_contact,
            get_history,
            send_text
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска Tauri-приложения");
}

/// TOFU: если пир ещё не в контактах — добавить с именем-заглушкой,
/// начать поддерживать сессию и уведомить фронтенд об изменении списка.
fn ensure_contact(
    contacts: &Contacts,
    messenger: &Arc<Messenger>,
    handle: &tauri::AppHandle,
    peer: [u8; 32],
) {
    let is_new = {
        let mut store = contacts.lock().unwrap();
        if store.contains(&peer) {
            false
        } else {
            let _ = store.add(&peer, "Без имени", now());
            true
        }
    };
    if is_new {
        spawn_connect(messenger.clone(), peer);
        let _ = handle.emit("contacts-changed", serde_json::json!({}));
    }
}
