//! Гейт контактов (замена TOFU-автодобавления): первое сообщение незнакомца НЕ
//! сыплется в чат, а ждёт одобрения как «запрос в контакты». Accept делает его
//! контактом, Decline тихо убирает, Block глушит будущие сообщения. Весь путь на
//! loopback без устройств. Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

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
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("test-gate-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

/// Есть ли у `e` контакт с ключом `hex`?
fn has_contact(e: &Engine, hex: &str) -> bool {
    let v: serde_json::Value = serde_json::from_str(&e.contacts_json()).unwrap();
    v.as_array().map(|a| a.iter().any(|c| c["key"] == hex)).unwrap_or(false)
}

/// Найти запрос по ключу и вернуть его превью (или None).
fn request_preview(e: &Engine, hex: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&e.contact_requests_json()).unwrap();
    v.as_array()?
        .iter()
        .find(|r| r["key"] == hex)
        .and_then(|r| r["preview"].as_str().map(str::to_string))
}

/// Дождаться, пока у `e` появится запрос от `hex`. Паника по таймауту.
fn await_request(e: &Engine, hex: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(p) = request_preview(e, hex) {
            return p;
        }
        assert!(Instant::now() < deadline, "запрос от {hex} не пришёл за 10с");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Первое сообщение незнакомца → «запрос в контакты», НЕ в чат и НЕ в контакты.
#[test]
fn stranger_text_becomes_request_not_chat() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(7), id(8));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B"); // A знает B; B про A НЕ знает
    a.send_text(&b_key, "привет незнакомец");

    let a_hex = hex::encode(a_key);
    assert_eq!(await_request(&b, &a_hex), "привет незнакомец", "превью = первое сообщение");
    assert!(!has_contact(&b, &a_hex), "незнакомец НЕ попадает в контакты сам");
    let hist: serde_json::Value = serde_json::from_str(&b.history_json(&a_key)).unwrap();
    assert_eq!(hist.as_array().unwrap().len(), 0, "сообщение незнакомца НЕ в чате");

    a.shutdown();
    b.shutdown();
}

/// Accept → незнакомец становится контактом с ПУСТЫМ локальным именем (чтобы в UI
/// показывалось его самоназвание, а не плейсхолдер — displayName: local→profile→hex).
/// Запрос при этом снимается.
#[test]
fn accept_makes_blank_named_contact() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(9), id(10));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    a.send_text(&b_key, "впустишь?");

    let a_hex = hex::encode(a_key);
    await_request(&b, &a_hex);
    assert!(b.accept_contact_request(&a_key), "accept успешен");

    let v: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    let c = v.as_array().unwrap().iter().find(|c| c["key"] == a_hex).expect("A теперь контакт");
    assert_eq!(c["name"], "", "локальное имя пустое → покажется самоназвание");
    assert!(request_preview(&b, &a_hex).is_none(), "запрос снят после accept");
    assert!(!b.accept_contact_request(&a_key), "повторный accept — уже нет запроса");

    // Первое сообщение не потерялось: A получил на него Ack и второй раз не пришлёт,
    // поэтому оно обязано лечь в чат при одобрении — с автором A, как входящее.
    let hist: serde_json::Value = serde_json::from_str(&b.history_json(&a_key)).unwrap();
    let hist = hist.as_array().unwrap();
    assert_eq!(hist.len(), 1, "первое сообщение в чате");
    assert_eq!(hist[0]["text"], "впустишь?");
    assert_eq!(hist[0]["sender"], a_hex);
    assert_eq!(hist[0]["mine"], false);

    a.shutdown();
    b.shutdown();
}

/// Decline тихо убирает запрос: ни контакта, ни повторного accept.
#[test]
fn decline_drops_request() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(11), id(12));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    a.send_text(&b_key, "спам?");

    let a_hex = hex::encode(a_key);
    await_request(&b, &a_hex);
    assert!(b.decline_contact_request(&a_key), "decline успешен");
    assert!(request_preview(&b, &a_hex).is_none(), "запрос снят");
    assert!(!has_contact(&b, &a_hex), "decline не создаёт контакт");

    a.shutdown();
    b.shutdown();
}

/// Block снимает запрос и глушит будущие сообщения (ни запроса, ни чата). Unblock
/// возвращает пира к обычному гейту — новое сообщение снова создаёт запрос.
#[test]
fn block_silences_then_unblock_restores() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(13), id(14));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    a.send_text(&b_key, "первый");

    let a_hex = hex::encode(a_key);
    await_request(&b, &a_hex);
    assert!(b.block_contact(&a_key), "block успешен");
    assert!(request_preview(&b, &a_hex).is_none(), "запрос снят блоком");
    assert!(!has_contact(&b, &a_hex), "заблокированный не контакт");

    // Заблокированный шлёт ещё — не должно быть ни запроса, ни чата.
    a.send_text(&b_key, "второй");
    std::thread::sleep(Duration::from_millis(500));
    assert!(request_preview(&b, &a_hex).is_none(), "сообщение блокнутого не создаёт запрос");
    let hist: serde_json::Value = serde_json::from_str(&b.history_json(&a_key)).unwrap();
    assert_eq!(hist.as_array().unwrap().len(), 0, "сообщение блокнутого не в чате");

    // Разблокировали → новое сообщение снова доходит как запрос.
    assert!(b.unblock_contact(&a_key), "unblock успешен");
    a.send_text(&b_key, "третий");
    assert_eq!(await_request(&b, &a_hex), "третий", "после unblock снова запрос");

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
    b.set_verified(&a_key, true);

    for _ in 0..100 {
        a_raw.write(&b_key, &[0x01; 64]).unwrap();
    }
    std::thread::sleep(Duration::from_millis(500));
    let flood: serde_json::Value = serde_json::from_str(&b.flood_json()).unwrap();
    assert!(
        !flood.as_array().unwrap().iter().any(|f| f["peer"] == hex::encode(a_key)),
        "сверенный контакт не режется как флуд: {flood}"
    );
}
