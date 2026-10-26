//! Настраиваемый узел чата для реальной сети.
//!
//!   chat key <seed_hex>
//!       — напечатать публичный ключ для seed и выйти (для оркестрации).
//!
//!   chat run <seed_hex> <peer_pub_hex> <peers_file> <label>
//!       — поднять узел, подключиться к публичным пирам, установить сессию с
//!         контактом и обменяться сообщениями ЧЕРЕЗ ГЛОБАЛЬНУЮ СЕТЬ (без прямого
//!         пиринга между собой). Завершается успехом, если получил текст пира.
//!
//! Запуск (WSL): cargo run -p peerage-messenger --example chat -- run <...>

use std::time::{Duration, Instant};

use peerage_core::Identity;
use peerage_messenger::{Event, Messenger};
use peerage_ygg::Node;

fn parse_seed(hex_str: &str) -> [u8; 32] {
    let bytes = hex::decode(hex_str).expect("seed: неверный hex");
    bytes.as_slice().try_into().expect("seed: нужно 32 байта")
}

fn parse_key(hex_str: &str) -> [u8; 32] {
    let bytes = hex::decode(hex_str).expect("ключ: неверный hex");
    bytes.as_slice().try_into().expect("ключ: нужно 32 байта")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("");

    if mode == "key" {
        let id = Identity::from_seed(&parse_seed(&args[2])).unwrap();
        println!("{}", hex::encode(id.public_key_bytes()));
        return;
    }
    if mode != "run" {
        eprintln!("usage: chat key <seed_hex> | chat run <seed_hex> <peer_pub_hex> <peers_file> <label>");
        std::process::exit(2);
    }

    let seed = parse_seed(&args[2]);
    let peer = parse_key(&args[3]);
    let peers_file = &args[4];
    let label = &args[5];

    let id = Identity::from_seed(&seed).unwrap();
    let node = Node::start(&seed).expect("старт узла");
    println!("[{label}] адрес {} pk {}", node.address(), hex::encode(id.public_key_bytes()));

    // Подключаемся к публичным пирам и ждём входа в сеть.
    let content = std::fs::read_to_string(peers_file).expect("файл пиров");
    for uri in content.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let _ = node.add_peer(uri);
    }
    let t = Instant::now();
    while !(node.peer_count() >= 1 && node.tree_count() > 2) {
        if t.elapsed() > Duration::from_secs(30) {
            eprintln!("[{label}] ✗ не вошли в сеть");
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    println!("[{label}] ✓ в глобальной сети (пиров {}, дерево {})", node.peer_count(), node.tree_count());

    let (m, rx) = Messenger::start(id, node);

    // Рукопожатие с контактом через глобальный mesh (двунаправленный Hello).
    match m.connect(&peer, Duration::from_secs(45)) {
        Ok(()) => println!("[{label}] ✓ сессия с контактом установлена"),
        Err(e) => {
            eprintln!("[{label}] ✗ рукопожатие не удалось: {e}");
            std::process::exit(1);
        }
    }

    // Отправляем приветствие; повторяем пока не получим Ack.
    let text = format!("привет от {label} через глобальную сеть");
    let mut msg_id = m.send_text(&peer, &text).unwrap();
    let mut acked = false;
    let mut got_peer_text = false;
    let mut last_send = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(40);

    while Instant::now() < deadline {
        if !acked && last_send.elapsed() > Duration::from_secs(3) {
            msg_id = m.send_text(&peer, &text).unwrap();
            last_send = Instant::now();
        }
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Event::Text { peer: p, id, text }) if p == peer => {
                println!("[{label}] 📥 от контакта [#{id}]: {text:?}");
                got_peer_text = true;
            }
            Ok(Event::Ack { id, .. }) if id == msg_id => {
                if !acked {
                    println!("[{label}] ✓✓ наше сообщение доставлено (Ack)");
                }
                acked = true;
            }
            Ok(_) => {}
            Err(_) => {}
        }
        if got_peer_text && acked {
            break;
        }
    }

    if got_peer_text {
        println!("[{label}] ✓ ОБМЕН СОСТОЯЛСЯ через глобальную сеть");
    } else {
        eprintln!("[{label}] ✗ не получили сообщение от контакта");
        std::process::exit(1);
    }
}
