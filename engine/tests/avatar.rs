//! Свой аватар (M2): контент-адресное хранение поверх блобов + снятие метаданных
//! (scrub) + персист через «рестарт» движка. Локально, без сети. Прогон из WSL:
//! LD_LIBRARY_PATH=ygg/libygg.

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
    let d = std::env::temp_dir().join(format!("test-av-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
}

/// JPEG-заглушка с EXIF (APP1) для проверки scrub.
fn jpeg_with_exif() -> Vec<u8> {
    let mut j = vec![0xFF, 0xD8]; // SOI
    j.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0]); // APP1/EXIF
    j.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34, 0xFF, 0xD9]); // SOS + поток + EOI
    j
}

#[test]
fn self_avatar_roundtrip_scrub_and_persist() {
    let net = LoopbackNet::new();
    let dir = tmpdir("a");
    let e = start(&net, id(1), dir.clone());

    let raw = jpeg_with_exif();
    let id_hex = e.set_self_avatar(raw.clone(), "image/jpeg");

    let got = e.self_avatar().expect("аватар сохранён и читается");
    // Метаданные сняты: EXIF-нагрузки в сохранённом аватаре нет.
    assert!(!got.windows(4).any(|w| w == b"Exif"), "EXIF снят scrub");
    // id — контент-адрес ОЧИЩЕННЫХ байт (а не исходных с EXIF).
    assert_eq!(id_hex, e.blob_id_hex(&got), "id = контент-адрес очищенного");
    assert_ne!(
        id_hex,
        e.blob_id_hex(&raw),
        "id ≠ адрес исходного (scrub изменил байты)"
    );
    assert_eq!(e.self_avatar_id().as_deref(), Some(id_hex.as_str()));

    // Персист: пересоздаём движок на том же каталоге — аватар на месте.
    e.shutdown();
    drop(e);
    let e2 = start(&net, id(1), dir);
    assert_eq!(e2.self_avatar().expect("аватар пережил рестарт"), got);
    e2.shutdown();
}
