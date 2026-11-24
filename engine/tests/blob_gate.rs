//! Блобы только по своей воле и только своим: манифест без нашего запроса не
//! качается, а наши файлы отдаются контактам и участникам общих групп, но не
//! незнакомцам. «Чужой» — голый мессенджер в той же loopback-сети.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::{Engine, MediaInfo};
use peerage_messenger::loopback::LoopbackNet;
use peerage_messenger::{Config, Event, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-blobgate-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
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
    let (manifest, _chunks) = peerage_blob::build(&content, "application/octet-stream");
    for p in manifest.split() {
        let hashes: Vec<u8> = p.hashes.iter().flatten().copied().collect();
        m.send_blob_manifest_part(&a_key, &p.id, &p.mime, p.size, p.part, p.total, &hashes)
            .unwrap();
    }
    let asked = count_events(&m_rx, 2, |ev| matches!(ev, Event::BlobRequest { .. }));
    assert_eq!(asked, 0, "A не запросил ни одного чанка");
    assert!(
        !a.blob_exists(&hex::encode(manifest.id)),
        "файл без спроса не появился"
    );
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
    let blob = peerage_blob::blob_id(&content);
    a.send_media(&c_key, content, "application/pdf", "doc.pdf")
        .unwrap();
    assert!(
        wait(5, || a.blob_exists(&hex::encode(blob))),
        "своя копия сохранена"
    );
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
    let blob = hex::encode(peerage_blob::blob_id(&content));
    a.send_media_file(&b_key, &src, "video/mp4", "video.mp4", MediaInfo::default())
        .unwrap();

    assert!(
        wait(10, || b.history_json(&a_key).contains(&blob)),
        "карточка пришла"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(!b.blob_exists(&blob), "крупное не качается само");

    assert!(b.download_media(&a_key, &blob));
    assert!(wait(60, || b.blob_exists(&blob)), "скачалось по кнопке");
    assert_eq!(b.blob_read(&blob).unwrap(), content, "байт-в-байт");

    // R-044: открыть/проиграть — потоковой выгрузкой в файл, без чтения в память.
    let out = tmpdir("out").join("video.mp4");
    assert!(b.blob_export(&blob, &out), "выгружено");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        content,
        "выгрузка байт-в-байт"
    );
    assert!(
        !out.with_extension("part").exists(),
        "временный .part убран"
    );
    assert!(
        !b.blob_export("00", &tmpdir("out2").join("x")),
        "нет блоба — false"
    );
}

/// R-030: «Очистить вложения» убирает сами файлы, но не свой аватар и не
/// миниатюры; пометка «скачано» снимается, идущая докачка бросается.
#[test]
fn clearing_attachments_keeps_avatar_and_thumbs() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(98), id(99));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let dir_a = tmpdir("a4");
    let a = start(&net, a_id, dir_a.clone());
    let b = start(&net, b_id, tmpdir("b4"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    let avatar = a.set_self_avatar(vec![5u8; 300], "image/png");
    // Мелкое вложение с миниатюрой — приезжает само.
    let thumb = vec![6u8; 200];
    let thumb_id = hex::encode(peerage_blob::blob_id(&thumb));
    let small: Vec<u8> = (0..20_000u32).map(|i| (i % 13) as u8).collect();
    let small_id = hex::encode(peerage_blob::blob_id(&small));
    let info = MediaInfo {
        thumb: Some(thumb),
        ..Default::default()
    };
    b.send_media_with(&a_key, small, "application/pdf", "doc.pdf", info)
        .unwrap();
    assert!(
        wait(10, || a.blob_exists(&small_id) && a.blob_exists(&thumb_id)),
        "вложение и миниатюра у A"
    );

    // Крупное — качается по кнопке; очистка застаёт его на середине.
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let src = tmpdir("src4").join("v.mp4");
    std::fs::write(&src, &big).unwrap();
    let big_id = hex::encode(peerage_blob::blob_id(&big));
    b.send_media_file(&a_key, &src, "video/mp4", "v.mp4", MediaInfo::default())
        .unwrap();
    assert!(
        wait(10, || a.history_json(&b_key).contains(&big_id)),
        "карточка крупного пришла"
    );
    assert!(a.download_media(&b_key, &big_id));
    std::thread::sleep(Duration::from_millis(200));

    a.clear_blobs();
    assert!(a.blob_exists(&avatar), "свой аватар на месте");
    assert!(a.blob_exists(&thumb_id), "миниатюра на месте");
    assert!(!a.blob_exists(&small_id), "вложение удалено");
    let hist: serde_json::Value = serde_json::from_str(&a.history_json(&b_key)).unwrap();
    let row = hist
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["blob_id"] == small_id.as_str())
        .unwrap();
    assert_eq!(
        row["downloaded"], false,
        "карточка снова предлагает «Скачать»"
    );

    // Брошенная докачка не дописывается и не оставляет недособранных файлов.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!a.blob_exists(&big_id), "докачка брошена");
    let parts = std::fs::read_dir(dir_a.join("blobs"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "part")
        })
        .count();
    assert_eq!(parts, 0, "недособранных файлов не осталось");
}

/// Очистка вложений по чатам и типам (п. 27 с телефона): уходит только то, что под фильтром;
/// блоб, который тот же файл держит и в другом чате, остаётся; предпросмотр называет ровно
/// столько, сколько потом освободится; пустой фильтр — всё, как раньше.
#[test]
fn clear_by_chat_and_kind_keeps_what_others_still_show() {
    use peerage_engine::{ChatKey, ClearFilter};
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(101), id(102), id(103));
    let (a_key, b_key, c_key) = (
        a_id.public_key_bytes(),
        b_id.public_key_bytes(),
        c_id.public_key_bytes(),
    );
    let dir_a = tmpdir("a5");
    let a = start(&net, a_id, dir_a.clone());
    let b = start(&net, b_id, tmpdir("b5"));
    let c = start(&net, c_id, tmpdir("c5"));
    a.add_contact(&b_key, "B");
    a.add_contact(&c_key, "C");
    b.add_contact(&a_key, "A");
    c.add_contact(&a_key, "A");

    let bytes =
        |seed: u32| -> Vec<u8> { (0..20_000u32).map(|i| ((i * seed) % 251) as u8).collect() };
    let blob = |v: &Vec<u8>| hex::encode(peerage_blob::blob_id(v));
    let (shared, only_b, doc, only_c) = (bytes(3), bytes(5), bytes(7), bytes(11));
    let (shared_id, only_b_id, doc_id, only_c_id) =
        (blob(&shared), blob(&only_b), blob(&doc), blob(&only_c));
    b.send_media(&a_key, shared.clone(), "image/png", "s.png")
        .unwrap();
    b.send_media(&a_key, only_b.clone(), "image/png", "b.png")
        .unwrap();
    b.send_media(&a_key, doc.clone(), "application/pdf", "d.pdf")
        .unwrap();
    c.send_media(&a_key, shared.clone(), "image/png", "s.png")
        .unwrap();
    c.send_media(&a_key, only_c.clone(), "image/png", "c.png")
        .unwrap();
    assert!(
        wait(15, || [&shared_id, &only_b_id, &doc_id, &only_c_id]
            .iter()
            .all(|i| a.blob_exists(i))
            && a.history_json(&c_key)
                .matches("\"downloaded\":true")
                .count()
                == 2),
        "все вложения у A"
    );

    // Картинки чата B: общая с C картинка остаётся — её показывает чат C.
    let b_images = ClearFilter {
        chats: vec![ChatKey::Dm(b_key)],
        kinds: vec!["image".into()],
    };
    // Освобождаются байты на диске (блоб лежит запечатанным), а не длина содержимого.
    let on_disk = |id: &str| {
        std::fs::metadata(dir_a.join("blobs").join(id))
            .unwrap()
            .len()
    };
    let only_b_disk = on_disk(&only_b_id);
    assert_eq!(
        a.clear_preview(&b_images),
        only_b_disk,
        "предпросмотр = то, что уйдёт"
    );
    assert_eq!(a.clear_blobs_where(&b_images), only_b_disk);
    assert!(!a.blob_exists(&only_b_id), "картинка только из B удалена");
    assert!(a.blob_exists(&shared_id), "общая с C — на месте");
    assert!(a.blob_exists(&doc_id), "файл B — другой тип, на месте");
    assert!(a.blob_exists(&only_c_id), "чат C не трогали");
    let downloaded = |peer: &[u8; 32], id: &str| -> bool {
        let h: serde_json::Value = serde_json::from_str(&a.history_json(peer)).unwrap();
        h.as_array()
            .unwrap()
            .iter()
            .find(|m| m["blob_id"] == id)
            .unwrap()["downloaded"]
            == true
    };
    assert!(
        !downloaded(&b_key, &only_b_id),
        "удалённая снова предлагает «Скачать»"
    );
    assert!(
        downloaded(&b_key, &shared_id),
        "оставшаяся — по-прежнему скачана"
    );

    // Только тип — во всех чатах.
    let files = ClearFilter {
        chats: vec![],
        kinds: vec!["file".into()],
    };
    let doc_disk = on_disk(&doc_id);
    assert_eq!(a.clear_blobs_where(&files), doc_disk);
    assert!(!a.blob_exists(&doc_id) && a.blob_exists(&shared_id));

    // Пустой фильтр — всё.
    a.clear_blobs_where(&ClearFilter::default());
    assert!(
        !a.blob_exists(&shared_id) && !a.blob_exists(&only_c_id),
        "всё удалено"
    );
}

/// Фильтр с клиента: битый чат или незнакомый тип не расширяют очистку до «всего».
#[test]
fn clear_filter_from_json_rejects_what_it_cannot_read() {
    use peerage_engine::ClearFilter;
    let key = hex::encode([7u8; 32]);
    let f = ClearFilter::from_json(&format!(
        r#"{{"chats":[{{"chat":"{key}"}}],"kinds":["image","file"]}}"#
    ))
    .unwrap();
    assert_eq!((f.chats.len(), f.kinds.len()), (1, 2));
    assert!(
        ClearFilter::from_json(r#"{"chats":[{"chat":"zz"}]}"#).is_none(),
        "битый чат"
    );
    assert!(
        ClearFilter::from_json(r#"{"kinds":["document"]}"#).is_none(),
        "незнакомый тип"
    );
    assert!(ClearFilter::from_json("{").is_none());
    let all = ClearFilter::from_json("{}").unwrap();
    assert!(all.chats.is_empty() && all.kinds.is_empty(), "пустой — всё");
}

/// «По чатам» считает файлы, а не сообщения: одно фото, отправленное дважды, весит в чате
/// один раз — иначе чат «весил» больше всего хранилища.
#[test]
fn storage_by_chat_counts_a_file_once() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(111), id(112));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let dir_a = tmpdir("a6");
    let a = start(&net, a_id, dir_a.clone());
    let b = start(&net, b_id, tmpdir("b6"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");
    let pic: Vec<u8> = (0..20_000u32).map(|i| ((i * 13) % 251) as u8).collect();
    let pic_id = hex::encode(peerage_blob::blob_id(&pic));
    b.send_media(&a_key, pic.clone(), "image/png", "p.png")
        .unwrap();
    b.send_media(&a_key, pic.clone(), "image/png", "p.png")
        .unwrap();
    assert!(
        wait(15, || a.blob_exists(&pic_id)
            && a.history_json(&b_key)
                .matches("\"downloaded\":true")
                .count()
                == 2),
        "оба сообщения с одним файлом у A"
    );
    let on_disk = std::fs::metadata(dir_a.join("blobs").join(&pic_id))
        .unwrap()
        .len();
    let st: serde_json::Value = serde_json::from_str(&a.storage_stats_json()).unwrap();
    let chat = &st["chats"][0];
    assert_eq!(chat["files"], 1, "один файл");
    assert_eq!(chat["bytes"], on_disk, "и его размер — один раз");
}
