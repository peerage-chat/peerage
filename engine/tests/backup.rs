//! Резервная копия ДАННЫХ: экспорт → импорт на чистый движок.
//!
//! Смысл проверок — не «файл создался», а три свойства, ради которых копия и делается:
//! выбранное действительно попадает в архив (и невыбранное НЕ попадает), данные
//! доезжают до другого устройства целыми, а восстановление СЛИВАЕТ, а не затирает.
//!
//! Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_engine::backup::Parts;
use p2p_engine::Engine;
use p2p_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-backup-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

fn all() -> Parts {
    Parts { contacts: true, messages: true, groups: true, notes: true, calls: true, attachments: true }
}

/// Наполнить движок: контакт, переписка, заметка, вложение.
fn fill(e: &Engine, peer: &[u8; 32]) {
    e.add_contact(peer, "Аня");
    e.send_text(peer, "привет");
    e.send_text(peer, "как дела?");
    e.save_note("не забыть про хлеб");
    e.send_media(peer, b"pretend jpeg bytes".to_vec(), "image/jpeg", "photo.jpg");
}

#[test]
fn export_import_restores_conversation_on_a_clean_device() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(81), id(82));
    let peer = b_id.public_key_bytes();
    let a = start(&net, a_id, tmpdir("src"));
    fill(&a, &peer);

    let file = tmpdir("file").join("copy.p2pbak");
    let out = a.backup_export(&file, all(), "пароль-копии").unwrap();
    assert_eq!(out.contacts, 1);
    assert_eq!(out.messages, 3, "два текста + медиа");
    assert_eq!(out.notes, 1);
    assert_eq!(out.attachments, 1, "байты вложения в копии");

    // Чистое устройство: другая личность, пустая база.
    let fresh = start(&net, id(83), tmpdir("dst"));
    let got = fresh.backup_import(&file, "пароль-копии").unwrap();
    assert_eq!(got.contacts, 1);
    assert_eq!(got.messages, 3);
    assert_eq!(got.notes, 1);

    let hist: serde_json::Value = serde_json::from_str(&fresh.history_json(&peer)).unwrap();
    let texts: Vec<&str> = hist
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["text"].as_str())
        .collect();
    assert!(texts.contains(&"привет") && texts.contains(&"как дела?"), "тексты на месте: {texts:?}");

    // Вложение доступно локально, а не только ссылкой на него.
    let blob_id = hist
        .as_array()
        .unwrap()
        .iter()
        .find_map(|m| m["blob_id"].as_str().filter(|s| !s.is_empty()))
        .expect("медиа-сообщение с блобом");
    assert_eq!(fresh.blob_read(blob_id).as_deref(), Some(&b"pretend jpeg bytes"[..]));
}

#[test]
fn restore_does_not_grant_verification() {
    // Файл могли подсунуть: «вот твоя копия, вот пароль». Верификация означает
    // «я сверил отпечаток вживую» — из файла это утверждение не восстанавливается,
    // иначе чужой ключ приезжал бы сразу с высшим доверием и без плашки в UI.
    let net = LoopbackNet::new();
    let peer = id(98).public_key_bytes();
    let a = start(&net, id(97), tmpdir("ver-src"));
    fill(&a, &peer);
    a.set_verified(&peer, true); // у источника контакт сверен вживую

    let file = tmpdir("ver").join("copy.p2pbak");
    a.backup_export(&file, all(), "пк").unwrap();

    let b = start(&net, id(99), tmpdir("ver-dst"));
    b.backup_import(&file, "пк").unwrap();
    let contacts: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    assert_eq!(contacts[0]["key"], hex::encode(peer), "контакт восстановлен");
    assert_eq!(contacts[0]["verified"], false, "но сверять его нужно заново");
}

#[test]
fn only_selected_parts_are_included() {
    let net = LoopbackNet::new();
    let peer = id(85).public_key_bytes();
    let a = start(&net, id(84), tmpdir("sel-src"));
    fill(&a, &peer);

    // Только контакты: ни переписки, ни заметок, ни файлов.
    let file = tmpdir("sel").join("contacts-only.p2pbak");
    let out = a
        .backup_export(&file, Parts { contacts: true, ..Default::default() }, "пк")
        .unwrap();
    assert_eq!(out.contacts, 1);
    assert_eq!((out.messages, out.notes, out.calls), (0, 0, 0));

    let fresh = start(&net, id(86), tmpdir("sel-dst"));
    let got = fresh.backup_import(&file, "пк").unwrap();
    assert_eq!(got.contacts, 1);
    assert_eq!(got.messages, 0, "невыбранная переписка не должна появиться");
    let hist: serde_json::Value = serde_json::from_str(&fresh.history_json(&peer)).unwrap();
    assert!(hist.as_array().unwrap().is_empty());
}

#[test]
fn notes_are_separate_from_conversations() {
    // Заметки себе — чат с собой; человек может хотеть переписку без личных заметок.
    let net = LoopbackNet::new();
    let peer = id(88).public_key_bytes();
    let a = start(&net, id(87), tmpdir("notes-src"));
    fill(&a, &peer);

    let file = tmpdir("notes").join("msgs.p2pbak");
    let out = a
        .backup_export(&file, Parts { messages: true, ..Default::default() }, "пк")
        .unwrap();
    assert_eq!(out.notes, 0, "заметки не выбраны — их в копии нет");
    assert_eq!(out.messages, 3);
}

#[test]
fn import_merges_and_is_idempotent() {
    // Восстановление не должно ни затирать более новое, ни задваивать при повторе.
    let net = LoopbackNet::new();
    let peer = id(90).public_key_bytes();
    let a = start(&net, id(89), tmpdir("merge-src"));
    fill(&a, &peer);
    let file = tmpdir("merge").join("copy.p2pbak");
    a.backup_export(&file, all(), "пк").unwrap();

    let b = start(&net, id(91), tmpdir("merge-dst"));
    b.add_contact(&peer, "Аня (переименована после копии)");
    b.send_text(&peer, "сообщение, которого не было в копии");

    let first = b.backup_import(&file, "пк").unwrap();
    assert_eq!(first.contacts, 0, "существующий контакт не перезаписывается");
    assert_eq!(first.messages, 3);

    let second = b.backup_import(&file, "пк").unwrap();
    assert_eq!(second.messages, 0, "повторный импорт того же файла ничего не добавляет");

    let hist: serde_json::Value = serde_json::from_str(&b.history_json(&peer)).unwrap();
    let texts: Vec<&str> = hist.as_array().unwrap().iter().filter_map(|m| m["text"].as_str()).collect();
    assert!(texts.contains(&"привет"), "восстановленное на месте");
    assert!(texts.contains(&"сообщение, которого не было в копии"), "местное не затёрто");
    // Локальное имя контакта важнее того, что лежало в архиве.
    let contacts: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    assert_eq!(contacts[0]["name"], "Аня (переименована после копии)");
}

#[test]
fn wrong_password_does_not_restore_anything() {
    let net = LoopbackNet::new();
    let peer = id(93).public_key_bytes();
    let a = start(&net, id(92), tmpdir("pw-src"));
    fill(&a, &peer);
    let file = tmpdir("pw").join("copy.p2pbak");
    a.backup_export(&file, all(), "правильный").unwrap();

    let b = start(&net, id(94), tmpdir("pw-dst"));
    assert!(b.backup_import(&file, "неправильный").is_err());
    let hist: serde_json::Value = serde_json::from_str(&b.history_json(&peer)).unwrap();
    assert!(hist.as_array().unwrap().is_empty(), "ничего не должно просочиться");
}

#[test]
fn groups_are_not_promised_by_the_archive() {
    // Явная фиксация границы: MLS-состояние в копию не входит (устаревший ратчет
    // ломает группу). Если это когда-нибудь изменится — тест обязан упасть первым.
    let net = LoopbackNet::new();
    let a = start(&net, id(95), tmpdir("grp-src"));
    assert!(a.group_create("Тестовая"));
    // Актор создаёт группу в своём потоке — дожидаемся, иначе проверка вырождается
    // в «пустое не восстановилось из пустого».
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let list: serde_json::Value = serde_json::from_str(&a.group_list_json()).unwrap();
        if !list.as_array().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let list: serde_json::Value = serde_json::from_str(&a.group_list_json()).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1, "группа у источника есть");
    let gid = hex::decode(list[0]["gid"].as_str().unwrap()).unwrap();
    a.group_send_text(gid, "сообщение в группе".into());

    let file = tmpdir("grp").join("copy.p2pbak");
    a.backup_export(&file, all(), "пк").unwrap();

    let b = start(&net, id(96), tmpdir("grp-dst"));
    b.backup_import(&file, "пк").unwrap();
    let groups: serde_json::Value = serde_json::from_str(&b.group_list_json()).unwrap();
    assert!(groups.as_array().unwrap().is_empty(), "группы не восстанавливаются из копии");
}
