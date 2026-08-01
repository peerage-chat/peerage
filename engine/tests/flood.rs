//! Детектор флуда (ADR-007, п.1): движок должен УМЕТЬ СКАЗАТЬ, кто нас заваливает.
//!
//! Остановить чужой поток на устройстве нельзя — байты уже оплачены радио
//! (threat-model N8). Но молчать об этом худший вариант: пользователь видит только
//! тающий трафик и греющийся телефон. Здесь проверяем, что превышение приёмного
//! бюджета попадает в снимок `flood_json`, а заблокированный источник в нём помечен.
//!
//! Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

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
    let d = std::env::temp_dir().join(format!("p2p-flood-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

fn flood(e: &Engine) -> serde_json::Value {
    serde_json::from_str(&e.flood_json()).unwrap()
}

/// Дождаться условия, опрашивая раз в 30 мс.
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

#[test]
fn flood_from_stranger_shows_up_in_snapshot() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(51), id(52));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a")); // флудер
    let b = start(&net, b_id, tmpdir("b")); // жертва

    // Незнакомцу положен скромный бюджет (Tier::Stranger) — заваливаем его с запасом.
    assert!(flood(&b).as_array().unwrap().is_empty(), "до флуда снимок пуст");
    for i in 0..600 {
        a.send_text(&b_key, &format!("флуд {i}"));
    }

    let seen = wait(20, || {
        flood(&b)
            .as_array()
            .map(|v| v.iter().any(|f| f["peer"] == hex::encode(a_key)))
            .unwrap_or(false)
    });
    assert!(seen, "флудер должен попасть в снимок: {}", b.flood_json());

    let entry = flood(&b)
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["peer"] == hex::encode(a_key))
        .cloned()
        .unwrap();
    assert!(entry["dropped"].as_u64().unwrap() > 0, "счётчик отброшенных растёт");
    assert_eq!(entry["blocked"], false, "пока не блокировали — флаг снят");

    // После блокировки источник помечен: его режет и транспорт (до FFI), и бюджет.
    b.block_contact(&a_key);
    assert!(
        wait(10, || {
            flood(&b)
                .as_array()
                .map(|v| v.iter().any(|f| f["peer"] == hex::encode(a_key) && f["blocked"] == true))
                .unwrap_or(false)
        }),
        "после блокировки флаг должен подняться: {}",
        b.flood_json(),
    );

    a.shutdown();
    b.shutdown();
}
