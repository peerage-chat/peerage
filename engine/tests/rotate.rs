//! Смена адреса (ADR-007): личность прежняя, адрес новый.
//!
//! Покрыты обе стороны: приём чужого объявления (подпись, анти-спуфинг с
//! разделёнными ключами, запись в стор) и собственная ротация движком — гашение
//! узла, пауза офлайн, подъём на новом ключе, выборочная рассылка. Ключевая
//! проверка последней — что после смены адреса СВЯЗЬ С КОНТАКТОМ ЖИВА: иначе
//! ротация лечит флуд ценой потери всех собеседников.
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

/// Полный цикл своей ротации: движок гасит узел, поднимается на новом адресе,
/// оповещает выбранный контакт — и переписка с ним продолжается.
#[test]
fn own_rotation_keeps_the_contact_reachable() {
    let net = LoopbackNet::new();
    let alice = id(66);
    let bob_id = id(67);
    let (a_key, b_key) = (alice.public_key_bytes(), bob_id.public_key_bytes());

    let a = Engine::start_with_transport(tmpdir("a3"), alice.clone(), net.endpoint(a_key))
        .unwrap()
        .engine;
    let b = Engine::start_with_transport(tmpdir("b3"), bob_id, net.endpoint(b_key)).unwrap().engine;
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Алиса");

    // Ротация поднимает узел на новом ключе; на loopback это новая точка сети.
    let net_for_factory = net.clone();
    a.set_transport_factory(std::sync::Arc::new(move |seed: &[u8; 32]| {
        let key = p2p_core::Identity::from_seed(seed).ok()?.public_key_bytes();
        Some(net_for_factory.endpoint(key) as std::sync::Arc<dyn p2p_messenger::Transport>)
    }));

    assert_eq!(a.transport_epoch(), 0, "до ротации живём на ключе личности");
    // Пауза 0: здесь проверяется механизм, а не выпадение ключа из дерева — оно
    // свойство настоящего меша и измерено отдельным стендом (flood_pubmesh).
    a.rotate_transport(vec![b_key], 0);

    // Боб узнал новый адрес Алисы, и это адрес эпохи 1.
    let tk1 = alice.transport_public(1);
    assert!(
        wait(20, || transport_of(&b, &a_key) == hex::encode(tk1)),
        "Боб должен получить новый адрес, получено {:?}",
        transport_of(&b, &a_key),
    );
    assert!(wait(5, || a.transport_epoch() == 1), "эпоха сохранена на диске");

    // Главное: связь жива в обе стороны. Боб шлёт на НОВЫЙ адрес (он его знает),
    // Алиса отвечает — её сообщения тоже доходят.
    b.send_text(&a_key, "ты сменила адрес?");
    assert!(
        wait(20, || a.history_json(&b_key).contains("ты сменила адрес?")),
        "сообщение на новый адрес должно дойти: {}",
        a.history_json(&b_key),
    );
    a.send_text(&b_key, "да, флудера отрезала");
    assert!(
        wait(20, || b.history_json(&a_key).contains("флудера отрезала")),
        "ответ с нового адреса должен дойти: {}",
        b.history_json(&a_key),
    );

    a.shutdown();
    b.shutdown();
}

/// Заблокированный не должен выходить из-под блока сменой адреса: объявление от
/// него не применяется, а его сообщения с нового ключа не доходят.
#[test]
fn blocked_contact_cannot_escape_by_rotating() {
    let net = LoopbackNet::new();
    let mallory = id(68);
    let bob_id = id(69);
    let (m_key, b_key) = (mallory.public_key_bytes(), bob_id.public_key_bytes());

    let b = Engine::start_with_transport(tmpdir("b4"), bob_id, net.endpoint(b_key)).unwrap().engine;
    b.add_contact(&m_key, "Мэллори");
    b.block_contact(&m_key);

    // Мэллори «переезжает» на новый адрес и объявляет об этом.
    let tk1 = mallory.transport_public(1);
    let (m_msgr, _rx) =
        Messenger::start_with_transport(mallory.clone(), net.endpoint(tk1), Config::default());
    let sig = mallory.sign_transport_rotate(1, &tk1);
    let _ = m_msgr.send_transport_rotate(&b_key, 1, &tk1, &sig);
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(transport_of(&b, &m_key), "", "адрес заблокированного не записывается");

    // И само сообщение с нового адреса до приложения не доходит.
    let _ = m_msgr.send_text(&b_key, "я вернулся");
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !b.history_json(&m_key).contains("я вернулся"),
        "блок должен пережить смену адреса: {}",
        b.history_json(&m_key),
    );

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
