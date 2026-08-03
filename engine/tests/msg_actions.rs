//! Действия с сообщением, у которых есть СЕТЕВАЯ часть: ответ и отзыв.
//!
//! Копирование и удаление у себя не проверяются здесь намеренно — они локальны и
//! ничего не отправляют; их место в юнит-тестах хранилища и в UI.
//!
//! Прогон из WSL: scripts/test-all.sh -p p2p-engine --test msg_actions

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
    let d = std::env::temp_dir().join(format!("p2p-msgact-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
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
    assert!(wait(10, || !history(&b, &a_key).is_empty()), "первое сообщение дошло");

    // Боб отвечает на сообщение Ани — для него оно ЧУЖОЕ.
    b.send_text_reply(&a_key, "в семь", first, false);
    assert!(wait(10, || history(&a, &b_key).len() == 2), "ответ дошёл");

    let at_a = history(&a, &b_key);
    let reply = at_a.iter().find(|m| m["text"] == "в семь").expect("ответ в истории");
    assert_eq!(reply["reply_to"].as_u64(), Some(first));
    assert_eq!(
        reply["reply_sender"].as_str(),
        Some(hex::encode(a_key).as_str()),
        "у Ани цитируемое сообщение — ЕЁ собственное"
    );

    // И у самого Боба та же ссылка указывает на Аню.
    let at_b = history(&b, &a_key);
    let own = at_b.iter().find(|m| m["text"] == "в семь").expect("свой ответ");
    assert_eq!(own["reply_to"].as_u64(), Some(first));
    assert_eq!(own["reply_sender"].as_str(), Some(hex::encode(a_key).as_str()));
    // Автор самого ответа — Боб, и это видно без всяких флагов.
    assert_eq!(own["sender"].as_str(), Some(hex::encode(b_key).as_str()));
}

#[test]
fn retract_removes_the_message_on_both_sides() {
    let (a, b, a_key, b_key) = pair("retract", 23, 24);

    let id1 = a.send_text(&b_key, "это останется").unwrap() as u64;
    let id2 = a.send_text(&b_key, "а это я заберу").unwrap() as u64;
    assert!(wait(10, || history(&b, &a_key).len() == 2), "оба дошли");

    assert_eq!(a.retract_messages(&b_key, &[id2]), 1);
    assert!(
        wait(10, || history(&b, &a_key).len() == 1),
        "отозванное исчезло у собеседника"
    );
    assert_eq!(history(&b, &a_key)[0]["text"], "это останется");

    // И у себя тоже — «удалил у всех» не должно оставлять его в своей истории.
    let mine = history(&a, &b_key);
    assert_eq!(mine.len(), 1);
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
    let (a_key, b_key, c_key) =
        (a_id.public_key_bytes(), b_id.public_key_bytes(), c_id.public_key_bytes());
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
    assert!(wait(10, || !history(&b, &a_key).is_empty()), "сообщение дошло до Боба");
    let at_b = history(&b, &a_key);
    let msg = &at_b[0];
    let refs = vec![(msg["id"].as_u64().unwrap(), msg["sender"].as_str().unwrap().to_string())];

    // Боб пересылает Вере сообщение Ани.
    let sent = b.forward_messages(
        p2p_engine::ChatKey::Dm(a_key),
        &refs,
        p2p_engine::ChatKey::Dm(c_key),
        false,
    );
    assert_eq!(sent, 1);
    assert!(wait(10, || !history(&c, &b_key).is_empty()), "пересланное дошло до Веры");

    let at_c = history(&c, &b_key);
    let fwd = &at_c[0];
    assert_eq!(fwd["text"], "встречаемся в семь");
    assert_eq!(
        fwd["fwd_from"].as_str(),
        Some(hex::encode(a_key).as_str()),
        "у Веры видно, что автор — Аня, а не переславший Боб"
    );
    assert_eq!(fwd["sender"].as_str(), Some(hex::encode(b_key).as_str()), "прислал всё же Боб");

    // И у самого Боба его копия помнит источник — иначе «переслать дальше» потеряло
    // бы автора уже на втором шаге.
    let own = history(&b, &c_key);
    assert_eq!(own.len(), 1);
    assert_eq!(own[0]["fwd_from"].as_str(), Some(hex::encode(a_key).as_str()));

    // Пересылка пересланного: источник остаётся Аня, посредник не подменяет её.
    let refs2 = vec![(own[0]["id"].as_u64().unwrap(), own[0]["sender"].as_str().unwrap().to_string())];
    b.forward_messages(
        p2p_engine::ChatKey::Dm(c_key),
        &refs2,
        p2p_engine::ChatKey::Dm(a_key),
        false,
    );
    // Ищем ВХОДЯЩЕЕ: в этом чате уже лежит её собственный оригинал с тем же текстом.
    assert!(wait(10, || history(&a, &b_key).iter().any(|m| m["mine"] == false)));
    let back = history(&a, &b_key);
    let again = back.iter().find(|m| m["mine"] == false).unwrap();
    assert_eq!(again["text"], "встречаемся в семь");
    assert_eq!(again["fwd_from"].as_str(), Some(hex::encode(a_key).as_str()));
}

/// «Скрыть автора» скрывает его ПО-НАСТОЯЩЕМУ: ключ не уходит на провод вовсе, а
/// получатель видит «переслано» без имени. Иначе скрытие было бы косметикой в
/// интерфейсе — данные всё равно у собеседника.
#[test]
fn hidden_forward_sends_no_author_at_all() {
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(41), id(42), id(43));
    let (a_key, b_key, c_key) =
        (a_id.public_key_bytes(), b_id.public_key_bytes(), c_id.public_key_bytes());
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
        p2p_engine::ChatKey::Dm(a_key),
        &refs,
        p2p_engine::ChatKey::Dm(c_key),
        true, // скрыть автора
    );
    assert!(wait(10, || !history(&c, &b_key).is_empty()));

    let got = history(&c, &b_key);
    assert_eq!(got[0]["text"], "это между нами");
    assert_eq!(got[0]["fwd_from"].as_str(), Some(""), "ключа автора у Веры нет");
    assert_eq!(got[0]["fwd_anon"], true, "но видно, что это пересылка");

    // И своя копия у Боба совпадает с тем, что ушло: показывать себе имя, которого
    // собеседник не получил, значит расходиться с реальностью.
    let own = history(&b, &c_key);
    assert_eq!(own[0]["fwd_from"].as_str(), Some(""));
    assert_eq!(own[0]["fwd_anon"], true);
}
