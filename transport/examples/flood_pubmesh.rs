//! **Прогон к ADR-007 на НАСТОЯЩЕМ публичном меше Yggdrasil.**
//!
//! Линейный стенд (`flood_rotation.rs`) показал: после ротации поток шёл к новому
//! узлу жертвы. Но там транзит имел ровно одного соседа снизу — «флуд идёт за
//! жертвой» мог быть артефактом этой топологии. Здесь проверяем на реальной сети:
//! наши узлы A и B входят в глобальный меш через ПУБЛИЧНЫЕ пиры, прямого линка
//! A↔B нет, маршрут между ними строит настоящее дерево из тысяч узлов.
//!
//! Вопрос: когда B гасит старый ключ и поднимает новый, доходит ли поток,
//! адресованный СТАРОМУ ключу, до нового узла — или умирает где-то в меше, не имея
//! маршрута к исчезнувшему ключу?
//!
//! Ключевой параметр — `--gap N`: сколько секунд узел жертвы держится ПОЛНОСТЬЮ
//! офлайн между гашением старого ключа и входом под новым. Итог:
//!   * `--gap 5` (мгновенный перезаход): флуд доходит до нового узла (~88%) — тот
//!     наследует маршрутную позицию старого ключа;
//!   * `--gap 30`+ : старый ключ выпадает из дерева, флудеру слать НЕКУДА
//!     (его вывод в сеть падает до нуля) — пауза офлайн сбрасывает поток.
//!
//! **Вежливость к чужой инфраструктуре:** темп низкий (по умолчанию 50 пкт/с ×
//! 400 Б = 20 КБ/с), окна короткие. Полезная нагрузка — мусор, адресованный НАШЕМУ
//! же узлу; публичные пиры лишь маршрутизируют. Не поднимать темп без нужды.
//!
//! Запуск (WSL, нужна libygg и выход в интернет):
//!   LD_LIBRARY_PATH=ygg/libygg cargo run -p peerage-ygg --example flood_pubmesh -- \
//!     peers/public-peers.txt [--secs N] [--pps N] [--size N]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use peerage_ygg::Node;

fn link_rx_tx(n: &Node) -> (u64, u64) {
    let v: serde_json::Value = serde_json::from_str(&n.peers_json()).unwrap_or_default();
    v.as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .fold((0u64, 0u64), |(r, t), p| {
            (r + p["rx_bytes"].as_u64().unwrap_or(0), t + p["tx_bytes"].as_u64().unwrap_or(0))
        })
}

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

/// Дождаться входа в глобальный меш: дерево из многих узлов и хотя бы один пир.
fn wait_mesh(n: &Node, min_tree: i32, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if n.peer_count() >= 1 && n.tree_count() >= min_tree {
            return true;
        }
        thread::sleep(Duration::from_millis(500));
    }
    false
}

fn add_all(n: &Node, peers: &[String]) {
    for p in peers {
        let _ = n.add_peer(p);
    }
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
    let peers_file = std::env::args()
        .nth(1)
        .filter(|a| !a.starts_with("--"))
        .unwrap_or_else(|| "peers/public-peers.txt".to_string());
    let peers: Vec<String> = std::fs::read_to_string(&peers_file)
        .expect("не прочитать файл пиров")
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert!(!peers.is_empty(), "список публичных пиров пуст");

    let secs = arg("--secs", 10);
    let pps = arg("--pps", 50); // щадящий темп к чужой инфраструктуре
    let size = arg("--size", 400) as usize;

    let a = Arc::new(Node::start(&[0x1Bu8; 32]).expect("старт A"));
    let b_old = Arc::new(Node::start(&[0x2Bu8; 32]).expect("старт B(старый ключ)"));
    let (a_key, b_old_key) = (a.public_key(), b_old.public_key());

    // --split: A и B входят через НЕПЕРЕСЕКАЮЩИЕСЯ половины списка. Проверяет
    // гипотезу «выбрать других публичных пиров, чем у атакующего, = уйти от флуда».
    let split = std::env::args().any(|x| x == "--split");
    let (a_peers, b_peers): (&[String], &[String]) = if split {
        let mid = peers.len() / 2;
        (&peers[..mid], &peers[mid..])
    } else {
        (&peers[..], &peers[..])
    };
    println!(
        "вход в публичный меш (A через {} пиров, B через {}{})…",
        a_peers.len(),
        b_peers.len(),
        if split { ", непересекающиеся" } else { ", общие" },
    );
    add_all(&a, a_peers);
    add_all(&b_old, b_peers);
    assert!(wait_mesh(&a, 5, 90), "A не вошёл в глобальный меш");
    assert!(wait_mesh(&b_old, 5, 90), "B не вошёл в глобальный меш");
    thread::sleep(Duration::from_secs(3));
    println!(
        "A: адрес={} пиров={} дерево={}",
        a.address(),
        a.peer_count(),
        a.tree_count(),
    );
    println!(
        "B0: адрес={} пиров={} дерево={}\n",
        b_old.address(),
        b_old.peer_count(),
        b_old.tree_count(),
    );

    let stop = Arc::new(AtomicBool::new(false));
    let got_b = spawn_reader(b_old.clone(), stop.clone());
    let got_a = spawn_reader(a.clone(), stop.clone());

    // Рукопожатие через реальный меш: A и B шлют друг другу зонды, пока сессия не
    // встанет. Это заодно проверяет, что два НАШИХ узла вообще находят друг друга
    // по ключу в глобальной сети.
    {
        let probe = vec![0x55u8; 64];
        let deadline = Instant::now() + Duration::from_secs(60);
        while (got_a.load(Ordering::Relaxed) == 0 || got_b.load(Ordering::Relaxed) == 0)
            && Instant::now() < deadline
        {
            let _ = a.write(&b_old_key, &probe);
            let _ = b_old.write(&a_key, &probe);
            thread::sleep(Duration::from_millis(300));
        }
        let (ga, gb) = (got_a.load(Ordering::Relaxed), got_b.load(Ordering::Relaxed));
        println!("рукопожатие через меш: A принял {ga}, B принял {gb}");
        assert!(gb > 0 && ga > 0, "узлы не нашли друг друга в публичном меше");
    }

    let kb = |x: u64| x as f64 / 1024.0;

    // Флудер на СТАРЫЙ ключ B. Управляется флагом: на публичном меше линки ловят
    // ЧУЖОЙ транзитный трафик, поэтому «сколько принял B» само по себе ничего не
    // говорит — нужен контроль «флуд выключен» vs «включён» на том же узле.
    let sent = Arc::new(AtomicU64::new(0));
    let flood_on = Arc::new(AtomicBool::new(true));
    {
        let (a, sent, on, stop) = (a.clone(), sent.clone(), flood_on.clone(), stop.clone());
        let payload = vec![0x41u8; size];
        let gap = Duration::from_micros(1_000_000 / pps.max(1));
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if on.load(Ordering::Relaxed) {
                    sent.fetch_add(1, Ordering::Relaxed);
                    let _ = a.write(&b_old_key, &payload);
                }
                thread::sleep(gap);
            }
        });
    }

    // Замерить приём узла за окно (возвращает КБ, принятые линками).
    let measure_rx = |node: &Node, secs: u64| -> u64 {
        let r0 = link_rx_tx(node).0;
        thread::sleep(Duration::from_secs(secs));
        link_rx_tx(node).0 - r0
    };

    // ── ФАЗА 1: жертва в меше со старым ключом ────────────────────────────────
    // Чистый сигнал, что флуд доходит, — доставка в ПРИЛОЖЕНИЕ (ключ совпадает).
    let g0 = got_b.load(Ordering::Relaxed);
    let base_b_rx = measure_rx(&b_old, secs);
    let delivered_b0 = got_b.load(Ordering::Relaxed) - g0;
    println!("\n-- ФАЗА 1: жертва в меше, старый ключ ({secs}с) --");
    println!("  B0 принял линками: {:.0} КБ (фон меша + флуд)", kb(base_b_rx));
    println!("  доставлено в приложение B0: {delivered_b0} (флуд реально доходит)\n");

    // ── Ротация: старый ключ гаснет; ПАУЗА офлайн; новый входит в меш ──────────
    // Гипотеза: если узел уходит из сети целиком и держится офлайн, пока анонсы
    // старого ключа в дереве не выветрятся, то вернувшийся под новым ключом узел
    // уже не окажется точкой посадки для пакетов на старый ключ. --gap задаёт
    // длину этой офлайн-паузы (0 = как раньше, мгновенный перезаход).
    let gap = arg("--gap", 5);
    println!(">>> ротация: B гасит старый ключ, уходит офлайн на {gap}с, входит под новым\n");
    b_old.stop();
    drop(b_old);
    thread::sleep(Duration::from_secs(gap)); // жертвы в сети НЕТ всё это время
    let b_new = Arc::new(Node::start(&[0x3Bu8; 32]).expect("старт B(новый ключ)"));
    add_all(&b_new, b_peers);
    assert!(wait_mesh(&b_new, 5, 90), "новый узел не вошёл в меш");
    thread::sleep(Duration::from_secs(3));
    let _got_bnew = spawn_reader(b_new.clone(), stop.clone());
    println!(
        "B1: адрес={} пиров={} дерево={}\n",
        b_new.address(),
        b_new.peer_count(),
        b_new.tree_count(),
    );

    // ── ФАЗА 2: КОНТРОЛЬ. Приём B1 с выключенным и включённым флудом ───────────
    // Если поток к мёртвому ключу доходит до нового узла, приём при включённом
    // флуде будет выше на объём флуда. Если ротация сбрасывает поток — разницы нет
    // (обе цифры = фон публичных пиров).
    flood_on.store(false, Ordering::Relaxed);
    thread::sleep(Duration::from_secs(3)); // дать буферам стечь
    let idle_bn_rx = measure_rx(&b_new, secs);
    flood_on.store(true, Ordering::Relaxed);
    thread::sleep(Duration::from_secs(2));
    let a_tx0 = link_rx_tx(&a).1;
    let flood_bn_rx = measure_rx(&b_new, secs);
    let a_tx = link_rx_tx(&a).1 - a_tx0;

    println!("-- ФАЗА 2: ключ сменён, контроль на B1 ({secs}с каждое окно) --");
    println!("  флуд ВЫКЛ: B1 принял {:.0} КБ (чистый фон меша)", kb(idle_bn_rx));
    println!("  флуд ВКЛ:  B1 принял {:.0} КБ (фон + флуд, если доходит)", kb(flood_bn_rx));
    println!("  A отдала в меш при флуде: {:.0} КБ ({:.0} КБ/с)\n", kb(a_tx), kb(a_tx) / secs as f64);

    stop.store(true, Ordering::Relaxed);
    thread::sleep(Duration::from_millis(300));

    // Прирост приёма B1 от включения флуда, в долях от объёма самого флуда.
    let delta = flood_bn_rx.saturating_sub(idle_bn_rx);
    println!("== ИТОГ (публичный меш, пауза офлайн {gap}с) ==");
    // Ключевой различитель — отдаёт ли флудер что-то В СЕТЬ. Если A влила заметный
    // объём, а до B1 он дошёл — ротация не спасает. Если A почти ничего не отдала —
    // старый ключ выпал из дерева, слать стало НЕКУДА, и это уже не про B1.
    let a_kbps = kb(a_tx) / secs as f64;
    let verdict = if a_kbps < 2.0 {
        "старый ключ выпал из маршрутизации — флудер БОЛЬШЕ НЕ МОЖЕТ слать на него \
         (write() уходит в никуда). Достаточно длинная пауза офлайн СБРАСЫВАЕТ флуд"
    } else if delta as f64 * 100.0 / a_tx as f64 >= 25.0 {
        "флудер шлёт, и поток дошёл до нового узла — при коротком перезаходе ротация \
         НЕ спасает (новый узел наследует маршрутную позицию старого ключа)"
    } else {
        "флудер шлёт, но до B1 поток не дошёл — промежуточный случай, повторить прогон"
    };
    println!("  A отдала в сеть при флуде: {:.1} КБ/с | прирост приёма B1: {:.0} КБ", a_kbps, kb(delta));
    println!("  вывод: {verdict}");

    a.stop();
    b_new.stop();
}
