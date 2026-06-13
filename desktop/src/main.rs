//! Десктоп-приложение (Tauri v2): Holo-интерфейс поверх движка p2p-messenger.
//!
//! Бэкенд хранит идентичность/контакты/мессенджер, отдаёт команды фронтенду и
//! пробрасывает события движка (presence, входящие сообщения, ack) в UI.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use p2p_core::store::{ContactStore, Profile};
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

/// Встроенный список публичных пиров (входим в глобальную сеть при старте).
const PUBLIC_PEERS: &str = include_str!("../../peers/public-peers.txt");

struct AppState {
    messenger: Arc<Messenger>,
    contacts: Mutex<ContactStore>,
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

/// Фоновый цикл установления/поддержания сессии с контактом.
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
    Net {
        peers,
        tree,
        online: peers >= 1 && tree > 2,
    }
}

#[derive(Serialize)]
struct ContactView {
    key: String,
    name: String,
    fingerprint: String,
    online: bool,
}

#[tauri::command]
fn list_contacts(state: State<AppState>) -> Vec<ContactView> {
    let store = state.contacts.lock().unwrap();
    store
        .list()
        .iter()
        .map(|c| {
            let online = parse_key(&c.key)
                .map(|k| state.messenger.is_online(&k))
                .unwrap_or(false);
            ContactView {
                key: c.key.clone(),
                name: c.name.clone(),
                fingerprint: c.fingerprint.clone(),
                online,
            }
        })
        .collect()
}

#[tauri::command]
fn add_contact(state: State<AppState>, key_hex: String, name: String) -> Result<String, String> {
    let key = parse_key(&key_hex).ok_or("неверный ключ (нужно 64 hex-символа)")?;
    if key == state.me_pub {
        return Err("это ваш собственный ключ".into());
    }
    let name = if name.trim().is_empty() {
        "Без имени".to_string()
    } else {
        name.trim().to_string()
    };
    {
        let mut store = state.contacts.lock().unwrap();
        store.add(&key, &name, now()).map_err(|e| e.to_string())?;
    }
    spawn_connect(state.messenger.clone(), key);
    Ok(hex::encode(key))
}

#[tauri::command]
fn send_text(state: State<AppState>, key_hex: String, text: String) -> Result<u64, String> {
    let key = parse_key(&key_hex).ok_or("неверный ключ")?;
    state
        .messenger
        .send_text(&key, &text)
        .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Каталог данных: P2P_DATA_DIR (для запуска нескольких копий локально)
            // либо стандартный app_data_dir.
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

            let contacts = ContactStore::load(&dir.join("contacts.json"))
                .expect("не удалось загрузить контакты");
            let contact_keys: Vec<[u8; 32]> =
                contacts.list().iter().filter_map(|c| parse_key(&c.key)).collect();

            let (messenger, rx) = Messenger::start(identity, node);
            let messenger = Arc::new(messenger);

            // Проброс событий движка во фронтенд.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for ev in rx.iter() {
                    let _ = match ev {
                        Event::Presence { peer, online } => handle.emit(
                            "presence",
                            serde_json::json!({ "peer": hex::encode(peer), "online": online }),
                        ),
                        Event::Text { peer, id, text } => handle.emit(
                            "message",
                            serde_json::json!({ "peer": hex::encode(peer), "id": id, "text": text }),
                        ),
                        Event::Ack { peer, id } => handle.emit(
                            "ack",
                            serde_json::json!({ "peer": hex::encode(peer), "id": id }),
                        ),
                        Event::Undelivered { peer, id } => handle.emit(
                            "undelivered",
                            serde_json::json!({ "peer": hex::encode(peer), "id": id }),
                        ),
                    };
                }
            });

            // Поддерживаем сессии с уже известными контактами.
            for key in contact_keys {
                spawn_connect(messenger.clone(), key);
            }

            app.manage(AppState {
                messenger,
                contacts: Mutex::new(contacts),
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
            add_contact,
            send_text
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска Tauri-приложения");
}
