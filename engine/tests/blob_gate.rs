//! Блобы только по своей воле и только своим: манифест без нашего запроса не
//! качается, а наши файлы отдаются контактам и участникам общих групп, но не
//! незнакомцам. «Чужой» — голый мессенджер в той же loopback-сети.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_engine::{Engine, MediaInfo};
use p2p_messenger::loopback::LoopbackNet;
use p2p_messenger::{Config, Event, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-blobgate-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

fn raw(net: &LoopbackNet, ident: Identity) -> (Messenger, Receiver<Event>) {
    let key = ident.public_key_bytes();
    Messenger::start_with_transport(ident, net.endpoint(key), Config::default())
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

/// Сколько событий `pred` пришло голому мессенджеру за `secs`.
fn count_events(rx: &Receiver<Event>, secs: u64, pred: impl Fn(&Event) -> bool) -> usize {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut n = 0;
    while Instant::now() < deadline {
        while let Ok(ev) = rx.try_recv() {
            if pred(&ev) {
                n += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    n
}

/// R-020: манифест, который мы не просили, не качается — даже от контакта.
#[test]
fn unsolicited_manifest_is_ignored() {
    let net = LoopbackNet::new();
    let (a_id, m_id) = (id(91), id(92));
    let (a_key, m_key) = (a_id.public_key_bytes(), m_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    a.add_contact(&m_key, "M");
    let (m, m_rx) = raw(&net, m_id);

    let content = vec![0x42u8; 50_000];
    let (manifest, _chunks) = p2p_blob::build(&content, "application/octet-stream");
    for p in manifest.split() {
        let hashes: Vec<u8> = p.hashes.iter().flatten().copied().collect();
        m.send_blob_manifest_part(&a_key, &p.id, &p.mime, p.size, p.part, p.total, &hashes).unwrap();
    }
    let asked = count_events(&m_rx, 2, |ev| matches!(ev, Event::BlobRequest { .. }));
    assert_eq!(asked, 0, "A не запросил ни одного чанка");
    assert!(!a.blob_exists(&hex::encode(manifest.id)), "файл без спроса не появился");
}

/// R-009: свой блоб отдаём контакту, но не незнакомцу, знающему хэш.
#[test]
fn stored_blob_is_served_to_contact_only() {
    let net = LoopbackNet::new();
    let (a_id, c_id, s_id) = (id(93), id(94), id(95));
    let (a_key, c_key) = (a_id.public_key_bytes(), c_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a2"));
    a.add_contact(&c_key, "C");
    let (c, c_rx) = raw(&net, c_id);
    let (s, s_rx) = raw(&net, s_id);

    let content: Vec<u8> = (0..30_000u32).map(|i| (i * 13) as u8).collect();
    let blob = p2p_blob::blob_id(&content);
    a.send_media(&c_key, content, "application/pdf", "doc.pdf").unwrap();
    assert!(wait(5, || a.blob_exists(&hex::encode(blob))), "своя копия сохранена");
    // Отправитель больше не толкает файл сам: получатель попросит по хэшу.
    let pushed = count_events(&c_rx, 1, |ev| matches!(ev, Event::BlobManifestPart { .. }));
    assert_eq!(pushed, 0, "манифест не уходит без запроса");

    s.request_blob_manifest(&a_key, &blob).unwrap();
    let to_stranger = count_events(&s_rx, 2, |ev| matches!(ev, Event::BlobManifestPart { .. }));
    assert_eq!(to_stranger, 0, "незнакомцу блоб не отдан");

    c.request_blob_manifest(&a_key, &blob).unwrap();
    let to_contact = count_events(&c_rx, 2, |ev| matches!(ev, Event::BlobManifestPart { .. }));
    assert!(to_contact > 0, "контакту отдан");
}

/// R-003, R-021: крупное вложение, отправленное файлом, скачивается по кнопке —
/// отдающий берёт чанки из своего хранилища, получатель тянет окном.
#[test]
fn large_file_downloads_by_button() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(96), id(97));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a3"));
    let b = start(&net, b_id, tmpdir("b3"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    // Больше порога автодокачки (2 МБ): сам не приедет.
    let content: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let src = tmpdir("src").join("video.mp4");
    std::fs::write(&src, &content).unwrap();
    let blob = hex::encode(p2p_blob::blob_id(&content));
    a.send_media_file(&b_key, &src, "video/mp4", "video.mp4", MediaInfo::default()).unwrap();

    assert!(wait(10, || b.history_json(&a_key).contains(&blob)), "карточка пришла");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!b.blob_exists(&blob), "крупное не качается само");

    assert!(b.download_media(&a_key, &blob));
    assert!(wait(60, || b.blob_exists(&blob)), "скачалось по кнопке");
    assert_eq!(b.blob_read(&blob).unwrap(), content, "байт-в-байт");
}
