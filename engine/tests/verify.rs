//! Сверка ключа (шаг 3.6): один набор на пару и отметка по коду — когда и как.
//!
//! Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let d = std::env::temp_dir().join(format!("test-verify-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, tag: &str) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(tmpdir(tag), ident, net.endpoint(key))
        .unwrap()
        .engine
}

fn contact(e: &Engine, key: &[u8; 32]) -> serde_json::Value {
    let all: serde_json::Value = serde_json::from_str(&e.contacts_json()).unwrap();
    all.as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == hex::encode(key))
        .cloned()
        .unwrap()
}

/// Оба телефона показывают один и тот же набор — его и сверяют.
#[test]
fn both_sides_see_the_same_set() {
    let net = LoopbackNet::new();
    let (a, b) = (start(&net, id(61), "a"), start(&net, id(62), "b"));
    let (ak, bk) = (a.me_pub(), b.me_pub());
    let at_a: serde_json::Value = serde_json::from_str(&a.pair_fingerprint_json(&bk)).unwrap();
    let at_b: serde_json::Value = serde_json::from_str(&b.pair_fingerprint_json(&ak)).unwrap();
    assert_eq!(at_a, at_b);
    assert_eq!(at_a["glyphs"].as_str().unwrap().len(), 24, "12 знаков hex");
    assert_eq!(at_a["words"].as_array().unwrap().len(), 8);
}

/// Код собеседника совпал — сверен «по коду» с датой; чужой код и мусор отметку не дают.
#[test]
fn scanned_code_marks_verified_with_date_and_way() {
    let net = LoopbackNet::new();
    let (a, b, c) = (
        start(&net, id(63), "a2"),
        start(&net, id(64), "b2"),
        start(&net, id(65), "c2"),
    );
    let bk = b.me_pub();
    a.add_contact(&bk, "B");

    assert_eq!(a.verify_by_code(&bk, "не код"), "invalid");
    assert_eq!(a.verify_by_code(&bk, &c.my_contact_code()), "mismatch");
    assert_eq!(
        contact(&a, &bk)["verified"],
        false,
        "чужой код отметку не даёт"
    );

    assert_eq!(a.verify_by_code(&bk, &b.my_contact_code()), "match");
    let got = contact(&a, &bk);
    assert_eq!(
        (got["verified"].clone(), got["verified_how"].clone()),
        (true.into(), "code".into())
    );
    assert!(got["verified_at"].as_i64().unwrap() > 0);

    // Снять — снимается и «когда/как».
    a.set_verified(&bk, None);
    let got = contact(&a, &bk);
    assert_eq!(
        (got["verified"].clone(), got["verified_at"].clone()),
        (false.into(), 0.into())
    );
    assert!(got["verified_how"].is_null());
}
