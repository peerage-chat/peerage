//! Автоудаление у себя (5.4): по правилу уходят старые сообщения и крупные старые
//! вложения; «Сохранённое», закреплённое, свой аватар и чат со своим «никогда» — нет.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::autodel::{Age, MinSize, Rule};
use peerage_core::Identity;
use peerage_engine::{now, ChatKey, Engine, MediaInfo};
use peerage_messenger::loopback::LoopbackNet;

const DAY: i64 = 24 * 60 * 60;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-autodel-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
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

fn rows(json: &str) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(json)
        .unwrap()
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn rule_prunes_old_history_but_keeps_protected() {
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(71), id(72), id(73));
    let (a_key, b_key, c_key) = (
        a_id.public_key_bytes(),
        b_id.public_key_bytes(),
        c_id.public_key_bytes(),
    );
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    let c = start(&net, c_id, tmpdir("c"));
    a.add_contact(&b_key, "B");
    a.add_contact(&c_key, "C");
    b.add_contact(&a_key, "A");
    c.add_contact(&a_key, "C");

    let avatar = a.set_self_avatar(vec![5u8; 300], "image/png");
    a.save_note("заметка").unwrap();
    b.send_text(&a_key, "старое").unwrap();
    b.send_text(&a_key, "закреплённое").unwrap();
    c.send_text(&a_key, "у C своё «никогда»").unwrap();
    let file: Vec<u8> = (0..40_000u32).map(|i| (i % 7) as u8).collect();
    let file_id = hex::encode(peerage_blob::blob_id(&file));
    b.send_media_with(
        &a_key,
        file,
        "application/pdf",
        "doc.pdf",
        MediaInfo::default(),
    )
    .unwrap();
    assert!(
        wait(10, || rows(&a.history_json(&b_key)).len() == 3
            && a.blob_exists(&file_id)
            && !rows(&a.history_json(&c_key)).is_empty()),
        "всё доехало до A"
    );
    let hist = rows(&a.history_json(&b_key));
    let pinned = hist.iter().find(|m| m["text"] == "закреплённое").unwrap();
    assert!(a.pin_message(
        ChatKey::Dm(b_key),
        pinned["id"].as_u64().unwrap(),
        &hex::encode(b_key),
        true
    ));

    // Только вложения «от любого размера» старше недели: сообщения остаются.
    let att = Rule {
        attachments: Age::Week,
        min_size: MinSize::Any,
        ..Default::default()
    };
    let preview = a.autodel_preview(&att, None);
    assert_eq!(preview.messages, 0, "сегодняшнее под правило не попадает");
    a.set_autodel(&att);
    assert!(a.blob_exists(&file_id), "вложение свежее — на месте");
    let later = now() + 8 * DAY;
    let done = a.autodel_run_at(later);
    assert_eq!(done.messages, 0);
    assert!(done.bytes > 0);
    assert!(!a.blob_exists(&file_id), "старое вложение удалено");
    assert!(a.blob_exists(&avatar), "свой аватар на месте");
    let row = rows(&a.history_json(&b_key))
        .into_iter()
        .find(|m| m["blob_id"] == file_id.as_str())
        .unwrap();
    assert_eq!(
        row["downloaded"], false,
        "карточка снова предлагает «Скачать»"
    );

    // Сообщения старше месяца; у C своё «никогда».
    a.set_chat_autodel(ChatKey::Dm(c_key), Some(Age::Never));
    a.set_autodel(&Rule {
        messages: Age::Month,
        ..att
    });
    let done = a.autodel_run_at(now() + 31 * DAY);
    assert_eq!(done.messages, 2, "старое и вложение у B");
    let left: Vec<String> = rows(&a.history_json(&b_key))
        .iter()
        .map(|m| m["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(left, vec!["закреплённое".to_string()]);
    assert_eq!(
        rows(&a.history_json(&c_key)).len(),
        1,
        "у C ничего не тронуто"
    );
    assert_eq!(
        rows(&a.history_json(&a_key)).len(),
        1,
        "«Сохранённое» не тронуто"
    );
    let mut pruned = false;
    while let Some(ev) = a.poll_event() {
        pruned |= ev.contains("\"history_pruned\"");
    }
    assert!(pruned, "клиент узнал, что история изменилась");

    // Повтор ничего не находит.
    assert_eq!(a.autodel_run_at(now() + 31 * DAY), Default::default());
    // Своё правило видно на экране «Автоудаление».
    let state: serde_json::Value = serde_json::from_str(&a.autodel_json()).unwrap();
    assert_eq!(state["rule"]["messages"], "month");
    assert_eq!(state["chats"][0]["chat"], hex::encode(c_key));
    assert_eq!(state["chats"][0]["age"], "never");
}
