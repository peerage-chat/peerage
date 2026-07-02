//! Интеграционные тесты группового слоя поверх loopback-транспорта — весь путь
//! инвайта и вступления в одном процессе, без Yggdrasil и устройств (ровно то, что
//! раньше требовало двух телефонов). Прогонять из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_engine::groups::GroupManager;
use p2p_messenger::{loopback::LoopbackNet, Config, Event, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-it-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Разослать событие мессенджера соответствующему менеджеру группы (как event_pump).
fn dispatch(gm: &mut GroupManager, ev: Event) {
    match ev {
        Event::GroupKeyRequest { peer } => gm.on_key_request(peer),
        Event::GroupKeyReply { peer, key_package } => {
            gm.on_key_reply(peer, &key_package);
        }
        Event::Group { data, .. } => {
            gm.on_group(&data);
        }
        Event::GroupResyncRequest { peer, gid } => gm.on_resync_request(peer, &gid),
        Event::GroupLeaveRequest { peer, gid } => {
            gm.on_leave_request(peer, &gid);
        }
        _ => {}
    }
}

/// Гонять события между двумя менеджерами, пока не выполнится `done` (или таймаут).
fn pump_until(
    a: &mut GroupManager,
    a_rx: &Receiver<Event>,
    b: &mut GroupManager,
    b_rx: &Receiver<Event>,
    done: impl Fn(&GroupManager, &GroupManager) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let mut progressed = false;
        while let Ok(ev) = a_rx.try_recv() {
            dispatch(a, ev);
            progressed = true;
        }
        while let Ok(ev) = b_rx.try_recv() {
            dispatch(b, ev);
            progressed = true;
        }
        if done(a, b) {
            return true;
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    done(a, b)
}

/// Два узла в общей loopback-сети с уже пройденным инвайтом: A (админ) + B в одной
/// группе. `drop_every` эмулирует потери на всём протяжении.
struct Ctx {
    gm_a: GroupManager,
    a_rx: Receiver<Event>,
    gm_b: GroupManager,
    b_rx: Receiver<Event>,
    gid: Vec<u8>,
    b_key: [u8; 32],
}

fn setup_and_join(drop_every: u32) -> Ctx {
    let net = LoopbackNet::new();
    if drop_every > 0 {
        net.set_drop_every(drop_every);
    }
    let (a_id, b_id) = (id(10), id(20));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config { retry_interval: Duration::from_millis(40), ..Config::default() };
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let mut gm_a = GroupManager::new(a_id, Arc::new(a_msgr), tmpdir("a"));
    let mut gm_b = GroupManager::new(b_id, Arc::new(b_msgr), tmpdir("b"));

    let gid = gm_a.create("тест").expect("группа создана");
    gm_a.invite(&gid, b_key);
    let joined = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
        a.members(&gid).contains(&b_key) && b.group_ids().contains(&gid)
    });
    assert!(joined, "инвайт должен довести обе стороны в группу (drop_every={drop_every})");
    assert!(gm_b.members(&gid).contains(&a_key), "B видит A в ростере");
    Ctx { gm_a, a_rx, gm_b, b_rx, gid, b_key }
}

#[test]
fn invite_join_flow_over_loopback() {
    setup_and_join(0);
}

/// Тот же путь под потерями — доказывает, что надёжный invite-хендшейк дожимает
/// приглашение сквозь дропы (регрессия против «инвайт не срабатывает»).
#[test]
fn invite_join_flow_survives_packet_loss() {
    setup_and_join(3);
}

/// ADR-004: админ отзывает участника → у него SelfRemoved (группа исчезает), из
/// ростера админа он тоже пропадает.
#[test]
fn admin_kick_removes_member() {
    let mut c = setup_and_join(0);
    let (gid, b_key) = (c.gid.clone(), c.b_key);
    assert!(c.gm_a.remove_member(&gid, b_key), "админ инициировал отзыв");
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |a, b| {
        !a.members(&gid).contains(&b_key) && !b.group_ids().contains(&gid)
    });
    assert!(ok, "после kick: A убрал B из ростера, B забыл группу (SelfRemoved)");
}

/// ADR-004: участник выходит → админ отзывает его из общего ростера; локально он
/// забывает группу сразу.
#[test]
fn member_leave_revoked_by_admin() {
    let mut c = setup_and_join(0);
    let (gid, b_key) = (c.gid.clone(), c.b_key);
    assert!(c.gm_b.leave(&gid), "B инициировал выход");
    assert!(!c.gm_b.group_ids().contains(&gid), "B забыл группу локально сразу");
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |a, _b| {
        !a.members(&gid).contains(&b_key)
    });
    assert!(ok, "после leave: админ A отозвал B из ростера");
}
