//! Сквозной тест чата: два узла в одном процессе устанавливают зашифрованную
//! сессию через настоящий Yggdrasil и обмениваются текстом с подтверждением.
//!
//! Запуск (WSL, после scripts/build-ygg.sh):
//!   cargo run -p peerage-messenger --example chat_loopback

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use peerage_core::Identity;
use peerage_messenger::{Event, Messenger};
use peerage_ygg::Node;

/// Дождаться текстового события от конкретного пира (пропуская Connected/Ack).
fn wait_text(rx: &Receiver<Event>, from: &[u8; 32], timeout: Duration) -> Option<(u64, String)> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        match rx.recv_timeout(left) {
            Ok(Event::Text { peer, id, text }) if &peer == from => return Some((id, text)),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

/// Дождаться Ack на сообщение с данным id.
fn wait_ack(rx: &Receiver<Event>, id: u64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        match rx.recv_timeout(left) {
            Ok(Event::Ack { id: got, .. }) if got == id => return true,
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}

fn main() {
    let id_a = Identity::from_seed(&[1u8; 32]).unwrap();
    let id_b = Identity::from_seed(&[2u8; 32]).unwrap();
    let pk_a = id_a.public_key_bytes();
    let pk_b = id_b.public_key_bytes();

    let node_a = Node::start(&id_a.seed()).unwrap();
    let node_b = Node::start(&id_b.seed()).unwrap();
    println!("A: {}", node_a.address());
    println!("B: {}", node_b.address());

    // Прямой пиринг по localhost + ожидание дерева маршрутизации.
    let uri = node_a.listen("tls://127.0.0.1:0").unwrap();
    node_b.add_peer(&uri).unwrap();
    let t = Instant::now();
    while !(node_a.tree_count() > 1 && node_b.tree_count() > 1) {
        if t.elapsed() > Duration::from_secs(10) {
            eprintln!("✗ дерево маршрутизации не сформировалось");
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(3)); // стабилизация

    let (a, rx_a) = Messenger::start(id_a, node_a);
    let (b, rx_b) = Messenger::start(id_b, node_b);

    // Обе стороны устанавливают сессию одновременно (двунаправленный Hello).
    std::thread::scope(|s| {
        let ha = s.spawn(|| a.connect(&pk_b, Duration::from_secs(20)));
        b.connect(&pk_a, Duration::from_secs(20)).expect("B connect");
        ha.join().unwrap().expect("A connect");
    });
    println!("✓ сессия установлена (handshake завершён)");

    // A → B: текст + ожидание Ack от B.
    let id1 = a.send_text(&pk_b, "привет, B!").unwrap();
    match wait_text(&rx_b, &pk_a, Duration::from_secs(10)) {
        Some((id, text)) => println!("✓ B получил от A [#{id}]: {text:?}"),
        None => {
            eprintln!("✗ B не получил сообщение");
            std::process::exit(1);
        }
    }
    if wait_ack(&rx_a, id1, Duration::from_secs(10)) {
        println!("✓ A получил подтверждение доставки (✓✓) для #{id1}");
    } else {
        eprintln!("✗ A не дождался Ack");
        std::process::exit(1);
    }

    // B → A: ответ.
    b.send_text(&pk_a, "и тебе привет, A!").unwrap();
    match wait_text(&rx_a, &pk_b, Duration::from_secs(10)) {
        Some((id, text)) => println!("✓ A получил от B [#{id}]: {text:?}"),
        None => {
            eprintln!("✗ A не получил ответ");
            std::process::exit(1);
        }
    }

    println!("\n✓✓ Зашифрованный P2P текст-чат работает сквозь Yggdrasil");
}
