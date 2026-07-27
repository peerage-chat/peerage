//! Headless-узел движка на РЕАЛЬНОМ меше — для интеграционных прогонов «эмулятор ↔
//! нативный узел» без второго эмулятора (экономит CPU). Поднимает `Engine::start`
//! (дозвон до публичных пиров, как Android), задаёт своё имя/аватар, добавляет
//! контакты и крутится, периодически печатая свой `contacts_json` (чтобы наблюдать
//! профиль второй стороны). Управление — одноразовыми аргументами.
//!
//! Запуск из WSL:
//!   LD_LIBRARY_PATH=ygg/libygg cargo run -p p2p-engine --example mesh_node -- \
//!       /tmp/nodeb --name Bob --avatar --add <PEER_HEX>
//!
//! Выводит строки с префиксами: `PUB <hex>`, `ADDR <addr>`, `AVATAR <id>`,
//! `CONTACTS <json>`. Останавливается по SIGINT/kill.

use std::path::PathBuf;
use std::time::Duration;

use p2p_engine::Engine;

/// Мини-JPEG с EXIF и заданными байтами энтропии — проходит scrub, даёт стабильный
/// контент-адрес. Разные байты → разный аватар.
fn tiny_jpeg(a: u8, b: u8) -> Vec<u8> {
    let mut j = vec![0xFF, 0xD8];
    j.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0]);
    j.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, a, b, 0xFF, 0xD9]);
    j
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: mesh_node <datadir> [--name N] [--add HEX] [--avatar]"));

    let mut name: Option<String> = None;
    let mut avatar = false;
    let mut avatar_file: Option<String> = None;
    let mut send_media: Option<String> = None;
    let mut send_text: Option<String> = None;
    let mut create_group: Option<String> = None;
    let mut adds: Vec<String> = Vec::new();
    let mut rest = args.collect::<Vec<_>>().into_iter();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--name" => name = rest.next(),
            "--add" => {
                if let Some(h) = rest.next() {
                    adds.push(h);
                }
            }
            "--avatar" => avatar = true,
            "--avatar-file" => avatar_file = rest.next(),
            "--send-media" => send_media = rest.next(),
            "--send-text" => send_text = rest.next(),
            // Создать группу с этим именем и пригласить всех --add (тест инвайт-карточки).
            "--create-group" => create_group = rest.next(),
            other => eprintln!("неизвестный аргумент: {other}"),
        }
    }

    // Фиксированный vault-ключ: шифрует ТОЛЬКО локальную БД at-rest (не личность),
    // для тест-узла достаточно константы, стабильной между рестартами.
    let key = [0x42u8; 32];
    let start = Engine::start(dir, Some(key)).expect("engine start");
    let engine = start.engine;
    println!("PUB {}", hex::encode(start.pubkey));
    println!("ADDR {}", start.address);

    // Добавляем контактов ДО имени/аватара — тогда broadcast_profile сразу их достанет.
    for h in &adds {
        if let Ok(bytes) = hex::decode(h) {
            if let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice()) {
                let r = engine.add_contact(&key, "peer");
                println!("ADD {h} {r}");
            }
        }
    }
    if let Some(n) = &name {
        engine.set_self_name(n);
        println!("NAME {n}");
    }
    if let Some(path) = &avatar_file {
        // Реальная картинка из файла (PNG/JPEG) — чтобы получатель смог её ДЕКОДИРОВАТЬ.
        let bytes = std::fs::read(path).expect("avatar file");
        let mime = if path.ends_with(".png") { "image/png" } else { "image/jpeg" };
        let id = engine.set_self_avatar(bytes, mime);
        println!("AVATAR {id}");
    } else if avatar {
        let id = engine.set_self_avatar(tiny_jpeg(0xAB, 0xCD), "image/jpeg");
        println!("AVATAR {id}");
    }
    use std::io::Write;
    let _ = std::io::stdout().flush();

    // Создать группу и пригласить всех --add (тест инвайт-карточки на клиенте).
    if let Some(gname) = &create_group {
        engine.group_create(gname);
        // Ждём, пока актор создаст группу и она появится в списке (берём её gid).
        let mut gid_hex = String::new();
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(200));
            let v: serde_json::Value =
                serde_json::from_str(&engine.group_list_json()).unwrap_or_default();
            if let Some(g) = v.as_array().and_then(|a| a.first()) {
                if let Some(h) = g["gid"].as_str() {
                    gid_hex = h.to_string();
                    break;
                }
            }
        }
        println!("GROUP {gid_hex} {gname}");
        if let Ok(gid) = hex::decode(&gid_hex) {
            for h in &adds {
                if let Some(peer) = hex::decode(h).ok().and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()) {
                    engine.group_invite(gid.clone(), peer);
                    println!("INVITED {h}");
                }
            }
        }
        let _ = std::io::stdout().flush();
    }

    // Одноразовая отправка картинки первому --add контакту (тест приёма медиа на клиенте).
    let media_peer: Option<[u8; 32]> = adds.first()
        .and_then(|h| hex::decode(h).ok())
        .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok());
    let mut sent_media = false;

    // Крутимся: движок сам обрабатывает входящие (ProfileUpdate/блобы) на своём потоке.
    // Раз в 2с печатаем контакты — видно, подтянулся ли профиль второй стороны.
    let mut ticks = 0;
    loop {
        std::thread::sleep(Duration::from_secs(2));
        ticks += 1;
        // Даём presence подняться (~6с), потом шлём текст/картинку один раз.
        if !sent_media && ticks >= 3 {
            if let (Some(text), Some(peer)) = (&send_text, media_peer) {
                let id = engine.send_text(&peer, text);
                println!("SENT_TEXT {id:?}");
                let _ = std::io::stdout().flush();
            }
            if let (Some(path), Some(peer)) = (&send_media, media_peer) {
                let bytes = std::fs::read(path).expect("send-media file");
                let (mime, name) = if path.ends_with(".png") {
                    ("image/png", "from-bob.png")
                } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                    ("image/jpeg", "from-bob.jpg")
                } else if path.ends_with(".txt") {
                    ("text/plain", "from-bob.txt")
                } else {
                    ("application/octet-stream", "from-bob.bin")
                };
                let id = engine.send_media(&peer, bytes, mime, name);
                println!("SENT_MEDIA {id:?}");
                let _ = std::io::stdout().flush();
            }
            sent_media = true;
        }
        // Входящие события движка (текст/медиа/запросы/блобы) — чтобы двухузловой
        // живой прогон на РЕАЛЬНОМ меше был наблюдаем без Android-клиента.
        while let Some(ev) = engine.poll_event() {
            println!("EVENT {ev}");
        }
        println!("CONTACTS {}", engine.contacts_json());
        let _ = std::io::stdout().flush();
    }
}
