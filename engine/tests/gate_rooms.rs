//! Кто вправе быть в нашей комнате и звать нас: присутствие в звонке — это адрес,
//! куда уходят наш голос и видео, поэтому чужой маячок или приглашение не должны
//! ничего менять. «Злоумышленник» — голый мессенджер в той же loopback-сети: шлёт
//! пакеты, которые настоящий клиент никогда бы не отправил.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::Engine;
use peerage_messenger::loopback::LoopbackNet;
use peerage_messenger::{Config, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-gaterooms-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    // Как на телефоне: узел на транспортном ключе эпохи, не на ключе личности.
    let key = ident.transport_public(peerage_engine::node_epoch(&dir));
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
}

/// Знакомство, как на телефоне: оба добавляют друг друга по коду (встречные запросы
/// принимают друг друга). Голый ключ адресом не бывает — контакт по одному ключу
/// недостижим. `b_name` — как A подписывает B, `a_name` — как B подписывает A.
fn befriend(a: &Engine, b: &Engine, b_name: &str, a_name: &str) {
    assert_eq!(a.request_contact(&b.my_contact_code(), b_name, ""), "sent");
    assert_eq!(b.request_contact(&a.my_contact_code(), a_name, ""), "sent");
    let settled = |e: &Engine| {
        let v: serde_json::Value = serde_json::from_str(&e.contacts_json()).unwrap();
        v.as_array()
            .is_some_and(|cs| cs.len() == 1 && cs[0]["awaiting"] == false)
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !(settled(a) && settled(b)) {
        assert!(
            Instant::now() < deadline,
            "знакомство по коду не состоялось"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Адрес движка — из его кода обмена (подписанная запись).
fn addr_of(e: &Engine) -> [u8; 32] {
    let code =
        peerage_core::code::ContactCode::parse(&e.my_contact_code(), peerage_engine::NET).unwrap();
    let a = code.address.unwrap();
    let rec =
        peerage_core::record::AddressRecord::verify(&code.identity, &a.bytes, &a.sig).unwrap();
    rec.addr.as_slice().try_into().unwrap()
}

/// Код обмена голого мессенджера (у него нет движка, код собираем сами): по нему
/// движок узнаёт адрес атакующего, как узнал бы адрес настоящего контакта.
fn attacker_code(ident: &Identity) -> String {
    peerage_core::code::ContactCode {
        kind: peerage_engine::NET,
        identity: ident.public_key_bytes(),
        address: Some(ident.address_record(ATTACKER_EPOCH)),
        name: None,
    }
    .encode()
}

/// Эпоха адреса атакующего: узел на её транспортном ключе, запись — в его запросе.
const ATTACKER_EPOCH: u32 = 1;

/// Голый мессенджер: умеет слать любые пакеты, гейтов и движка у него нет. Сидит на
/// транспортном ключе, как настоящий узел.
fn attacker(net: &LoopbackNet, ident: Identity) -> Messenger {
    let key = ident.transport_public(ATTACKER_EPOCH);
    let (m, rx) = Messenger::start_with_transport(ident, net.endpoint(key), Config::default());
    std::mem::forget(rx); // события атакующего не интересуют, но канал должен жить
    m
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

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

/// Есть ли ключ `key` среди участников комнаты по маячкам?
fn in_room(e: &Engine, room: &[u8], key: &[u8; 32]) -> bool {
    e.room_presence_peers_json(room).contains(&hex::encode(key))
}

/// Пришёл ли от `hex` запрос в контакты — им атакующий «синхронизирует» тест:
/// пакеты от одного отправителя обрабатываются по порядку, значит всё, что он
/// отправил до текста, движок к этому моменту уже разобрал.
fn has_request(e: &Engine, hex: &str) -> bool {
    json(&e.contact_requests_json())
        .as_array()
        .map(|a| a.iter().any(|r| r["key"] == hex))
        .unwrap_or(false)
}

/// Выбрать из очереди событий движка, был ли среди них `room_invite`.
fn drained_room_invite(e: &Engine) -> bool {
    let mut seen = false;
    while let Some(ev) = e.poll_event() {
        seen |= json(&ev)["type"] == "room_invite";
    }
    seen
}

/// R-001: третий, знающий ключи A и B, вычисляет id их DM-комнаты и шлёт A маячок
/// «я тут». Раньше он попадал в список адресатов голоса A; теперь отбрасывается —
/// даже если он контакт A.
#[test]
fn forged_presence_in_dm_room_is_dropped() {
    let net = LoopbackNet::new();
    let (a_id, b_id, m_id) = (id(71), id(72), id(73));
    let (a_key, b_key, m_key) = (
        a_id.public_key_bytes(),
        b_id.public_key_bytes(),
        m_id.public_key_bytes(),
    );
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    let m_code = attacker_code(&m_id);
    let m = attacker(&net, m_id);
    befriend(&a, &b, "B", "A");
    // M у A — контакт (ждёт принятия, но адрес известен из кода).
    assert_eq!(a.request_contact(&m_code, "M", ""), "sent");
    m.set_peer_transports(&[(a_key, addr_of(&a))]);

    let room = a.room_join_dm(b_key, true, false);
    assert_eq!(room, peerage_core::dm_room_id(&a_key, &b_key).to_vec());
    for _ in 0..3 {
        m.send_room_presence(&a_key, &room, 0).unwrap();
    }
    // Контроль: настоящий собеседник в ту же комнату попадает.
    b.room_join_dm(a_key, true, false);
    assert!(
        wait(15, || in_room(&a, &room, &b_key)),
        "B виден в комнате A"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !in_room(&a, &room, &m_key),
        "чужой маячок не сделал M участником DM-звонка"
    );

    std::mem::forget(net);
}

/// R-026: незнакомец не может позвонить — DM-звонок только от контакта. Карточка
/// группы от незнакомца (R-008) и запрос KeyPackage от не-админа (R-002) тоже
/// ни к чему не ведут.
#[test]
fn stranger_cannot_ring_or_invite_to_group() {
    let net = LoopbackNet::new();
    let (a_id, m_id) = (id(74), id(75));
    let (a_key, m_key) = (a_id.public_key_bytes(), m_id.public_key_bytes());
    let dir = tmpdir("a2");
    let a_addr = a_id.transport_public(peerage_engine::node_epoch(&dir));
    let a = start(&net, a_id, dir);
    let record = m_id.address_record(ATTACKER_EPOCH);
    let m = attacker(&net, m_id);
    // Адрес A — из его кода, как у настоящего отправителя запроса.
    m.set_peer_transports(&[(a_key, a_addr)]);

    // Сначала запрос: с ним A узнаёт адрес незнакомца, и звонок с карточкой ниже
    // доходят до гейта, а не режутся анти-спуфингом раньше него.
    m.send_contact_request(&a_key, "привет", "", &record)
        .unwrap();
    assert!(
        wait(15, || has_request(&a, &hex::encode(m_key))),
        "запрос незнакомца дошёл"
    );
    let room = peerage_core::dm_room_id(&a_key, &m_key);
    m.send_room_invite(&a_key, &room, true, false).unwrap();
    m.send_group_invite(&a_key, &[7u8; 32], "Бесплатные призы")
        .unwrap();
    std::thread::sleep(Duration::from_millis(1500));

    assert!(!drained_room_invite(&a), "звонок незнакомца не звонит");
    assert_eq!(
        json(&a.calls_json(10)).as_array().map(Vec::len),
        Some(0),
        "и в журнал не попал"
    );
    assert_eq!(
        json(&a.group_invites_json()).as_array().map(Vec::len),
        Some(0),
        "карточки группы нет"
    );

    std::mem::forget(net);
}

/// Контакт по-прежнему звонит: гейт режет только чужих.
#[test]
fn contact_still_rings() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(76), id(77));
    let a_key = a_id.public_key_bytes();
    let a = start(&net, a_id, tmpdir("a3"));
    let b = start(&net, b_id, tmpdir("b3"));
    befriend(&a, &b, "B", "A");

    b.room_invite_dm(a_key, true, false);
    let mut rung = false;
    assert!(
        wait(15, || {
            rung |= drained_room_invite(&a);
            rung
        }),
        "контакт дозвонился"
    );

    std::mem::forget(net);
}

/// Класс Signal RingRTC CVE-2026-113380: участник звонка заваливает нас трафиком,
/// а мы кладём трубку. Отправка нашего голоса обязана погаснуть сразу — выход не
/// стоит ни в какой очереди за входящим. «Микрофон» A продолжает подавать звук и
/// после выхода (захват на устройстве останавливается своим путём и может
/// запоздать) — до собеседника не должно дойти ничего.
#[test]
fn hangup_stops_our_audio_at_once_under_peer_flood() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let net = LoopbackNet::new();
    let (a_id, m_id) = (id(78), id(79));
    let (a_key, m_key) = (a_id.public_key_bytes(), m_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a-flood"));
    let m_code = attacker_code(&m_id);
    let m = Arc::new(attacker(&net, m_id));
    let heard = m.media_channel(8192);
    assert_eq!(a.request_contact(&m_code, "M", ""), "sent");
    m.set_peer_transports(&[(a_key, addr_of(&a))]);

    let room = a.room_invite_dm(m_key, true, false);
    m.send_room_presence(&a_key, &room, 0b01).unwrap();
    assert!(wait(15, || in_room(&a, &room, &m_key)), "M в комнате A");
    let params = json(&a.room_audio_start(room.clone(), 0, false));
    let frame = params["frame_len"].as_u64().unwrap() as usize;

    // Флуд от M: медиа и маячки, без пауз.
    let stop_flood = Arc::new(AtomicBool::new(false));
    let flood = {
        let (m, stop, room) = (m.clone(), stop_flood.clone(), room.clone());
        std::thread::spawn(move || {
            let junk = vec![0u8; 1000];
            let mut seq = 0u16;
            while !stop.load(Ordering::Relaxed) {
                let _ = m.send_media(&a_key, litertc::TRACK_VIDEO, seq, 0, &junk);
                if seq % 64 == 0 {
                    let _ = m.send_room_presence(&a_key, &room, 0b01);
                }
                seq = seq.wrapping_add(1);
            }
        })
    };
    // «Микрофон» A: подаёт звук всё время теста, в том числе после выхода.
    let mic_on = Arc::new(AtomicBool::new(true));
    let mic = {
        let (a, on) = (a.clone(), mic_on.clone());
        std::thread::spawn(move || {
            let tone: Vec<i16> = (0..frame)
                .map(|i| if i % 2 == 0 { 8000 } else { -8000 })
                .collect();
            while on.load(Ordering::Relaxed) {
                a.room_push_audio(&tone, std::time::Duration::ZERO);
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };
    let is_audio = |p: &peerage_messenger::MediaPacket| {
        p.src == a_key
            && p.frame()
                .is_some_and(|f| f.header.track == litertc::TRACK_AUDIO)
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = false;
    while !got && Instant::now() < deadline {
        got = heard
            .recv_timeout(Duration::from_millis(100))
            .is_ok_and(|p| is_audio(&p));
    }
    assert!(got, "до выхода голос A доходит до M");

    let t = Instant::now();
    a.room_audio_stop();
    a.room_leave(room.clone());
    let took = t.elapsed();
    assert!(
        took < Duration::from_millis(250),
        "выход под флудом занял {took:?}"
    );

    // То, что уже было в пути, дочитываем; дальше — ни одного кадра голоса.
    std::thread::sleep(Duration::from_millis(100));
    while heard.try_recv().is_ok() {}
    let window = Instant::now() + Duration::from_millis(700);
    let mut leaked = 0;
    while Instant::now() < window {
        if heard
            .recv_timeout(Duration::from_millis(50))
            .is_ok_and(|p| is_audio(&p))
        {
            leaked += 1;
        }
    }
    mic_on.store(false, Ordering::Relaxed);
    stop_flood.store(true, Ordering::Relaxed);
    let _ = (mic.join(), flood.join());
    assert_eq!(leaked, 0, "после выхода голос A не уходит собеседнику");

    std::mem::forget(net);
}
