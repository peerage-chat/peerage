//! Сквозной тест группового чата (3 узла) поверх Yggdrasil, БЕЗ сервера, с ПОЛНЫМ
//! сетевым онбордингом: админ A приглашает B и C → они присылают KeyPackage по
//! сети → A добавляет (Commit/Welcome по mesh) → все обмениваются E2E-сообщениями.
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

/// Прокрутить события админа `secs` секунд: на KeyReply — добавить участника,
/// на Group — обработать (Commit/app). Вернуть прикладные тексты.
fn pump_admin(rx: &Receiver<Event>, a: &mut GroupChat, secs: u64) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut texts = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(Event::GroupKeyReply { peer, key_package }) => {
                let _ = a.add_member(peer, &key_package);
            }
            Ok(Event::Group { data, .. }) => {
                if let Ok(Some(t)) = a.handle(&data) {
                    texts.push(t);
                }
            }
            _ => {}
        }
    }
    texts
}

/// Прокрутить события участника: на KeyRequest — создать pending и отправить KP;
/// на Group — обработать (Welcome → вступление, либо прикладное сообщение).
fn pump_member(
    rx: &Receiver<Event>,
    chat: &mut Option<GroupChat>,
    id: &Identity,
    m: &Arc<Messenger>,
    secs: u64,
) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut texts = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(Event::GroupKeyRequest { peer }) => {
                if chat.is_none() {
                    if let Ok((pending, kp)) = GroupChat::pending(id, m.clone()) {
                        let _ = m.send_group_key_reply(&peer, &kp);
                        *chat = Some(pending);
                    }
                }
            }
            Ok(Event::Group { data, .. }) => {
                if let Some(c) = chat.as_mut() {
                    if let Ok(Some(t)) = c.handle(&data) {
                        texts.push(t);
                    }
                }
            }
            _ => {}
        }
    }
    texts
}

fn main() {
    let ids: Vec<Identity> = (1u8..=3).map(|i| Identity::from_seed(&[i; 32]).unwrap()).collect();
    let pks: Vec<[u8; 32]> = ids.iter().map(|i| i.public_key_bytes()).collect();
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

    let mut it = nodes.into_iter();
    let (ma, rxa) = Messenger::start(ids[0].clone(), it.next().unwrap());
    let (mb, rxb) = Messenger::start(ids[1].clone(), it.next().unwrap());
    let (mc, rxc) = Messenger::start(ids[2].clone(), it.next().unwrap());
    let (ma, mb, mc) = (Arc::new(ma), Arc::new(mb), Arc::new(mc));

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

    let mut a = GroupChat::create(&ids[0], ma.clone()).unwrap();
    let mut b: Option<GroupChat> = None;
    let mut c: Option<GroupChat> = None;

    // Добавляем B по сети: invite → KP → add → welcome → join.
    a.invite(kb);
    pump_member(&rxb, &mut b, &ids[1], &mb, 5); // B отвечает KeyPackage
    pump_admin(&rxa, &mut a, 5); // A получает KP и добавляет B (welcome)
    pump_member(&rxb, &mut b, &ids[1], &mb, 5); // B вступает

    // Добавляем C аналогично.
    a.invite(kc);
    pump_member(&rxc, &mut c, &ids[2], &mc, 5);
    pump_admin(&rxa, &mut a, 5); // add C: commit → B, welcome → C
    pump_member(&rxb, &mut b, &ids[1], &mb, 5); // B применяет commit
    pump_member(&rxc, &mut c, &ids[2], &mc, 5); // C вступает

    let bc = b.as_ref().map(|g| g.member_count()).unwrap_or(0);
    let cc = c.as_ref().map(|g| g.member_count()).unwrap_or(0);
    println!("участников: A={} B={bc} C={cc}", a.member_count());

    // A → группа.
    a.send_text("привет, группа").unwrap();
    let tb = pump_member(&rxb, &mut b, &ids[1], &mb, 5);
    let tc = pump_member(&rxc, &mut c, &ids[2], &mc, 5);

    // C → группа.
    c.as_mut().unwrap().send_text("ответ от C").unwrap();
    let ta = pump_admin(&rxa, &mut a, 5);
    let tb2 = pump_member(&rxb, &mut b, &ids[1], &mb, 5);

    let ok = a.member_count() == 3
        && bc == 3
        && cc == 3
        && tb.iter().any(|t| t == "привет, группа")
        && tc.iter().any(|t| t == "привет, группа")
        && ta.iter().any(|t| t == "ответ от C")
        && tb2.iter().any(|t| t == "ответ от C");

    println!("B от A: {tb:?}\nC от A: {tc:?}\nA от C: {ta:?}\nB от C: {tb2:?}");
    if ok {
        println!("\n✓✓ Групповой E2E-чат с сетевым онбордингом (MLS) — БЕЗ сервера");
    } else {
        eprintln!("\n✗ проверка не пройдена");
        std::process::exit(1);
    }
}
