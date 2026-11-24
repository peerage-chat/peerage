//! Демонстрация персистентности и надёжности сессии (localhost, два узла):
//!   1. профиль и контакты сохраняются/перезагружаются с диска;
//!   2. presence: пир становится онлайн после рукопожатия;
//!   3. доставка с подтверждением (Ack), очередь повторов;
//!   4. обнаружение офлайна при остановке узла;
//!   5. сдача недоставленного сообщения по TTL (Undelivered).
//!
//! Запуск (WSL): cargo run -p peerage-messenger --example reliability

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::store::{ContactStore, Profile};
use peerage_messenger::{Config, Event, Messenger};
use peerage_ygg::Node;

fn tmp_dir() -> std::path::PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("test-reliability-{nanos}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Дождаться события по предикату (пропуская прочие). true если дождались.
fn wait_for<F: Fn(&Event) -> bool>(rx: &Receiver<Event>, pred: F, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        match rx.recv_timeout(left) {
            Ok(ev) if pred(&ev) => return true,
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}

fn main() {
    let dir = tmp_dir();

    // 1. Персистентность профиля: создать → перезагрузить → ключ стабилен.
    let pa = dir.join("a/profile.json");
    let id_a = Profile::load_or_create(&pa, peerage_core::record::NetKind::Yggdrasil).unwrap().identity;
    let id_a_reloaded = Profile::load_or_create(&pa, peerage_core::record::NetKind::Yggdrasil).unwrap().identity;
    assert_eq!(id_a.public_key_bytes(), id_a_reloaded.public_key_bytes());
    println!("✓ профиль сохранён и перезагружен (ключ стабилен)");

    let id_b = Profile::load_or_create(&dir.join("b/profile.json"), peerage_core::record::NetKind::Yggdrasil).unwrap().identity;
    let pk_a = id_a.public_key_bytes();
    let pk_b = id_b.public_key_bytes();

    // 1b. Контакты: TOFU-добавление, перезагрузка с диска.
    let contacts_path = dir.join("a/contacts.json");
    {
        let mut store = ContactStore::load(&contacts_path).unwrap();
        store.add(&pk_b, "Bob", 0).unwrap();
    }
    let store = ContactStore::load(&contacts_path).unwrap();
    let bob = store.get(&pk_b).expect("Bob сохранён");
    println!("✓ контакт сохранён: {} / fingerprint: {}", bob.name, bob.fingerprint);

    // 2. Транспорт: два узла по localhost.
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
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(3));

    // Короткие тайминги, чтобы наблюдать offline/TTL быстро.
    let cfg = Config {
        ping_interval: Duration::from_secs(1),
        offline_after: Duration::from_secs(3),
        retry_interval: Duration::from_secs(1),
        message_ttl: Duration::from_secs(5),
        ..Config::default()
    };
    let (a, rx_a) = Messenger::start_with_config(id_a, node_a, cfg.clone());
    let (b, rx_b) = Messenger::start_with_config(id_b, node_b, cfg);

    // 3. Рукопожатие + presence online.
    std::thread::scope(|s| {
        let ha = s.spawn(|| a.connect(&pk_b, Duration::from_secs(20)));
        b.connect(&pk_a, Duration::from_secs(20)).expect("B connect");
        ha.join().unwrap().expect("A connect");
    });
    assert!(a.is_online(&pk_b) && b.is_online(&pk_a));
    println!("✓ presence: оба узла онлайн после рукопожатия");

    // 4. Доставка с Ack (очередь повторов отрабатывает успех).
    let id1 = a.send_text(&pk_b, "сообщение с подтверждением").unwrap();
    assert!(
        wait_for(&rx_b, |e| matches!(e, Event::Text { id, .. } if *id == id1), Duration::from_secs(10)),
        "B должен получить текст"
    );
    assert!(
        wait_for(&rx_a, |e| matches!(e, Event::Ack { id, .. } if *id == id1), Duration::from_secs(10)),
        "A должен получить Ack"
    );
    println!("✓ доставка подтверждена (Ack) для #{id1}");

    // 5. Останавливаем B → A должен обнаружить офлайн.
    println!("… останавливаем узел B");
    b.shutdown();
    assert!(
        wait_for(
            &rx_a,
            |e| matches!(e, Event::Presence { peer, online: false, .. } if *peer == pk_b),
            Duration::from_secs(8),
        ),
        "A должен заметить, что B офлайн"
    );
    assert!(!a.is_online(&pk_b));
    println!("✓ presence: A обнаружил, что B ушёл в офлайн");

    // 6. Сообщение офлайн-пиру повторяется и сдаётся по TTL.
    let id2 = a.send_text(&pk_b, "это уже не дойдёт").unwrap();
    assert!(
        wait_for(
            &rx_a,
            |e| matches!(e, Event::Undelivered { id, .. } if *id == id2),
            Duration::from_secs(10),
        ),
        "A должен сдаться по TTL"
    );
    println!("✓ недоставленное сообщение #{id2} помечено Undelivered по TTL");

    a.shutdown();
    std::fs::remove_dir_all(&dir).ok();
    println!("\n✓✓ Персистентность и надёжность сессии работают");
}
