//! «Пригласить» из группового звонка: участник группы получает приглашение в комнату
//! группы, вход по нему отмечается в журнале как отвеченный, а позвать в группу
//! постороннего нельзя.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

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
    let d = std::env::temp_dir().join(format!("p2p-ring-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

fn wait(secs: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

fn non_empty(s: &str) -> bool {
    json(s).as_array().map(|v| !v.is_empty()).unwrap_or(false)
}

/// Дождаться события `room_invite` в очереди движка; остальные события пропускаем.
fn next_room_invite(e: &Engine, secs: u64) -> Option<serde_json::Value> {
    let mut found = None;
    wait(secs, || {
        while let Some(ev) = e.poll_event() {
            let v = json(&ev);
            if v["type"] == "room_invite" {
                found = Some(v);
                return true;
            }
        }
        false
    });
    found
}

#[test]
fn group_member_is_rung_into_the_group_room_and_stranger_is_not() {
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(61), id(62), id(63));
    let (a_key, b_key, c_key) = (a_id.public_key_bytes(), b_id.public_key_bytes(), c_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    let c = start(&net, c_id, tmpdir("c"));
    a.add_contact(&b_key, "B");
    a.add_contact(&c_key, "C");
    b.add_contact(&a_key, "A");
    c.add_contact(&a_key, "A");

    // Группа из A и B; C — знакомый A, но не в группе.
    assert!(a.group_create("созвон"), "группа создана");
    assert!(wait(15, || non_empty(&a.group_list_json())), "группа в списке");
    let gid_hex = json(&a.group_list_json())[0]["gid"].as_str().unwrap().to_string();
    let gid = hex::decode(&gid_hex).unwrap();
    a.group_invite(gid.clone(), b_key);
    assert!(wait(15, || non_empty(&b.group_invites_json())), "B получил приглашение в группу");
    b.accept_group_invite(gid.clone());
    assert!(wait(15, || a.group_members_json(&gid).contains(&hex::encode(b_key))), "A видит B в ростере");
    assert!(wait(15, || b.group_members_json(&gid).contains(&hex::encode(a_key))), "B видит A в ростере");

    // A в комнате группы зовёт обоих: уйти должно только участнику.
    a.room_join(gid.clone(), true, false);
    assert_eq!(a.room_ring(&gid, &[b_key, c_key], true, false), 1, "посторонний отсеян до отправки");

    let inv = next_room_invite(&b, 15).expect("B получил приглашение в звонок");
    assert_eq!(inv["room_id"].as_str(), Some(gid_hex.as_str()), "зовут именно в комнату группы");
    assert_eq!(inv["peer"].as_str(), Some(hex::encode(a_key).as_str()));
    assert!(next_room_invite(&c, 2).is_none(), "C ничего не получил");

    // B входит по приглашению — в журнале звонок отвеченный, а не пропущенный.
    b.room_join(gid.clone(), true, false);
    let calls = json(&b.calls_json(10));
    assert_eq!(calls[0]["gid"].as_str(), Some(gid_hex.as_str()));
    assert_eq!(calls[0]["outcome"].as_str(), Some("answered"));

    std::mem::forget(net);
}

/// R-007: позвонил и положил трубку, пока собеседник был офлайн, — появившись,
/// он звонка не получает. Приглашение снято при выходе из комнаты.
#[test]
fn hung_up_call_does_not_ring_later() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(81), id(82));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a-hang"));
    a.add_contact(&b_key, "B");

    // B офлайн: его в сети ещё нет.
    let room = a.room_invite_dm(b_key, true, false);
    std::thread::sleep(Duration::from_millis(300));
    a.room_leave(room);

    let b = start(&net, b_id, tmpdir("b-hang"));
    b.add_contact(&a_key, "A");
    assert!(next_room_invite(&b, 8).is_none(), "сброшенный звонок не зазвонил");
}
