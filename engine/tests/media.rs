//! Медиа в чате (M2): A отправляет картинку → B получает медиа-сообщение (ссылку на
//! блоб по хэшу), подтягивает байты блоб-каналом и может их прочитать. Контент-адрес
//! совпадает у обеих сторон, EXIF снят (scrub). Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

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
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("peerage-media-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

/// JPEG с EXIF-сегментом (scrub должен его снять на отправке).
fn jpeg_with_exif() -> Vec<u8> {
    let mut j = vec![0xFF, 0xD8];
    j.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0]);
    j.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34, 0xFF, 0xD9]);
    j
}

/// Найти в истории (JSON) первое сообщение заданного kind; вернуть (blob_id, mime, name).
fn find_media(engine: &Engine, peer_hex_key: &[u8; 32], kind: &str) -> Option<(String, String, String)> {
    let hist: serde_json::Value = serde_json::from_str(&engine.history_json(peer_hex_key)).ok()?;
    hist.as_array()?.iter().find(|m| m["kind"] == kind).map(|m| {
        (
            m["blob_id"].as_str().unwrap_or_default().to_string(),
            m["mime"].as_str().unwrap_or_default().to_string(),
            m["name"].as_str().unwrap_or_default().to_string(),
        )
    })
}

#[test]
fn image_message_delivers_and_blob_pulls() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(1), id(2));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    let msg_id = a.send_media(&b_key, jpeg_with_exif(), "image/jpeg", "photo.jpg").expect("send_media");
    assert!(msg_id >= 0);

    // У A вложение видно сразу (kind=image, блоб лежит локально).
    let (a_blob, a_mime, a_name) = find_media(&a, &b_key, "image").expect("A видит своё вложение");
    assert_eq!(a_mime, "image/jpeg");
    assert_eq!(a_name, "photo.jpg");
    let a_bytes = a.blob_read(&a_blob).expect("A хранит очищенный блоб");
    assert!(a_bytes.len() < jpeg_with_exif().len(), "scrub снял EXIF");

    // B получает медиа-сообщение и подтягивает тот же блоб по хэшу.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some((b_blob, b_mime, b_name)) = find_media(&b, &a_key, "image") {
            if let Some(b_bytes) = b.blob_read(&b_blob) {
                assert_eq!(b_blob, a_blob, "контент-адрес совпадает у обеих сторон");
                assert_eq!(b_bytes, a_bytes, "B подтянул очищенные байты A");
                assert_eq!(b_mime, "image/jpeg");
                assert_eq!(b_name, "photo.jpg");

                // M3, репутация: A потратил на нас трафик, отдав блоб → у B заслуги A
                // выросли (это и повышает бюджет ресурсов, см. budget::reputation_boost).
                let helpful = helpful_of(&b, &a_key);
                assert!(helpful > 0, "отдавший блоб получает заслугу, было {helpful}");

                a.shutdown();
                b.shutdown();
                return;
            }
        }
        assert!(Instant::now() < deadline, "B не получил картинку за 10с");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// Отправка ФАЙЛОМ: движок считает манифест потоково, отдаёт чанки с диска, а
/// получатель собирает тот же контент-адрес. Плюс подпись едет ВНУТРИ сообщения.
#[test]
fn file_attachment_with_caption_delivers() {
    peerage_core::vault::init_with_key([9u8; 32]);
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(21), id(22));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("fa"));
    let b = start(&net, b_id, tmpdir("fb"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    // 300 КБ — заведомо больше одного чанка и меньше порога автозагрузки.
    let content: Vec<u8> = (0..300 * 1024).map(|i| (i * 17 ^ 0x3c) as u8).collect();
    let dir = tmpdir("src");
    let path = dir.join("clip.bin");
    std::fs::write(&path, &content).unwrap();

    let info = peerage_engine::MediaInfo { caption: "подпись к файлу".into(), ..Default::default() };
    a.send_media_file(&b_key, &path, "application/octet-stream", "clip.bin", info)
        .expect("send_media_file");

    let want_id = hex::encode(peerage_blob::blob_id(&content));
    // Отправитель сохранил свою копию потоково и читает её обратно байт-в-байт.
    assert_eq!(a.blob_read(&want_id).as_deref(), Some(&content[..]), "своя копия цела");

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let hist: serde_json::Value = serde_json::from_str(&b.history_json(&a_key)).unwrap();
        if let Some(m) = hist.as_array().unwrap().iter().find(|m| m["blob_id"] == want_id) {
            assert_eq!(m["text"], "подпись к файлу", "подпись пришла вместе с файлом");
            if let Some(bytes) = b.blob_read(&want_id) {
                assert_eq!(bytes, content, "содержимое доехало байт-в-байт");
                a.shutdown();
                b.shutdown();
                return;
            }
        }
        assert!(Instant::now() < deadline, "файл не доехал за 20с");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// Счётчик заслуг пира из списка контактов.
fn helpful_of(engine: &Engine, peer: &[u8; 32]) -> u64 {
    let hex_key = hex::encode(peer);
    let v: serde_json::Value = serde_json::from_str(&engine.contacts_json()).unwrap();
    v.as_array()
        .and_then(|a| a.iter().find(|c| c["key"] == hex_key))
        .and_then(|c| c["helpful"].as_u64())
        .unwrap_or(0)
}
