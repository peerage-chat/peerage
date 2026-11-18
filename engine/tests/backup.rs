//! Резервная копия ДАННЫХ: экспорт → импорт на чистый движок.
//!
//! Смысл проверок — не «файл создался», а три свойства, ради которых копия и делается:
//! выбранное действительно попадает в архив (и невыбранное НЕ попадает), данные
//! доезжают до другого устройства целыми, а восстановление СЛИВАЕТ, а не затирает.
//!
//! Копию открывает только та же личность (шаг 7k: ключ из 24 слов), поэтому «чистое
//! устройство» в тестах — та же личность с пустой базой на своей сети ([`twin`]).
//!
//! Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::backup::Parts;
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
    let d = std::env::temp_dir().join(format!("test-backup-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
}

/// Тот же человек на новом телефоне: личность `n` (восстановлена по 24 словам), пустая
/// база. Своя сеть: два узла с одним ключом в одной сети делили бы входящие.
fn twin(n: u8, tag: &str) -> Arc<Engine> {
    start(&LoopbackNet::new(), id(n), tmpdir(tag))
}

fn all() -> Parts {
    Parts {
        contacts: true,
        messages: true,
        groups: true,
        notes: true,
        calls: true,
        attachments: true,
    }
}

/// Подождать условия: групповые команды исполняет актор в своём потоке, поэтому
/// сразу после вызова читать результат рано.
fn wait(secs: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    cond()
}

/// Наполнить движок: контакт, переписка, заметка, вложение.
fn fill(e: &Engine, peer: &[u8; 32]) {
    e.add_contact(peer, "Аня");
    e.send_text(peer, "привет");
    e.send_text(peer, "как дела?");
    e.save_note("не забыть про хлеб");
    e.send_media(
        peer,
        b"pretend jpeg bytes".to_vec(),
        "image/jpeg",
        "photo.jpg",
    );
}

#[test]
fn export_import_restores_conversation_on_a_clean_device() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(81), id(82));
    let peer = b_id.public_key_bytes();
    let a = start(&net, a_id, tmpdir("src"));
    fill(&a, &peer);

    let file = tmpdir("file").join("copy.peerage");
    let out = a.backup_export(&file, all(), Some("пароль-копии")).unwrap();
    assert_eq!(out.contacts, 1);
    assert_eq!(out.messages, 3, "два текста + медиа");
    assert_eq!(out.notes, 1);
    assert_eq!(out.attachments, 1, "байты вложения в копии");

    // Чистое устройство: та же личность, пустая база.
    let fresh = twin(81, "dst");
    let got = fresh.backup_import(&file, Some("пароль-копии")).unwrap();
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
    assert!(
        texts.contains(&"привет") && texts.contains(&"как дела?"),
        "тексты на месте: {texts:?}"
    );

    // Вложение доступно локально, а не только ссылкой на него.
    let blob_id = hist
        .as_array()
        .unwrap()
        .iter()
        .find_map(|m| m["blob_id"].as_str().filter(|s| !s.is_empty()))
        .expect("медиа-сообщение с блобом");
    assert_eq!(
        fresh.blob_read(blob_id).as_deref(),
        Some(&b"pretend jpeg bytes"[..])
    );
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
    a.set_verified(&peer, Some(peerage_core::store::VerifyHow::Set)); // у источника контакт сверен вживую

    let file = tmpdir("ver").join("copy.peerage");
    a.backup_export(&file, all(), Some("пк")).unwrap();

    let b = twin(97, "ver-dst");
    b.backup_import(&file, Some("пк")).unwrap();
    let contacts: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    assert_eq!(
        contacts[0]["key"],
        hex::encode(peer),
        "контакт восстановлен"
    );
    assert_eq!(
        contacts[0]["verified"], false,
        "но сверять его нужно заново"
    );
}

#[test]
fn only_selected_parts_are_included() {
    let net = LoopbackNet::new();
    let peer = id(85).public_key_bytes();
    let a = start(&net, id(84), tmpdir("sel-src"));
    fill(&a, &peer);

    // Только контакты: ни переписки, ни заметок, ни файлов.
    let file = tmpdir("sel").join("contacts-only.peerage");
    let out = a
        .backup_export(
            &file,
            Parts {
                contacts: true,
                ..Default::default()
            },
            Some("пк"),
        )
        .unwrap();
    assert_eq!(out.contacts, 1);
    assert_eq!((out.messages, out.notes, out.calls), (0, 0, 0));

    let fresh = twin(84, "sel-dst");
    let got = fresh.backup_import(&file, Some("пк")).unwrap();
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

    let file = tmpdir("notes").join("msgs.peerage");
    let out = a
        .backup_export(
            &file,
            Parts {
                messages: true,
                ..Default::default()
            },
            Some("пк"),
        )
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
    let file = tmpdir("merge").join("copy.peerage");
    a.backup_export(&file, all(), Some("пк")).unwrap();

    let b = twin(89, "merge-dst");
    b.add_contact(&peer, "Аня (переименована после копии)");
    b.send_text(&peer, "сообщение, которого не было в копии");

    let first = b.backup_import(&file, Some("пк")).unwrap();
    assert_eq!(
        first.contacts, 0,
        "существующий контакт не перезаписывается"
    );
    assert_eq!(first.messages, 3);

    let second = b.backup_import(&file, Some("пк")).unwrap();
    assert_eq!(
        second.messages, 0,
        "повторный импорт того же файла ничего не добавляет"
    );

    let hist: serde_json::Value = serde_json::from_str(&b.history_json(&peer)).unwrap();
    let texts: Vec<&str> = hist
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["text"].as_str())
        .collect();
    assert!(texts.contains(&"привет"), "восстановленное на месте");
    assert!(
        texts.contains(&"сообщение, которого не было в копии"),
        "местное не затёрто"
    );
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
    let file = tmpdir("pw").join("copy.peerage");
    a.backup_export(&file, all(), Some("правильный")).unwrap();

    let b = twin(92, "pw-dst");
    assert!(b.backup_import(&file, Some("неправильный")).is_err());
    let hist: serde_json::Value = serde_json::from_str(&b.history_json(&peer)).unwrap();
    assert!(
        hist.as_array().unwrap().is_empty(),
        "ничего не должно просочиться"
    );
}

/// Граница, которую копия НЕ переходит: членство в группе. MLS-состояние в неё не
/// входит (устаревший ратчет ломает группу), поэтому восстановленная группа — только
/// архив: писать в неё нельзя, пока не позовут заново. Если это когда-нибудь
/// изменится — тест обязан упасть первым.
#[test]
fn restored_group_is_read_only() {
    let net = LoopbackNet::new();
    let a = start(&net, id(95), tmpdir("grp-src"));
    assert!(a.group_create("Тестовая"));
    assert!(
        wait(10, || a.group_list_json().contains("Тестовая")),
        "группа у источника есть"
    );
    let list: serde_json::Value = serde_json::from_str(&a.group_list_json()).unwrap();
    let gid = hex::decode(list[0]["gid"].as_str().unwrap()).unwrap();
    a.group_send_text(gid.clone(), "сообщение в группе".into());
    assert!(wait(5, || a
        .group_history_json(&gid)
        .contains("сообщение в группе")));

    let file = tmpdir("grp").join("copy.peerage");
    a.backup_export(&file, all(), Some("пк")).unwrap();

    let b = twin(95, "grp-dst");
    b.backup_import(&file, Some("пк")).unwrap();

    // Попытка написать в архивную группу ничего не меняет: MLS-состояния нет.
    b.group_send_text(gid.clone(), "а я всё равно напишу".into());
    std::thread::sleep(std::time::Duration::from_millis(500));
    let hist = b.group_history_json(&gid);
    assert!(
        hist.contains("сообщение в группе"),
        "история из копии на месте"
    );
    assert!(
        !hist.contains("а я всё равно напишу"),
        "в архив писать нельзя: {hist}"
    );
}

/// Групповая переписка входит в копию и переживает переезд на чистое устройство.
/// Членство при этом НЕ восстанавливается (MLS-состояние в копию не входит, ADR-004),
/// поэтому группа приезжает архивом: история читается, писать нельзя — и в списке она
/// именно так и помечена. Раньше групп в копии не было вовсе: «восстановился — а
/// групповых чатов нет».
#[test]
fn group_history_survives_backup_as_an_archive() {
    let net = LoopbackNet::new();
    let a = start(&net, id(87), tmpdir("gsrc"));
    a.group_create("Поход");
    // Команды группы исполняет актор в своём потоке — ждём, пока группа появится.
    assert!(
        wait(5, || a.group_list_json().contains("Поход")),
        "группа создана"
    );
    let list: serde_json::Value = serde_json::from_str(&a.group_list_json()).unwrap();
    let gid_hex = list[0]["gid"].as_str().expect("gid группы").to_string();
    let gid = hex::decode(&gid_hex).unwrap();
    a.group_send_text(gid.clone(), "выходим в семь".into());
    assert!(
        wait(5, || a.group_history_json(&gid).contains("выходим в семь")),
        "сообщение записано в историю"
    );

    let file = tmpdir("gfile").join("groups.peerage");
    let out = a.backup_export(&file, all(), None).unwrap();
    // Строка «Группа создана» + сообщение: системные строки — часть истории.
    assert_eq!(out.group_messages, 2, "история группы попала в копию");

    // Чистое устройство: ни группы, ни истории.
    let b = twin(87, "gdst");
    let got = b.backup_import(&file, None).unwrap();
    assert_eq!(got.group_messages, 2);

    let hist = b.group_history_json(&gid);
    assert!(
        hist.contains("выходим в семь"),
        "переписка группы восстановлена: {hist}"
    );

    let list: serde_json::Value = serde_json::from_str(&b.group_list_json()).unwrap();
    let row = list
        .as_array()
        .and_then(|a| a.iter().find(|g| g["gid"] == gid_hex))
        .expect("группа видна в списке");
    assert_eq!(row["name"], "Поход", "имя приехало вместе с историей");
    assert_eq!(row["archived"], true, "мы в ней не состоим — только чтение");
    assert_eq!(row["members"], 0);
}

/// R-035: в копии блоб лежит под адресом-хэшем. Подложенный файл с чужими байтами
/// под чужим адресом не принимается: иначе мы раздавали бы их другим как тот блоб.
/// Честный блоб рядом восстанавливается как обычно.
#[test]
fn forged_blob_in_backup_is_rejected() {
    let net = LoopbackNet::new();
    let e = start(&net, id(84), tmpdir("forged"));
    let honest = b"honest bytes".to_vec();
    let honest_id = hex::encode(peerage_blob::blob_id(&honest));
    let victim_id = hex::encode(peerage_blob::blob_id(b"real picture"));

    let file = tmpdir("forged-file").join("copy.peerage");
    let key = id(84).derive_subkey(peerage_core::backup::SUBKEY_CONTEXT);
    let mut w =
        peerage_core::backup::Writer::create(std::fs::File::create(&file).unwrap(), &key, None)
            .unwrap();
    w.blob(&victim_id, b"forged").unwrap();
    w.blob(&honest_id, &honest).unwrap();
    w.finish().unwrap();

    let got = e.backup_import(&file, None).unwrap();
    assert_eq!(got.attachments, 1, "принят только честный блоб");
    assert!(
        !e.blob_exists(&victim_id),
        "подмена не легла под чужой адрес"
    );
    assert_eq!(e.blob_read(&honest_id).as_deref(), Some(&honest[..]));
}

/// Реакции едут вместе с перепиской: восстановленный чат без них выглядел бы иначе,
/// чем до копии. Слияние, как у сообщений: повторный импорт ничего не задваивает.
#[test]
fn reactions_ride_along_with_their_chat() {
    let net = LoopbackNet::new();
    let a_id = id(89);
    let author = hex::encode(a_id.public_key_bytes());
    let peer = id(90).public_key_bytes();
    let a = start(&net, a_id, tmpdir("rsrc"));
    a.add_contact(&peer, "Аня");
    let msg = a.send_text(&peer, "едем?").unwrap() as u64;
    assert!(a.react(peerage_engine::ChatKey::Dm(peer), msg, &author, "👍"));

    let file = tmpdir("rfile").join("copy.peerage");
    a.backup_export(&file, all(), None).unwrap();
    let fresh = twin(89, "rdst");
    fresh.backup_import(&file, None).unwrap();
    let hist: serde_json::Value = serde_json::from_str(&fresh.history_json(&peer)).unwrap();
    let r = &hist[0]["reactions"];
    assert_eq!(r[0]["emoji"], "👍", "реакция восстановлена: {hist}");
    assert_eq!(r[0]["who"][0], author.as_str());

    // Повторный импорт того же файла реакцию не задваивает.
    fresh.backup_import(&file, None).unwrap();
    let hist: serde_json::Value = serde_json::from_str(&fresh.history_json(&peer)).unwrap();
    assert_eq!(
        hist[0]["reactions"][0]["who"].as_array().map(|w| w.len()),
        Some(1)
    );
}

/// Шаг 7k: копия без пароля — «24 слова → файл → готово» на той же личности; чужая
/// личность её не открывает и ничего из неё не получает.
#[test]
fn copy_without_password_opens_only_for_the_same_identity() {
    let net = LoopbackNet::new();
    let peer = id(101).public_key_bytes();
    let a = start(&net, id(100), tmpdir("k-src"));
    fill(&a, &peer);
    let file = tmpdir("k").join("copy.peerage");
    a.backup_export(&file, all(), None).unwrap();
    assert!(!peerage_engine::backup::backup_needs_password(&file).unwrap());

    let stranger = twin(102, "k-other");
    let err = stranger.backup_import(&file, None).unwrap_err();
    assert!(
        matches!(err, peerage_core::backup::BackupError::Foreign),
        "чужая личность: {err}"
    );
    let hist: serde_json::Value = serde_json::from_str(&stranger.history_json(&peer)).unwrap();
    assert!(hist.as_array().unwrap().is_empty(), "ничего не просочилось");

    let me = twin(100, "k-dst");
    assert_eq!(me.backup_import(&file, None).unwrap().messages, 3);
}

/// С паролем нужны и личность, и пароль; флаг в заголовке говорит, спрашивать ли его.
#[test]
fn copy_with_password_needs_it_on_top_of_the_identity() {
    let net = LoopbackNet::new();
    let peer = id(104).public_key_bytes();
    let a = start(&net, id(103), tmpdir("kp-src"));
    fill(&a, &peer);
    let file = tmpdir("kp").join("copy.peerage");
    a.backup_export(&file, all(), Some("облако")).unwrap();
    assert!(peerage_engine::backup::backup_needs_password(&file).unwrap());

    let me = twin(103, "kp-dst");
    assert!(matches!(
        me.backup_import(&file, None),
        Err(peerage_core::backup::BackupError::PasswordRequired)
    ));
    assert_eq!(me.backup_import(&file, Some("облако")).unwrap().messages, 3);
}

/// Шаг 7g: своё имя и фото едут в копии — после переноса на новый телефон профиль не
/// пустой. Введённое на новом телефоне имя копия не затирает.
#[test]
fn own_name_and_photo_ride_along_and_do_not_overwrite() {
    let net = LoopbackNet::new();
    let a = start(&net, id(105), tmpdir("p-src"));
    a.set_self_name("Алиса");
    let photo = b"pretend png bytes".to_vec();
    let photo_id = a.set_self_avatar(photo, "image/png");
    let file = tmpdir("p").join("copy.peerage");
    a.backup_export(&file, Parts::default(), None).unwrap();

    let fresh = twin(105, "p-dst");
    fresh.backup_import(&file, None).unwrap();
    assert_eq!(fresh.self_name(), "Алиса", "имя из копии");
    assert_eq!(
        fresh.self_avatar_id().as_deref(),
        Some(photo_id.as_str()),
        "фото из копии"
    );
    assert!(fresh.self_avatar().is_some(), "байты фото на месте");

    let typed = twin(105, "p-typed");
    typed.set_self_name("Аля");
    typed.backup_import(&file, None).unwrap();
    assert_eq!(typed.self_name(), "Аля", "введённое не затёрто");
    assert_eq!(
        typed.self_avatar_id().as_deref(),
        Some(photo_id.as_str()),
        "фото не было — взято из копии"
    );
}
