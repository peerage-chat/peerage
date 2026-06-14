//! Сквозной тест группового чата (3 узла) поверх реального Yggdrasil, БЕЗ сервера:
//! A создаёт группу и добавляет B и C (MLS Commit/Welcome рассылаются по mesh),
//! затем все обмениваются E2E-сообщениями группы.
//!
//! Запуск (WSL): cargo run -p p2p-groupchat --example group_chat

use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use p2p_core::Identity;
use p2p_groupchat::GroupChat;
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

/// Обработать входящие групповые события в течение `secs`, вернуть тексты.
fn pump(rx: &Receiver<Event>, chat: &mut GroupChat, secs: u64) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut texts = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(Event::Group { data, .. }) => {
                if let Ok(Some(t)) = chat.handle(&data) {
                    texts.push(t);
                }
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    texts
}

fn main() {
    let ids: Vec<Identity> = (1u8..=3).map(|i| Identity::from_seed(&[i; 32]).unwrap()).collect();
    let pks: Vec<[u8; 32]> = ids.iter().map(|i| i.public_key_bytes()).collect();

    // Узел A слушает; B и C подключаются к нему (B↔C маршрутизируются через mesh).
    let nodes: Vec<Node> = ids.iter().map(|i| Node::start(&i.seed()).unwrap()).collect();
    let uri = nodes[0].listen("tls://127.0.0.1:0").unwrap();
    nodes[1].add_peer(&uri).unwrap();
    nodes[2].add_peer(&uri).unwrap();
    let t0 = Instant::now();
    while nodes.iter().any(|n| n.tree_count() < 2) {
        if t0.elapsed() > Duration::from_secs(15) {
            eprintln!("✗ дерево не сошлось");
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(3));

    let mut nodes = nodes.into_iter();
    let (ma, rxa) = Messenger::start(ids[0].clone(), nodes.next().unwrap());
    let (mb, rxb) = Messenger::start(ids[1].clone(), nodes.next().unwrap());
    let (mc, rxc) = Messenger::start(ids[2].clone(), nodes.next().unwrap());
    let (ma, mb, mc) = (Arc::new(ma), Arc::new(mb), Arc::new(mc));

    // Поднимаем шифр-сессии между всеми парами (двунаправленный Hello).
    let (ka, kb, kc) = (pks[0], pks[1], pks[2]);
    thread::scope(|s| {
        let to = Duration::from_secs(25);
        let (ma1, ma2) = (ma.clone(), ma.clone());
        let (mb1, mb2) = (mb.clone(), mb.clone());
        let (mc1, mc2) = (mc.clone(), mc.clone());
        s.spawn(move || ma1.connect(&kb, to));
        s.spawn(move || ma2.connect(&kc, to));
        s.spawn(move || mb1.connect(&ka, to));
        s.spawn(move || mb2.connect(&kc, to));
        s.spawn(move || mc1.connect(&ka, to));
        s.spawn(move || mc2.connect(&kb, to));
    });
    println!("✓ все пары соединены");

    // A — админ; B и C готовят KeyPackage.
    let mut a = GroupChat::create(&ids[0], ma.clone()).unwrap();
    let (mut b, kpb) = GroupChat::pending(&ids[1], mb.clone()).unwrap();
    let (mut c, kpc) = GroupChat::pending(&ids[2], mc.clone()).unwrap();

    // A добавляет B.
    a.add_member(pks[1], &kpb).unwrap();
    pump(&rxb, &mut b, 5); // B получает Welcome → вступает

    // A добавляет C (Commit → B, Welcome → C).
    a.add_member(pks[2], &kpc).unwrap();
    pump(&rxb, &mut b, 5); // B применяет Commit
    pump(&rxc, &mut c, 5); // C вступает

    println!("участников: A={} B={} C={}", a.member_count(), b.member_count(), c.member_count());

    // A → группа.
    a.send_text("привет, группа").unwrap();
    let tb = pump(&rxb, &mut b, 5);
    let tc = pump(&rxc, &mut c, 5);

    // C → группа.
    c.send_text("ответ от C").unwrap();
    let ta = pump(&rxa, &mut a, 5);
    let tb2 = pump(&rxb, &mut b, 5);

    let ok = a.member_count() == 3
        && b.member_count() == 3
        && c.member_count() == 3
        && tb.iter().any(|t| t == "привет, группа")
        && tc.iter().any(|t| t == "привет, группа")
        && ta.iter().any(|t| t == "ответ от C")
        && tb2.iter().any(|t| t == "ответ от C");

    println!("B от A: {tb:?}");
    println!("C от A: {tc:?}");
    println!("A от C: {ta:?}");
    println!("B от C: {tb2:?}");

    if ok {
        println!("\n✓✓ Групповой E2E-чат (MLS) работает поверх Yggdrasil БЕЗ сервера");
    } else {
        eprintln!("\n✗ проверка не пройдена");
        std::process::exit(1);
    }
}
