//! Обезличивание лога (шаг беты 7n). «Сохранить лог» человек пересылает сам: в файле не должно
//! быть имён контактов и групп, текста сообщений и ключей целиком. Тест ловит ВСЁ, что пишут
//! движок и мессенджер (уровень trace), пока двое знакомятся, переписываются и заводят группу.
//!
//! Клиент дополнительно укорачивает длинный hex и вырезает адреса (`LogExport.sanitize`), но
//! на это не полагаемся: содержимого в логе не должно быть с самого начала.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::Engine;
use peerage_messenger::loopback::LoopbackNet;

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, r: &log::Record) {
        LINES
            .lock()
            .unwrap()
            .push(format!("{} {}: {}", r.level(), r.target(), r.args()));
    }
    fn flush(&self) {}
}

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-logpriv-{tag}-{n}"));
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

#[test]
fn log_has_no_names_text_or_keys() {
    log::set_logger(&Capture).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    peerage_core::vault::init_with_key([7u8; 32]);

    const NAME_A: &str = "Ксенофонт-Имя";
    const NAME_B: &str = "Аглая-Имя";
    const TEXT: &str = "секретный-текст-сообщения-42";
    const REPLY: &str = "ответ-тоже-секрет-17";
    const GROUP: &str = "дача-секретная-группа";

    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(51), id(52));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.set_self_name("Я-Алиса-Секрет");
    a.add_contact(&b_key, NAME_B);
    b.add_contact(&a_key, NAME_A);

    a.send_text(&b_key, TEXT).expect("отправлено");
    assert!(
        wait(20, || b.history_json(&a_key).contains(TEXT)),
        "сообщение дошло — значит лог написан всем путём"
    );
    b.send_text(&a_key, REPLY).expect("ответ");
    assert!(wait(20, || a.history_json(&b_key).contains(REPLY)));
    assert!(a.group_create(GROUP));
    std::thread::sleep(Duration::from_millis(500));

    let diag = a.diagnostics_json();
    a.shutdown();
    b.shutdown();

    let lines = LINES.lock().unwrap().clone();
    assert!(!lines.is_empty(), "лог вообще пишется");
    let keys = [hex::encode(a_key), hex::encode(b_key)];
    let secrets = [NAME_A, NAME_B, TEXT, REPLY, GROUP, "Я-Алиса-Секрет"];
    for l in lines.iter().chain(std::iter::once(&diag)) {
        for s in secrets {
            assert!(!l.contains(s), "в лог попало «{s}»: {l}");
        }
        for k in &keys {
            assert!(!l.contains(k.as_str()), "в лог попал ключ целиком: {l}");
        }
    }
    // Снимок — одни числа: ни одной строки длиннее имени поля.
    let v: serde_json::Value = serde_json::from_str(&diag).unwrap();
    for part in ["net", "sessions", "outbox", "budget_drops"] {
        assert!(v[part].is_object(), "в снимке есть {part}");
    }
}
