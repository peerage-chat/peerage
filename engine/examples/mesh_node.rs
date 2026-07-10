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

    // Крутимся: движок сам обрабатывает входящие (ProfileUpdate/блобы) на своём потоке.
    // Раз в 2с печатаем контакты — видно, подтянулся ли профиль второй стороны.
    loop {
        std::thread::sleep(Duration::from_secs(2));
        println!("CONTACTS {}", engine.contacts_json());
        let _ = std::io::stdout().flush();
    }
}
