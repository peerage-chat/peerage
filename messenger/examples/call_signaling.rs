//! Проверка сигнализации звонка: A приглашает → B принимает → A завершает.
//! Чисто сигнальный поток (без медиа), по надёжному каналу через Yggdrasil.
//!
//! Запуск (WSL): cargo run -p peerage-messenger --example call_signaling

use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use peerage_core::Identity;
use peerage_messenger::{Event, Messenger};
use peerage_ygg::Node;

fn wait<F: Fn(&Event) -> bool>(rx: &Receiver<Event>, pred: F, t: Duration) -> bool {
    let dl = Instant::now() + t;
    loop {
        let Some(left) = dl.checked_duration_since(Instant::now()) else { return false };
        match rx.recv_timeout(left) {
            Ok(e) if pred(&e) => return true,
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
    let uri = node_a.listen("tls://127.0.0.1:0").unwrap();
    node_b.add_peer(&uri).unwrap();
    let t0 = Instant::now();
    while !(node_a.tree_count() > 1 && node_b.tree_count() > 1) {
        if t0.elapsed() > Duration::from_secs(10) {
            eprintln!("✗ нет дерева");
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(3));

    let (a, rx_a) = Messenger::start(id_a, node_a);
    let (b, rx_b) = Messenger::start(id_b, node_b);
    let a = Arc::new(a);
    thread::scope(|s| {
        let ha = s.spawn(|| a.connect(&pk_b, Duration::from_secs(20)));
        b.connect(&pk_a, Duration::from_secs(20)).unwrap();
        ha.join().unwrap().unwrap();
    });

    let call_id = 0xC0FFEE;
    a.send_call_invite(&pk_b, call_id, true, false).unwrap();
    let invited = wait(
        &rx_b,
        |e| matches!(e, Event::CallInvite { peer, call_id: c, audio, .. } if *peer == pk_a && *c == call_id && *audio),
        Duration::from_secs(10),
    );
    println!("B получил приглашение: {}", if invited { "✓" } else { "✗" });

    b.send_call_accept(&pk_a, call_id).unwrap();
    let accepted = wait(
        &rx_a,
        |e| matches!(e, Event::CallAccept { peer, call_id: c } if *peer == pk_b && *c == call_id),
        Duration::from_secs(10),
    );
    println!("A получил приём: {}", if accepted { "✓" } else { "✗" });

    a.send_call_end(&pk_b, call_id).unwrap();
    let ended = wait(
        &rx_b,
        |e| matches!(e, Event::CallEnd { peer, call_id: c } if *peer == pk_a && *c == call_id),
        Duration::from_secs(10),
    );
    println!("B получил завершение: {}", if ended { "✓" } else { "✗" });

    if invited && accepted && ended {
        println!("\n✓✓ Сигнализация звонка работает (invite → accept → end)");
    } else {
        std::process::exit(1);
    }
}
