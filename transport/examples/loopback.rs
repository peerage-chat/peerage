//! Loopback-тест настоящего Yggdrasil через FFI: два узла в одном процессе
//! пирятся по localhost, узел A шлёт сообщение узлу B по его публичному ключу.
//!
//! Запуск (в WSL/Linux, после scripts/build-ygg.sh):
//!   cargo run -p p2p-ygg --example loopback
//!
//! Завершается с кодом 0 при успешной доставке, 1 — при таймауте.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use p2p_ygg::Node;

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn main() {
    let a = Arc::new(Node::start(&[1u8; 32]).expect("старт A"));
    let b = Arc::new(Node::start(&[2u8; 32]).expect("старт B"));

    println!("A: addr={} pk={}", a.address(), hexs(&a.public_key()));
    println!("B: addr={} pk={}", b.address(), hexs(&b.public_key()));

    let uri = b.listen("tls://127.0.0.1:0").expect("B listen");
    println!("B слушает на {uri}");
    a.add_peer(&uri).expect("A add_peer");

    // Ждём формирования дерева маршрутизации (как WaitConnected в тестах ygg).
    let conn_start = Instant::now();
    loop {
        let (ta, tb) = (a.tree_count(), b.tree_count());
        if ta > 1 && tb > 1 {
            println!(
                "сеть согласована за {:?}: peers A={} B={}, tree A={} B={}",
                conn_start.elapsed(),
                a.peer_count(),
                b.peer_count(),
                ta,
                tb
            );
            // небольшой запас, как в эталонном тесте (внутренняя стабилизация)
            thread::sleep(Duration::from_secs(3));
            break;
        }
        if conn_start.elapsed() > Duration::from_secs(10) {
            println!(
                "⚠ дерево не сформировалось за 10с: peers A={} B={}, tree A={} B={}",
                a.peer_count(),
                b.peer_count(),
                ta,
                tb
            );
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let a_pk = a.public_key();
    let b_pk = b.public_key();
    // Сообщение помечено префиксом "A:", чтобы отличать данные от встречных зондов.
    let msg: &[u8] = b"A: hello over yggdrasil";

    // Зашифрованная сессия ironwood устанавливается только при ДВУНАПРАВЛЕННОМ
    // обмене: обе стороны должны слать, иначе рукопожатие не завершается и пакеты
    // теряются. Поэтому оба узла читают и оба шлют зонды до доставки.
    let (tx, rx) = mpsc::channel();

    // B ждёт именно сообщение от A (префикс "A:").
    let b_reader = b.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match b_reader.read(&mut buf) {
                Ok((n, src)) if n >= 2 && &buf[..2] == b"A:" => {
                    let _ = tx.send((buf[..n].to_vec(), src));
                    return;
                }
                Ok(_) => continue,    // встречный зонд от B самому себе/прочее
                Err(_) => return,     // узел остановлен
            }
        }
    });
    // A тоже читает — иначе входящие зонды от B не обрабатываются.
    let a_reader = a.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while a_reader.read(&mut buf).is_ok() {}
    });

    // Двунаправленные ретраи: A шлёт данные B, B шлёт зонд A (для рукопожатия).
    let start = Instant::now();
    let mut attempts = 0u32;
    let received = loop {
        attempts += 1;
        let _ = a.write(&b_pk, msg);
        let _ = b.write(&a_pk, b"B: probe");
        if let Ok(r) = rx.recv_timeout(Duration::from_millis(200)) {
            break Some(r);
        }
        if start.elapsed() > Duration::from_secs(20) {
            println!("  (всего попыток: {attempts})");
            break None;
        }
    };

    match received {
        Some((data, src)) => {
            assert_eq!(data, msg, "содержимое сообщения должно совпадать");
            assert_eq!(src, a_pk, "отправитель должен совпадать с публичным ключом A");
            println!(
                "✓ B получил {:?} ({} байт) от A за {:?}; src == A.pubkey",
                String::from_utf8_lossy(&data),
                data.len(),
                start.elapsed(),
            );
        }
        None => {
            eprintln!("✗ таймаут: сообщение не доставлено");
            std::process::exit(1);
        }
    }
}
