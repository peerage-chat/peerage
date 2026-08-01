//! Приём смены адреса контактом (ADR-007, точки 2–4): личность прежняя, адрес новый.
//!
//! Проверяем ровно приёмную сторону — она уже полностью собрана: подпись под
//! объявлением, анти-спуфинг с разделёнными ключами, запись в стор, обновление
//! карты адресов. Сама команда «сменить свой адрес» (перезапуск узла с паузой
//! офлайн и выборочной рассылкой) — следующий шаг, здесь её нет.
//!
//! Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_engine::Engine;
use p2p_messenger::loopback::LoopbackNet;
use p2p_messenger::{Config, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-rotate-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn wait(secs: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

/// Транспортный ключ, записанный у B для контакта A (пусто — эпоха 0).
fn transport_of(b: &Engine, a_key: &[u8; 32]) -> String {
    let v: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == hex::encode(a_key))
        .and_then(|c| c["transport_key"].as_str().map(str::to_string))
        .unwrap_or_default()
}

#[test]
fn contact_rotation_moves_address_but_keeps_identity() {
    let net = LoopbackNet::new();
    let alice = id(61); // сменит адрес
    let bob_id = id(62);
    let (a_key, b_key) = (alice.public_key_bytes(), bob_id.public_key_bytes());

    // B — обычный движок; A — «сырой» мессенджер, чтобы слать объявление руками
    // с НОВОГО адреса (движок этого пока не умеет — это следующий шаг).
    let b = Engine::start_with_transport(tmpdir("b"), bob_id, net.endpoint(b_key)).unwrap().engine;
    b.add_contact(&a_key, "Алиса");

    // Эпоха 0: адрес совпадает с личностью.
    assert_eq!(transport_of(&b, &a_key), "", "по умолчанию адрес = личность");

    // A поднимается на транспортном ключе эпохи 1 — то есть на ДРУГОМ адресе,
    // сохраняя ту же личность.
    let tk1 = alice.transport_public(1);
    let (a_msgr, _a_rx) = Messenger::start_with_transport(
        alice.clone(),
        net.endpoint(tk1),
        Config::default(),
    );

    // Объявление подписано ключом личности и приходит с нового адреса: получатель
    // не знает эту связку заранее, её доказывает подпись.
    let sig = alice.sign_transport_rotate(1, &tk1);
    a_msgr.send_transport_rotate(&b_key, 1, &tk1, &sig).expect("объявление ушло");

    assert!(
        wait(15, || transport_of(&b, &a_key) == hex::encode(tk1)),
        "B должен принять новый адрес, получено {:?}",
        transport_of(&b, &a_key),
    );

    // Личность не тронута: контакт тот же, верификация и имя на месте.
    let v: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    let c = v.as_array().unwrap().iter().find(|c| c["key"] == hex::encode(a_key)).unwrap();
    assert_eq!(c["name"], "Алиса", "смена адреса не трогает контакт");
    assert_eq!(c["transport_epoch"], 1);

    b.shutdown();
}

#[test]
fn forged_rotation_from_another_identity_is_ignored() {
    let net = LoopbackNet::new();
    let alice = id(63);
    let mallory = id(64);
    let bob_id = id(65);
    let (a_key, m_key, b_key) =
        (alice.public_key_bytes(), mallory.public_key_bytes(), bob_id.public_key_bytes());

    let b = Engine::start_with_transport(tmpdir("b2"), bob_id, net.endpoint(b_key)).unwrap().engine;
    b.add_contact(&a_key, "Алиса");

    // Мэллори объявляет СВОЙ адрес от СВОЕЙ личности — B знает Алису, но не её;
    // для него это объявление постороннего и применяться не должно.
    let m_tk = mallory.transport_public(1);
    let (m_msgr, _rx) =
        Messenger::start_with_transport(mallory.clone(), net.endpoint(m_tk), Config::default());
    let sig = mallory.sign_transport_rotate(1, &m_tk);
    let _ = m_msgr.send_transport_rotate(&b_key, 1, &m_tk, &sig);

    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(transport_of(&b, &a_key), "", "адрес Алисы не подменён");
    let v: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    assert!(
        !v.as_array().unwrap().iter().any(|c| c["key"] == hex::encode(m_key)),
        "посторонний не должен появиться в контактах",
    );

    b.shutdown();
}
