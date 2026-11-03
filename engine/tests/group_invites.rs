//! Новая группа сразу с приглашёнными и метка «приглашён · ждёт ответа» (UI 3.4).
//!
//! Мастер новой группы сначала выбирает людей, потом название: создание — одна команда
//! «создать и позвать этих». Экран «Пригласить» показывает, кого уже позвали: человек
//! в списке, пока не вступил. Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::Engine;
use peerage_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-ginv-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
}

/// Ждать условие, опрашивая раз в 30 мс. Возвращает false по таймауту.
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

/// gid единственной группы движка: создание идёт через актора — ждём списка.
fn only_gid(e: &Engine) -> Vec<u8> {
    assert!(
        wait(15, || non_empty(&e.group_list_json())),
        "группа появилась в списке"
    );
    let list = json(&e.group_list_json());
    hex::decode(list[0]["gid"].as_str().unwrap()).unwrap()
}

#[test]
fn group_created_with_invitees_lists_them_until_they_join() {
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(51), id(52), id(53));
    let (a_key, b_key, c_key) = (
        a_id.public_key_bytes(),
        b_id.public_key_bytes(),
        c_id.public_key_bytes(),
    );
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    let c = start(&net, c_id, tmpdir("c"));
    for (k, n) in [(b_key, "B"), (c_key, "C")] {
        a.add_contact(&k, n);
    }
    b.add_contact(&a_key, "A");
    c.add_contact(&a_key, "A");

    // Одна команда: группа и приглашения обоим.
    assert!(
        a.group_create_inviting("Дача", vec![b_key, c_key]),
        "команда принята"
    );
    let gid = only_gid(&a);
    for e in [&b, &c] {
        assert!(
            wait(15, || non_empty(&e.group_invites_json())),
            "карточка приглашения дошла"
        );
    }
    let invited = |e: &Engine| json(&e.group_invited_json(&gid));
    assert!(
        wait(5, || {
            let v = invited(&a);
            v.as_array().map(|v| v.len()) == Some(2)
        }),
        "оба в «приглашён»: {}",
        a.group_invited_json(&gid)
    );

    // B вступил — уходит из «приглашён» в участники; C ещё думает.
    b.accept_group_invite(gid.clone());
    assert!(
        wait(15, || a
            .group_members_json(&gid)
            .contains(&hex::encode(b_key))),
        "A видит B в ростере"
    );
    assert!(
        wait(5, || invited(&a) == serde_json::json!([hex::encode(c_key)])),
        "в «приглашён» остался только C: {}",
        a.group_invited_json(&gid)
    );

    // Приглашение по одному (экран «Пригласить») тоже видно сразу.
    let d_key = id(54).public_key_bytes();
    a.add_contact(&d_key, "D");
    a.group_invite(gid.clone(), d_key);
    assert!(
        wait(5, || invited(&a)
            .as_array()
            .map(|v| v.contains(&serde_json::json!(hex::encode(d_key))))
            == Some(true)),
        "D в «приглашён» после приглашения: {}",
        a.group_invited_json(&gid)
    );
}
