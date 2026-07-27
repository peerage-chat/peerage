//! Интеграционные тесты DM-чата на ДВУХ полных движках поверх loopback (без
//! устройств): доставка, порядок, дедуп при рецикле id, офлайн-досылка через
//! перезапуск отправителя. Ровно те стыки, где падало на телефоне. Прогон из WSL:
//! LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_engine::Engine;
use p2p_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-chat-{tag}-{n}"));
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
            if v["type"] == "text" {
                texts.push(v["text"].as_str().unwrap_or("").to_string());
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

/// РЕГРЕССИЯ (баг «не пережила рестарт»): A шлёт, пока B офлайн; A перезапускается
/// (in-memory pending теряется, остаётся только персистентный outbox); B возвращается
/// → сообщение догоняется из outbox on_start.
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
