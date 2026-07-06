//! Распространение профиля (M2): A задаёт имя + аватар → B (контакт) получает
//! `ProfileUpdate`, видит самоназвание, тянет аватар-блоб ПО ХЭШУ и собирает его.
//! Локальная заметка B об A не затирается. Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

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
    let d = std::env::temp_dir().join(format!("p2p-prof-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

fn jpeg_with_exif() -> Vec<u8> {
    let mut j = vec![0xFF, 0xD8];
    j.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0]);
    j.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34, 0xFF, 0xD9]);
    j
}

#[test]
fn profile_propagates_name_and_avatar_by_hash() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(1), id(2));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A-note"); // локальная заметка B об A — НЕ должна затереться

    // A задаёт профиль → broadcast_profile шлёт ProfileUpdate контакту B.
    a.set_self_name("Алиса");
    let avatar_id = a.set_self_avatar(jpeg_with_exif(), "image/jpeg");

    let a_hex = hex::encode(a_key);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cj: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
        let found = cj.as_array().and_then(|arr| arr.iter().find(|c| c["key"] == a_hex));
        if let Some(c) = found {
            if c["profile_name"] == "Алиса" && c["avatar_id"] == avatar_id {
                // B подтянул аватар по хэшу?
                if let Some(bytes) = b.blob_read(&avatar_id) {
                    assert_eq!(bytes, a.self_avatar().unwrap(), "аватар у B = очищенный аватар A");
                    assert_eq!(c["name"], "A-note", "локальная заметка B не затёрта");
                    // avatar_id — контент-адрес ОЧИЩЕННОГО (scrub снял EXIF).
                    assert_eq!(avatar_id, a.blob_id_hex(&bytes));
                    a.shutdown();
                    b.shutdown();
                    return;
                }
            }
        }
        assert!(Instant::now() < deadline, "B не получил профиль/аватар A за 10с");
        std::thread::sleep(Duration::from_millis(30));
    }
}
