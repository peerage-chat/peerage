//! Гейт контактов (шаг 7i): писать незнакомцу можно только через запрос в контакты с
//! приветствием. Запрос ждёт одобрения карточкой; пока его не приняли, остальное ждёт в
//! очереди отправителя. Accept — приветствие и очередь в чат по порядку; Decline —
//! молча; Block глушит. Весь путь на loopback без устройств. Прогон из WSL:
//! LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::Engine;
use peerage_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-gate-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
}

/// Контакт `hex` у `e` (JSON) или None.
fn contact(e: &Engine, hex: &str) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(&e.contacts_json()).unwrap();
    v.as_array()?.iter().find(|c| c["key"] == hex).cloned()
}

fn has_contact(e: &Engine, hex: &str) -> bool {
    contact(e, hex).is_some()
}

/// Запрос от `hex` у `e` (JSON) или None.
fn request(e: &Engine, hex: &str) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(&e.contact_requests_json()).unwrap();
    v.as_array()?.iter().find(|r| r["key"] == hex).cloned()
}

/// Тексты чата `e` с `peer` по порядку.
fn texts(e: &Engine, peer: &[u8; 32]) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(&e.history_json(peer)).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap_or("").to_string())
        .collect()
}

fn wait(secs: u64, mut ok: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    ok()
}

/// Запрос несёт приветствие и имя; пока его не приняли, написанное следом ждёт у
/// отправителя. Принятие — приветствие и удержанное в чат по порядку, приветствие у
/// отправителя «доставлено», очередь выпущена.
#[test]
fn request_holds_queue_until_accepted() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(7), id(8));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.set_self_name("Алиса");
    assert_eq!(
        a.request_contact(&b.my_contact_code(), "Боб", "привет, это Алиса"),
        "sent"
    );
    a.send_text(&b_key, "раз");
    a.send_text(&b_key, "два");

    let a_hex = hex::encode(a_key);
    assert!(wait(10, || request(&b, &a_hex).is_some()), "запрос дошёл");
    let r = request(&b, &a_hex).unwrap();
    assert_eq!(
        (r["greeting"].as_str(), r["name"].as_str()),
        (Some("привет, это Алиса"), Some("Алиса"))
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !has_contact(&b, &a_hex),
        "незнакомец НЕ попадает в контакты сам"
    );
    assert!(
        texts(&b, &a_key).is_empty(),
        "до принятия в чате пусто: остальное ждёт у Алисы"
    );
    assert_eq!(contact(&a, &hex::encode(b_key)).unwrap()["awaiting"], true);

    assert!(b.accept_contact_request(&a_key));
    assert!(
        wait(10, || texts(&b, &a_key)
            == ["привет, это Алиса", "раз", "два"]),
        "приветствие и удержанное — по порядку: {:?}",
        texts(&b, &a_key)
    );
    assert_eq!(
        contact(&b, &a_hex).unwrap()["name"],
        "",
        "локальное имя пустое → покажется самоназвание"
    );
    assert!(request(&b, &a_hex).is_none(), "запрос снят");
    assert!(
        !b.accept_contact_request(&a_key),
        "повторный accept — запроса уже нет"
    );
    assert!(wait(10, || contact(&a, &hex::encode(b_key)).unwrap()
        ["awaiting"]
        == false));
    let greeting_status = || {
        let v: serde_json::Value = serde_json::from_str(&a.history_json(&b_key)).unwrap();
        v[0]["status"].as_str().unwrap_or("").to_string()
    };
    assert!(
        wait(10, || greeting_status() == "delivered"),
        "приветствие у Алисы доставлено"
    );

    a.shutdown();
    b.shutdown();
}

/// Без запроса незнакомцу не написать: сообщение не ложится ни в чат, ни в запросы.
#[test]
fn stranger_text_without_request_is_dropped() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(9), id(10));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B"); // в обход запроса
    a.send_text(&b_key, "привет незнакомец");
    std::thread::sleep(Duration::from_millis(800));
    assert!(request(&b, &hex::encode(a_key)).is_none());
    assert!(texts(&b, &a_key).is_empty());

    a.shutdown();
    b.shutdown();
}

/// Добавили друг друга встречно — запросы принимаются сами, приветствия в чатах.
#[test]
fn mutual_requests_accept_each_other() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(11), id(12));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    assert_eq!(
        a.request_contact(&b.my_contact_code(), "", "я Алиса"),
        "sent"
    );
    assert_eq!(b.request_contact(&a.my_contact_code(), "", "я Боб"), "sent");
    let (a_hex, b_hex) = (hex::encode(a_key), hex::encode(b_key));
    assert!(wait(10, || {
        contact(&a, &b_hex).is_some_and(|c| c["awaiting"] == false)
            && contact(&b, &a_hex).is_some_and(|c| c["awaiting"] == false)
    }));
    assert!(wait(10, || texts(&a, &b_key).contains(&"я Боб".to_string())));
    assert!(wait(10, || texts(&b, &a_key).contains(&"я Алиса".to_string())));
    assert!(
        request(&a, &b_hex).is_none() && request(&b, &a_hex).is_none(),
        "карточек нет"
    );

    a.shutdown();
    b.shutdown();
}

/// Decline — молча: отправитель так и ждёт, получатель ничего не шлёт и в чат не берёт.
#[test]
fn decline_is_silent() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(13), id(14));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.request_contact(&b.my_contact_code(), "", "можно?");
    a.send_text(&b_key, "ну пожалуйста");
    let a_hex = hex::encode(a_key);
    assert!(wait(10, || request(&b, &a_hex).is_some()));
    assert!(b.decline_contact_request(&a_key));
    std::thread::sleep(Duration::from_millis(800));
    assert!(request(&b, &a_hex).is_none() && !has_contact(&b, &a_hex));
    assert!(texts(&b, &a_key).is_empty());
    assert_eq!(
        contact(&a, &hex::encode(b_key)).unwrap()["awaiting"],
        true,
        "ответа нет — ждёт"
    );

    a.shutdown();
    b.shutdown();
}

/// Block снимает запрос и глушит пира; разблокировка снимает и запрет.
#[test]
fn block_drops_request_and_silences() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(15), id(16));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.request_contact(&b.my_contact_code(), "", "первый");
    let a_hex = hex::encode(a_key);
    assert!(wait(10, || request(&b, &a_hex).is_some()));
    assert!(b.block_contact(&a_key));
    assert!(request(&b, &a_hex).is_none() && !has_contact(&b, &a_hex));
    a.send_text(&b_key, "второй");
    std::thread::sleep(Duration::from_millis(500));
    assert!(request(&b, &a_hex).is_none() && texts(&b, &a_key).is_empty());
    assert!(b.unblock_contact(&a_key));

    a.shutdown();
    b.shutdown();
}

/// Код обмена разбирается до решения: свой, уже знакомый, чужой мусор.
#[test]
fn inspect_code_tells_what_it_is() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(17), id(18));
    let b_key = b_id.public_key_bytes();
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    b.set_self_name("Боб");
    let st =
        |code: &str| -> serde_json::Value { serde_json::from_str(&a.inspect_code(code)).unwrap() };
    let ok = st(&b.my_contact_code());
    assert_eq!(
        (ok["status"].as_str(), ok["name"].as_str()),
        (Some("ok"), Some("Боб"))
    );
    assert_eq!(ok["key"], hex::encode(b_key));
    assert_eq!(st("https://example.com")["status"], "invalid");
    assert_eq!(st(&a.my_contact_code())["status"], "self");
    assert_eq!(a.request_contact(&a.my_contact_code(), "", ""), "self");
    a.request_contact(&b.my_contact_code(), "", "");
    assert_eq!(st(&b.my_contact_code())["status"], "known");

    a.shutdown();
    b.shutdown();
}

/// R-031: сверка отпечатка поднимает бюджет сразу. Сверенному вдвое больший
/// всплеск (120 против 60): сотня пакетов подряд проходит приёмный бюджет без
/// дропа. Раньше тир обновлялся лишь при следующей смене доверия, и часть сотни
/// резалась как флуд. Пакеты шлём сырыми с эндпоинта A — бюджет режет до
/// расшифровки, так что содержимое неважно, а темп не зависит от подтверждений.
#[test]
fn verifying_a_contact_raises_its_budget_at_once() {
    use peerage_messenger::Transport;
    let net = LoopbackNet::new();
    let (a_key, b_id) = (id(91).public_key_bytes(), id(92));
    let b_key = b_id.public_key_bytes();
    let a_raw = net.endpoint(a_key);
    let b = start(&net, b_id, tmpdir("b-verify"));
    b.add_contact(&a_key, "A");
    b.set_verified(&a_key, Some(peerage_core::store::VerifyHow::Set));

    for _ in 0..100 {
        a_raw.write(&b_key, &[0x01; 64]).unwrap();
    }
    std::thread::sleep(Duration::from_millis(500));
    let flood: serde_json::Value = serde_json::from_str(&b.flood_json()).unwrap();
    assert!(
        !flood
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["peer"] == hex::encode(a_key)),
        "сверенный контакт не режется как флуд: {flood}"
    );
}
