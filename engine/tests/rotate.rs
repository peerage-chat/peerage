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

use peerage_core::Identity;
use peerage_engine::Engine;
use peerage_messenger::loopback::LoopbackNet;
use peerage_messenger::{Config, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-rotate-{tag}-{n}"));
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
    let b = Engine::start_with_transport(tmpdir("b"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    b.add_contact(&a_key, "Алиса");

    // Эпоха 0: адрес совпадает с личностью.
    assert_eq!(
        transport_of(&b, &a_key),
        "",
        "по умолчанию адрес = личность"
    );

    // A поднимается на транспортном ключе эпохи 1 — то есть на ДРУГОМ адресе,
    // сохраняя ту же личность.
    let tk1 = alice.transport_public(1);
    let (a_msgr, _a_rx) =
        Messenger::start_with_transport(alice.clone(), net.endpoint(tk1), Config::default());

    // Объявление подписано ключом личности и приходит с нового адреса: получатель
    // не знает эту связку заранее, её доказывает подпись.
    a_msgr
        .send_transport_rotate(&b_key, &alice.address_record(1))
        .expect("объявление ушло");

    assert!(
        wait(15, || transport_of(&b, &a_key) == hex::encode(tk1)),
        "B должен принять новый адрес, получено {:?}",
        transport_of(&b, &a_key),
    );

    // Личность не тронута: контакт тот же, верификация и имя на месте.
    let v: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    let c = v
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == hex::encode(a_key))
        .unwrap();
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
    let b = Engine::start_with_transport(tmpdir("b3"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Алиса");

    // Ротация поднимает узел на новом ключе; на loopback это новая точка сети.
    let net_for_factory = net.clone();
    a.set_transport_factory(std::sync::Arc::new(move |seed: &[u8; 32]| {
        let key = peerage_core::Identity::from_seed(seed)
            .ok()?
            .public_key_bytes();
        Some(net_for_factory.endpoint(key) as std::sync::Arc<dyn peerage_messenger::Transport>)
    }));

    assert_eq!(a.transport_epoch(), 0, "до ротации живём на ключе личности");
    assert_eq!(
        a.rotation_progress_json(),
        r#"{"told":0,"total":0}"#,
        "смены ещё не было"
    );
    // Пауза 0: здесь проверяется механизм, а не выпадение ключа из дерева — оно
    // свойство настоящего меша и измерено отдельным стендом (flood_pubmesh).
    a.rotate_transport(vec![b_key], 0);

    // Эпоха — время смены (шаг 7g), а не счётчик: сохранена на диске.
    assert!(
        wait(20, || a.transport_epoch() > 0),
        "эпоха сохранена на диске"
    );
    let epoch = a.transport_epoch();
    assert!(
        i64::from(epoch) >= peerage_engine::now() - 60,
        "эпоха — секунды Unix: {epoch}"
    );
    // Боб узнал новый адрес Алисы — адрес этой эпохи.
    let tk1 = alice.transport_public(epoch);
    assert!(
        wait(20, || transport_of(&b, &a_key) == hex::encode(tk1)),
        "Боб должен получить новый адрес, получено {:?}",
        transport_of(&b, &a_key),
    );
    // Ход смены видит, кому объявление уже ушло (экран «Уведомлено 1 из 1»).
    assert!(
        wait(20, || a.rotation_progress_json()
            == r#"{"told":1,"total":1}"#),
        "Боб — в счёте уведомлённых: {}",
        a.rotation_progress_json(),
    );

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

    let b = Engine::start_with_transport(tmpdir("b4"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    b.add_contact(&m_key, "Мэллори");
    b.block_contact(&m_key);

    // Мэллори «переезжает» на новый адрес и объявляет об этом.
    let tk1 = mallory.transport_public(1);
    let (m_msgr, _rx) =
        Messenger::start_with_transport(mallory.clone(), net.endpoint(tk1), Config::default());
    let _ = m_msgr.send_transport_rotate(&b_key, &mallory.address_record(1));
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        transport_of(&b, &m_key),
        "",
        "адрес заблокированного не записывается"
    );

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
    let (a_key, m_key, b_key) = (
        alice.public_key_bytes(),
        mallory.public_key_bytes(),
        bob_id.public_key_bytes(),
    );

    let b = Engine::start_with_transport(tmpdir("b2"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    b.add_contact(&a_key, "Алиса");

    // Мэллори объявляет СВОЙ адрес от СВОЕЙ личности — B знает Алису, но не её;
    // для него это объявление постороннего и применяться не должно.
    let m_tk = mallory.transport_public(1);
    let (m_msgr, _rx) =
        Messenger::start_with_transport(mallory.clone(), net.endpoint(m_tk), Config::default());
    let _ = m_msgr.send_transport_rotate(&b_key, &mallory.address_record(1));

    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(transport_of(&b, &a_key), "", "адрес Алисы не подменён");
    let v: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    assert!(
        !v.as_array()
            .unwrap()
            .iter()
            .any(|c| c["key"] == hex::encode(m_key)),
        "посторонний не должен появиться в контактах",
    );

    b.shutdown();
}

/// Групповая переписка переживает смену адреса участником: MLS-ростер ведётся по
/// личностям, а отправка резолвит адрес — значит после ротации сообщения группы
/// продолжают доходить, ничего пересоздавать не надо.
#[test]
fn group_survives_member_rotation() {
    let net = LoopbackNet::new();
    let alice = id(70);
    let bob_id = id(71);
    let (a_key, b_key) = (alice.public_key_bytes(), bob_id.public_key_bytes());

    let a = Engine::start_with_transport(tmpdir("a5"), alice.clone(), net.endpoint(a_key))
        .unwrap()
        .engine;
    let b = Engine::start_with_transport(tmpdir("b5"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Алиса");

    // Общая группа: A создаёт, зовёт B, B принимает.
    assert!(a.group_create("клуб"));
    assert!(wait(15, || !a
        .group_list_json()
        .trim_matches(['[', ']', ' '])
        .is_empty()));
    let list: serde_json::Value = serde_json::from_str(&a.group_list_json()).unwrap();
    let gid_hex = list.as_array().unwrap()[0]["gid"]
        .as_str()
        .unwrap()
        .to_string();
    let gid = hex::decode(&gid_hex).unwrap();
    a.group_invite(gid.clone(), b_key);
    assert!(
        wait(15, || b.group_invites_json().contains(&gid_hex)),
        "инвайт доехал"
    );
    b.accept_group_invite(gid.clone());
    assert!(
        wait(15, || a
            .group_members_json(&gid)
            .contains(&hex::encode(b_key))),
        "B в ростере"
    );

    // A меняет адрес и сообщает об этом B.
    let net_for_factory = net.clone();
    a.set_transport_factory(std::sync::Arc::new(move |seed: &[u8; 32]| {
        let key = peerage_core::Identity::from_seed(seed)
            .ok()?
            .public_key_bytes();
        Some(net_for_factory.endpoint(key) as std::sync::Arc<dyn peerage_messenger::Transport>)
    }));
    a.rotate_transport(vec![b_key], 0);
    assert!(wait(20, || a.transport_epoch() > 0), "A на новой эпохе");
    let tk1 = alice.transport_public(a.transport_epoch());
    assert!(
        wait(20, || transport_of(&b, &a_key) == hex::encode(tk1)),
        "B узнал новый адрес A",
    );

    // Групповые сообщения ходят в обе стороны — по новому адресу A.
    b.group_send_text(gid.clone(), "все на месте?".into());
    assert!(
        wait(20, || a.group_history_json(&gid).contains("все на месте?")),
        "групповое от B должно дойти на новый адрес A: {}",
        a.group_history_json(&gid),
    );
    a.group_send_text(gid.clone(), "да, я тут".into());
    assert!(
        wait(20, || b.group_history_json(&gid).contains("да, я тут")),
        "групповое от A с нового адреса должно дойти: {}",
        b.group_history_json(&gid),
    );

    a.shutdown();
    b.shutdown();
}

/// Код обмена (QR) после смены адреса несёт подписанную привязку: тот, кто добавит
/// нас по нему, сразу знает верный адрес и достучится с первого сообщения.
#[test]
fn contact_code_carries_signed_address() {
    let net = LoopbackNet::new();
    let alice = id(72);
    let carol_id = id(73);
    let (a_key, c_key) = (alice.public_key_bytes(), carol_id.public_key_bytes());

    let a = Engine::start_with_transport(tmpdir("a6"), alice.clone(), net.endpoint(a_key))
        .unwrap()
        .engine;
    let c = Engine::start_with_transport(tmpdir("c6"), carol_id, net.endpoint(c_key))
        .unwrap()
        .engine;

    // До ротации код — это просто hex личности (старые сканеры продолжают работать).
    assert_eq!(a.my_contact_code(), hex::encode(a_key));

    let net_for_factory = net.clone();
    a.set_transport_factory(std::sync::Arc::new(move |seed: &[u8; 32]| {
        let key = peerage_core::Identity::from_seed(seed)
            .ok()?
            .public_key_bytes();
        Some(net_for_factory.endpoint(key) as std::sync::Arc<dyn peerage_messenger::Transport>)
    }));
    a.rotate_transport(vec![], 0); // никого не оповещаем — знакомимся заново по коду
    assert!(wait(20, || a.transport_epoch() > 0));
    let epoch = a.transport_epoch();

    // Кэрол добавляет Алису по коду и сразу знает её текущий адрес.
    let code = a.my_contact_code();
    assert!(code.contains(':'), "после ротации код расширенный: {code}");
    assert_eq!(c.request_contact(&code, "Алиса", ""), "sent");
    assert_eq!(
        transport_of(&c, &a_key),
        hex::encode(alice.transport_public(epoch))
    );

    // Проверяем главное: сообщение доходит с первой попытки, без «а куда слать».
    // Алиса держит Кэрол в контактах — запрос Кэрол принимается сам и выпускает очередь.
    a.add_contact(&c_key, "Кэрол");
    c.send_text(&a_key, "привет, это Кэрол");
    assert!(
        wait(20, || a.history_json(&c_key).contains("привет, это Кэрол")),
        "по коду обмена должно достучаться: {}",
        a.history_json(&c_key),
    );

    a.shutdown();
    c.shutdown();
}

/// R-032: новый узел не поднялся — движок возвращается на прежний адрес, а не
/// остаётся офлайн навсегда, и говорит клиенту, что смена не удалась.
#[test]
fn failed_rotation_falls_back_to_the_old_address() {
    let net = LoopbackNet::new();
    let alice = id(68);
    let bob_id = id(69);
    let (a_key, b_key) = (alice.public_key_bytes(), bob_id.public_key_bytes());
    let a = Engine::start_with_transport(tmpdir("a-fail"), alice.clone(), net.endpoint(a_key))
        .unwrap()
        .engine;
    let b = Engine::start_with_transport(tmpdir("b-fail"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Алиса");

    // Сначала удачная смена: у Алисы — адрес эпохи E, и Боб его знает. Ключ личности
    // адресом не бывает (ADR-015 F1), «прежний адрес» — всегда адрес прошлой смены.
    let net_any = net.clone();
    a.set_transport_factory(std::sync::Arc::new(move |seed: &[u8; 32]| {
        let key = peerage_core::Identity::from_seed(seed)
            .ok()?
            .public_key_bytes();
        Some(net_any.endpoint(key) as std::sync::Arc<dyn peerage_messenger::Transport>)
    }));
    a.rotate_transport(vec![b_key], 0);
    assert!(
        wait(15, || !transport_of(&b, &a_key).is_empty()),
        "Боб узнал адрес эпохи E"
    );
    let e = a.transport_epoch();
    let old = alice.transport_public(e);

    // Узел новой эпохи не поднимается; прежний (эпохи E) — поднимается.
    let net_for_factory = net.clone();
    a.set_transport_factory(std::sync::Arc::new(move |seed: &[u8; 32]| {
        let key = peerage_core::Identity::from_seed(seed)
            .ok()?
            .public_key_bytes();
        (key == old).then(|| {
            net_for_factory.endpoint(key) as std::sync::Arc<dyn peerage_messenger::Transport>
        })
    }));
    a.rotate_transport(vec![b_key], 0);

    let mut failed = None;
    assert!(
        wait(10, || {
            while let Some(ev) = a.poll_event() {
                let v: serde_json::Value = serde_json::from_str(&ev).unwrap();
                if v["type"] == "transport_rotate_failed" {
                    failed = Some(v);
                }
            }
            failed.is_some()
        }),
        "клиент узнал, что смена не удалась"
    );
    assert_eq!(failed.unwrap()["online"], true);
    assert!(!a.is_offline(), "вернулись в сеть");
    assert_eq!(a.transport_epoch(), e, "адрес прежний");

    b.send_text(&a_key, "ты здесь?");
    assert!(
        wait(20, || a.history_json(&b_key).contains("ты здесь?")),
        "связь на прежнем адресе жива"
    );
}

/// Шаг 7g (R-118): Алиса сменила адрес, потеряла телефон и восстановилась по 24 словам
/// на новом. Прошлой эпохи новый телефон не знает — поднимается на «сейчас» и сообщает
/// адрес контакту при встрече; Боб принимает (эпоха больше прежней), и связь жива.
#[test]
fn restored_identity_announces_its_new_address() {
    let net = LoopbackNet::new();
    let alice = id(70);
    let bob_id = id(71);
    let (a_key, b_key) = (alice.public_key_bytes(), bob_id.public_key_bytes());
    let factory = |net: LoopbackNet| -> peerage_engine::TransportFactory {
        std::sync::Arc::new(move |seed: &[u8; 32]| {
            let key = peerage_core::Identity::from_seed(seed)
                .ok()?
                .public_key_bytes();
            Some(net.endpoint(key) as std::sync::Arc<dyn peerage_messenger::Transport>)
        })
    };

    let a = Engine::start_with_transport(tmpdir("a7"), alice.clone(), net.endpoint(a_key))
        .unwrap()
        .engine;
    let b = Engine::start_with_transport(tmpdir("b7"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Алиса");
    a.set_transport_factory(factory(net.clone()));
    a.rotate_transport(vec![b_key], 0);
    assert!(
        wait(20, || a.transport_epoch() > 0),
        "старый телефон сменил адрес"
    );
    let old = alice.transport_public(a.transport_epoch());
    assert!(
        wait(20, || transport_of(&b, &a_key) == hex::encode(old)),
        "Боб знает адрес"
    );
    a.shutdown(); // телефон потерян
                  // Эпоха — секунды: восстановление в ту же секунду дало бы ту же эпоху.
    std::thread::sleep(Duration::from_millis(1100));

    // Новый телефон: та же личность из 24 слов, пустой каталог.
    let dir = tmpdir("a7-new");
    let epoch = peerage_engine::begin_restored_epoch(&dir);
    let tk = alice.transport_public(epoch);
    let a2 = Engine::start_with_transport(dir, alice, net.endpoint(tk))
        .unwrap()
        .engine;
    a2.set_transport_factory(factory(net.clone()));
    assert_eq!(a2.transport_epoch(), epoch);
    a2.add_contact(&b_key, "Боб"); // из копии данных или заново по коду

    assert!(
        wait(30, || transport_of(&b, &a_key) == hex::encode(tk)),
        "Боб должен принять новый адрес, у него {:?}",
        transport_of(&b, &a_key),
    );
    b.send_text(&a_key, "это ты с нового телефона?");
    assert!(
        wait(20, || a2.history_json(&b_key).contains("с нового телефона")),
        "сообщение на восстановленный телефон дошло: {}",
        a2.history_json(&b_key),
    );
    a2.shutdown();
    b.shutdown();
}

/// Оба сменили адрес, не услышав друг друга (или объявление потерялось): связь
/// восстанавливается кодом — код известного контакта (добавление заново или сверка по
/// коду) несёт подписанную запись адреса, и она применяется, если эпоха новее.
#[test]
fn known_contacts_code_carries_its_new_address() {
    let net = LoopbackNet::new();
    let alice = id(72);
    let bob_id = id(73);
    let (a_key, b_key) = (alice.public_key_bytes(), bob_id.public_key_bytes());
    let a = Engine::start_with_transport(tmpdir("a-code"), alice.clone(), net.endpoint(a_key))
        .unwrap()
        .engine;
    let b = Engine::start_with_transport(tmpdir("b-code"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Алиса");

    // Алиса меняет адрес и никому не говорит.
    let net_any = net.clone();
    a.set_transport_factory(std::sync::Arc::new(move |seed: &[u8; 32]| {
        let key = peerage_core::Identity::from_seed(seed)
            .ok()?
            .public_key_bytes();
        Some(net_any.endpoint(key) as std::sync::Arc<dyn peerage_messenger::Transport>)
    }));
    a.rotate_transport(vec![], 0);
    assert!(wait(10, || a.transport_epoch() > 0 && !a.is_offline()));
    let new_addr = hex::encode(alice.transport_public(a.transport_epoch()));
    assert_eq!(transport_of(&b, &a_key), "", "Боб нового адреса не знает");

    // Боб сканирует её код как сверку — адрес применяется.
    assert_eq!(b.verify_by_code(&a_key, &a.my_contact_code()), "match");
    assert_eq!(transport_of(&b, &a_key), new_addr);

    // И экран «Добавить контакт» со сканом кода известного контакта — тоже.
    a.rotate_transport(vec![], 0);
    assert!(wait(10, || hex::encode(
        alice.transport_public(a.transport_epoch())
    ) != new_addr
        && !a.is_offline()));
    let seen: serde_json::Value =
        serde_json::from_str(&b.inspect_code(&a.my_contact_code())).unwrap();
    assert_eq!(seen["status"], "known");
    assert_eq!(
        transport_of(&b, &a_key),
        hex::encode(alice.transport_public(a.transport_epoch()))
    );

    b.send_text(&a_key, "нашёл тебя");
    assert!(
        wait(20, || a.history_json(&b_key).contains("нашёл тебя")),
        "связь жива"
    );
}

/// R-120: телефон восстановлен с часами позади прошлой смены адреса — его эпоха меньше
/// той, что знает Боб. Боб не молчит: отвечает отказом с подписанной записью Алисы, и
/// Алиса поднимает эпоху выше неё; связь жива без исправления часов.
#[test]
fn stale_announcement_is_answered_and_epoch_climbs_above() {
    let net = LoopbackNet::new();
    let alice = id(74);
    let bob_id = id(75);
    let (a_key, b_key) = (alice.public_key_bytes(), bob_id.public_key_bytes());
    let factory = |net: LoopbackNet| -> peerage_engine::TransportFactory {
        std::sync::Arc::new(move |seed: &[u8; 32]| {
            let key = peerage_core::Identity::from_seed(seed)
                .ok()?
                .public_key_bytes();
            Some(net.endpoint(key) as std::sync::Arc<dyn peerage_messenger::Transport>)
        })
    };
    let a = Engine::start_with_transport(tmpdir("a-stale"), alice.clone(), net.endpoint(a_key))
        .unwrap()
        .engine;
    let b = Engine::start_with_transport(tmpdir("b-stale"), bob_id, net.endpoint(b_key))
        .unwrap()
        .engine;
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Алиса");
    a.set_transport_factory(factory(net.clone()));
    a.rotate_transport(vec![b_key], 0);
    assert!(wait(20, || a.transport_epoch() > 0));
    let known = a.transport_epoch();
    assert!(wait(20, || transport_of(&b, &a_key)
        == hex::encode(alice.transport_public(known))));
    a.shutdown(); // телефон потерян

    // Новый телефон, часы на сутки позади прошлой смены.
    let dir = tmpdir("a-stale-new");
    let low = peerage_engine::begin_restored_epoch_at(&dir, i64::from(known) - 86_400);
    assert!(low < known);
    let a2 = Engine::start_with_transport(
        dir,
        alice.clone(),
        net.endpoint(alice.transport_public(low)),
    )
    .unwrap()
    .engine;
    a2.set_transport_factory(factory(net.clone()));
    a2.add_contact(&b_key, "Боб");

    assert!(
        wait(30, || a2.transport_epoch() > known),
        "отказ Боба поднял эпоху выше известной ему: {} ≤ {known}",
        a2.transport_epoch()
    );
    let now_addr = hex::encode(alice.transport_public(a2.transport_epoch()));
    assert!(
        wait(30, || transport_of(&b, &a_key) == now_addr),
        "Боб принял новый адрес"
    );
    b.send_text(&a_key, "часы отстают, а связь есть");
    assert!(wait(20, || a2.history_json(&b_key).contains("связь есть")));
    a2.shutdown();
    b.shutdown();
}
