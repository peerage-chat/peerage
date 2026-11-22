//! Интеграционные тесты группового слоя поверх loopback-транспорта — весь путь
//! инвайта и вступления в одном процессе, без Yggdrasil и устройств (ровно то, что
//! раньше требовало двух телефонов). Прогонять из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::store::{Chat, GroupStore, MessageStore};
use peerage_core::Identity;
use peerage_engine::groups::GroupManager;
use peerage_engine::MediaBody;
use peerage_messenger::{loopback::LoopbackNet, Config, Event, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-it-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Менеджер групп со СВОИМ хранилищем сообщений в том же каталоге — как в движке,
/// где история группы лежит в общей базе рядом с личными чатами. Второй менеджер на
/// том же каталоге получает ту же базу: так тесты проверяют и перезапуск.
fn manager(id: Identity, msgr: Arc<Messenger>, dir: PathBuf) -> GroupManager {
    manager_with_store(id, msgr, dir).0
}

/// То же, но отдаёт и хранилище: часть проверок (закрепы) смотрит прямо в него.
fn manager_with_store(
    id: Identity,
    msgr: Arc<Messenger>,
    dir: PathBuf,
) -> (GroupManager, Arc<Mutex<MessageStore>>) {
    let db = peerage_core::db::open(&dir).expect("база");
    let store = Arc::new(Mutex::new(MessageStore::open(db.clone())));
    (
        GroupManager::new(id, msgr, dir, store.clone(), GroupStore::new(db)),
        store,
    )
}

/// Разослать событие мессенджера соответствующему менеджеру группы (как event_pump).
fn dispatch(gm: &mut GroupManager, ev: Event) {
    match ev {
        Event::GroupKeyRequest { peer, gid } => gm.on_key_request(peer, &gid),
        Event::GroupInvite { peer, gid, name } => {
            gm.on_invite(peer, gid, name);
        }
        Event::GroupKeyReply {
            peer,
            gid,
            key_package,
        } => {
            gm.on_key_reply(peer, &gid, &key_package);
        }
        Event::Group { peer, data } => {
            gm.on_group(peer, &data);
        }
        Event::GroupResyncRequest { peer, gid } => gm.on_resync_request(peer, &gid),
        Event::GroupLeaveRequest { peer, gid } => {
            gm.on_leave_request(peer, &gid);
        }
        _ => {}
    }
}

/// Гонять события между двумя менеджерами, пока не выполнится `done` (или таймаут).
fn pump_until(
    a: &mut GroupManager,
    a_rx: &Receiver<Event>,
    b: &mut GroupManager,
    b_rx: &Receiver<Event>,
    done: impl Fn(&GroupManager, &GroupManager) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let mut progressed = false;
        while let Ok(ev) = a_rx.try_recv() {
            dispatch(a, ev);
            progressed = true;
        }
        while let Ok(ev) = b_rx.try_recv() {
            dispatch(b, ev);
            progressed = true;
        }
        if done(a, b) {
            return true;
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    done(a, b)
}

/// Два узла в общей loopback-сети с уже пройденным инвайтом: A (админ) + B в одной
/// группе. `drop_every` эмулирует потери на всём протяжении.
struct Ctx {
    gm_a: GroupManager,
    a_rx: Receiver<Event>,
    gm_b: GroupManager,
    b_rx: Receiver<Event>,
    gid: Vec<u8>,
    b_key: [u8; 32],
}

fn setup_and_join(drop_every: u32) -> Ctx {
    let net = LoopbackNet::new();
    if drop_every > 0 {
        net.set_drop_every(drop_every);
    }
    let (a_id, b_id) = (id(10), id(20));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let mut gm_a = manager(a_id, Arc::new(a_msgr), tmpdir("a"));
    let mut gm_b = manager(b_id, Arc::new(b_msgr), tmpdir("b"));

    let gid = gm_a.create("тест").expect("группа создана");
    gm_a.invite(&gid, b_key);
    // Приглашение приходит карточкой — B должен ЯВНО его принять (решение владельца).
    let got_invite = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains(&hex::encode(&gid))
    });
    assert!(
        got_invite,
        "B получил приглашение-карточку (drop_every={drop_every})"
    );
    assert!(gm_b.accept_invite(&gid), "B принял приглашение");
    let joined = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
        a.members(&gid).contains(&b_key) && b.group_ids().contains(&gid)
    });
    assert!(
        joined,
        "после accept инвайт доводит обе стороны в группу (drop_every={drop_every})"
    );
    assert!(gm_b.members(&gid).contains(&a_key), "B видит A в ростере");
    Ctx {
        gm_a,
        a_rx,
        gm_b,
        b_rx,
        gid,
        b_key,
    }
}

#[test]
fn invite_join_flow_over_loopback() {
    setup_and_join(0);
}

/// M2: медиа-сообщение в группе. A шлёт MLS-ссылку на блоб → B получает его как
/// медиа (kind/blob_id/mime/name), а не текст. (Байты блоба пушит слой Engine —
/// тот же blobs.offer, что в DM, покрыт media.rs.)
#[test]
fn group_media_message_propagates() {
    let mut ctx = setup_and_join(0);
    let media = MediaBody {
        kind: "image".into(),
        hash: "abcd1234".into(),
        mime: "image/jpeg".into(),
        name: "photo.jpg".into(),
        caption: "подпись в группе".into(),
        size: 900_000_000,
        thumb: "beef".into(),
        ..Default::default()
    };
    assert!(ctx.gm_a.send_media(&ctx.gid, &media));

    let field = |gm: &GroupManager, gid: &[u8], key: &str| -> Option<String> {
        let h: serde_json::Value = serde_json::from_str(&gm.history_json(gid)).ok()?;
        h.as_array()?
            .iter()
            .find(|m| m["kind"] == "image")
            .and_then(|m| m[key].as_str().map(str::to_string))
    };

    let got = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| field(b, &ctx.gid, "blob_id").as_deref() == Some("abcd1234"),
    );
    assert!(got, "B получил групповое медиа-сообщение");
    assert_eq!(
        field(&ctx.gm_b, &ctx.gid, "mime").as_deref(),
        Some("image/jpeg")
    );
    assert_eq!(
        field(&ctx.gm_b, &ctx.gid, "name").as_deref(),
        Some("photo.jpg")
    );
    // Размер и превью едут В САМОМ сообщении: без них получателю нечего показать
    // вместо молчаливой докачки, и карточка «скачать» невозможна.
    let num = |gm: &GroupManager, key: &str| -> Option<u64> {
        let h: serde_json::Value = serde_json::from_str(&gm.history_json(&ctx.gid)).ok()?;
        h.as_array()?
            .iter()
            .find(|m| m["kind"] == "image")
            .and_then(|m| m[key].as_u64())
    };
    assert_eq!(num(&ctx.gm_b, "size"), Some(900_000_000));
    assert_eq!(
        field(&ctx.gm_b, &ctx.gid, "thumb_id").as_deref(),
        Some("beef")
    );
    // Чужое вложение не считается скачанным, пока байты не приехали.
    assert_eq!(
        {
            let h: serde_json::Value =
                serde_json::from_str(&ctx.gm_b.history_json(&ctx.gid)).unwrap();
            h.as_array()
                .unwrap()
                .iter()
                .find(|m| m["kind"] == "image")
                .unwrap()["downloaded"]
                .as_bool()
        },
        Some(false),
    );
    // Подпись едет ВНУТРИ того же MLS-сообщения, а не отдельным текстом следом.
    assert_eq!(
        field(&ctx.gm_b, &ctx.gid, "text").as_deref(),
        Some("подпись в группе")
    );

    // У A своё сообщение тоже записано как медиа (mine).
    let ha: serde_json::Value = serde_json::from_str(&ctx.gm_a.history_json(&ctx.gid)).unwrap();
    let mine = ha
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["kind"] == "image")
        .unwrap();
    assert_eq!(mine["mine"], true);
    assert_eq!(mine["blob_id"], "abcd1234");
}

/// Приглашение приходит карточкой и НЕ присоединяет молча: без accept B не в группе;
/// decline тихо убирает карточку. (Решение владельца 2026-06-28 — инвайт-как-сообщение.)
#[test]
fn invite_card_requires_accept_and_decline_clears() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(30), id(31));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let mut gm_a = manager(a_id, Arc::new(a_msgr), tmpdir("a"));
    let mut gm_b = manager(b_id, Arc::new(b_msgr), tmpdir("b"));

    let gid = gm_a.create("тайный клуб").expect("группа создана");
    gm_a.invite(&gid, b_key);

    // Карточка доехала с именем группы; B при этом НЕ в группе.
    let got = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains("тайный клуб")
    });
    assert!(got, "B получил карточку приглашения с именем группы");
    assert!(
        !gm_b.group_ids().contains(&gid),
        "без accept B в группу НЕ вступает"
    );

    // Decline тихо убирает карточку; повторный accept уже нечему.
    assert!(gm_b.decline_invite(&gid), "decline убрал карточку");
    assert!(
        !gm_b.invites_json().contains(&hex::encode(&gid)),
        "карточка снята"
    );
    assert!(!gm_b.accept_invite(&gid), "после decline принимать нечего");
    assert!(!gm_b.group_ids().contains(&gid), "B так и не в группе");
}

/// Имя группы доезжает до вступившего: админ создаёт «тест», после add шлёт имя
/// служебным AppMsg — joiner видит «тест», а не дефолт «Группа».
#[test]
fn group_name_propagates_to_joiner() {
    let mut ctx = setup_and_join(0);

    let name_of = |gm: &GroupManager, gid: &[u8]| -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(&gm.list_json()).ok()?;
        v.as_array()?
            .iter()
            .find(|g| g["gid"] == hex::encode(gid))
            .and_then(|g| g["name"].as_str().map(str::to_string))
    };

    let got = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| name_of(b, &ctx.gid).as_deref() == Some("тест"),
    );
    assert!(got, "joiner получил настоящее имя группы");
    assert_eq!(
        name_of(&ctx.gm_a, &ctx.gid).as_deref(),
        Some("тест"),
        "у админа имя не изменилось"
    );
}

/// Ключ админа в списке групп (UI 3.1 — метка «админ» в участниках): у обоих — создатель.
#[test]
fn admin_key_is_listed_for_every_member() {
    let ctx = setup_and_join(0);
    let admin_of = |gm: &GroupManager| -> String {
        let v: serde_json::Value = serde_json::from_str(&gm.list_json()).unwrap();
        v.as_array()
            .unwrap()
            .iter()
            .find(|g| g["gid"] == hex::encode(&ctx.gid))
            .unwrap()["admin_key"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    let a_key = id(10).public_key_bytes(); // setup_and_join: A — id(10)
    assert_eq!(admin_of(&ctx.gm_a), hex::encode(a_key), "у админа — он сам");
    assert_eq!(
        admin_of(&ctx.gm_b),
        hex::encode(a_key),
        "у участника — пригласивший админ"
    );
}

/// Тот же путь под потерями — доказывает, что надёжный invite-хендшейк дожимает
/// приглашение сквозь дропы (регрессия против «инвайт не срабатывает»).
#[test]
fn invite_join_flow_survives_packet_loss() {
    setup_and_join(3);
}

/// ADR-004: админ отзывает участника → у него SelfRemoved (группа исчезает), из
/// ростера админа он тоже пропадает.
#[test]
fn admin_kick_removes_member() {
    let mut c = setup_and_join(0);
    let (gid, b_key) = (c.gid.clone(), c.b_key);
    assert!(c.gm_a.remove_member(&gid, b_key), "админ инициировал отзыв");
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |a, b| {
        !a.members(&gid).contains(&b_key) && !b.group_ids().contains(&gid)
    });
    assert!(
        ok,
        "после kick: A убрал B из ростера, B забыл группу (SelfRemoved)"
    );
}

/// ADR-004: участник выходит → админ отзывает его из общего ростера; локально он
/// забывает группу сразу.
#[test]
fn member_leave_revoked_by_admin() {
    let mut c = setup_and_join(0);
    let (gid, b_key) = (c.gid.clone(), c.b_key);
    assert!(c.gm_b.leave(&gid), "B инициировал выход");
    assert!(
        !c.gm_b.group_ids().contains(&gid),
        "B забыл группу локально сразу"
    );
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |a, _b| {
        !a.members(&gid).contains(&b_key)
    });
    assert!(ok, "после leave: админ A отозвал B из ростера");
}

/// Действия с групповым сообщением (ADR-006). Ссылка на сообщение в группе — пара
/// «автор + id»: одного id мало, собеседников много.
#[test]
fn group_reply_and_retract() {
    let mut ctx = setup_and_join(0);
    let a_hex = hex::encode(id(10).public_key_bytes());

    assert!(ctx.gm_a.send_text(&ctx.gid, "во сколько встречаемся?"));
    let got = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| b.history_json(&ctx.gid).contains("во сколько"),
    );
    assert!(got, "сообщение дошло до B");

    // B отвечает на сообщение A — ссылка = (автор A, id сообщения).
    let hist_b: serde_json::Value = serde_json::from_str(&ctx.gm_b.history_json(&ctx.gid)).unwrap();
    let first = hist_b
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["text"] == "во сколько встречаемся?")
        .unwrap();
    let target = first["id"]
        .as_u64()
        .expect("у группового сообщения есть id");
    assert!(target != 0, "id проставлен отправителем");
    assert!(ctx.gm_b.send_text_reply(&ctx.gid, "в семь", target, &a_hex));

    let replied = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |a, _| a.history_json(&ctx.gid).contains("в семь"),
    );
    assert!(replied, "ответ дошёл до A");
    let hist_a: serde_json::Value = serde_json::from_str(&ctx.gm_a.history_json(&ctx.gid)).unwrap();
    let reply = hist_a
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["text"] == "в семь")
        .unwrap();
    assert_eq!(reply["reply_to"].as_u64(), Some(target));
    assert_eq!(
        reply["reply_sender"], a_hex,
        "автор цитируемого — тот, на кого отвечали"
    );

    // A отзывает своё сообщение — оно исчезает и у B.
    assert_eq!(ctx.gm_a.retract(&ctx.gid, &[target]), 1);
    let gone = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| !b.history_json(&ctx.gid).contains("во сколько"),
    );
    assert!(gone, "отозванное исчезло у участника");
    assert!(
        !ctx.gm_a.history_json(&ctx.gid).contains("во сколько"),
        "и у автора тоже"
    );
    // На месте отозванного — строка «Сообщение удалено» у обоих.
    for gm in [&ctx.gm_a, &ctx.gm_b] {
        let h: serde_json::Value = serde_json::from_str(&gm.history_json(&ctx.gid)).unwrap();
        assert!(h
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"].as_u64() == Some(target) && m["kind"] == "deleted"));
    }
}

/// Упоминание — ссылка «участок + ключ»: доходит до участника, хранится у обоих,
/// заменяется при правке; кривой участок (посреди буквы) отбрасывается ещё у
/// отправителя, а не рисуется у получателя.
#[test]
fn group_mentions_travel_and_follow_edits() {
    use peerage_core::mention::Mention;
    let mut ctx = setup_and_join(0);
    let b_hex = hex::encode(ctx.b_key);
    let text = "@Боб, глянь";
    let span = Mention {
        at: 0,
        len: "@Боб".len() as u32,
        key: b_hex.clone(),
    };
    // Второй участок режет «Б» пополам — его быть не должно ни у кого.
    let broken = Mention {
        at: 2,
        len: 1,
        key: b_hex.clone(),
    };
    assert!(ctx
        .gm_a
        .send_text_full(&ctx.gid, text, 0, "", "", false, vec![span.clone(), broken]));
    let got = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| b.history_json(&ctx.gid).contains("глянь"),
    );
    assert!(got, "сообщение с упоминанием дошло");

    let find = |gm: &GroupManager, needle: &str| -> serde_json::Value {
        let h: serde_json::Value = serde_json::from_str(&gm.history_json(&ctx.gid)).unwrap();
        h.as_array()
            .unwrap()
            .iter()
            .find(|m| m["text"].as_str().unwrap_or("").contains(needle))
            .unwrap()
            .clone()
    };
    let at_b = find(&ctx.gm_b, "глянь");
    let want = serde_json::json!([{ "at": 0, "len": span.len, "key": b_hex }]);
    assert_eq!(
        at_b["mentions"], want,
        "у получателя ровно один честный участок"
    );
    assert_eq!(
        find(&ctx.gm_a, "глянь")["mentions"],
        want,
        "у автора то же, что ушло в сеть"
    );

    // Правка сдвигает текст — упоминание едет заново, со своим участком.
    let id = at_b["id"].as_u64().unwrap();
    let new_text = "Ну что, @Боб?";
    let moved = Mention {
        at: "Ну что, ".len() as u32,
        len: "@Боб".len() as u32,
        key: b_hex.clone(),
    };
    assert!(ctx
        .gm_a
        .edit(&ctx.gid, id, new_text, &[moved.clone()])
        .is_some());
    let edited = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| b.history_json(&ctx.gid).contains("Ну что"),
    );
    assert!(edited, "правка дошла");
    assert_eq!(find(&ctx.gm_b, "Ну что")["mentions"][0]["at"], moved.at);

    // Правка без упоминаний снимает их, а не оставляет висеть на чужом участке.
    assert!(ctx.gm_a.edit(&ctx.gid, id, "отбой", &[]).is_some());
    let cleared = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| b.history_json(&ctx.gid).contains("отбой"),
    );
    assert!(cleared);
    assert!(
        find(&ctx.gm_b, "отбой").get("mentions").is_none(),
        "упоминаний больше нет"
    );
}

/// Отозвать ЧУЖОЕ сообщение нельзя: автор берётся из MLS, а не из тела пакета,
/// иначе любой участник вычищал бы чужую переписку.
#[test]
fn group_retract_ignores_foreign_messages() {
    let mut ctx = setup_and_join(0);
    assert!(ctx.gm_a.send_text(&ctx.gid, "сообщение A"));
    let got = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| b.history_json(&ctx.gid).contains("сообщение A"),
    );
    assert!(got);

    let hist_b: serde_json::Value = serde_json::from_str(&ctx.gm_b.history_json(&ctx.gid)).unwrap();
    let foreign = first_message(&hist_b)["id"].as_u64().unwrap();
    assert_eq!(
        ctx.gm_b.retract(&ctx.gid, &[foreign]),
        0,
        "B не отзывает чужое"
    );
    assert!(
        ctx.gm_a.history_json(&ctx.gid).contains("сообщение A"),
        "у автора всё на месте"
    );

    // Удалить у СЕБЯ — можно любое.
    let a_hex = hex::encode(id(10).public_key_bytes());
    assert_eq!(ctx.gm_b.delete_local(&ctx.gid, &[(foreign, a_hex)]), 1);
    assert!(
        !ctx.gm_b.history_json(&ctx.gid).contains("сообщение A"),
        "у себя убрано"
    );
    assert!(
        ctx.gm_a.history_json(&ctx.gid).contains("сообщение A"),
        "у автора осталось"
    );
}

/// Перезапуск ОТПРАВИТЕЛЯ не должен ломать группу.
///
/// Найдено на живом меше 2026-08-02: после рестарта сообщения узла переставали
/// доходить до участника — молча, без ошибок. Сценарий бытовой («закрыл приложение,
/// потом написал в группу»), поэтому проверяем его на петле.
#[test]
fn sender_restart_keeps_group_working() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(30), id(40));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let (a_msgr, b_msgr) = (Arc::new(a_msgr), Arc::new(b_msgr));
    let (a_dir, b_dir) = (tmpdir("restart-a"), tmpdir("restart-b"));
    let mut gm_a = manager(a_id.clone(), a_msgr.clone(), a_dir.clone());
    let mut gm_b = manager(b_id.clone(), b_msgr.clone(), b_dir.clone());

    let gid = gm_a.create("рестарт").expect("группа создана");
    gm_a.invite(&gid, b_key);
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains(&hex::encode(&gid))
    }));
    assert!(gm_b.accept_invite(&gid));
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
        a.members(&gid).contains(&b_key) && b.group_ids().contains(&gid)
    }));

    assert!(gm_a.send_text(&gid, "до перезапуска"));
    assert!(
        pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
            b.history_json(&gid).contains("до перезапуска")
        }),
        "обычная отправка работает"
    );

    // «Перезапуск приложения»: менеджер групп поднимается заново из того же каталога.
    drop(gm_a);
    let mut gm_a = manager(a_id, a_msgr, a_dir);
    assert!(gm_a.group_ids().contains(&gid), "группа поднялась с диска");
    let list: serde_json::Value = serde_json::from_str(&gm_a.list_json()).unwrap();
    assert_eq!(list[0]["admin"], true, "и мы в ней по-прежнему админ");
    assert_eq!(list[0]["name"], "рестарт");

    assert!(gm_a.send_text(&gid, "после перезапуска"));
    assert!(
        pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
            b.history_json(&gid).contains("после перезапуска")
        }),
        "после рестарта отправителя сообщение обязано дойти"
    );

    // Участник после перезапуска помнит, кто админ: от него зависит, чьи закрепы
    // и запросы KeyPackage принимать.
    drop(gm_b);
    let mut gm_b = manager(b_id.clone(), b_msgr.clone(), b_dir.clone());
    assert_eq!(
        gm_b.admin_key(&gid),
        Some(a_key),
        "админ известен после перезапуска"
    );
    let list: serde_json::Value = serde_json::from_str(&gm_b.list_json()).unwrap();
    assert_eq!(list[0]["admin"], false);

    // Вышел — после перезапуска группа не воскресает.
    assert!(gm_b.leave(&gid));
    drop(gm_b);
    let gm_b = manager(b_id, b_msgr, b_dir);
    assert!(
        !gm_b.group_ids().contains(&gid),
        "покинутая группа не вернулась"
    );
    assert_eq!(gm_b.admin_key(&gid), None);
}

/// Вступление не должно теряться, если пригласивший офлайн в момент «Вступить».
///
/// Найдено на живом меше 2026-08-02: карточка исчезала, KeyPackage уходил в никуда,
/// группа не появлялась НИКОГДА — даже когда пригласивший возвращался. Повторить
/// было нечем: приглашения в списке уже нет.
#[test]
fn accept_survives_offline_inviter() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(50), id(60));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let mut gm_a = manager(a_id, Arc::new(a_msgr), tmpdir("off-a"));
    let mut gm_b = manager(b_id, Arc::new(b_msgr), tmpdir("off-b"));

    let gid = gm_a.create("офлайн").expect("группа создана");
    gm_a.invite(&gid, b_key);
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains(&hex::encode(&gid))
    }));

    // Пригласивший «офлайн»: его события никто не обрабатывает, KeyPackage повисает.
    gm_b.accept_invite(&gid);
    for _ in 0..20 {
        while let Ok(ev) = b_rx.try_recv() {
            dispatch(&mut gm_b, ev);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !gm_b.group_ids().contains(&gid),
        "пока он офлайн — вступления нет"
    );
    assert!(
        gm_b.invites_json().contains("\"waiting\":true"),
        "карточка остаётся и честно показывает ожидание: {}",
        gm_b.invites_json()
    );

    // Пригласивший вернулся: клиент повторяет KeyPackage сам, без действий человека.
    gm_b.on_peer_online(a_key);
    assert!(
        pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
            a.members(&gid).contains(&b_key) && b.group_ids().contains(&gid)
        }),
        "после возвращения пригласившего вступление доводится до конца"
    );
    assert!(
        !gm_b.invites_json().contains(&hex::encode(&gid)),
        "карточка снята после вступления"
    );
}

/// Приняли приглашение, KeyPackage ПОТЕРЯЛСЯ насовсем (пригласивший офлайн дольше,
/// чем мессенджер повторяет неподтверждённое), да ещё и приложение перезапустили.
/// Вступление всё равно доводится до конца: повтор висит на маячке и не зависит ни
/// от presence (пригласивший может не быть контактом), ни от состояния в памяти.
#[test]
fn join_survives_lost_key_package_and_restart() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(51), id(61));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    // TTL короткий: недоставленное мессенджер бросает почти сразу — как в жизни
    // бросает через 5 минут, а пригласивший возвращается через час.
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        message_ttl: Duration::from_millis(200),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let b_msgr = Arc::new(b_msgr);
    let b_dir = tmpdir("lost-b");
    let mut gm_a = manager(a_id, Arc::new(a_msgr), tmpdir("lost-a"));
    let mut gm_b = manager(b_id.clone(), b_msgr.clone(), b_dir.clone());

    let gid = gm_a.create("потеря").expect("группа создана");
    gm_a.invite(&gid, b_key);
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains(&hex::encode(&gid))
    }));

    // Сеть глухая: KeyPackage не дойдёт ни с первой попытки, ни с ретраями — и
    // протухнет в очереди мессенджера.
    net.set_drop_every(1);
    gm_b.accept_invite(&gid);
    std::thread::sleep(Duration::from_millis(400));
    while a_rx.try_recv().is_ok() {}
    assert!(
        !gm_a.members(&gid).contains(&b_key),
        "KeyPackage до админа не дошёл"
    );

    // Перезапуск клиента: состояние вступления в памяти потеряно, на диске — только
    // приглашение с отметкой «принято».
    drop(gm_b);
    let mut gm_b = manager(b_id, b_msgr, b_dir);
    assert!(
        gm_b.invites_json().contains("\"waiting\":true"),
        "принятое приглашение пережило перезапуск: {}",
        gm_b.invites_json()
    );

    // Связь вернулась. Никто не жмёт «Вступить» второй раз — работает маячок.
    net.set_drop_every(0);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut done = false;
    while Instant::now() < deadline && !done {
        gm_b.retry_pending_joins();
        while let Ok(ev) = a_rx.try_recv() {
            dispatch(&mut gm_a, ev);
        }
        while let Ok(ev) = b_rx.try_recv() {
            dispatch(&mut gm_b, ev);
        }
        done = gm_a.members(&gid).contains(&b_key) && gm_b.group_ids().contains(&gid);
        if !done {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    assert!(done, "вступление доведено до конца само");
    assert!(
        !gm_b.invites_json().contains(&hex::encode(&gid)),
        "карточка снята после вступления"
    );
}

/// Вторая половина той же истории: пригласивший ПЕРЕЗАПУСТИЛСЯ, пока приглашённый
/// думал. Раньше «кого я позвал» жило только в памяти, и пришедший KeyPackage было
/// не с чем сопоставить — он молча выбрасывался, вступление не завершалось никогда.
#[test]
fn inviter_restart_still_accepts_key_package() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(52), id(62));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let a_msgr = Arc::new(a_msgr);
    let a_dir = tmpdir("re-a");
    let mut gm_a = manager(a_id.clone(), a_msgr.clone(), a_dir.clone());
    let mut gm_b = manager(b_id, Arc::new(b_msgr), tmpdir("re-b"));

    let gid = gm_a.create("перезапуск").expect("группа создана");
    gm_a.invite(&gid, b_key);
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains(&hex::encode(&gid))
    }));

    // Пригласивший перезапустился ДО того, как приглашённый решился.
    drop(gm_a);
    let mut gm_a = manager(a_id, a_msgr, a_dir);

    gm_b.accept_invite(&gid);
    assert!(
        pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
            a.members(&gid).contains(&b_key) && b.group_ids().contains(&gid)
        }),
        "KeyPackage узнан и после перезапуска пригласившего"
    );
}

/// Закреп в группе ставит ТОЛЬКО админ — и он доезжает до участников. Иначе любой
/// участник вешал бы своё сообщение в шапку всем остальным, а проверять право на
/// стороне отправителя бессмысленно: клиент можно пропатчить.
#[test]
fn only_admin_can_pin_in_a_group() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(101), id(102));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let (mut gm_a, store_a) = manager_with_store(a_id, Arc::new(a_msgr), tmpdir("pin-a"));
    let (mut gm_b, store_b) = manager_with_store(b_id, Arc::new(b_msgr), tmpdir("pin-b"));

    let gid = gm_a.create("закрепы").expect("группа создана");
    gm_a.invite(&gid, b_key);
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains(&hex::encode(&gid))
    }));
    gm_b.accept_invite(&gid);
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
        a.members(&gid).contains(&b_key) && b.group_ids().contains(&gid)
    }));

    // Админ пишет и закрепляет своё сообщение.
    assert!(gm_a.send_text(&gid, "встречаемся у входа"));
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.history_json(&gid).contains("встречаемся у входа")
    }));
    let hist: serde_json::Value = serde_json::from_str(&gm_a.history_json(&gid)).unwrap();
    let msg = hist
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["text"] == "встречаемся у входа")
        .unwrap();
    let (msg_id, msg_sender) = (
        msg["id"].as_u64().unwrap(),
        msg["sender"].as_str().unwrap().to_string(),
    );

    assert!(gm_a.pin(&gid, msg_id, &msg_sender, true), "админ закрепил");
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, _| {
        !store_b.lock().unwrap().pinned(Chat::Group(&gid)).is_empty()
    }));
    let at_b = store_b.lock().unwrap().pinned(Chat::Group(&gid));
    assert_eq!(at_b.len(), 1);
    assert_eq!(
        at_b[0].text, "встречаемся у входа",
        "закреп доехал до участника"
    );

    // Участник закрепить не может — ни у себя, ни тем более у админа.
    assert!(
        !gm_b.pin(&gid, msg_id, &msg_sender, true),
        "не админ — отказ"
    );
    for _ in 0..10 {
        while let Ok(ev) = a_rx.try_recv() {
            dispatch(&mut gm_a, ev);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        store_a.lock().unwrap().pinned(Chat::Group(&gid)).len(),
        1,
        "чужого закрепа нет"
    );

    // Открепление админом убирает закреп у всех.
    assert!(gm_a.pin(&gid, msg_id, &msg_sender, false));
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, _| {
        store_b.lock().unwrap().pinned(Chat::Group(&gid)).is_empty()
    }));
}

/// Первое обычное сообщение истории — мимо системных строк («Группа создана» и т.п.).
fn first_message(hist: &serde_json::Value) -> serde_json::Value {
    hist.as_array()
        .and_then(|a| a.iter().find(|m| m["system"] != true))
        .cloned()
        .unwrap_or_default()
}

/// Есть ли в истории группы у `gm` текст `text`?
fn has_text(gm: &GroupManager, gid: &[u8], text: &str) -> bool {
    let h: serde_json::Value = serde_json::from_str(&gm.history_json(gid)).unwrap_or_default();
    h.as_array()
        .map(|a| a.iter().any(|m| m["text"] == text))
        .unwrap_or(false)
}

/// R-002: resync по-прежнему работает — админ просит KeyPackage, перевыпускает
/// участника, и тот продолжает читать группу.
#[test]
fn admin_resync_still_rejoins_member() {
    let mut c = setup_and_join(0);
    let gid = c.gid.clone();
    c.gm_a.on_resync_request(c.b_key, &gid);
    // Даём обмену KeyPackage → Remove+Add → Welcome пройти, затем проверяем чтение.
    pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, _| false);
    assert!(
        c.gm_b.group_ids().contains(&gid),
        "B остался в группе после resync"
    );
    assert!(c.gm_a.send_text(&gid, "после resync"));
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, b| {
        has_text(b, &gid, "после resync")
    });
    assert!(ok, "B читает группу после перевыпуска");
}

/// R-002: KeyPackage по запросу отдаём только админу своей группы. Посторонний,
/// получивший его, мог бы вписать нас в свою группу без всякой карточки.
#[test]
fn key_request_from_non_admin_is_ignored() {
    let net = LoopbackNet::new();
    let (m_id, v_id) = (id(31), id(32));
    let (m_key, v_key) = (m_id.public_key_bytes(), v_id.public_key_bytes());
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (m_msgr, m_rx) = Messenger::start_with_transport(m_id, net.endpoint(m_key), cfg.clone());
    let (v_msgr, v_rx) = Messenger::start_with_transport(v_id.clone(), net.endpoint(v_key), cfg);
    let mut gm_v = manager(v_id, Arc::new(v_msgr), tmpdir("v"));
    m_msgr.send_group_key_request(&v_key, &[7u8; 16]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut leaked = false;
    while Instant::now() < deadline {
        while let Ok(ev) = v_rx.try_recv() {
            dispatch(&mut gm_v, ev);
        }
        while let Ok(ev) = m_rx.try_recv() {
            leaked |= matches!(ev, Event::GroupKeyReply { .. });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!leaked, "посторонний не получил KeyPackage");
}

/// R-002: даже админ нашей группы не впишет нас в ДРУГУЮ свою группу: KeyPackage,
/// отданный на resync, годится только для групп, где он нас и ведёт.
#[test]
fn resync_key_package_cannot_join_another_group() {
    let mut c = setup_and_join(0);
    let other = c.gm_a.create("чужая").expect("вторая группа");
    c.gm_a.on_resync_request(c.b_key, &c.gid);
    // Подменяем адрес: KeyPackage от B админ применит ко второй группе.
    c.gm_a.invite(&other, c.b_key);
    pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, _| false);
    assert!(
        !c.gm_b.group_ids().contains(&other),
        "B не вписан во вторую группу"
    );
    assert!(
        c.gm_b.group_ids().contains(&c.gid),
        "первая группа на месте"
    );
}

/// R-027: одного человека можно позвать в две группы сразу. Второе приглашение не
/// затирает первое, и каждый KeyPackage добавляет ровно в ту группу, куда звали.
#[test]
fn one_person_invited_into_two_groups() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(33), id(34));
    let b_key = b_id.public_key_bytes();
    let a_key = a_id.public_key_bytes();
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let mut gm_a = manager(a_id, Arc::new(a_msgr), tmpdir("a2g"));
    let mut gm_b = manager(b_id, Arc::new(b_msgr), tmpdir("b2g"));

    let first = gm_a.create("первая").unwrap();
    let second = gm_a.create("вторая").unwrap();
    gm_a.invite(&first, b_key);
    gm_a.invite(&second, b_key);
    let both = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        let j = b.invites_json();
        j.contains(&hex::encode(&first)) && j.contains(&hex::encode(&second))
    });
    assert!(both, "обе карточки на месте");

    // Принимаем ПЕРВОЕ приглашение (позвали в него раньше, затёрто было бы оно).
    assert!(gm_b.accept_invite(&first));
    gm_b.accept_invite(&second); // встанет в очередь за первым
    let joined_first = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
        b.group_ids().contains(&first) && a.members(&first).contains(&b_key)
    });
    assert!(joined_first, "B вступил именно в первую группу");
    assert!(
        !gm_a.members(&second).contains(&b_key),
        "во вторую — пока нет"
    );

    // Очередь вступлений: маячок продолжает со второй.
    gm_b.retry_pending_joins();
    let joined_second = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
        b.group_ids().contains(&second) && a.members(&second).contains(&b_key)
    });
    assert!(joined_second, "и во вторую тоже");
}

/// Реакция в группе доходит до участника, а «кто поставил» берётся из MLS: в
/// наборе у получателя стоит ключ отправителя пакета.
#[test]
fn group_reactions_reach_members_with_mls_author() {
    let mut ctx = setup_and_join(0);
    assert!(ctx.gm_a.send_text(&ctx.gid, "шашлык беру на себя"));
    let got = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |_, b| b.history_json(&ctx.gid).contains("шашлык"),
    );
    assert!(got);
    let hist: serde_json::Value = serde_json::from_str(&ctx.gm_b.history_json(&ctx.gid)).unwrap();
    let msg = &first_message(&hist);
    let (target, author) = (
        msg["id"].as_u64().unwrap(),
        msg["sender"].as_str().unwrap().to_string(),
    );

    assert!(ctx.gm_b.react(&ctx.gid, target, &author, "👍", 10));
    let b_hex = hex::encode(ctx.b_key);
    let reached = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |a, _| {
            let h: serde_json::Value = serde_json::from_str(&a.history_json(&ctx.gid)).unwrap();
            first_message(&h)["reactions"][0]["who"][0] == b_hex.as_str()
        },
    );
    assert!(reached, "реакция участника дошла до автора, кто — из MLS");

    // Снятие тоже доезжает.
    assert!(ctx.gm_b.react(&ctx.gid, target, &author, "", 11));
    let cleared = pump_until(
        &mut ctx.gm_a,
        &ctx.a_rx,
        &mut ctx.gm_b,
        &ctx.b_rx,
        |a, _| {
            let h: serde_json::Value = serde_json::from_str(&a.history_json(&ctx.gid)).unwrap();
            first_message(&h).get("reactions").is_none()
        },
    );
    assert!(cleared);
}

/// Системные строки группы: (вид, о ком, параметр) по порядку.
fn system_rows(gm: &GroupManager, gid: &[u8]) -> Vec<(String, String, String)> {
    let h: serde_json::Value = serde_json::from_str(&gm.history_json(gid)).unwrap_or_default();
    h.as_array()
        .map(|a| {
            a.iter()
                .filter(|m| m["system"] == true)
                .map(|m| {
                    let f = |k: &str| m[k].as_str().unwrap_or_default().to_string();
                    (f("kind"), f("sender"), f("text"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 2.8а: строка говорит, что произошло и с кем. У админа — «создана» и «вступил,
/// пригласил я»; у вступившего — «вступил я, пригласил админ». Перевыпуск
/// отставшего строк не добавляет; удаление — «больше не в группе».
#[test]
fn group_system_rows_say_what_happened_and_to_whom() {
    let mut c = setup_and_join(0);
    let gid = c.gid.clone();
    let a = hex::encode(id(10).public_key_bytes());
    let b = hex::encode(c.b_key);
    let row = |k: &str, who: &str, p: &str| (k.to_string(), who.to_string(), p.to_string());

    assert_eq!(
        system_rows(&c.gm_a, &gid),
        vec![row("created", &a, ""), row("joined", &b, &a)]
    );
    assert_eq!(system_rows(&c.gm_b, &gid), vec![row("joined", &b, &a)]);

    // Перевыпуск отставшего — не событие для ленты.
    c.gm_a.on_resync_request(c.b_key, &gid);
    pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, _| false);
    assert_eq!(
        system_rows(&c.gm_a, &gid).len(),
        2,
        "resync без строк у админа"
    );
    assert_eq!(
        system_rows(&c.gm_b, &gid).len(),
        1,
        "resync без строк у участника"
    );
    assert!(c.gm_a.send_text(&gid, "после resync"));
    assert!(pump_until(
        &mut c.gm_a,
        &c.a_rx,
        &mut c.gm_b,
        &c.b_rx,
        |_, b| { has_text(b, &gid, "после resync") }
    ));

    assert!(c.gm_a.remove_member(&gid, c.b_key));
    assert_eq!(
        system_rows(&c.gm_a, &gid).last(),
        Some(&row("left", &b, ""))
    );
}

/// Ключ админа группы `gid` в списке групп менеджера (пусто — неизвестен).
fn admin_in_list(gm: &GroupManager, gid: &[u8]) -> String {
    let v: serde_json::Value = serde_json::from_str(&gm.list_json()).unwrap();
    v.as_array()
        .and_then(|a| a.iter().find(|g| g["gid"] == hex::encode(gid)))
        .and_then(|g| g["admin_key"].as_str())
        .unwrap_or_default()
        .to_string()
}

/// Шаг 7j: админ передаёт права — у обоих новый админ; отзывать может уже он, а
/// бывший админ — нет.
#[test]
fn admin_transfer_moves_rights() {
    let mut c = setup_and_join(0);
    let (gid, b_key) = (c.gid.clone(), c.b_key);
    let a_key = id(10).public_key_bytes();
    assert!(
        c.gm_a.transfer_admin(&gid, b_key, false),
        "A передал права B"
    );
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |a, b| {
        admin_in_list(a, &gid) == hex::encode(b_key) && admin_in_list(b, &gid) == hex::encode(b_key)
    });
    assert!(ok, "у обоих админ — B");
    assert!(
        !c.gm_a.remove_member(&gid, b_key),
        "бывший админ больше не отзывает"
    );
    assert!(!c.gm_a.rename(&gid, "чужое"), "и не переименовывает");
    assert!(c.gm_b.remove_member(&gid, a_key), "новый админ отзывает");
    let gone = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |a, b| {
        !b.members(&gid).contains(&a_key) && !a.group_ids().contains(&gid)
    });
    assert!(gone, "A отозван новым админом");
}

/// Звать в группу вправе только админ: у рядового участника приглашение не уходит,
/// бывший админ после передачи прав тоже больше не зовёт. Чужой Add-коммит
/// остальные не применяют — это проверяет `peerage-group` (roster_change_by_non_admin).
#[test]
fn only_admin_invites() {
    let mut c = setup_and_join(0);
    let (gid, b_key) = (c.gid.clone(), c.b_key);
    let stranger = id(77).public_key_bytes();
    c.gm_b.invite(&gid, stranger);
    assert_eq!(c.gm_b.invited_json(&gid), "[]", "B не админ — не зовёт");

    assert!(c.gm_a.transfer_admin(&gid, b_key, false));
    let moved = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, b| {
        admin_in_list(b, &gid) == hex::encode(b_key)
    });
    assert!(moved, "права у B");
    c.gm_a.invite(&gid, stranger);
    assert_eq!(c.gm_a.invited_json(&gid), "[]", "бывший админ не зовёт");
    c.gm_b.invite(&gid, stranger);
    assert!(
        c.gm_b.invited_json(&gid).contains(&hex::encode(stranger)),
        "новый админ зовёт"
    );
}

/// Шаг 7j: админ уходит с передачей — у себя забывает группу сразу, новый админ
/// отзывает его из ростера по тому же сообщению.
#[test]
fn admin_leaves_with_transfer() {
    let mut c = setup_and_join(0);
    let (gid, b_key) = (c.gid.clone(), c.b_key);
    let a_key = id(10).public_key_bytes();
    assert!(
        c.gm_a.transfer_admin(&gid, b_key, true),
        "A передал права и вышел"
    );
    assert!(!c.gm_a.group_ids().contains(&gid), "A забыл группу сразу");
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, b| {
        admin_in_list(b, &gid) == hex::encode(b_key) && !b.members(&gid).contains(&a_key)
    });
    assert!(ok, "B стал админом и отозвал A");
}

/// Шаг 7j: название меняет только админ, и оно доходит до участников.
#[test]
fn rename_only_by_admin() {
    let mut c = setup_and_join(0);
    let gid = c.gid.clone();
    let name_of = |gm: &GroupManager| -> String {
        let v: serde_json::Value = serde_json::from_str(&gm.list_json()).unwrap();
        v.as_array()
            .and_then(|a| a.iter().find(|g| g["gid"] == hex::encode(&gid)))
            .and_then(|g| g["name"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        !c.gm_b.rename(&gid, "захват"),
        "участник не переименовывает"
    );
    assert!(c.gm_a.rename(&gid, "Дача"), "админ переименовал");
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, b| {
        name_of(b) == "Дача"
    });
    assert!(ok, "новое название дошло до участника");
}

/// Шаг 7j: фото группы ставит только админ; ссылка на блоб доходит до участника.
#[test]
fn photo_only_by_admin_reaches_member() {
    let mut c = setup_and_join(0);
    let gid = c.gid.clone();
    let photo_of = |gm: &GroupManager| -> String {
        let v: serde_json::Value = serde_json::from_str(&gm.list_json()).unwrap();
        v.as_array()
            .and_then(|a| a.iter().find(|g| g["gid"] == hex::encode(&gid)))
            .and_then(|g| g["photo"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let blob = "ab".repeat(32);
    assert!(!c.gm_b.set_photo(&gid, &blob), "участник не ставит фото");
    assert!(c.gm_a.set_photo(&gid, &blob), "админ поставил фото");
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, b| {
        photo_of(b) == blob
    });
    assert!(ok, "ссылка на фото дошла до участника");
    assert!(c.gm_a.set_photo(&gid, ""), "админ убрал фото");
    let gone = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, b| {
        photo_of(b).is_empty()
    });
    assert!(gone, "и у участника его нет");
}

/// Режим уведомлений по умолчанию ставит админ (notifications.md): участнику он
/// приходит, от участника — нет; «ничего» навязать нельзя.
#[test]
fn notify_default_only_by_admin_reaches_member() {
    use peerage_core::notify::NotifyMode;
    let mut c = setup_and_join(0);
    let gid = c.gid.clone();
    let default_of = |gm: &GroupManager| -> String {
        let v: serde_json::Value = serde_json::from_str(&gm.list_json()).unwrap();
        v.as_array()
            .and_then(|a| a.iter().find(|g| g["gid"] == hex::encode(&gid)))
            .and_then(|g| g["notify_default"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(default_of(&c.gm_b), "all", "сначала — все сообщения");
    assert!(
        !c.gm_b.set_notify_default(&gid, NotifyMode::Mentions),
        "участник не ставит"
    );
    assert!(
        !c.gm_a.set_notify_default(&gid, NotifyMode::Nothing),
        "«ничего» — не умолчание"
    );
    assert!(
        c.gm_a.set_notify_default(&gid, NotifyMode::Mentions),
        "админ поставил"
    );
    let ok = pump_until(&mut c.gm_a, &c.a_rx, &mut c.gm_b, &c.b_rx, |_, b| {
        default_of(b) == "mentions"
    });
    assert!(ok, "умолчание дошло до участника");
}

/// Инвариант MLS «двинул состояние — сохрани»: служебные отправки админа (имя и фото
/// новичку при вступлении) двигают храповик отправителя. Без сохранения после
/// перезапуска админ повторял уже потраченное поколение, и участник молча выбрасывал
/// следующее сообщение (живой прогон 3.5: переименование не дошло, фото — дошло).
#[test]
fn admin_restart_after_join_keeps_next_message() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(31), id(41));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config {
        retry_interval: Duration::from_millis(40),
        ..Config::default()
    };
    let (a_msgr, a_rx) =
        Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let a_msgr = Arc::new(a_msgr);
    let a_dir = tmpdir("restart-name-a");
    let mut gm_a = manager(a_id.clone(), a_msgr.clone(), a_dir.clone());
    let mut gm_b = manager(b_id, Arc::new(b_msgr), tmpdir("restart-name-b"));

    let gid = gm_a.create("дача").expect("группа создана");
    // Первый перезапуск тратит суточную ротацию ключа (новая эпоха с нуля прятала бы
    // повтор поколения); на телефоне её к этому моменту уже давно сделали.
    drop(gm_a);
    let mut gm_a = manager(a_id.clone(), a_msgr.clone(), a_dir.clone());
    gm_a.invite(&gid, b_key);
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains(&hex::encode(&gid))
    }));
    assert!(gm_b.accept_invite(&gid));
    assert!(pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
        a.members(&gid).contains(&b_key) && b.group_ids().contains(&gid)
    }));
    // Имя группы новичку — первое прикладное сообщение админа в эпохе: даём ему дойти
    // и потратить у B это поколение.
    let until = Instant::now() + Duration::from_millis(1500);
    pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, _| {
        Instant::now() > until
    });

    drop(gm_a);
    let mut gm_a = manager(a_id, a_msgr, a_dir);
    assert!(
        gm_a.rename(&gid, "дача 2"),
        "админ переименовал после перезапуска"
    );
    let ok = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.list_json().contains("дача 2")
    });
    assert!(
        ok,
        "новое название дошло: храповик после имени-новичку был сохранён"
    );
}
