//! **Эксперимент к ADR-007, пункт 0.** Останавливает ли ротация ключа поток флудера?
//!
//! Вопрос, от которого зависит вся ценность ротации адреса: если жертва перестала
//! анонсировать свой ключ, поток атакующего **не входит в меш** (умирает на его
//! стороне), доходит до транзита и там гаснет — или доезжает до канала жертвы?
//! От ответа зависит, что даёт ротация: экономию трафика жертвы или только
//! прекращение прикладной доставки.
//!
//! Топология — линия, СПЕЦИАЛЬНО без прямого линка A↔B (иначе вопроса о
//! маршрутизации нет вовсе):
//!
//! ```text
//!   A (флудер) ──tls──► R (транзит) ◄──tls── B (жертва)
//! ```
//!
//! Фазы:
//!   1. **База.** Сессия A↔B установлена, A льёт поток на ключ B. Меряем байты на
//!      линках всех трёх узлов.
//!   2. **Ротация.** B гасит узел и поднимает НОВЫЙ ключ на том же физическом
//!      линке («сменил адрес, но остался в сети»). A продолжает лить на СТАРЫЙ
//!      ключ. Меряем то же самое.
//!
//! Мерило — накопительные `rx_bytes`/`tx_bytes` **линков** (`peers_json`), а не
//! прикладные счётчики: важно, что происходит с байтами на проводе. `write()` при
//! этом может продолжать «успешно» принимать пакеты — приём в очередь не значит
//! отправку в сеть.
//!
//! Особенность транспорта, которую стенд обязан учитывать: зашифрованная сессия
//! ironwood завершается только при ДВУСТОРОННЕМ обмене (см. `loopback.rs`). Поэтому
//! базу меряем после явного рукопожатия зондами — иначе «базой» окажется поток,
//! который не доставляется и безо всякой ротации.
//!
//! Запуск (WSL, нужна libygg):
//!   LD_LIBRARY_PATH=ygg/libygg cargo run -p p2p-ygg --example flood_rotation
//!   [--secs N] [--pps N] [--size N] [--direct]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use p2p_ygg::Node;

/// Байты по каждому линку узла: (ключ пира, принято, отправлено).
fn links(n: &Node) -> Vec<(String, u64, u64)> {
    let v: serde_json::Value = serde_json::from_str(&n.peers_json()).unwrap_or_default();
    v.as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .map(|p| {
            (
                p["key"].as_str().unwrap_or("?").to_string(),
                p["rx_bytes"].as_u64().unwrap_or(0),
                p["tx_bytes"].as_u64().unwrap_or(0),
            )
        })
        .collect()
}

/// Суммарные байты по всем линкам узла (принято, отправлено).
fn link_bytes(n: &Node) -> (u64, u64) {
    links(n).iter().fold((0, 0), |(r, t), (_, rx, tx)| (r + rx, t + tx))
}

/// Разница по линкам между двумя снимками, с именами узлов вместо hex-ключей.
/// Формат: `имя <-принято/отправлено-> КБ`.
fn diff_links(
    before: &[(String, u64, u64)],
    after: &[(String, u64, u64)],
    names: &[(String, &'static str)],
) -> String {
    let name = |k: &str| {
        names
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| k.chars().take(6).collect())
    };
    after
        .iter()
        .map(|(k, rx, tx)| {
            let (rx0, tx0) = before
                .iter()
                .find(|(k0, _, _)| k0 == k)
                .map(|(_, r, t)| (*r, *t))
                .unwrap_or((0, 0));
            format!(
                "{} <-{:.0}/{:.0}->",
                name(k),
                rx.saturating_sub(rx0) as f64 / 1024.0,
                tx.saturating_sub(tx0) as f64 / 1024.0,
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Поток-читатель: считает пакеты, дошедшие до ПРИЛОЖЕНИЯ. Осечка чтения — не
/// повод бросить приём: иначе одна ошибка молча превратит «жертва не читала» в
/// «не дошло».
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
                _ => thread::sleep(Duration::from_millis(50)),
            }
        }
    });
    got
}

fn wait_peers(n: &Node, want: i32, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if n.peer_count() >= want {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn wait_tree(n: &Node, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if n.tree_count() > 1 {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn arg(name: &str, default: u64) -> u64 {
    let mut it = std::env::args();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().and_then(|v| v.parse().ok()).unwrap_or(default);
        }
    }
    default
}

fn main() {
    let secs = arg("--secs", 12);
    let pps = arg("--pps", 400);
    let size = arg("--size", 1000) as usize;
    let direct = std::env::args().any(|x| x == "--direct");
    // Вариант «жертва ушла совсем»: после ротации она не возвращается в сеть.
    // Отделяет «транзит льёт тому, кто остался снизу» от «транзит гасит поток».
    let no_rejoin = std::env::args().any(|x| x == "--no-rejoin");

    let r = Arc::new(Node::start(&[0x0Au8; 32]).expect("старт R"));
    let a = Arc::new(Node::start(&[0x1Au8; 32]).expect("старт A"));
    let b_old = Arc::new(Node::start(&[0x2Au8; 32]).expect("старт B(старый ключ)"));
    let (a_key, b_old_key) = (a.public_key(), b_old.public_key());

    let uri = r.listen("tls://127.0.0.1:0").expect("R listen");
    if direct {
        // Контроль: прямой линк A↔B, вопроса о маршрутизации нет.
        let b_uri = b_old.listen("tls://127.0.0.1:0").expect("B listen");
        a.add_peer(&b_uri).expect("A→B");
        println!("КОНТРОЛЬ: прямой линк A↔B ({b_uri})");
    } else {
        a.add_peer(&uri).expect("A→R");
        b_old.add_peer(&uri).expect("B→R");
    }

    println!("R  pk={}", hexs(&r.public_key()));
    println!("A  pk={}", hexs(&a_key));
    println!("B0 pk={}  addr={}", hexs(&b_old_key), b_old.address());

    if direct {
        assert!(wait_peers(&a, 1, 20), "прямой линк не поднялся");
    } else {
        assert!(wait_peers(&r, 2, 20), "R не увидел обоих пиров");
    }
    assert!(wait_tree(&a, 20) && wait_tree(&b_old, 20), "дерево не сошлось");
    thread::sleep(Duration::from_secs(3)); // запас на стабилизацию
    println!(
        "дерево: A={} R={} B0={} | пиров: A={} R={} B0={}\n",
        a.tree_count(),
        r.tree_count(),
        b_old.tree_count(),
        a.peer_count(),
        r.peer_count(),
        b_old.peer_count(),
    );

    let stop = Arc::new(AtomicBool::new(false));
    let got_b = spawn_reader(b_old.clone(), stop.clone());
    let got_a = spawn_reader(a.clone(), stop.clone());

    // Рукопожатие: обе стороны шлют зонды, пока каждая не примет пакет. Без этого
    // сессия ironwood не устанавливается и «база» ничего не измеряет.
    {
        let probe = vec![0x55u8; 64];
        let deadline = Instant::now() + Duration::from_secs(45);
        while (got_a.load(Ordering::Relaxed) == 0 || got_b.load(Ordering::Relaxed) == 0)
            && Instant::now() < deadline
        {
            let _ = a.write(&b_old_key, &probe);
            let _ = b_old.write(&a_key, &probe);
            thread::sleep(Duration::from_millis(200));
        }
        println!(
            "рукопожатие: A принял {}, B принял {} пакетов",
            got_a.load(Ordering::Relaxed),
            got_b.load(Ordering::Relaxed),
        );
        assert!(
            got_b.load(Ordering::Relaxed) > 0,
            "сессия не установилась — база измеряла бы не поток к жертве, а мусор",
        );
    }

    // Флудер: льёт на ЗАФИКСИРОВАННЫЙ старый ключ B до конца эксперимента.
    let sent = Arc::new(AtomicU64::new(0));
    let accepted = Arc::new(AtomicU64::new(0));
    {
        let (a, sent, accepted, stop) = (a.clone(), sent.clone(), accepted.clone(), stop.clone());
        let payload = vec![0x41u8; size];
        let gap = Duration::from_micros(1_000_000 / pps.max(1));
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                sent.fetch_add(1, Ordering::Relaxed);
                if a.write(&b_old_key, &payload).is_ok() {
                    accepted.fetch_add(1, Ordering::Relaxed);
                }
                thread::sleep(gap);
            }
        });
    }

    let phase = |label: &str,
                 victim: &Node,
                 got: &AtomicU64,
                 names: &[(String, &'static str)],
                 secs: u64| {
        let (al0, rl0, vl0) = (links(&a), links(&r), links(victim));
        let (a0, v0) = (link_bytes(&a), link_bytes(victim));
        let (s0, g0) = (sent.load(Ordering::Relaxed), got.load(Ordering::Relaxed));
        thread::sleep(Duration::from_secs(secs));
        let (al1, rl1, vl1) = (links(&a), links(&r), links(victim));
        let (a1, v1) = (link_bytes(&a), link_bytes(victim));
        let (s1, g1) = (sent.load(Ordering::Relaxed), got.load(Ordering::Relaxed));
        let kb = |x: u64| x as f64 / 1024.0;
        println!("-- {label} ({secs}с) --");
        println!("  линки A: {}", diff_links(&al0, &al1, names));
        println!("  линки R: {}", diff_links(&rl0, &rl1, names));
        println!("  линки B: {}", diff_links(&vl0, &vl1, names));
        println!(
            "  A -> в сеть: {:.1} КБ ({:.0} КБ/с) | жертва <- из сети: {:.1} КБ",
            kb(a1.1 - a0.1),
            kb(a1.1 - a0.1) / secs as f64,
            kb(v1.0 - v0.0),
        );
        println!("  послано пакетов: {} | дошло в приложение: {}\n", s1 - s0, g1 - g0);
        (a1.1 - a0.1, v1.0 - v0.0)
    };

    let names_old: Vec<(String, &'static str)> = vec![
        (hexs(&r.public_key()), "R"),
        (hexs(&a_key), "A"),
        (hexs(&b_old_key), "B0"),
    ];
    let (a_wire_before, victim_before) =
        phase("ФАЗА 1: жертва онлайн со старым ключом", &b_old, &got_b, &names_old, secs);

    // ── Ротация: старый ключ исчезает, новый поднимается на том же линке ──────
    println!(">>> ротация: гасим старый ключ, поднимаем новый на том же линке\n");
    let b_new = Arc::new(Node::start(&[0x3Au8; 32]).expect("старт B(новый ключ)"));
    if no_rejoin {
        b_new.stop(); // узел жертвы не возвращается в сеть вовсе
    } else if direct {
        let b_uri = b_new.listen("tls://127.0.0.1:0").expect("B' listen");
        a.add_peer(&b_uri).expect("A→B'");
    } else {
        b_new.add_peer(&uri).expect("B'→R");
    }
    // ЯВНЫЙ stop, а не drop: поток-читатель держит свой Arc, поэтому сброс нашего
    // хендла узел не гасит — а незамеченная такая мелочь превращает эксперимент в
    // измерение живого узла под другим именем.
    b_old.stop();
    drop(b_old);
    if !no_rejoin {
        assert!(wait_peers(&b_new, 1, 20), "новый узел жертвы не поднял линк");
        assert!(wait_tree(&b_new, 20), "новый узел не вошёл в дерево");
    }
    thread::sleep(Duration::from_secs(3));
    let got_bnew = spawn_reader(b_new.clone(), stop.clone());
    println!("B1 pk={}  addr={}", hexs(&b_new.public_key()), b_new.address());

    // Транзит какое-то время держит ЗОМБИ-линк на погашенный узел и продолжает
    // писать в него. Пока он жив, «куда деваются байты» измерять бессмысленно:
    // цифры покажут запись в мёртвый сокет. Ждём, пока R выбросит старый линк.
    if !direct {
        let stale = hexs(&b_old_key);
        let deadline = Instant::now() + Duration::from_secs(90);
        while links(&r).iter().any(|(k, _, _)| *k == stale) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(500));
        }
        let gone = !links(&r).iter().any(|(k, _, _)| *k == stale);
        println!(
            "транзит: старый линк {} (пиров у R: {})\n",
            if gone { "выброшен" } else { "ВСЁ ЕЩЁ ЖИВ — цифры фазы 2 включают зомби" },
            r.peer_count(),
        );
    } else {
        println!();
    }

    let mut names_new = names_old.clone();
    names_new.push((hexs(&b_new.public_key()), "B1"));
    let (a_wire_after, victim_after) = phase(
        "ФАЗА 2: ключ сменён, поток идёт на старый",
        &b_new,
        &got_bnew,
        &names_new,
        secs,
    );

    stop.store(true, Ordering::Relaxed);
    thread::sleep(Duration::from_millis(300));

    let pct =
        |after: u64, before: u64| if before == 0 { 0.0 } else { after as f64 * 100.0 / before as f64 };
    println!("== ИТОГ ==");
    println!(
        "  байт от флудера в провод: {:.0}% от базы  ({a_wire_before} -> {a_wire_after})",
        pct(a_wire_after, a_wire_before),
    );
    println!(
        "  байт в линк жертвы:       {:.0}% от базы  ({victim_before} -> {victim_after})",
        pct(victim_after, victim_before),
    );
    println!(
        "  write() принял пакетов:   {} из {} (приём в очередь != отправка в сеть)",
        accepted.load(Ordering::Relaxed),
        sent.load(Ordering::Relaxed),
    );
    let verdict = if pct(a_wire_after, a_wire_before) < 20.0 {
        "поток УМИРАЕТ у флудера -> ротация останавливает байты"
    } else if pct(victim_after, victim_before) < 20.0 {
        "поток входит в меш, но НЕ доходит до жертвы -> ротация спасает жертву, грузит транзит"
    } else {
        "поток доходит до линка жертвы -> ротация НЕ останавливает байты"
    };
    println!("  вывод: {verdict}");
}
