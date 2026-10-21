//! Событие о новом сообщении несёт ту же строку, что легла в историю (R-048, R-050):
//! открытый чат не теряет подпись и «переслано», а групповое приходит с id — на
//! него сразу можно ответить, и ленту не надо перечитывать.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_engine::{ChatKey, Engine, MediaInfo};
use p2p_messenger::loopback::LoopbackNet;
use serde_json::Value;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-uiev-{tag}-{n}"));
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

fn json(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

/// Ждать событие `message`, подходящее под `pred`; остальные события пропускаются.
fn wait_message(e: &Engine, secs: u64, pred: impl Fn(&Value) -> bool) -> Option<Value> {
    let mut found = None;
    wait(secs, || {
        while let Some(js) = e.poll_event() {
            let v = json(&js);
            if v["type"] == "message" && pred(&v) {
                found = Some(v);
                return true;
            }
        }
        false
    });
    found
}

/// Та же строка в истории чата, что приехала событием.
fn in_history(history: &str, msg: &Value) -> Value {
    json(history)
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == msg["id"] && m["sender"] == msg["sender"])
        .cloned()
        .expect("строка из события есть в истории")
}

#[test]
fn dm_event_carries_caption_and_forward_mark() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(71), id(72));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");
    let a_hex = hex::encode(a_key);

    // Вложение с подписью: подпись — в той же строке, а не потеряна до перезахода.
    let info = MediaInfo { caption: "подпись".into(), ..Default::default() };
    a.send_media_with(&b_key, vec![7u8; 5000], "application/pdf", "doc.pdf", info).unwrap();
    let ev = wait_message(&b, 15, |v| v["msg"]["kind"] == "file").expect("вложение пришло событием");
    assert_eq!(ev["chat"], a_hex);
    assert_eq!(ev["group"], false);
    assert_eq!(ev["msg"]["text"], "подпись");
    assert!(ev["msg"]["row"].as_i64().unwrap() > 0, "курсор пагинации есть");
    let row = in_history(&b.history_json(&a_key), &ev["msg"]);
    assert_eq!(row["text"], ev["msg"]["text"]);
    assert_eq!(row["blob_id"], ev["msg"]["blob_id"]);
    assert_eq!(row["row"], ev["msg"]["row"]);

    // Пересылка: пометка «переслано» тоже приезжает сразу.
    let own = a.send_text(&b_key, "оригинал").unwrap() as u64;
    assert!(wait_message(&b, 15, |v| v["msg"]["text"] == "оригинал").is_some());
    let n = a.forward_messages(ChatKey::Dm(b_key), &[(own, a_hex.clone())], ChatKey::Dm(b_key), false);
    assert_eq!(n, 1);
    let ev = wait_message(&b, 15, |v| v["msg"]["text"] == "оригинал" && v["msg"]["fwd_from"] != "")
        .expect("пересланное пришло событием");
    assert_eq!(ev["msg"]["fwd_from"], a_hex);
    a.shutdown();
    b.shutdown();
}

#[test]
fn group_message_arrives_as_a_row_with_id() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(73), id(74));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("ga"));
    let b = start(&net, b_id, tmpdir("gb"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    assert!(a.group_create("клуб"));
    assert!(wait(15, || json(&a.group_list_json()).as_array().map(|v| !v.is_empty()).unwrap_or(false)));
    let gid_hex = json(&a.group_list_json())[0]["gid"].as_str().unwrap().to_string();
    let gid = hex::decode(&gid_hex).unwrap();
    a.group_invite(gid.clone(), b_key);
    assert!(wait(15, || json(&b.group_invites_json()).as_array().map(|v| !v.is_empty()).unwrap_or(false)));
    b.accept_group_invite(gid.clone());
    assert!(wait(15, || a.group_members_json(&gid).contains(&hex::encode(b_key))), "B в ростере");

    a.group_send_text(gid.clone(), "привет".into());
    // Своё — тоже событием, с id, который присвоил слой групп.
    let mine = wait_message(&a, 15, |v| v["msg"]["text"] == "привет").expect("эхо своего");
    assert_eq!(mine["group"], true);
    assert_eq!(mine["chat"], gid_hex);
    assert_eq!(mine["msg"]["mine"], true);
    let id = mine["msg"]["id"].as_u64().unwrap();
    assert!(id > 0);

    let got = wait_message(&b, 15, |v| v["msg"]["text"] == "привет").expect("входящее групповое");
    assert_eq!(got["chat"], gid_hex);
    assert_eq!(got["msg"]["id"].as_u64(), Some(id), "тот же id, что у отправителя");
    assert_eq!(got["msg"]["sender"], hex::encode(a_key));
    assert_eq!(got["msg"]["mine"], false);
    let row = in_history(&b.group_history_json(&gid), &got["msg"]);
    assert_eq!(row["row"], got["msg"]["row"]);
    a.shutdown();
    b.shutdown();
}
