//! Интеграционные тесты DM-чата на ДВУХ полных движках поверх loopback (без
//! устройств): доставка, порядок, дедуп при рецикле id, офлайн-досылка через
//! перезапуск отправителя. Ровно те стыки, где падало на телефоне. Прогон из WSL:
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
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("test-chat-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

/// Собрать до `want` НОВЫХ text-событий из очереди движка (или до таймаута).
fn poll_texts(engine: &Engine, want: usize, timeout: Duration) -> Vec<String> {
    let mut texts = Vec::new();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline && texts.len() < want {
        while let Some(js) = engine.poll_event() {
            let v: serde_json::Value = serde_json::from_str(&js).unwrap();
            if v["type"] == "message" && v["msg"]["kind"] == "text" {
                texts.push(v["msg"]["text"].as_str().unwrap_or("").to_string());
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    texts
}

/// Базовая доставка: A→B три сообщения приходят в порядке 1,2,3.
#[test]
fn basic_delivery_in_order() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(1), id(2));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    a.send_text(&b_key, "1");
    a.send_text(&b_key, "2");
    a.send_text(&b_key, "3");
    let texts = poll_texts(&b, 3, Duration::from_secs(6));
    a.shutdown();
    b.shutdown();
    assert_eq!(texts, vec!["1", "2", "3"], "порядок сохранён");
}

/// РЕГРЕССИЯ (баг «не показывается после рестарта»): отправитель шлёт, ПЕРЕЗАПУСКАЕТСЯ,
/// шлёт снова — получатель видит ОБА (id сеются от времени, не рециклятся → нет
/// ложного дедупа/затирания по (peer,id)).
#[test]
fn sender_restart_no_false_dedup() {
    let net = LoopbackNet::new();
    let a_dir = tmpdir("a");
    let (a_id, b_id) = (id(3), id(4));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let b = start(&net, b_id, tmpdir("b"));
    b.add_contact(&a_key, "A");

    let a1 = start(&net, a_id.clone(), a_dir.clone());
    a1.add_contact(&b_key, "B");
    a1.send_text(&b_key, "first");
    assert_eq!(poll_texts(&b, 1, Duration::from_secs(6)), vec!["first"]);

    // Перезапуск отправителя: гасим и поднимаем на том же каталоге (тот же id-ключ).
    a1.shutdown();
    drop(a1);
    let a2 = start(&net, a_id, a_dir);
    a2.send_text(&b_key, "second");

    // «second» ДОЛЖЕН прийти (без фикса id-рецикла его бы отбросили как дубль id=1).
    let texts = poll_texts(&b, 1, Duration::from_secs(6));
    a2.shutdown();
    b.shutdown();
    assert_eq!(texts, vec!["second"], "второе сообщение после рестарта показано");
}

/// Сессии 1:1 живут только в памяти (ADR-012): ПОЛУЧАТЕЛЬ перезапустился, а у
/// отправителя ещё живая сессия, которой на том конце больше нет. Первое
/// сообщение уходит в пустоту, но связь обязана восстановиться сама — получатель
/// видит пакет с незнакомым индексом и начинает новое рукопожатие, ретрай
/// отправителя доставляет. Без этого чат молчал бы до смены ключей (2 мин).
#[test]
fn receiver_restart_mid_session_recovers() {
    let net = LoopbackNet::new();
    let b_dir = tmpdir("b");
    let (a_id, b_id) = (id(9), id(10));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    a.add_contact(&b_key, "B");
    let b1 = start(&net, b_id.clone(), b_dir.clone());
    b1.add_contact(&a_key, "A");
    a.send_text(&b_key, "до");
    assert_eq!(poll_texts(&b1, 1, Duration::from_secs(6)), vec!["до"]);

    b1.shutdown();
    drop(b1);
    let b2 = start(&net, b_id, b_dir);
    a.send_text(&b_key, "после");
    let texts = poll_texts(&b2, 1, Duration::from_secs(10));
    a.shutdown();
    b2.shutdown();
    assert_eq!(texts, vec!["после"], "после перезапуска получателя связь восстановилась");
}

/// Честный статус: пока сообщение ждёт доставки, оно НЕ помечается как ошибка.
/// Раньше по истечении in-memory ретраев в БД писалось "failed", и после
/// перезапуска ждущее в очереди сообщение показывалось красным «!».
#[test]
fn undelivered_message_is_queued_not_failed() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(7), id(8));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    // Короткий message_ttl: штатные 5 минут в тесте не подождать.
    let cfg = peerage_messenger::Config {
        message_ttl: Duration::from_secs(2),
        retry_interval: Duration::from_millis(200),
        ..peerage_messenger::Config::default()
    };
    let a = Engine::start_with_transport_cfg(tmpdir("a"), a_id, net.endpoint(a_key), cfg)
        .unwrap()
        .engine;
    a.add_contact(&b_key, "B"); // B вообще не поднимаем — доставки не будет

    let msg_id = a.send_text(&b_key, "в пустоту").expect("поставлено в очередь");
    let status_of = |e: &Engine| -> String {
        let h: serde_json::Value = serde_json::from_str(&e.history_json(&b_key)).unwrap();
        h.as_array()
            .and_then(|arr| arr.iter().find(|m| m["id"].as_i64() == Some(msg_id)))
            .and_then(|m| m["status"].as_str().map(str::to_string))
            .unwrap_or_default()
    };

    // Ждём, пока in-memory ретраи выдохнутся (message_ttl) и придёт Undelivered.
    // Изначально статус "sent" (отдано в сеть, подтверждения нет).
    assert_eq!(status_of(&a), "sent", "сразу после отправки — ✓, не ✓✓");
    let deadline = Instant::now() + Duration::from_secs(20);
    while status_of(&a) == "sent" && Instant::now() < deadline {
        while a.poll_event().is_some() {}
        std::thread::sleep(Duration::from_millis(50));
    }

    let st = status_of(&a);
    assert_ne!(st, "failed", "ждущее в очереди сообщение — не ошибка");
    assert_eq!(st, "queued", "после выдохшихся ретраев — «ждёт в очереди»");
    a.shutdown();
    let _ = a_key;
}

/// РЕГРЕССИЯ (баг «не пережила рестарт»): A шлёт, пока B офлайн; A перезапускается
/// (in-memory pending теряется, остаётся только персистентный outbox); B возвращается
/// → сообщение догоняется из персистентной очереди мессенджера.
#[test]
fn offline_message_survives_sender_restart() {
    let net = LoopbackNet::new();
    let a_dir = tmpdir("a");
    let b_dir = tmpdir("b");
    let (a_id, b_id) = (id(5), id(6));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());

    // A и B онлайн, знакомимся.
    let a1 = start(&net, a_id.clone(), a_dir.clone());
    let b1 = start(&net, b_id.clone(), b_dir.clone());
    a1.add_contact(&b_key, "B");
    b1.add_contact(&a_key, "A");
    a1.send_text(&b_key, "warmup");
    assert_eq!(poll_texts(&b1, 1, Duration::from_secs(6)), vec!["warmup"]);

    // B уходит офлайн.
    b1.shutdown();
    drop(b1);
    std::thread::sleep(Duration::from_millis(100));

    // A шлёт офлайн-сообщение → ложится в outbox (и in-memory pending).
    a1.send_text(&b_key, "queued");
    std::thread::sleep(Duration::from_millis(100));

    // A перезапускается: in-memory pending теряется, outbox персистентен (SQLite).
    a1.shutdown();
    drop(a1);

    // B возвращается ПЕРВЫМ (чтобы досылка on_start сразу попала в живой эндпоинт).
    // ВАЖНО: на СВОЁМ каталоге — иначе это «новый B», для которого A незнакомец, и
    // догнанное сообщение уйдёт в гейт «запрос в контакты», а не в чат (см. gate.rs).
    let b2 = start(&net, b_id, b_dir);
    let a2 = start(&net, a_id, a_dir); // держим A2 живым (иначе досылка не уйдёт)

    let texts = poll_texts(&b2, 1, Duration::from_secs(12));
    a2.shutdown();
    b2.shutdown();
    assert_eq!(texts, vec!["queued"], "офлайн-сообщение догналось после рестарта A");
}
