//! Проверка лёгкого медиа-пути: два узла, поверх одного канала Yggdrasil идут
//! ОДНОВРЕМЕННО надёжный чат и реалтайм-медиа (демукс по байту канала).
//! A шлёт поток «голосовых» пакетов + один текст; B принимает медиа из bounded-
//! канала и текст из событий.
//!
//! Запуск (WSL): cargo run -p peerage-messenger --example media_loopback

use std::collections::HashSet;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use litertc::TRACK_AUDIO;
use peerage_core::Identity;
use peerage_messenger::{Event, MediaPacket, Messenger};
use peerage_ygg::Node;

const N: u16 = 200;

fn wait_chat(rx: &Receiver<Event>, from: &[u8; 32], timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        match rx.recv_timeout(left) {
            Ok(Event::Text { peer, text, .. }) if &peer == from => return Some(text),
            Ok(_) => continue,
            Err(_) => return None,
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
    let t = Instant::now();
    while !(node_a.tree_count() > 1 && node_b.tree_count() > 1) {
        if t.elapsed() > Duration::from_secs(10) {
            eprintln!("✗ нет дерева");
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(3));

    let (a, _rx_a) = Messenger::start(id_a, node_a);
    let (b, rx_b) = Messenger::start(id_b, node_b);
    let a = Arc::new(a);

    // Сессия (двунаправленный Hello).
    thread::scope(|s| {
        let ha = s.spawn(|| a.connect(&pk_b, Duration::from_secs(20)));
        b.connect(&pk_a, Duration::from_secs(20)).expect("B connect");
        ha.join().unwrap().expect("A connect");
    });
    println!("✓ сессия установлена");

    // B принимает медиа.
    let media_rx: Receiver<MediaPacket> = b.media_channel(512);
    let reader = thread::spawn(move || {
        let mut seen: HashSet<u16> = HashSet::new();
        let mut corrupt = 0u32;
        let deadline = Instant::now() + Duration::from_secs(15);
        while seen.len() < N as usize && Instant::now() < deadline {
            if let Ok(p) = media_rx.recv_timeout(Duration::from_millis(500)) {
                if p.track == TRACK_AUDIO && p.payload == format!("opus#{}", p.seq).into_bytes() {
                    seen.insert(p.seq);
                } else {
                    corrupt += 1;
                }
            }
        }
        (seen.len(), corrupt)
    });

    // Текст параллельно с медиа (проверка демукса).
    a.send_text(&pk_b, "текст идёт вместе с медиа").unwrap();

    // Поток «голоса»: N пакетов по 20мс (48кГц → 960 семплов/кадр).
    for seq in 0..N {
        let payload = format!("opus#{seq}");
        a.send_media(&pk_b, TRACK_AUDIO, seq, seq as u32 * 960, payload.as_bytes())
            .unwrap();
        thread::sleep(Duration::from_millis(5));
    }

    let (received, corrupt) = reader.join().unwrap();
    let chat = wait_chat(&rx_b, &pk_a, Duration::from_secs(5));

    println!("медиа: получено {received}/{N}, повреждено {corrupt}");
    println!("чат: {}", if chat.is_some() { "✓ доставлен параллельно с медиа" } else { "✗ не дошёл" });

    if received >= (N as usize * 95 / 100) && corrupt == 0 && chat.is_some() {
        println!("\n✓✓ Лёгкий медиа-путь работает (демукс чат+медиа, без Ack/крипто на пакет)");
    } else {
        eprintln!("\n✗ проверка не пройдена");
        std::process::exit(1);
    }
}
