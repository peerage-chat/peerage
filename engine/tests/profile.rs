//! Распространение профиля (M2): A задаёт имя + аватар → B (контакт) получает
//! `ProfileUpdate`, видит самоназвание, тянет аватар-блоб ПО ХЭШУ и собирает его.
//! Локальная заметка B об A не затирается. Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

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
    let d = std::env::temp_dir().join(format!("p2p-prof-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

fn jpeg_with_exif() -> Vec<u8> {
    jpeg_variant(0x12, 0x34)
}

/// JPEG с EXIF и заданными байтами энтропии — разные байты → разный scrubbed-контент
/// → разный контент-адрес (для теста смены аватара).
fn jpeg_variant(a: u8, b: u8) -> Vec<u8> {
    let mut j = vec![0xFF, 0xD8];
    j.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0]);
    j.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, a, b, 0xFF, 0xD9]);
    j
}

/// Дождаться, пока B увидит у контакта A заданные profile_name+avatar_id и подтянет
/// байты аватара по хэшу. Возвращает подтянутые байты. Паника по таймауту.
fn await_profile(b: &Engine, a_hex: &str, name: &str, avatar_id: &str) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cj: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
        let found = cj.as_array().and_then(|arr| arr.iter().find(|c| c["key"] == a_hex));
        if let Some(c) = found {
            if c["profile_name"] == name && c["avatar_id"] == avatar_id {
                if let Some(bytes) = b.blob_read(avatar_id) {
                    assert_eq!(c["name"], "A-note", "локальная заметка B не должна затираться");
                    return bytes;
                }
            }
        }
        assert!(Instant::now() < deadline, "B не получил профиль {name}/{avatar_id} за 10с");
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn profile_propagates_name_and_avatar_by_hash() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(1), id(2));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A-note"); // локальная заметка B об A — НЕ должна затереться

    // A задаёт профиль → broadcast_profile шлёт ProfileUpdate контакту B.
    a.set_self_name("Алиса");
    let avatar_id = a.set_self_avatar(jpeg_with_exif(), "image/jpeg");

    let a_hex = hex::encode(a_key);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cj: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
        let found = cj.as_array().and_then(|arr| arr.iter().find(|c| c["key"] == a_hex));
        if let Some(c) = found {
            if c["profile_name"] == "Алиса" && c["avatar_id"] == avatar_id {
                // B подтянул аватар по хэшу?
                if let Some(bytes) = b.blob_read(&avatar_id) {
                    assert_eq!(bytes, a.self_avatar().unwrap(), "аватар у B = очищенный аватар A");
                    assert_eq!(c["name"], "A-note", "локальная заметка B не затёрта");
                    // avatar_id — контент-адрес ОЧИЩЕННОГО (scrub снял EXIF).
                    assert_eq!(avatar_id, a.blob_id_hex(&bytes));
                    a.shutdown();
                    b.shutdown();
                    return;
                }
            }
        }
        assert!(Instant::now() < deadline, "B не получил профиль/аватар A за 10с");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// Смена профиля (дизайн M2: «аватар может меняться → шлём хэш → получатель
/// перезапрашивает по несовпадению»). A меняет имя и аватар на новые; B видит
/// НОВЫЙ хэш, тянет НОВЫЕ байты, а старый блоб остаётся читаемым (контент-адрес →
/// дедуп/ретенция). Локальная заметка B об A всё время не тронута.
#[test]
fn profile_change_repropagates_by_new_hash() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(3), id(4));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a2"));
    let b = start(&net, b_id, tmpdir("b2"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A-note");
    let a_hex = hex::encode(a_key);

    // Первая версия профиля.
    a.set_self_name("Алиса");
    let id1 = a.set_self_avatar(jpeg_variant(0x12, 0x34), "image/jpeg");
    let got1 = await_profile(&b, &a_hex, "Алиса", &id1);
    assert_eq!(got1, a.self_avatar().unwrap(), "B собрал первый аватар A");

    // Вторая версия: другое имя и ДРУГИЕ байты аватара → другой контент-адрес.
    a.set_self_name("Алиса-2");
    let id2 = a.set_self_avatar(jpeg_variant(0x56, 0x78), "image/jpeg");
    assert_ne!(id1, id2, "разный аватар → разный хэш");
    let got2 = await_profile(&b, &a_hex, "Алиса-2", &id2);
    assert_eq!(got2, a.self_avatar().unwrap(), "B подтянул НОВЫЙ аватар по новому хэшу");
    assert_ne!(got1, got2, "новые байты аватара отличаются от старых");

    // Старый блоб у B остаётся читаемым (контент-адрес — без перезаписи/удаления).
    assert_eq!(b.blob_read(&id1).as_deref(), Some(got1.as_slice()), "старый аватар сохранён у B");

    a.shutdown();
    b.shutdown();
}

/// Принятый через гейт контакт НЕ получает плейсхолдер-имя: локальная заметка пустая,
/// поэтому в UI показывается самоназвание пира (Contact.displayName: local →
/// profile_name → hex). Раньше это тестировалось на TOFU-автодобавлении по presence;
/// теперь незнакомец добавляется только после явного accept (гейт контактов, gate.rs).
/// После accept A меняет имя → ProfileUpdate долетает уже КОНТАКТУ B.
#[test]
fn accepted_contact_keeps_blank_name_so_self_name_shows() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(5), id(6));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a3"));
    let b = start(&net, b_id, tmpdir("b3"));
    a.add_contact(&b_key, "B"); // A знает B; B про A узнаёт из запроса
    a.send_text(&b_key, "постучался"); // незнакомец → запрос у B

    let a_hex = hex::encode(a_key);
    // Ждём запрос и одобряем — A становится контактом с пустым локальным именем.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rj: serde_json::Value = serde_json::from_str(&b.contact_requests_json()).unwrap();
        if rj.as_array().map(|a| a.iter().any(|r| r["key"] == a_hex)).unwrap_or(false) {
            break;
        }
        assert!(Instant::now() < deadline, "запрос от A не пришёл за 10с");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(b.accept_contact_request(&a_key));

    // A меняет имя → ProfileUpdate долетает контакту B; локальная заметка пустая.
    a.set_self_name("Алиса");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cj: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
        if let Some(c) = cj.as_array().and_then(|a| a.iter().find(|c| c["key"] == a_hex)) {
            if c["profile_name"] == "Алиса" {
                assert_eq!(c["name"], "", "принятый контакт без плейсхолдер-имени");
                a.shutdown();
                b.shutdown();
                return;
            }
        }
        assert!(Instant::now() < deadline, "B не увидел самоназвание A за 10с");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// Имена заданы ДО знакомства (обычный случай: имя вводится при первом запуске).
/// A стучится к B; профиль A, посланный на коннекте, B выбрасывает — A ему ещё
/// незнакомец. После одобрения B шлёт свой профиль с просьбой ответить — и имена
/// видят обе стороны без переподключения.
#[test]
fn names_set_before_contact_reach_both_sides_on_accept() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(7), id(8));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a4"));
    let b = start(&net, b_id, tmpdir("b4"));
    a.set_self_name("Алиса");
    b.set_self_name("Боб");
    a.add_contact(&b_key, "");
    a.send_text(&b_key, "привет");

    let (a_hex, b_hex) = (hex::encode(a_key), hex::encode(b_key));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rj: serde_json::Value = serde_json::from_str(&b.contact_requests_json()).unwrap();
        if rj.as_array().map(|r| r.iter().any(|r| r["key"] == a_hex)).unwrap_or(false) {
            break;
        }
        assert!(Instant::now() < deadline, "запрос от A не пришёл за 10с");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(b.accept_contact_request(&a_key));

    let profile_name = |e: &Engine, key: &str| -> String {
        let cj: serde_json::Value = serde_json::from_str(&e.contacts_json()).unwrap();
        cj.as_array()
            .and_then(|arr| arr.iter().find(|c| c["key"] == key))
            .and_then(|c| c["profile_name"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while profile_name(&b, &a_hex) != "Алиса" || profile_name(&a, &b_hex) != "Боб" {
        assert!(
            Instant::now() < deadline,
            "имена не дошли: у B — «{}», у A — «{}»",
            profile_name(&b, &a_hex),
            profile_name(&a, &b_hex)
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    a.shutdown();
    b.shutdown();
}

/// Код обмена (QR) несёт самоназвание: добавивший по нему сразу видит имя, а не
/// обрывок ключа. Пустое локальное имя остаётся пустым — иначе заглушка навсегда
/// перекрыла бы самоназвание. Кривой хвост имени не мешает добавлению.
#[test]
fn contact_code_carries_self_name() {
    let net = LoopbackNet::new();
    let (a_id, b_id, c_id) = (id(9), id(10), id(11));
    let a_key = a_id.public_key_bytes();
    let a = start(&net, a_id, tmpdir("a5"));
    let b = start(&net, b_id, tmpdir("b5"));
    let c = start(&net, c_id, tmpdir("c5"));

    // Без имени — код как раньше, голый hex.
    assert_eq!(a.my_contact_code(), hex::encode(a_key));
    a.set_self_name("Алиса");
    let code = a.my_contact_code();
    assert!(code.starts_with(&format!("{}#", hex::encode(a_key))), "имя хвостом: {code}");

    assert_eq!(b.add_contact_code(&code, ""), "added");
    let cj: serde_json::Value = serde_json::from_str(&b.contacts_json()).unwrap();
    let c0 = cj.as_array().unwrap().iter().find(|c| c["key"] == hex::encode(a_key)).unwrap();
    assert_eq!(c0["profile_name"], "Алиса");
    assert_eq!(c0["name"], "", "без заглушки: локальное имя пустое");

    // Хвост не hex — ключ всё равно добавляется, имя просто не берётся.
    assert_eq!(c.add_contact_code(&format!("{}#не-hex", hex::encode(a_key)), ""), "added");

    a.shutdown();
    b.shutdown();
    c.shutdown();
}
