//! Офлайн-очередь целиком в мессенджере: в очередь ложится само сообщение (с
//! цитатой, правкой, отзывом, групповым MLS), досылается окном по порядку и
//! переживает и долгий офлайн получателя, и перезапуск отправителя.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_engine::{ChatKey, Engine};
use p2p_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-outbox-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Короткие сроки: штатные 5 минут ретраев в тесте не подождать, а нам важно
/// именно то, что происходит ПОСЛЕ них. Офлайн-порог тоже короткий: возвращение
/// получателя отправитель замечает по переходу «офлайн → онлайн».
fn cfg() -> p2p_messenger::Config {
    p2p_messenger::Config {
        message_ttl: Duration::from_secs(2),
        retry_interval: Duration::from_millis(200),
        ping_interval: Duration::from_millis(300),
        offline_after: Duration::from_secs(1),
        ..Default::default()
    }
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport_cfg(dir, ident, net.endpoint(key), cfg()).unwrap().engine
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

/// Сообщение с текстом `text` в истории чата с `peer`.
fn find(e: &Engine, peer: &[u8; 32], text: &str) -> Option<serde_json::Value> {
    json(&e.history_json(peer)).as_array()?.iter().find(|m| m["text"] == text).cloned()
}

/// Поднять пару контактов и прогреть связь первым сообщением.
fn pair(net: &LoopbackNet, a: (Identity, PathBuf), b: (Identity, PathBuf)) -> (Arc<Engine>, Arc<Engine>, u64) {
    let (a_key, b_key) = (a.0.public_key_bytes(), b.0.public_key_bytes());
    let ea = start(net, a.0, a.1);
    let eb = start(net, b.0, b.1);
    ea.add_contact(&b_key, "B");
    eb.add_contact(&a_key, "A");
    let warm = ea.send_text(&b_key, "warmup").unwrap() as u64;
    assert!(wait(10, || find(&eb, &a_key, "warmup").is_some()), "прогрев дошёл");
    (ea, eb, warm)
}

/// R-005, R-006: ответ и его правка, сделанные пока получатель офлайн дольше срока
/// ретраев, доезжают после перезапуска отправителя — с цитатой и с правкой.
#[test]
fn reply_and_edit_survive_offline_and_restart() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(81), id(82));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let (a_dir, b_dir) = (tmpdir("a"), tmpdir("b"));
    let (a1, b1, warm) = pair(&net, (a_id.clone(), a_dir.clone()), (b_id.clone(), b_dir.clone()));

    b1.shutdown();
    drop(b1);
    let reply = a1.send_text_reply(&b_key, "ответ", warm, true).unwrap() as u64;
    assert!(a1.edit_message(ChatKey::Dm(b_key), reply, "ответ!"));
    std::thread::sleep(Duration::from_secs(3)); // ретраи в памяти выдохлись
    a1.shutdown();
    drop(a1);

    let b2 = start(&net, b_id, b_dir);
    let a2 = start(&net, a_id, a_dir);
    assert!(wait(15, || find(&b2, &a_key, "ответ!").is_some()), "правка доехала");
    let m = find(&b2, &a_key, "ответ!").unwrap();
    assert_eq!(m["reply_to"].as_u64(), Some(warm), "цитата не потерялась при досылке");
    assert!(m["edited_at"].as_i64().unwrap_or(0) > 0, "помечено как изменённое");
    a2.shutdown();
    b2.shutdown();
}

/// R-006: отзыв, сделанный пока получатель офлайн, тоже доезжает.
#[test]
fn retract_survives_offline() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(83), id(84));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let b_dir = tmpdir("b");
    let (a, b1, warm) = pair(&net, (a_id, tmpdir("a")), (b_id.clone(), b_dir.clone()));

    b1.shutdown();
    drop(b1);
    assert_eq!(a.retract_messages(&b_key, &[warm]), 1);
    std::thread::sleep(Duration::from_secs(3));

    let b2 = start(&net, b_id, b_dir);
    assert!(wait(15, || find(&b2, &a_key, "warmup").is_none()), "отозванное исчезло у получателя");
    a.shutdown();
    b2.shutdown();
}

/// R-011: длинная очередь уходит окном и доходит вся и по порядку.
#[test]
fn long_backlog_arrives_whole_and_in_order() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(85), id(86));
    let (_a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let b_dir = tmpdir("b");
    let (a, b1, _) = pair(&net, (a_id, tmpdir("a")), (b_id.clone(), b_dir.clone()));

    b1.shutdown();
    drop(b1);
    const N: usize = 150;
    for i in 0..N {
        a.send_text(&b_key, &format!("m{i:03}")).unwrap();
    }
    let b2 = start(&net, b_id, b_dir);
    let mut order = Vec::new();
    let got_all = wait(30, || {
        while let Some(ev) = b2.poll_event() {
            let v = json(&ev);
            if v["type"] == "message" && v["msg"]["kind"] == "text" {
                order.push(v["msg"]["text"].as_str().unwrap_or_default().to_string());
            }
        }
        order.len() >= N
    });
    assert!(got_all, "дошли все {N}, а не {}", order.len());
    let want: Vec<String> = (0..N).map(|i| format!("m{i:03}")).collect();
    assert_eq!(order, want, "в порядке отправки");
    a.shutdown();
    b2.shutdown();
}

/// R-023: групповое сообщение участнику, который офлайн дольше срока ретраев,
/// доезжает, когда он возвращается.
#[test]
fn group_message_survives_member_offline() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(87), id(88));
    let b_dir = tmpdir("b");
    let (a, b1, _) = pair(&net, (a_id, tmpdir("a")), (b_id.clone(), b_dir.clone()));
    let b_key = b_id.public_key_bytes();

    assert!(a.group_create("офлайн"));
    assert!(wait(10, || json(&a.group_list_json()).as_array().is_some_and(|v| !v.is_empty())));
    let gid = hex::decode(json(&a.group_list_json())[0]["gid"].as_str().unwrap()).unwrap();
    a.group_invite(gid.clone(), b_key);
    assert!(wait(10, || json(&b1.group_invites_json()).as_array().is_some_and(|v| !v.is_empty())));
    b1.accept_group_invite(gid.clone());
    assert!(wait(15, || b1.group_list_json().contains(&hex::encode(&gid))), "B в группе");

    b1.shutdown();
    drop(b1);
    a.group_send_text(gid.clone(), "пока тебя не было".into());
    std::thread::sleep(Duration::from_secs(3));

    let b2 = start(&net, b_id, b_dir);
    let got = wait(20, || b2.group_history_json(&gid).contains("пока тебя не было"));
    assert!(got, "групповое сообщение догнало вернувшегося участника");
    a.shutdown();
    b2.shutdown();
}
