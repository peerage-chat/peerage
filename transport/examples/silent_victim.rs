//! **Дополнение к эксперименту ADR-007.** Что происходит, когда получатель молчит?
//!
//! Наблюдение из основного стенда: пока жертва не пошлёт что-нибудь в ответ,
//! прикладной трафик до неё не доходит. Вопрос: это «гейт навсегда» (молчащего
//! нельзя заDoS-ить прикладными пакетами) или просто задержка?
//!
//! Разбор ironwood (`encrypted/session.go`) даёт версию:
//!   * отправитель без сессии не шлёт полезную нагрузку вовсе — он буферизует
//!     ПОСЛЕДНЕЕ сообщение и шлёт `sessionInit` (~200 Б);
//!   * получатель на первый init заводит сессию с `seq = init.seq`, а затем
//!     `handleInit` отбрасывает этот же init по условию `init.seq <= info.seq` —
//!     и **ack не отправляет**;
//!   * `init.seq` = unix-время в СЕКУНДАХ, а буфер отправителя живёт
//!     `sessionTimeout = 1 минуту` и всё это время шлёт один и тот же init;
//!   * значит после протухания буфера отправитель создаст init с новым
//!     timestamp — и вот тогда получатель ответит ack, сессия встанет БЕЗ участия
//!     приложения получателя.
//!
//! Стенд меряет время до первого доставленного пакета при полностью молчащем
//! приложении получателя. Если версия верна — доставка начнётся через ~минуту.
//!
//! Запуск (WSL): LD_LIBRARY_PATH=ygg/libygg cargo run -p p2p-ygg --example silent_victim

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use p2p_ygg::Node;

fn main() {
    let limit = std::env::args()
        .skip_while(|a| a != "--wait")
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(180);

    let a = Arc::new(Node::start(&[0x5Au8; 32]).expect("старт A"));
    let b = Arc::new(Node::start(&[0x6Au8; 32]).expect("старт B"));
    let b_key = b.public_key();

    let uri = b.listen("tls://127.0.0.1:0").expect("B listen");
    a.add_peer(&uri).expect("A→B");

    let deadline = Instant::now() + Duration::from_secs(20);
    while (a.tree_count() < 2 || b.tree_count() < 2) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(2));
    println!("линк поднят: дерево A={} B={}", a.tree_count(), b.tree_count());

    // Приложение получателя ТОЛЬКО читает и никогда не пишет.
    let got = Arc::new(AtomicU64::new(0));
    {
        let (b, got) = (b.clone(), got.clone());
        thread::spawn(move || {
            let mut buf = vec![0u8; 65535];
            loop {
                match b.read(&mut buf) {
                    Ok((n, _)) if n > 0 => {
                        got.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => thread::sleep(Duration::from_millis(50)),
                }
            }
        });
    }

    println!("A шлёт 5 пкт/с, приложение B молчит. Ждём до {limit}с…");
    let start = Instant::now();
    let payload = vec![0x41u8; 500];
    let mut announced = false;
    while start.elapsed() < Duration::from_secs(limit) {
        let _ = a.write(&b_key, &payload);
        thread::sleep(Duration::from_millis(200));
        let n = got.load(Ordering::Relaxed);
        if n > 0 && !announced {
            announced = true;
            println!(
                ">>> первая доставка через {:.1}с (пакетов: {n})",
                start.elapsed().as_secs_f64(),
            );
            // Дать ещё немного времени: убедиться, что дальше идёт поток, а не
            // единичный пакет.
            thread::sleep(Duration::from_secs(5));
            println!(
                "    через 5с после этого: {} пакетов",
                got.load(Ordering::Relaxed),
            );
            break;
        }
        if start.elapsed().as_secs() % 15 == 0 && !announced {
            println!("    {:>3}с: доставлено {n}", start.elapsed().as_secs());
            thread::sleep(Duration::from_millis(900));
        }
    }

    // Сколько байт на пакет реально ушло в провод: если версия верна, полезная
    // нагрузка (500 Б) вообще не отправляется — уходят только init'ы (~200 Б).
    let wire: u64 = serde_json::from_str::<serde_json::Value>(&a.peers_json())
        .ok()
        .and_then(|v| {
            v.as_array()
                .map(|arr| arr.iter().map(|p| p["tx_bytes"].as_u64().unwrap_or(0)).sum())
        })
        .unwrap_or(0);
    let pkts = (start.elapsed().as_secs_f64() * 5.0).max(1.0);
    println!(
        "\nв провод ушло {:.0} КБ за {:.0} попыток записи = {:.0} Б на пакет (нагрузка была 500 Б)",
        wire as f64 / 1024.0,
        pkts,
        wire as f64 / pkts,
    );

    let n = got.load(Ordering::Relaxed);
    println!("\n== ИТОГ ==");
    if n == 0 {
        println!("  за {limit}с молчащему получателю не доставлено НИЧЕГО");
        println!("  => молчание работает как гейт на уровне транспорта");
    } else {
        println!("  доставка началась сама, без единого пакета от приложения получателя");
        println!("  => молчание НЕ гейт: это лишь задержка (буфер сессии протухает за минуту)");
    }
    a.stop();
    b.stop();
}
