//! Действия с сообщением, у которых есть СЕТЕВАЯ часть: ответ и отзыв.
//!
//! Копирование и удаление у себя не проверяются здесь намеренно — они локальны и
//! ничего не отправляют; их место в юнит-тестах хранилища и в UI.
//!
//! Прогон из WSL: scripts/test-all.sh -p peerage-engine --test msg_actions

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
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-msgact-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
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

fn history(e: &Engine, peer: &[u8; 32]) -> Vec<serde_json::Value> {
    serde_json::from_str(&e.history_json(peer)).unwrap()
}

/// Пара знакомых друг с другом движков (гейт контактов пропускает только своих).
fn pair(tag: &str, a_seed: u8, b_seed: u8) -> (Arc<Engine>, Arc<Engine>, [u8; 32], [u8; 32]) {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(a_seed), id(b_seed));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir(&format!("{tag}-a")));
    let b = start(&net, b_id, tmpdir(&format!("{tag}-b")));
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Аня");
    std::mem::forget(net); // сеть должна пережить движки до конца теста
    (a, b, a_key, b_key)
}

#[test]
fn reply_keeps_the_reference_and_flips_the_author_side() {
    // Ссылка на сообщение — пара «автор + id» (ADR-006). На проводе «чьё» едет с
    // точки зрения отправителя, поэтому у получателя оно обязано разрешиться в
    // ДРУГОГО человека: иначе ответ на СВОЁ и ответ на ЧУЖОЕ с тем же id были бы
    // неразличимы. В истории хранится сам автор, а не флаг.
    let (a, b, a_key, b_key) = pair("reply", 21, 22);

    let first = a.send_text(&b_key, "во сколько встречаемся?").unwrap() as u64;
    assert!(
        wait(10, || !history(&b, &a_key).is_empty()),
        "первое сообщение дошло"
    );

    // Боб отвечает на сообщение Ани — для него оно ЧУЖОЕ.
    b.send_text_reply(&a_key, "в семь", first, false);
    assert!(wait(10, || history(&a, &b_key).len() == 2), "ответ дошёл");

    let at_a = history(&a, &b_key);
    let reply = at_a
        .iter()
        .find(|m| m["text"] == "в семь")
        .expect("ответ в истории");
    assert_eq!(reply["reply_to"].as_u64(), Some(first));
    assert_eq!(
        reply["reply_sender"].as_str(),
        Some(hex::encode(a_key).as_str()),
        "у Ани цитируемое сообщение — ЕЁ собственное"
    );

    // И у самого Боба та же ссылка указывает на Аню.
    let at_b = history(&b, &a_key);
    let own = at_b
        .iter()
        .find(|m| m["text"] == "в семь")
        .expect("свой ответ");
    assert_eq!(own["reply_to"].as_u64(), Some(first));
    assert_eq!(
        own["reply_sender"].as_str(),
        Some(hex::encode(a_key).as_str())
    );
    // Автор самого ответа — Боб, и это видно без всяких флагов.
    assert_eq!(own["sender"].as_str(), Some(hex::encode(b_key).as_str()));
}

#[test]
fn retract_leaves_a_deleted_row_on_both_sides() {
    let (a, b, a_key, b_key) = pair("retract", 23, 24);

    let id1 = a.send_text(&b_key, "это останется").unwrap() as u64;
    let id2 = a.send_text(&b_key, "а это я заберу").unwrap() as u64;
    assert!(wait(10, || history(&b, &a_key).len() == 2), "оба дошли");

    // «Удалить у всех» оставляет на месте строку «Сообщение удалено» — и у
    // собеседника, и у себя; текста в ней нет ни у кого.
    let deleted = |h: Vec<serde_json::Value>| {
        h.iter()
            .any(|m| m["id"].as_u64() == Some(id2) && m["kind"] == "deleted" && m["text"] == "")
    };
    assert_eq!(a.retract_messages(&b_key, &[id2]), 1);
    assert!(
        wait(10, || deleted(history(&b, &a_key))),
        "у собеседника — строка на месте отозванного"
    );
    let theirs = history(&b, &a_key);
    assert_eq!(theirs.len(), 2);
    assert_eq!(theirs[0]["text"], "это останется");

    let mine = history(&a, &b_key);
    assert!(deleted(mine.clone()), "и у себя");
    assert_eq!(mine[0]["id"].as_u64(), Some(id1));
}

#[test]
fn retract_cannot_touch_someone_elses_message() {
    // Отзыв — про СВОЁ сообщение. Просьба удалить чужое (id из истории собеседника,
    // но принадлежащее ему) не должна ничего стирать у него: иначе любой контакт мог
    // бы вычищать чужую переписку.
    let (a, b, a_key, b_key) = pair("foreign", 25, 26);

    let his = b.send_text(&a_key, "сообщение Боба").unwrap() as u64;
    assert!(wait(10, || history(&a, &b_key).len() == 1), "дошло до Ани");

    // Аня пробует отозвать сообщение Боба, назвав его id.
    a.retract_messages(&b_key, &[his]);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        history(&b, &a_key).len(),
        1,
        "у Боба его собственное сообщение осталось на месте"
    );
}

#[test]
fn deleting_locally_does_not_touch_the_other_side() {
    // «Удалить у себя» обязано остаться локальным: иначе чистка своей истории
    // молча стирала бы переписку у собеседника.
    let (a, b, a_key, b_key) = pair("local", 27, 28);

    let id1 = a.send_text(&b_key, "первое").unwrap() as u64;
    a.send_text(&b_key, "второе");
    assert!(wait(10, || history(&b, &a_key).len() == 2), "оба дошли");

    assert_eq!(a.delete_messages(&b_key, &[(id1, true)]), 1);
    assert_eq!(history(&a, &b_key).len(), 1, "у себя удалено");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(history(&b, &a_key).len(), 2, "у собеседника всё на месте");
}

/// Пересылка сохраняет ПЕРВОГО автора — и когда пересылают чужое сообщение, и
/// когда пересылают уже пересланное. Иначе цепочка «переслал переславшего»
/// подменяла бы источник посредником, и подпись «переслано от» ничего не значила бы.
#[test]
fn forwarding_keeps_the_original_author() {
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(31), id(32), id(33));
    let (a_key, b_key, c_key) = (
        a_id.public_key_bytes(),
        b_id.public_key_bytes(),
        c_id.public_key_bytes(),
    );
    let a = start(&net, a_id, tmpdir("fwd-a"));
    let b = start(&net, b_id, tmpdir("fwd-b"));
    let c = start(&net, c_id, tmpdir("fwd-c"));
    // Гейт контактов пропускает только своих — знакомим все пары.
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Аня");
    b.add_contact(&c_key, "Вера");
    c.add_contact(&b_key, "Боб");
    std::mem::forget(net);

    a.send_text(&b_key, "встречаемся в семь");
    assert!(
        wait(10, || !history(&b, &a_key).is_empty()),
        "сообщение дошло до Боба"
    );
    let at_b = history(&b, &a_key);
    let msg = &at_b[0];
    let refs = vec![(
        msg["id"].as_u64().unwrap(),
        msg["sender"].as_str().unwrap().to_string(),
    )];

    // Боб пересылает Вере сообщение Ани.
    let sent = b.forward_messages(
        peerage_engine::ChatKey::Dm(a_key),
        &refs,
        peerage_engine::ChatKey::Dm(c_key),
        false,
    );
    assert_eq!(sent, 1);
    assert!(
        wait(10, || !history(&c, &b_key).is_empty()),
        "пересланное дошло до Веры"
    );

    let at_c = history(&c, &b_key);
    let fwd = &at_c[0];
    assert_eq!(fwd["text"], "встречаемся в семь");
    assert_eq!(
        fwd["fwd_from"].as_str(),
        Some(hex::encode(a_key).as_str()),
        "у Веры видно, что автор — Аня, а не переславший Боб"
    );
    assert_eq!(
        fwd["sender"].as_str(),
        Some(hex::encode(b_key).as_str()),
        "прислал всё же Боб"
    );

    // И у самого Боба его копия помнит источник — иначе «переслать дальше» потеряло
    // бы автора уже на втором шаге.
    let own = history(&b, &c_key);
    assert_eq!(own.len(), 1);
    assert_eq!(
        own[0]["fwd_from"].as_str(),
        Some(hex::encode(a_key).as_str())
    );

    // Пересылка пересланного: источник остаётся Аня, посредник не подменяет её.
    let refs2 = vec![(
        own[0]["id"].as_u64().unwrap(),
        own[0]["sender"].as_str().unwrap().to_string(),
    )];
    b.forward_messages(
        peerage_engine::ChatKey::Dm(c_key),
        &refs2,
        peerage_engine::ChatKey::Dm(a_key),
        false,
    );
    // Ищем ВХОДЯЩЕЕ: в этом чате уже лежит её собственный оригинал с тем же текстом.
    assert!(wait(10, || history(&a, &b_key)
        .iter()
        .any(|m| m["mine"] == false)));
    let back = history(&a, &b_key);
    let again = back.iter().find(|m| m["mine"] == false).unwrap();
    assert_eq!(again["text"], "встречаемся в семь");
    assert_eq!(
        again["fwd_from"].as_str(),
        Some(hex::encode(a_key).as_str())
    );
}

/// «Скрыть автора» скрывает его ПО-НАСТОЯЩЕМУ: ключ не уходит на провод вовсе, а
/// получатель видит «переслано» без имени. Иначе скрытие было бы косметикой в
/// интерфейсе — данные всё равно у собеседника.
#[test]
fn hidden_forward_sends_no_author_at_all() {
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(41), id(42), id(43));
    let (a_key, b_key, c_key) = (
        a_id.public_key_bytes(),
        b_id.public_key_bytes(),
        c_id.public_key_bytes(),
    );
    let a = start(&net, a_id, tmpdir("anon-a"));
    let b = start(&net, b_id, tmpdir("anon-b"));
    let c = start(&net, c_id, tmpdir("anon-c"));
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Аня");
    b.add_contact(&c_key, "Вера");
    c.add_contact(&b_key, "Боб");
    std::mem::forget(net);

    a.send_text(&b_key, "это между нами");
    assert!(wait(10, || !history(&b, &a_key).is_empty()));
    let at_b = history(&b, &a_key);
    let refs = vec![(
        at_b[0]["id"].as_u64().unwrap(),
        at_b[0]["sender"].as_str().unwrap().to_string(),
    )];

    b.forward_messages(
        peerage_engine::ChatKey::Dm(a_key),
        &refs,
        peerage_engine::ChatKey::Dm(c_key),
        true, // скрыть автора
    );
    assert!(wait(10, || !history(&c, &b_key).is_empty()));

    let got = history(&c, &b_key);
    assert_eq!(got[0]["text"], "это между нами");
    assert_eq!(
        got[0]["fwd_from"].as_str(),
        Some(""),
        "ключа автора у Веры нет"
    );
    assert_eq!(got[0]["fwd_anon"], true, "но видно, что это пересылка");

    // И своя копия у Боба совпадает с тем, что ушло: показывать себе имя, которого
    // собеседник не получил, значит расходиться с реальностью.
    let own = history(&b, &c_key);
    assert_eq!(own[0]["fwd_from"].as_str(), Some(""));
    assert_eq!(own[0]["fwd_anon"], true);
}

/// Закреп в личном чате общий для обоих: он живёт у ЧАТА, а не у того, кто его
/// поставил. Иначе «закрепил важное» означало бы «закрепил у себя», и собеседник
/// об этом не узнал бы.
#[test]
fn pin_in_a_dm_is_shared_by_both_sides() {
    let (a, b, a_key, b_key) = pair("pin", 51, 52);

    let id1 = a.send_text(&b_key, "код от домофона 47К").unwrap() as u64;
    assert!(
        wait(10, || !history(&b, &a_key).is_empty()),
        "сообщение дошло"
    );

    let me = hex::encode(a_key);
    assert!(a.pin_message(peerage_engine::ChatKey::Dm(b_key), id1, &me, true));

    // У себя закреп виден сразу.
    let mine: serde_json::Value =
        serde_json::from_str(&a.pinned_json(peerage_engine::ChatKey::Dm(b_key))).unwrap();
    assert_eq!(mine.as_array().unwrap().len(), 1);
    assert_eq!(mine[0]["text"], "код от домофона 47К");

    // И у собеседника — тоже, причём это ЕГО ссылка на то же сообщение.
    assert!(wait(10, || {
        let v: serde_json::Value =
            serde_json::from_str(&b.pinned_json(peerage_engine::ChatKey::Dm(a_key))).unwrap();
        !v.as_array().unwrap().is_empty()
    }));
    let at_b: serde_json::Value =
        serde_json::from_str(&b.pinned_json(peerage_engine::ChatKey::Dm(a_key))).unwrap();
    assert_eq!(at_b[0]["text"], "код от домофона 47К");
    assert_eq!(
        at_b[0]["sender"].as_str(),
        Some(me.as_str()),
        "автор — Аня, а не получатель"
    );
    assert_eq!(at_b[0]["mine"], false);

    // Открепление тоже общее.
    assert!(a.pin_message(peerage_engine::ChatKey::Dm(b_key), id1, &me, false));
    assert!(wait(10, || {
        let v: serde_json::Value =
            serde_json::from_str(&b.pinned_json(peerage_engine::ChatKey::Dm(a_key))).unwrap();
        v.as_array().unwrap().is_empty()
    }));
}

/// Поиск по переписке идёт через движок и находит и заметки, и сообщения чата —
/// с указанием, ГДЕ нашлось. Проверяется на настоящем движке, а не на голом
/// хранилище: между ними лежит JSON-граница, на которой всё и ломается.
#[test]
fn search_finds_messages_across_chats() {
    let (a, b, _a_key, b_key) = pair("search", 61, 62);

    a.save_note("пароль от вайфая: 12345678");
    a.send_text(&b_key, "встречаемся в семь у входа");
    assert!(
        wait(10, || !history(&b, &_a_key).is_empty()),
        "сообщение дошло"
    );

    let hits: serde_json::Value = serde_json::from_str(&a.search_json("встреч", None, 50)).unwrap();
    let arr = hits.as_array().unwrap();
    assert_eq!(arr.len(), 1, "нашлось одно: {hits}");
    assert_eq!(arr[0]["text"], "встречаемся в семь у входа");
    assert_eq!(
        arr[0]["chat"].as_str(),
        Some(hex::encode(b_key).as_str()),
        "видно, где нашлось"
    );
    assert_eq!(arr[0]["group"], false);

    // Заметка себе — тот же поиск, чат с самим собой.
    let notes: serde_json::Value =
        serde_json::from_str(&a.search_json("пароль", None, 50)).unwrap();
    assert_eq!(notes.as_array().unwrap().len(), 1);
    assert_eq!(
        notes[0]["chat"].as_str(),
        Some(hex::encode(_a_key).as_str())
    );

    // По началу слова тоже (а не только по целому).
    let prefix: serde_json::Value = serde_json::from_str(&a.search_json("вайф", None, 50)).unwrap();
    assert_eq!(
        prefix.as_array().unwrap().len(),
        1,
        "префиксный поиск: {prefix}"
    );

    // Чужой чат в результат не лезет.
    let scoped: serde_json::Value = serde_json::from_str(&a.search_json(
        "пароль",
        Some(peerage_engine::ChatKey::Dm(b_key)),
        50,
    ))
    .unwrap();
    assert!(
        scoped.as_array().unwrap().is_empty(),
        "поиск в чате не видит заметок"
    );
}

/// Правка доезжает до собеседника и помечается как правка; чужое сообщение
/// исправить нельзя — иначе это подмена чужих слов, а не редактирование.
#[test]
fn editing_replaces_text_on_both_sides_and_only_for_the_author() {
    let (a, b, a_key, b_key) = pair("edit", 71, 72);

    let id1 = a.send_text(&b_key, "встречаемся в семь").unwrap() as u64;
    assert!(
        wait(10, || !history(&b, &a_key).is_empty()),
        "сообщение дошло"
    );

    assert!(a.edit_message(
        peerage_engine::ChatKey::Dm(b_key),
        id1,
        "встречаемся в восемь"
    ));

    // У автора текст заменён и помечен «изменено».
    let mine = history(&a, &b_key);
    let m = mine.iter().find(|m| m["id"].as_u64() == Some(id1)).unwrap();
    assert_eq!(m["text"], "встречаемся в восемь");
    assert!(
        m["edited_at"].as_i64().unwrap_or(0) > 0,
        "пометка о правке осталась"
    );

    // И у собеседника — тот же текст, а не второе сообщение.
    assert!(wait(10, || {
        history(&b, &a_key)
            .iter()
            .any(|m| m["text"] == "встречаемся в восемь")
    }));
    let at_b = history(&b, &a_key);
    assert_eq!(at_b.len(), 1, "правка заменила текст, а не добавила строку");
    assert!(at_b[0]["edited_at"].as_i64().unwrap_or(0) > 0);

    // Боб пытается исправить сообщение Ани — отказ, и текст у него прежний.
    assert!(!b.edit_message(
        peerage_engine::ChatKey::Dm(a_key),
        id1,
        "встречаемся в девять"
    ));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(history(&b, &a_key)[0]["text"], "встречаемся в восемь");
    assert_eq!(
        history(&a, &b_key)[0]["text"],
        "встречаемся в восемь",
        "и у автора тоже"
    );
}

/// «Очистить историю» стирает переписку только у себя: собеседник свою копию
/// сохраняет, контакт остаётся, а закрепы и очередь отправки уходят вместе с
/// историей — иначе неотправленное ушло бы позже и вернулось в пустой чат.
#[test]
fn clearing_history_is_local_and_takes_pins_and_queue_with_it() {
    let (a, b, a_key, b_key) = pair("clear", 81, 82);

    let id1 = a.send_text(&b_key, "первое").unwrap() as u64;
    a.send_text(&b_key, "второе");
    assert!(wait(10, || history(&b, &a_key).len() == 2), "оба дошли");
    assert!(a.pin_message(
        peerage_engine::ChatKey::Dm(b_key),
        id1,
        &hex::encode(a_key),
        true
    ));

    // Собеседник, которого нет в сети: сообщение ему висит в очереди отправки.
    let ghost = id(83).public_key_bytes();
    a.add_contact(&ghost, "Призрак");
    a.send_text(&ghost, "дойдёт, когда появится");
    let pending = |e: &Engine| json_field(&e.storage_stats_json(), "outbox_pending");
    assert!(pending(&a) >= 1, "в очереди есть неотправленное");

    a.clear_dm_history(&b_key);
    a.clear_dm_history(&ghost);

    assert!(history(&a, &b_key).is_empty(), "у себя история пуста");
    let pins: serde_json::Value =
        serde_json::from_str(&a.pinned_json(peerage_engine::ChatKey::Dm(b_key))).unwrap();
    assert_eq!(
        pins.as_array().map(|v| v.len()),
        Some(0),
        "закреп ушёл вместе с историей"
    );
    assert_eq!(pending(&a), 0, "неотправленное не вернётся в чат");
    assert!(
        a.contacts_json().contains(&hex::encode(b_key)),
        "контакт остался"
    );
    assert_eq!(history(&b, &a_key).len(), 2, "у собеседника переписка цела");

    // Удаление контакта стирает и переписку — так обещает подтверждение в UI.
    b.remove_contact(&a_key);
    assert!(
        history(&b, &a_key).is_empty(),
        "удалённый контакт уходит вместе с историей"
    );
}

fn json_field(s: &str, key: &str) -> u64 {
    serde_json::from_str::<serde_json::Value>(s).unwrap()[key]
        .as_u64()
        .unwrap_or(0)
}

/// R-056: пересылка вложения — ссылка на блоб, который у пересылающего уже есть:
/// файл не перечитывается и не перешифровывается, а новый получатель тянет байты
/// у пересылающего по кнопке.
#[test]
fn forwarded_attachment_is_a_reference_the_recipient_pulls() {
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(34), id(35), id(36));
    let (a_key, b_key, c_key) = (
        a_id.public_key_bytes(),
        b_id.public_key_bytes(),
        c_id.public_key_bytes(),
    );
    let a = start(&net, a_id, tmpdir("fwdm-a"));
    let b = start(&net, b_id, tmpdir("fwdm-b"));
    let c = start(&net, c_id, tmpdir("fwdm-c"));
    a.add_contact(&b_key, "Боб");
    b.add_contact(&a_key, "Аня");
    b.add_contact(&c_key, "Вера");
    c.add_contact(&b_key, "Боб");
    std::mem::forget(net);

    // Крупнее порога автодокачки: Боб качает по кнопке у Ани.
    let content: Vec<u8> = (0..2_500_000u32).map(|i| (i % 241) as u8).collect();
    let blob = hex::encode(peerage_blob::blob_id(&content));
    let src = tmpdir("fwdm-src").join("clip.mp4");
    std::fs::write(&src, &content).unwrap();
    a.send_media_file(
        &b_key,
        &src,
        "video/mp4",
        "clip.mp4",
        peerage_engine::MediaInfo::default(),
    )
    .unwrap();
    assert!(
        wait(10, || !history(&b, &a_key).is_empty()),
        "карточка дошла до Боба"
    );
    assert!(b.download_media(&a_key, &blob));
    assert!(wait(60, || b.blob_exists(&blob)), "Боб скачал");

    let msg = &history(&b, &a_key)[0];
    let refs = vec![(
        msg["id"].as_u64().unwrap(),
        msg["sender"].as_str().unwrap().to_string(),
    )];
    let sent = b.forward_messages(
        peerage_engine::ChatKey::Dm(a_key),
        &refs,
        peerage_engine::ChatKey::Dm(c_key),
        false,
    );
    assert_eq!(sent, 1);
    assert!(
        wait(10, || !history(&c, &b_key).is_empty()),
        "пересланное дошло до Веры"
    );
    let fwd = &history(&c, &b_key)[0];
    assert_eq!(
        fwd["blob_id"].as_str(),
        Some(blob.as_str()),
        "тот же блоб — ссылка, а не новый файл"
    );
    assert_eq!(
        fwd["size"].as_u64(),
        Some(content.len() as u64),
        "размер для карточки на месте"
    );
    assert_eq!(fwd["fwd_from"].as_str(), Some(hex::encode(a_key).as_str()));

    assert!(c.download_media(&b_key, &blob));
    assert!(wait(60, || c.blob_exists(&blob)), "Вера скачала у Боба");
    assert_eq!(c.blob_read(&blob).unwrap(), content, "байт-в-байт");
}

/// R-040: две заметки подряд (в одну секунду) — две заметки, а не одна.
#[test]
fn two_notes_in_one_second_are_both_kept() {
    let net = LoopbackNet::new();
    let a_id = id(63);
    let me = a_id.public_key_bytes();
    let a = start(&net, a_id, tmpdir("notes"));
    a.save_note("первая");
    a.save_note("вторая");
    a.save_note("третья");
    let notes = history(&a, &me);
    let texts: Vec<&str> = notes.iter().filter_map(|n| n["text"].as_str()).collect();
    assert_eq!(texts, ["первая", "вторая", "третья"]);
}

/// «Сохранённое» — чат с самим собой (UI 5.1): текст, вложение, правка, закреп и
/// реакция там идут обычными вызовами чата, но доставлять их некому — в очередь
/// исходящих ничего не встаёт, а в истории всё есть.
#[test]
fn chat_with_self_stays_local_and_never_queues() {
    use peerage_engine::ChatKey;
    let net = LoopbackNet::new();
    let a_id = id(64);
    let me = a_id.public_key_bytes();
    let a = start(&net, a_id, tmpdir("self"));
    let me_hex = hex::encode(me);

    let t = a.send_text(&me, "код от подъезда 4417").unwrap() as u64;
    a.save_note("фильтр для воды");
    a.send_media(&me, b"PNG..".to_vec(), "image/png", "plan.png")
        .unwrap();
    assert!(a.edit_message(ChatKey::Dm(me), t, "код от подъезда 4418"));
    assert!(a.pin_message(ChatKey::Dm(me), t, &me_hex, true));
    assert!(a.react(ChatKey::Dm(me), t, &me_hex, "👍"));

    let h = history(&a, &me);
    assert_eq!(h.len(), 3, "две заметки и вложение");
    assert_eq!(
        h[0]["text"], "код от подъезда 4418",
        "правка легла в историю"
    );
    assert_eq!(h[0]["reactions"][0]["emoji"], "👍");
    let stats: serde_json::Value = serde_json::from_str(&a.outbox_stats_json()).unwrap();
    assert_eq!(stats["pending"], 0, "себе ничего не ставится в очередь");
    assert_eq!(a.outbox_by_peer_json(), "[]");
}

/// Реакции в личном чате: доходят до собеседника, другая заменяет, пустая снимает —
/// и у обоих набор один и тот же; на чужое сообщение реагировать можно.
#[test]
fn reactions_travel_replace_and_clear() {
    let (a, b, a_key, b_key) = pair("react", 61, 62);
    let id1 = a.send_text(&b_key, "едем в субботу?").unwrap() as u64;
    assert!(
        wait(10, || !history(&b, &a_key).is_empty()),
        "сообщение дошло"
    );
    let author = hex::encode(a_key);
    let reactions = |e: &Engine, peer: &[u8; 32]| history(e, peer)[0]["reactions"].clone();

    // Боб реагирует на сообщение Ани — у Ани появляется его реакция.
    assert!(b.react(peerage_engine::ChatKey::Dm(a_key), id1, &author, "👍"));
    assert!(wait(10, || reactions(&a, &b_key)[0]["emoji"] == "👍"));
    assert_eq!(reactions(&a, &b_key)[0]["who"][0], hex::encode(b_key));

    // Аня ставит свою на своё — набор из двух, у обоих одинаковый.
    assert!(a.react(peerage_engine::ChatKey::Dm(b_key), id1, &author, "👍"));
    assert!(wait(10, || reactions(&b, &a_key)[0]["who"]
        .as_array()
        .map(|w| w.len())
        == Some(2)));

    // Боб меняет на другую — замена, а не вторая реакция.
    assert!(b.react(peerage_engine::ChatKey::Dm(a_key), id1, &author, "❤️"));
    assert!(wait(10, || reactions(&a, &b_key)
        .as_array()
        .map(|r| r.len())
        == Some(2)));
    // Аня снимает свою — у Боба остаётся только его.
    assert!(a.react(peerage_engine::ChatKey::Dm(b_key), id1, &author, ""));
    assert!(wait(10, || {
        let r = reactions(&b, &a_key);
        r.as_array().map(|r| r.len()) == Some(1) && r[0]["emoji"] == "❤️"
    }));

    // Кривая реакция отклоняется на входе.
    assert!(!a.react(peerage_engine::ChatKey::Dm(b_key), id1, &author, "a b"));
}

/// Поиск в чате (2.9) через JSON-границу движка: счётчик и листание, фильтр
/// отправителя, затем переход к находке — окно истории вокруг неё и дни месяца.
#[test]
fn chat_search_pages_and_jumps_to_a_hit() {
    let (a, b, a_key, b_key) = pair("chatsearch", 91, 92);
    let chat = peerage_engine::ChatKey::Dm(b_key);
    a.send_text(&b_key, "ключ у соседки");
    a.send_text(&b_key, "второй ключ под горшком");
    b.send_text(&a_key, "ключи заберу вечером");
    assert!(
        wait(10, || history(&a, &b_key).len() == 3),
        "все три в переписке"
    );

    let page: serde_json::Value =
        serde_json::from_str(&a.search_page_json(&chat, "ключ", "", 0, 2)).unwrap();
    assert_eq!(page["total"], 3, "{page}");
    assert_eq!(
        page["hits"].as_array().unwrap().len(),
        2,
        "страница по limit"
    );

    let theirs: serde_json::Value =
        serde_json::from_str(&a.search_page_json(&chat, "ключ", &hex::encode(b_key), 0, 10))
            .unwrap();
    assert_eq!(theirs["total"], 1, "только от собеседника: {theirs}");

    let both: serde_json::Value =
        serde_json::from_str(&a.search_page_json(&chat, "горшк ключ", "", 0, 10)).unwrap();
    assert_eq!(both["total"], 1, "слова по отдельности");

    // Переход к находке: окно вокруг неё содержит её саму.
    let hit = &both["hits"][0];
    let (t, r) = (hit["time"].as_i64().unwrap(), hit["row"].as_i64().unwrap());
    let around: serde_json::Value =
        serde_json::from_str(&a.history_around_json(&chat, t, r)).unwrap();
    assert!(
        around
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["row"] == hit["row"]),
        "{around}"
    );
    let after: serde_json::Value =
        serde_json::from_str(&a.history_after_json(&chat, t, r)).unwrap();
    assert!(
        after
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["row"] != hit["row"]),
        "строго ниже"
    );

    let days: serde_json::Value =
        serde_json::from_str(&a.message_days_json(&chat, t - 86_400, t + 86_400, 0)).unwrap();
    assert_eq!(
        days.as_array().unwrap().len(),
        1,
        "сегодня — день с сообщениями: {days}"
    );
}

/// Статус своего сообщения по id.
fn status_of(e: &Engine, peer: &[u8; 32], id: u64) -> String {
    history(e, peer)
        .iter()
        .find(|m| m["id"].as_u64() == Some(id) && m["mine"] == true)
        .and_then(|m| m["status"].as_str())
        .unwrap_or_default()
        .to_string()
}

/// Выбрать из очереди событий движка события одного типа (остальные выбрасываются).
fn drain(e: &Engine, ty: &str) -> Vec<serde_json::Value> {
    std::iter::from_fn(|| e.poll_event())
        .filter_map(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter(|v| v["type"] == ty)
        .collect()
}

/// Отчёты о прочтении (2.10, шаг 7c): по выбору, выключены по умолчанию и взаимны —
/// отметка уходит, только если включил читающий, и принимается, только если включил
/// получатель. ✓✓ «доставлено» становится «прочитано» у обоих концов одинаково.
#[test]
fn read_receipts_are_opt_in_and_mutual() {
    let (a, b, a_key, b_key) = pair("read", 71, 72);
    let first = a.send_text(&b_key, "прочтёшь?").unwrap() as u64;
    assert!(wait(10, || status_of(&a, &b_key, first) == "delivered"));
    let settle = || std::thread::sleep(Duration::from_millis(600));

    // Выключено у обоих: прочтение не уходит.
    b.mark_read(&a_key);
    settle();
    assert_eq!(status_of(&a, &b_key, first), "delivered");

    // Включил только читающий — Аня чужих отчётов не видит, пока не включит свои.
    b.set_read_receipts(true);
    b.mark_read(&a_key);
    settle();
    assert_eq!(status_of(&a, &b_key, first), "delivered");

    // Включили оба — «прочитано», и клиенту приходят id, что сменили статус.
    a.set_read_receipts(true);
    drain(&a, "read");
    b.mark_read(&a_key);
    assert!(wait(10, || status_of(&a, &b_key, first) == "read"));
    let ev = drain(&a, "read");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["peer"], hex::encode(b_key));
    assert_eq!(ev[0]["ids"], serde_json::json!([first]));
    assert!(
        serde_json::from_str::<serde_json::Value>(&a.privacy_json()).unwrap()["read_receipts"]
            == true
    );

    // Читающий выключил — новое так и остаётся «доставлено».
    b.set_read_receipts(false);
    let second = a.send_text(&b_key, "а это?").unwrap() as u64;
    assert!(wait(10, || status_of(&a, &b_key, second) == "delivered"));
    b.mark_read(&a_key);
    settle();
    assert_eq!(status_of(&a, &b_key, second), "delivered");
}

/// Индикатор набора (2.10): эфемерный, по выбору и взаимный; частые вызовы
/// отправитель сжимает в один пакет на несколько секунд.
#[test]
fn typing_is_opt_in_mutual_and_throttled() {
    let (a, b, a_key, b_key) = pair("typing", 73, 74);
    let _ = a.send_text(&b_key, "привет").unwrap();
    assert!(
        wait(10, || !history(&b, &a_key).is_empty()),
        "сессия поднялась"
    );
    let settle = || std::thread::sleep(Duration::from_millis(600));

    b.typing(&a_key);
    settle();
    assert!(drain(&a, "typing").is_empty(), "по умолчанию выключено");

    b.set_typing_indicator(true);
    b.typing(&a_key);
    settle();
    assert!(
        drain(&a, "typing").is_empty(),
        "у получателя выключено — не видит"
    );

    a.set_typing_indicator(true);
    std::thread::sleep(Duration::from_secs(3)); // окно сжатия после прошлой отправки
    for _ in 0..5 {
        b.typing(&a_key);
    }
    settle();
    let ev = drain(&a, "typing");
    assert_eq!(ev.len(), 1, "пять нажатий — один пакет");
    assert_eq!(ev[0]["peer"], hex::encode(b_key));
}

/// «Медиа и файлы» (3.2) через JSON-границу движка: счётчики вкладок и страница
/// ссылок у обеих сторон; неизвестная вкладка — пустая страница, не ошибка.
#[test]
fn media_tabs_count_and_page_links() {
    let (a, b, a_key, b_key) = pair("mediatabs", 93, 94);
    a.send_text(&b_key, "маршрут https://example.org/route");
    a.send_text(&b_key, "без ссылки");
    b.send_text(&a_key, "погода: https://weather.example/x");
    assert!(
        wait(10, || history(&a, &b_key).len() == 3),
        "все три в переписке"
    );

    let chat = peerage_engine::ChatKey::Dm(b_key);
    let counts: serde_json::Value = serde_json::from_str(&a.media_counts_json(&chat)).unwrap();
    assert_eq!(
        counts,
        serde_json::json!({"media": 0, "files": 0, "voice": 0, "links": 2})
    );

    let page: serde_json::Value =
        serde_json::from_str(&a.media_page_json(&chat, "links", 0, 1)).unwrap();
    assert_eq!(page["total"], 2, "{page}");
    assert_eq!(
        page["items"][0]["text"], "погода: https://weather.example/x",
        "свежие первыми"
    );

    let none: serde_json::Value =
        serde_json::from_str(&a.media_page_json(&chat, "photo", 0, 10)).unwrap();
    assert_eq!(none["total"], 0);
}
