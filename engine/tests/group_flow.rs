//! Интеграционные тесты группового слоя поверх loopback-транспорта — весь путь
//! инвайта и вступления в одном процессе, без Yggdrasil и устройств (ровно то, что
//! раньше требовало двух телефонов). Прогонять из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::store::MessageStore;
use p2p_core::Identity;
use p2p_engine::groups::GroupManager;
use p2p_engine::MediaBody;
use p2p_messenger::{loopback::LoopbackNet, Config, Event, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-it-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Менеджер групп со СВОИМ хранилищем сообщений в том же каталоге — как в движке,
/// где история группы лежит в общей базе рядом с личными чатами. Второй менеджер на
/// том же каталоге получает ту же базу: так тесты проверяют и перезапуск.
fn manager(id: Identity, msgr: Arc<Messenger>, dir: PathBuf) -> GroupManager {
    let me = id.public_key_bytes();
    let db = p2p_core::db::open(&dir).expect("база");
    let store = MessageStore::open(db, None, &me).expect("хранилище сообщений");
    GroupManager::new(id, msgr, dir, Arc::new(Mutex::new(store)))
}

/// Разослать событие мессенджера соответствующему менеджеру группы (как event_pump).
fn dispatch(gm: &mut GroupManager, ev: Event) {
    match ev {
        Event::GroupKeyRequest { peer } => gm.on_key_request(peer),
        Event::GroupInvite { peer, gid, name } => {
            gm.on_invite(peer, gid, name);
        }
        Event::GroupKeyReply { peer, key_package } => {
            gm.on_key_reply(peer, &key_package);
        }
        Event::Group { data, .. } => {
            gm.on_group(&data);
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
    let cfg = Config { retry_interval: Duration::from_millis(40), ..Config::default() };
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let mut gm_a = manager(a_id, Arc::new(a_msgr), tmpdir("a"));
    let mut gm_b = manager(b_id, Arc::new(b_msgr), tmpdir("b"));

    let gid = gm_a.create("тест").expect("группа создана");
    gm_a.invite(&gid, b_key);
    // Приглашение приходит карточкой — B должен ЯВНО его принять (решение владельца).
    let got_invite = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
        b.invites_json().contains(&hex::encode(&gid))
    });
    assert!(got_invite, "B получил приглашение-карточку (drop_every={drop_every})");
    assert!(gm_b.accept_invite(&gid), "B принял приглашение");
    let joined = pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |a, b| {
        a.members(&gid).contains(&b_key) && b.group_ids().contains(&gid)
    });
    assert!(joined, "после accept инвайт доводит обе стороны в группу (drop_every={drop_every})");
    assert!(gm_b.members(&gid).contains(&a_key), "B видит A в ростере");
    Ctx { gm_a, a_rx, gm_b, b_rx, gid, b_key }
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
        h.as_array()?.iter().find(|m| m["kind"] == "image")
            .and_then(|m| m[key].as_str().map(str::to_string))
    };

    let got = pump_until(&mut ctx.gm_a, &ctx.a_rx, &mut ctx.gm_b, &ctx.b_rx, |_, b| {
        field(b, &ctx.gid, "blob_id").as_deref() == Some("abcd1234")
    });
    assert!(got, "B получил групповое медиа-сообщение");
    assert_eq!(field(&ctx.gm_b, &ctx.gid, "mime").as_deref(), Some("image/jpeg"));
    assert_eq!(field(&ctx.gm_b, &ctx.gid, "name").as_deref(), Some("photo.jpg"));
    // Размер и превью едут В САМОМ сообщении: без них получателю нечего показать
    // вместо молчаливой докачки, и карточка «скачать» невозможна.
    let num = |gm: &GroupManager, key: &str| -> Option<u64> {
        let h: serde_json::Value = serde_json::from_str(&gm.history_json(&ctx.gid)).ok()?;
        h.as_array()?.iter().find(|m| m["kind"] == "image").and_then(|m| m[key].as_u64())
    };
    assert_eq!(num(&ctx.gm_b, "size"), Some(900_000_000));
    assert_eq!(field(&ctx.gm_b, &ctx.gid, "thumb_id").as_deref(), Some("beef"));
    // Чужое вложение не считается скачанным, пока байты не приехали.
    assert_eq!(
        {
            let h: serde_json::Value = serde_json::from_str(&ctx.gm_b.history_json(&ctx.gid)).unwrap();
            h.as_array().unwrap().iter().find(|m| m["kind"] == "image").unwrap()["downloaded"].as_bool()
        },
        Some(false),
    );
    // Подпись едет ВНУТРИ того же MLS-сообщения, а не отдельным текстом следом.
    assert_eq!(field(&ctx.gm_b, &ctx.gid, "text").as_deref(), Some("подпись в группе"));

    // У A своё сообщение тоже записано как медиа (mine).
    let ha: serde_json::Value = serde_json::from_str(&ctx.gm_a.history_json(&ctx.gid)).unwrap();
    let mine = ha.as_array().unwrap().iter().find(|m| m["kind"] == "image").unwrap();
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
    let cfg = Config { retry_interval: Duration::from_millis(40), ..Config::default() };
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
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
    assert!(!gm_b.group_ids().contains(&gid), "без accept B в группу НЕ вступает");

    // Decline тихо убирает карточку; повторный accept уже нечему.
    assert!(gm_b.decline_invite(&gid), "decline убрал карточку");
    assert!(!gm_b.invites_json().contains(&hex::encode(&gid)), "карточка снята");
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
        v.as_array()?.iter()
            .find(|g| g["gid"] == hex::encode(gid))
            .and_then(|g| g["name"].as_str().map(str::to_string))
    };

    let got = pump_until(&mut ctx.gm_a, &ctx.a_rx, &mut ctx.gm_b, &ctx.b_rx, |_, b| {
        name_of(b, &ctx.gid).as_deref() == Some("тест")
    });
    assert!(got, "joiner получил настоящее имя группы");
    assert_eq!(name_of(&ctx.gm_a, &ctx.gid).as_deref(), Some("тест"), "у админа имя не изменилось");
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
    assert!(ok, "после kick: A убрал B из ростера, B забыл группу (SelfRemoved)");
}

/// ADR-004: участник выходит → админ отзывает его из общего ростера; локально он
/// забывает группу сразу.
#[test]
fn member_leave_revoked_by_admin() {
    let mut c = setup_and_join(0);
    let (gid, b_key) = (c.gid.clone(), c.b_key);
    assert!(c.gm_b.leave(&gid), "B инициировал выход");
    assert!(!c.gm_b.group_ids().contains(&gid), "B забыл группу локально сразу");
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
    let got = pump_until(&mut ctx.gm_a, &ctx.a_rx, &mut ctx.gm_b, &ctx.b_rx, |_, b| {
        b.history_json(&ctx.gid).contains("во сколько")
    });
    assert!(got, "сообщение дошло до B");

    // B отвечает на сообщение A — ссылка = (автор A, id сообщения).
    let hist_b: serde_json::Value = serde_json::from_str(&ctx.gm_b.history_json(&ctx.gid)).unwrap();
    let first = hist_b.as_array().unwrap().iter().find(|m| m["text"] == "во сколько встречаемся?").unwrap();
    let target = first["id"].as_u64().expect("у группового сообщения есть id");
    assert!(target != 0, "id проставлен отправителем");
    assert!(ctx.gm_b.send_text_reply(&ctx.gid, "в семь", target, &a_hex));

    let replied = pump_until(&mut ctx.gm_a, &ctx.a_rx, &mut ctx.gm_b, &ctx.b_rx, |a, _| {
        a.history_json(&ctx.gid).contains("в семь")
    });
    assert!(replied, "ответ дошёл до A");
    let hist_a: serde_json::Value = serde_json::from_str(&ctx.gm_a.history_json(&ctx.gid)).unwrap();
    let reply = hist_a.as_array().unwrap().iter().find(|m| m["text"] == "в семь").unwrap();
    assert_eq!(reply["reply_to"].as_u64(), Some(target));
    assert_eq!(reply["reply_sender"], a_hex, "автор цитируемого — тот, на кого отвечали");

    // A отзывает своё сообщение — оно исчезает и у B.
    assert_eq!(ctx.gm_a.retract(&ctx.gid, &[target]), 1);
    let gone = pump_until(&mut ctx.gm_a, &ctx.a_rx, &mut ctx.gm_b, &ctx.b_rx, |_, b| {
        !b.history_json(&ctx.gid).contains("во сколько")
    });
    assert!(gone, "отозванное исчезло у участника");
    assert!(!ctx.gm_a.history_json(&ctx.gid).contains("во сколько"), "и у автора тоже");
}

/// Отозвать ЧУЖОЕ сообщение нельзя: автор берётся из MLS, а не из тела пакета,
/// иначе любой участник вычищал бы чужую переписку.
#[test]
fn group_retract_ignores_foreign_messages() {
    let mut ctx = setup_and_join(0);
    assert!(ctx.gm_a.send_text(&ctx.gid, "сообщение A"));
    let got = pump_until(&mut ctx.gm_a, &ctx.a_rx, &mut ctx.gm_b, &ctx.b_rx, |_, b| {
        b.history_json(&ctx.gid).contains("сообщение A")
    });
    assert!(got);

    let hist_b: serde_json::Value = serde_json::from_str(&ctx.gm_b.history_json(&ctx.gid)).unwrap();
    let foreign = hist_b.as_array().unwrap()[0]["id"].as_u64().unwrap();
    assert_eq!(ctx.gm_b.retract(&ctx.gid, &[foreign]), 0, "B не отзывает чужое");
    assert!(ctx.gm_a.history_json(&ctx.gid).contains("сообщение A"), "у автора всё на месте");

    // Удалить у СЕБЯ — можно любое.
    let a_hex = hex::encode(id(10).public_key_bytes());
    assert_eq!(ctx.gm_b.delete_local(&ctx.gid, &[(foreign, a_hex)]), 1);
    assert!(!ctx.gm_b.history_json(&ctx.gid).contains("сообщение A"), "у себя убрано");
    assert!(ctx.gm_a.history_json(&ctx.gid).contains("сообщение A"), "у автора осталось");
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
    let cfg = Config { retry_interval: Duration::from_millis(40), ..Config::default() };
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id.clone(), net.endpoint(b_key), cfg);
    let a_msgr = Arc::new(a_msgr);
    let a_dir = tmpdir("restart-a");
    let mut gm_a = manager(a_id.clone(), a_msgr.clone(), a_dir.clone());
    let mut gm_b = manager(b_id, Arc::new(b_msgr), tmpdir("restart-b"));

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

    assert!(gm_a.send_text(&gid, "после перезапуска"));
    assert!(
        pump_until(&mut gm_a, &a_rx, &mut gm_b, &b_rx, |_, b| {
            b.history_json(&gid).contains("после перезапуска")
        }),
        "после рестарта отправителя сообщение обязано дойти"
    );
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
    let cfg = Config { retry_interval: Duration::from_millis(40), ..Config::default() };
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
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
    assert!(!gm_b.group_ids().contains(&gid), "пока он офлайн — вступления нет");
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
    assert!(!gm_b.invites_json().contains(&hex::encode(&gid)), "карточка снята после вступления");
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
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
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
    assert!(!gm_a.members(&gid).contains(&b_key), "KeyPackage до админа не дошёл");

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
    assert!(!gm_b.invites_json().contains(&hex::encode(&gid)), "карточка снята после вступления");
}

/// Вторая половина той же истории: пригласивший ПЕРЕЗАПУСТИЛСЯ, пока приглашённый
/// думал. Раньше «кого я позвал» жило только в памяти, и пришедший KeyPackage было
/// не с чем сопоставить — он молча выбрасывался, вступление не завершалось никогда.
#[test]
fn inviter_restart_still_accepts_key_package() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(52), id(62));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config { retry_interval: Duration::from_millis(40), ..Config::default() };
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id.clone(), net.endpoint(a_key), cfg.clone());
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

/// История группы, накопленная в СТАРОМ формате (JSON-файл рядом с MLS-состоянием),
/// переезжает в общее хранилище при первом запуске — и встаёт в один ряд с новыми
/// сообщениями. Это переписка пользователя, поэтому проверяется отдельно.
#[test]
fn old_group_history_file_migrates_into_the_store() {
    let net = LoopbackNet::new();
    let a_id = id(71);
    let a_key = a_id.public_key_bytes();
    let (a_msgr, _a_rx) = Messenger::start_with_transport(
        a_id.clone(),
        net.endpoint(a_key),
        Config::default(),
    );
    let a_msgr = Arc::new(a_msgr);
    let dir = tmpdir("migrate-a");

    let gid = {
        let mut gm = manager(a_id.clone(), a_msgr.clone(), dir.clone());
        let gid = gm.create("архив").expect("группа создана");
        gm.send_text(&gid, "новое сообщение");
        gid
    };

    // Кладём рядом файл ровно того вида, что писала прошлая версия.
    let old = serde_json::json!([{
        "id": 1_700_000_000_000u64,
        "text": "сообщение из старой версии",
        "mine": false,
        "time": 1_700_000_000i64,
        "sender": hex::encode([9u8; 32]),
        "kind": "text",
    }]);
    let path = dir.join("groups").join(format!("{}.msgs.json", hex::encode(&gid)));
    std::fs::write(&path, p2p_core::vault::seal(&serde_json::to_vec(&old).unwrap())).unwrap();

    // Перезапуск: менеджер поднимает группу и по дороге переливает файл.
    let gm2 = manager(a_id, a_msgr, dir.clone());
    let hist: serde_json::Value = serde_json::from_str(&gm2.history_json(&gid)).unwrap();
    let texts: Vec<&str> = hist.as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap()).collect();
    assert!(texts.contains(&"сообщение из старой версии"), "старое переехало: {texts:?}");
    assert!(texts.contains(&"новое сообщение"), "новое на месте: {texts:?}");
    assert!(!path.exists(), "файл убран, чтобы не переливать его снова");
}
