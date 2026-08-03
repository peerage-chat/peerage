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
