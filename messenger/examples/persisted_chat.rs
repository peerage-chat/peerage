//! Headless-копия логики бэкенда десктопа: Messenger + MessageStore + ContactStore.
//! Два таких процесса через РЕАЛЬНУЮ сеть обмениваются сообщением и СОХРАНЯЮТ
//! историю на диск — затем messages.json сверяется с обеих сторон.
//!
//!   persisted_chat <seed_hex> <peer_pub_hex> <data_dir> <peers_file> <label> <msg>

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::store::{ContactStore, MessageStore, StoredMessage};
use p2p_core::Identity;
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
fn key(h: &str) -> [u8; 32] {
    hex::decode(h.trim()).unwrap().as_slice().try_into().unwrap()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed = key(&a[1]);
    let peer = key(&a[2]);
    let dir = std::path::PathBuf::from(&a[3]);
    let peers_file = &a[4];
    let label = &a[5];
    let msg = &a[6];
    std::fs::create_dir_all(&dir).unwrap();

    let id = Identity::from_seed(&seed).unwrap();
    // Хранилища — как в бэкенде десктопа.
    let mut contacts = ContactStore::load(&dir.join("contacts.json")).unwrap();
    contacts.add(&peer, "Peer", now()).ok();
    let mut messages = MessageStore::load(&dir.join("messages.json")).unwrap();

    let node = Node::start(&seed).unwrap();
    for uri in std::fs::read_to_string(peers_file).unwrap().lines()
        .map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let _ = node.add_peer(uri);
    }
    let t = Instant::now();
    while !(node.peer_count() >= 1 && node.tree_count() > 2) {
        if t.elapsed() > Duration::from_secs(30) {
            eprintln!("[{label}] нет сети");
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let (m, rx) = Messenger::start(id, node);
    if m.connect(&peer, Duration::from_secs(45)).is_err() {
        eprintln!("[{label}] handshake не удался");
        std::process::exit(1);
    }

    // Отправляем и СОХРАНЯЕМ исходящее (как send_text в бэкенде).
    let id1 = m.send_text(&peer, msg).unwrap();
    messages.append(&peer, StoredMessage {
        id: id1, text: msg.clone(), mine: true, time: now(), status: "sent".into(),
    }).unwrap();

    let mut got_in = false;
    let mut acked = false;
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline && !(got_in && acked) {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Event::Text { peer: p, id, text }) if p == peer => {
                messages.append(&peer, StoredMessage {
                    id, text, mine: false, time: now(), status: String::new(),
                }).unwrap();
                got_in = true;
            }
            Ok(Event::Ack { id, .. }) if id == id1 => {
                messages.set_status(&peer, id1, "delivered").unwrap();
                acked = true;
            }
            _ => {}
        }
    }

    println!("[{label}] got_in={got_in} acked={acked} data_dir={}", dir.display());
    if !(got_in && acked) {
        std::process::exit(1);
    }
}
