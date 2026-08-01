//! Чёрный список источников внутри libygg (ADR-007, п.1).
//!
//! Смысл: заблокированный пир режется приёмным бюджетом и так, но каждый его
//! пакет до бюджета стоит копирования через FFI и разбора заголовка. Здесь
//! проверяем, что после `block` пакеты вообще не поднимаются в Rust, а после
//! `unblock` доставка возвращается.
//!
//! Прогон из WSL (нужна libygg): `cargo test -p p2p-ygg`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use p2p_ygg::Node;

/// Поток-читатель: считает пакеты, дошедшие до приложения.
fn spawn_reader(node: Arc<Node>, stop: Arc<AtomicBool>) -> Arc<AtomicU64> {
    let got = Arc::new(AtomicU64::new(0));
    let counter = got.clone();
    thread::spawn(move || {
        let mut buf = vec![0u8; 65535];
        while !stop.load(Ordering::Relaxed) {
            match node.read(&mut buf) {
                Ok((n, _)) if n > 0 => {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
                _ => thread::sleep(Duration::from_millis(20)),
            }
        }
    });
    got
}

/// Слать `secs` секунд и вернуть, сколько пакетов принял получатель за это время.
fn burst(from: &Node, to_key: &[u8; 32], got: &AtomicU64, secs: u64) -> u64 {
    let before = got.load(Ordering::Relaxed);
    let payload = [0x41u8; 200];
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        let _ = from.write(to_key, &payload);
        thread::sleep(Duration::from_millis(20));
    }
    thread::sleep(Duration::from_millis(300)); // дать доехать хвосту
    got.load(Ordering::Relaxed) - before
}

#[test]
fn blocked_source_never_reaches_the_app() {
    let a = Arc::new(Node::start(&[0x7Au8; 32]).expect("старт A"));
    let b = Arc::new(Node::start(&[0x8Au8; 32]).expect("старт B"));
    let (a_key, b_key) = (a.public_key(), b.public_key());

    let uri = b.listen("tls://127.0.0.1:0").expect("B listen");
    a.add_peer(&uri).expect("A→B");
    let deadline = Instant::now() + Duration::from_secs(20);
    while (a.tree_count() < 2 || b.tree_count() < 2) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(2));

    let stop = Arc::new(AtomicBool::new(false));
    let got_b = spawn_reader(b.clone(), stop.clone());
    let got_a = spawn_reader(a.clone(), stop.clone());

    // Рукопожатие: сессия ironwood встаёт только при двустороннем обмене
    // (см. silent_victim.rs), иначе «до блокировки» не доставлялось бы ничего и
    // тест был бы зелёным по неверной причине.
    let probe = [0x55u8; 64];
    let deadline = Instant::now() + Duration::from_secs(45);
    while (got_a.load(Ordering::Relaxed) == 0 || got_b.load(Ordering::Relaxed) == 0)
        && Instant::now() < deadline
    {
        let _ = a.write(&b_key, &probe);
        let _ = b.write(&a_key, &probe);
        thread::sleep(Duration::from_millis(200));
    }
    assert!(got_b.load(Ordering::Relaxed) > 0, "сессия не встала — тест бессмыслен");

    // До блокировки доставка идёт.
    let before = burst(&a, &b_key, &got_b, 2);
    assert!(before > 10, "до блокировки пакеты должны доходить (дошло {before})");

    // Блокируем A на стороне B: пакеты не должны подниматься в приложение.
    b.block(&a_key);
    let dropped0 = b.dropped();
    let during = burst(&a, &b_key, &got_b, 2);
    assert_eq!(during, 0, "заблокированный источник не должен доходить (дошло {during})");
    assert!(
        b.dropped() > dropped0,
        "счётчик отброшенных должен расти (было {dropped0}, стало {})",
        b.dropped(),
    );

    // Снятие блокировки возвращает доставку.
    b.unblock(&a_key);
    let after = burst(&a, &b_key, &got_b, 2);
    assert!(after > 10, "после снятия блокировки доставка возвращается (дошло {after})");

    stop.store(true, Ordering::Relaxed);
    a.stop();
    b.stop();
}
