//! Крупное вложение в ГРУППЕ не приезжает участникам само.
//!
//! Раньше отправитель пушил байты всем участникам сразу: один человек с видео на
//! 800 МБ решал за всю группу, потратят ли они трафик. Теперь в MLS-сообщении едут
//! только метаданные карточки (имя/размер/превью), а байты каждый участник
//! запрашивает сам — как в личке. Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::{Engine, MediaInfo};
use peerage_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("test-gmedia-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

/// Ждать условие, опрашивая раз в 30 мс. Возвращает false по таймауту.
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

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

/// gid единственной группы движка (hex → байты). Создание идёт через актора, так
/// что список появляется не мгновенно — ждём.
fn only_gid(e: &Engine) -> Vec<u8> {
    assert!(
        wait(15, || json(&e.group_list_json()).as_array().map(|v| !v.is_empty()).unwrap_or(false)),
        "группа появилась в списке",
    );
    let list = json(&e.group_list_json());
    let hex_gid = list.as_array().unwrap()[0]["gid"].as_str().unwrap().to_string();
    hex::decode(hex_gid).unwrap()
}

/// Первое медиа-сообщение в истории группы.
fn media_msg(e: &Engine, gid: &[u8]) -> Option<serde_json::Value> {
    json(&e.group_history_json(gid))
        .as_array()?
        .iter()
        .find(|m| m["blob_id"].as_str().map(|s| !s.is_empty()).unwrap_or(false))
        .cloned()
}

#[test]
fn large_group_attachment_waits_for_explicit_download() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(41), id(42));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    // Группа: A создаёт и зовёт B; B принимает карточку приглашения.
    assert!(a.group_create("клуб"), "группа создана");
    let gid = only_gid(&a);
    a.group_invite(gid.clone(), b_key);
    assert!(
        wait(15, || json(&b.group_invites_json()).as_array().map(|v| !v.is_empty()).unwrap_or(false)),
        "B получил приглашение",
    );
    b.accept_group_invite(gid.clone());
    assert!(
        wait(15, || json(&b.group_list_json()).as_array().map(|v| !v.is_empty()).unwrap_or(false)),
        "B вступил в группу",
    );
    assert!(wait(15, || a.group_members_json(&gid).contains(&hex::encode(b_key))), "A видит B в ростере");

    // 3 МБ — заведомо больше порога автозагрузки.
    let content: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i * 7 ^ 0x5a) as u8).collect();
    let info = MediaInfo {
        reply_to: 0,
        reply_sender: String::new(),
        duration_ms: 12_000,
        width: 1920,
        height: 1080,
        thumb: Some(vec![0xAB; 64]),
        caption: "большое видео".into(),
        fwd_from: String::new(),
        fwd_anon: false,
    };
    a.group_send_media(gid.clone(), content.clone(), "video/mp4".into(), "clip.mp4".into(), info);

    // Карточка доехала: размер и превью известны ДО байт, само вложение — нет.
    assert!(wait(20, || media_msg(&b, &gid).is_some()), "B получил групповое медиа-сообщение");
    let card = media_msg(&b, &gid).unwrap();
    let blob_id = card["blob_id"].as_str().unwrap().to_string();
    assert_eq!(card["size"].as_u64(), Some(content.len() as u64), "размер приехал в сообщении");
    assert_eq!(card["duration_ms"].as_u64(), Some(12_000));
    assert_eq!(card["text"].as_str(), Some("большое видео"), "подпись — внутри того же сообщения");
    assert_eq!(card["downloaded"].as_bool(), Some(false));
    assert!(!card["thumb_id"].as_str().unwrap().is_empty(), "превью — отдельным блобом");
    assert!(b.blob_read(&blob_id).is_none(), "крупное групповое вложение не тянется само");

    // Миниатюру, наоборот, отдаём всем сразу — без неё карточка слепая.
    let thumb_id = card["thumb_id"].as_str().unwrap().to_string();
    assert!(wait(20, || b.blob_read(&thumb_id).is_some()), "превью доехало без спроса");

    // Явное «скачать» — байты приезжают целиком, сообщение помечается скачанным.
    assert!(b.download_media(&a_key, &blob_id), "заказ принят");
    assert!(
        wait(60, || b.blob_read(&blob_id).map(|v| v == content).unwrap_or(false)),
        "заказанное вложение доехало байт-в-байт",
    );
    assert!(
        wait(10, || media_msg(&b, &gid).unwrap()["downloaded"].as_bool() == Some(true)),
        "история группы отметила вложение скачанным",
    );

    a.shutdown();
    b.shutdown();
}

/// Мелкое вложение (голосовое, фото) участники получают сразу — его ждут, а не
/// выбирают: карточка «скачать» на 40 КБ была бы издевательством.
#[test]
fn small_group_attachment_arrives_by_itself() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(43), id(44));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a2"));
    let b = start(&net, b_id, tmpdir("b2"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    assert!(a.group_create("клуб"), "группа создана");
    let gid = only_gid(&a);
    a.group_invite(gid.clone(), b_key);
    assert!(
        wait(15, || json(&b.group_invites_json()).as_array().map(|v| !v.is_empty()).unwrap_or(false)),
        "B получил приглашение",
    );
    b.accept_group_invite(gid.clone());
    assert!(wait(15, || a.group_members_json(&gid).contains(&hex::encode(b_key))), "A видит B в ростере");

    let content: Vec<u8> = (0..40_000).map(|i| (i % 251) as u8).collect();
    a.group_send_media(
        gid.clone(),
        content.clone(),
        "audio/mp4".into(),
        "voice.m4a".into(),
        MediaInfo { duration_ms: 3000, ..Default::default() },
    );

    assert!(wait(20, || media_msg(&b, &gid).is_some()), "B получил сообщение");
    let blob_id = media_msg(&b, &gid).unwrap()["blob_id"].as_str().unwrap().to_string();
    assert!(
        wait(30, || b.blob_read(&blob_id).map(|v| v == content).unwrap_or(false)),
        "мелкое вложение приехало без нажатия кнопки",
    );

    a.shutdown();
    b.shutdown();
}

/// R-028: подготовка крупного вложения для группы (хэш, своя зашифрованная копия)
/// не держит актор — входящее сообщение, пришедшее следом, обрабатывается сразу,
/// а не после того, как файл перешифрован целиком.
#[test]
fn preparing_group_file_does_not_block_the_actor() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(45), id(46));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a-prep"));
    let b = start(&net, b_id, tmpdir("b-prep"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");
    assert!(a.group_create("клуб"));
    let gid = only_gid(&a);

    let big: Vec<u8> = (0..40 * 1024 * 1024u32).map(|i| (i * 31 ^ 0x77) as u8).collect();
    let src = tmpdir("src-prep").join("film.mp4");
    std::fs::write(&src, &big).unwrap();
    drop(big);
    while a.poll_event().is_some() {}

    a.group_send_media_file(gid.clone(), src, "video/mp4".into(), "film.mp4".into(), MediaInfo::default());
    b.send_text(&a_key, "ты тут?").unwrap();

    // Что придёт первым: входящий текст или эхо своего вложения?
    let mut order = Vec::new();
    assert!(
        wait(60, || {
            while let Some(js) = a.poll_event() {
                let v = json(&js);
                if v["type"] == "message" {
                    order.push(if v["group"] == true { "media" } else { "text" });
                }
            }
            order.len() >= 2
        }),
        "оба события пришли: {order:?}",
    );
    assert_eq!(order, vec!["text", "media"], "актор не ждал перешифровки файла");
}
