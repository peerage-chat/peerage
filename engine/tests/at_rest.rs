//! Шифрование на диске (R-038): файл привязан к своему назначению. Отдельный
//! бинарь, потому что ключ хранилища — один на процесс (`OnceLock`): здесь он
//! задан с первой строки каждого теста, а остальные наборы живут без него.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use p2p_core::store::{GroupStore, MessageStore};
use p2p_core::Identity;
use p2p_engine::groups::GroupManager;
use p2p_engine::Engine;
use p2p_messenger::loopback::LoopbackNet;
use p2p_messenger::{Config, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-atrest-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn with_key() {
    p2p_core::vault::init_with_key([42u8; 32]);
}

/// Файл блоба, переложенный в хранилище под чужим контент-адресом, не читается —
/// иначе его отдавали бы другим как тот блоб.
#[test]
fn blob_file_under_another_address_is_not_readable() {
    with_key();
    let net = LoopbackNet::new();
    let dir = tmpdir("blob");
    let ident = id(97);
    let a = Engine::start_with_transport(dir.clone(), ident.clone(), net.endpoint(ident.public_key_bytes()))
        .unwrap()
        .engine;
    let b_key = id(98).public_key_bytes();
    a.add_contact(&b_key, "B");
    let content = b"%PDF pretend document".to_vec();
    a.send_media(&b_key, content.clone(), "application/pdf", "doc.pdf").unwrap();
    let real = hex::encode(p2p_blob::blob_id(&content));
    assert_eq!(a.blob_read(&real).as_deref(), Some(&content[..]), "свой блоб читается");

    let other = hex::encode(p2p_blob::blob_id(b"something else"));
    std::fs::copy(dir.join("blobs").join(&real), dir.join("blobs").join(&other)).unwrap();
    assert!(a.blob_read(&other).is_none(), "под чужим адресом — не читается");
}

fn manager(ident: Identity, msgr: Arc<Messenger>, dir: PathBuf) -> GroupManager {
    let db = p2p_core::db::open(&dir).expect("база");
    let store = Arc::new(Mutex::new(MessageStore::open(db.clone())));
    GroupManager::new(ident, msgr, dir, store, GroupStore::new(db))
}

/// Состояние одной группы, подложенное под имя другой, не подменяет её: после
/// перезапуска подменённая группа просто не поднимается, а настоящая — на месте.
#[test]
fn group_state_under_another_name_is_rejected() {
    with_key();
    let net = LoopbackNet::new();
    let ident = id(99);
    let (msgr, _rx) = Messenger::start_with_transport(ident.clone(), net.endpoint(ident.public_key_bytes()), Config::default());
    let msgr = Arc::new(msgr);
    let dir = tmpdir("group");
    let (g1, g2) = {
        let mut gm = manager(ident.clone(), msgr.clone(), dir.clone());
        (gm.create("первая").unwrap(), gm.create("вторая").unwrap())
    };
    let groups = dir.join("groups");
    std::fs::copy(
        groups.join(format!("{}.group", hex::encode(&g1))),
        groups.join(format!("{}.group", hex::encode(&g2))),
    )
    .unwrap();

    let gm = manager(ident, msgr, dir);
    let ids = gm.group_ids();
    assert!(ids.contains(&g1), "настоящая группа на месте");
    assert!(!ids.contains(&g2), "подменённая не поднялась");
}
