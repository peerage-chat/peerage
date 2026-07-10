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
    jpeg_variant(0x12, 0x34)
}

/// JPEG с EXIF и заданными байтами энтропии — разные байты → разный scrubbed-контент
/// → разный контент-адрес (для теста смены аватара).
fn jpeg_variant(a: u8, b: u8) -> Vec<u8> {
    let mut j = vec![0xFF, 0xD8];
    j.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0]);
    j.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, a, b, 0xFF, 0xD9]);
    j
}

/// Дождаться, пока B увидит у контакта A заданные profile_name+avatar_id и подтянет
/// байты аватара по хэшу. Возвращает подтянутые байты. Паника по таймауту.
fn await_profile(b: &Engine, a_hex: &str, name: &str, avatar_id: &str) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cj: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
        let found = cj.as_array().and_then(|arr| arr.iter().find(|c| c["key"] == a_hex));
        if let Some(c) = found {
            if c["profile_name"] == name && c["avatar_id"] == avatar_id {
                if let Some(bytes) = b.blob_read(avatar_id) {
                    assert_eq!(c["name"], "A-note", "локальная заметка B не должна затираться");
                    return bytes;
                }
            }
        }
        assert!(Instant::now() < deadline, "B не получил профиль {name}/{avatar_id} за 10с");
        std::thread::sleep(Duration::from_millis(30));
    }
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

/// Смена профиля (дизайн M2: «аватар может меняться → шлём хэш → получатель
/// перезапрашивает по несовпадению»). A меняет имя и аватар на новые; B видит
/// НОВЫЙ хэш, тянет НОВЫЕ байты, а старый блоб остаётся читаемым (контент-адрес →
/// дедуп/ретенция). Локальная заметка B об A всё время не тронута.
#[test]
fn profile_change_repropagates_by_new_hash() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(3), id(4));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a2"));
    let b = start(&net, b_id, tmpdir("b2"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A-note");
    let a_hex = hex::encode(a_key);

    // Первая версия профиля.
    a.set_self_name("Алиса");
    let id1 = a.set_self_avatar(jpeg_variant(0x12, 0x34), "image/jpeg");
    let got1 = await_profile(&b, &a_hex, "Алиса", &id1);
    assert_eq!(got1, a.self_avatar().unwrap(), "B собрал первый аватар A");

    // Вторая версия: другое имя и ДРУГИЕ байты аватара → другой контент-адрес.
    a.set_self_name("Алиса-2");
    let id2 = a.set_self_avatar(jpeg_variant(0x56, 0x78), "image/jpeg");
    assert_ne!(id1, id2, "разный аватар → разный хэш");
    let got2 = await_profile(&b, &a_hex, "Алиса-2", &id2);
    assert_eq!(got2, a.self_avatar().unwrap(), "B подтянул НОВЫЙ аватар по новому хэшу");
    assert_ne!(got1, got2, "новые байты аватара отличаются от старых");

    // Старый блоб у B остаётся читаемым (контент-адрес — без перезаписи/удаления).
    assert_eq!(b.blob_read(&id1).as_deref(), Some(got1.as_slice()), "старый аватар сохранён у B");

    a.shutdown();
    b.shutdown();
}
