//! Стресс-тест пропускной способности транспорта Yggdrasil (ironwood iwe).
//! Два узла в одном процессе пирятся напрямую по localhost-TLS; узел A бластит
//! датаграммы узлу B на максимальной скорости, B считает принятые байты/пакеты.
//!
//! Запуск (WSL/Linux, после scripts/build-ygg.sh):
//!   cargo run -p peerage-ygg --release --example throughput
//!
//! ВАЖНО: это «потолок» транспорта на БЫСТРОМ канале (localhost) — шифрование
//! iwe + FFI + накладные на датаграмму. Реальный интернет-путь телефон↔телефон
//! ограничен сетью и меряется только между устройствами.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use peerage_ygg::Node;

const DATA: u8 = 0xAA; // маркер полезных датаграмм (B считает только их)

fn mb(bytes: u64) -> String {
    format!("{:.1}MB", bytes as f64 / 1e6)
}

fn main() {
    let a = Arc::new(Node::start(&[1u8; 32]).expect("старт A"));
    let b = Arc::new(Node::start(&[2u8; 32]).expect("старт B"));
    let uri = b.listen("tls://127.0.0.1:0").expect("B listen");
    a.add_peer(&uri).expect("A add_peer");

    // Ждём согласования дерева маршрутизации.
    let t = Instant::now();
    while !(a.tree_count() > 1 && b.tree_count() > 1) {
        if t.elapsed() > Duration::from_secs(10) {
            eprintln!("⚠ дерево не сформировалось за 10с");
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(2));
    println!("сеть согласована за {:?}", t.elapsed());

    let a_pk = a.public_key();
    let b_pk = b.public_key();

    let rx_bytes = Arc::new(AtomicU64::new(0));
    let rx_pkts = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    // B: считаем принятые датаграммы-данные.
    {
        let b = b.clone();
        let (rb, rp, st) = (rx_bytes.clone(), rx_pkts.clone(), stop.clone());
        thread::spawn(move || {
            let mut buf = vec![0u8; 70000];
            while !st.load(Ordering::Relaxed) {
                match b.read(&mut buf) {
                    Ok((n, _)) if n > 0 && buf[0] == DATA => {
                        rb.fetch_add(n as u64, Ordering::Relaxed);
                        rp.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
        });
    }
    // A: дренируем входящие зонды (иначе сессия не обслуживается).
    {
        let a = a.clone();
        let st = stop.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while !st.load(Ordering::Relaxed) {
                if a.read(&mut buf).is_err() {
                    return;
                }
            }
        });
    }

    // Прогрев: iwe-сессия встаёт только при ДВУНАПРАВЛЕННОМ обмене.
    for _ in 0..60 {
        let _ = a.write(&b_pk, &[DATA; 64]);
        let _ = b.write(&a_pk, b"\xBBprobe");
        thread::sleep(Duration::from_millis(30));
    }
    // Keepalive от B, чтобы сессия жила во время одностороннего блба.
    {
        let b = b.clone();
        let st = stop.clone();
        thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                let _ = b.write(&a_pk, b"\xBBprobe");
                thread::sleep(Duration::from_millis(100));
            }
        });
    }

    println!("\n  payload |   отправлено   |    принято     | приём, Мбит/с | потери");
    println!("  --------+----------------+----------------+---------------+-------");
    for &size in &[256usize, 1200, 8192, 32768, 65000] {
        let payload = vec![DATA; size];
        rx_bytes.store(0, Ordering::Relaxed);
        rx_pkts.store(0, Ordering::Relaxed);
        let (mut sent_pkts, mut sent_bytes) = (0u64, 0u64);
        let dur = Duration::from_secs(4);
        let t0 = Instant::now();
        while t0.elapsed() < dur {
            // Пачка по 64 датаграммы между проверками времени (меньше оверхед).
            for _ in 0..64 {
                if a.write(&b_pk, &payload).is_ok() {
                    sent_pkts += 1;
                    sent_bytes += size as u64;
                }
            }
        }
        thread::sleep(Duration::from_millis(400)); // дренаж «в полёте»
        let (rb, rp) = (rx_bytes.load(Ordering::Relaxed), rx_pkts.load(Ordering::Relaxed));
        let secs = dur.as_secs_f64();
        let loss = (1.0 - rp as f64 / sent_pkts.max(1) as f64) * 100.0;
        println!(
            "  {:>6}B | {:>6} / {:>6} | {:>6} / {:>6} | {:>9.1} ⬇ | {:>4.0}%",
            size,
            sent_pkts,
            mb(sent_bytes),
            rp,
            mb(rb),
            rb as f64 * 8.0 / 1e6 / secs,
            loss,
        );
    }

    stop.store(true, Ordering::Relaxed);
    thread::sleep(Duration::from_millis(200));
    a.stop();
    b.stop();
}
