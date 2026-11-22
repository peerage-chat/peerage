//! Заказанная пользователем докачка вложения: намерение живёт в движке, а не в UI.
//!
//! Проверяем две вещи, из-за которых на живом прогоне ловился вечный спиннер:
//!   1) заказ переживает перезапуск процесса (пир мог быть офлайн неделю);
//!   2) по факту получения блоба заказ снимается — карточка не «качается» вечно.
//! Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::Engine;
use peerage_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-wanted-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
}

fn wanted(engine: &Engine) -> Vec<String> {
    serde_json::from_str(&engine.wanted_json()).unwrap()
}

/// Крупное вложение (> AUTO_DOWNLOAD_LIMIT) не качается само; после явного «скачать»
/// оно приезжает целиком, а заказ снимается.
#[test]
fn explicit_download_completes_and_clears_intent() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(11), id(12));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    // 3 МБ — заведомо больше порога автозагрузки.
    let content: Vec<u8> = (0..3 * 1024 * 1024)
        .map(|i| (i * 13 ^ 0x3c) as u8)
        .collect();
    a.send_media(&b_key, content.clone(), "video/mp4", "clip.mp4")
        .expect("send_media");

    // Ждём карточку у B: сообщение пришло, а байты — нет (решает получатель).
    let deadline = Instant::now() + Duration::from_secs(10);
    let blob_id = loop {
        let hist: serde_json::Value = serde_json::from_str(&b.history_json(&a_key)).unwrap();
        if let Some(m) = hist
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["kind"] == "file" || m["kind"] == "video")
        {
            break m["blob_id"].as_str().unwrap().to_string();
        }
        assert!(Instant::now() < deadline, "B не получил карточку вложения");
        std::thread::sleep(Duration::from_millis(30));
    };
    assert!(
        b.blob_read(&blob_id).is_none(),
        "крупное вложение не должно тянуться само"
    );

    assert!(b.download_media(&a_key, &blob_id), "заказ принят");
    assert_eq!(wanted(&b), vec![blob_id.clone()], "намерение записано");

    // Докачка идёт вне анти-флуд бюджета: пользователь сам её попросил, значит
    // трафик ожидаемый и резать его своим же лимитом незачем.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(bytes) = b.blob_read(&blob_id) {
            assert_eq!(bytes, content, "содержимое доехало байт-в-байт");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "заказанное вложение не доехало за 30с"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    // Ждём снятия заказа, а не проверяем мгновенный снимок: блоб становится читаемым
    // на шаг раньше, чем движок вычёркивает намерение, и голая проверка здесь гоночна.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !wanted(&b).is_empty() {
        assert!(Instant::now() < deadline, "заказ не снят после получения");
        std::thread::sleep(Duration::from_millis(20));
    }

    a.shutdown();
    b.shutdown();
}

/// Пир офлайн — заказ не теряется ни при его отсутствии, ни при перезапуске
/// приложения: следить за появлением собеседника пользователь не обязан.
#[test]
fn intent_survives_restart() {
    let net = LoopbackNet::new();
    let b_id = id(13);
    let dir = tmpdir("restart");
    let absent = [0x5au8; 32]; // пира в сети нет
    let fake_blob = hex::encode([0x77u8; 32]);

    let b = start(&net, b_id.clone(), dir.clone());
    b.download_media(&absent, &fake_blob);
    assert_eq!(wanted(&b), vec![fake_blob.clone()]);
    b.shutdown();
    drop(b);

    let b2 = start(&net, b_id, dir);
    assert_eq!(wanted(&b2), vec![fake_blob], "заказ пережил перезапуск");
    b2.shutdown();
}

/// «Отмена» в уведомлении о передаче (notifications.md → «Передача файлов»): заказ
/// снимается и переживает перезапуск снятым, идущая докачка бросается — блоб не
/// доезжает, недособранного файла не остаётся.
#[test]
fn cancel_drops_intent_and_partial_file() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(14), id(15));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let b_dir = tmpdir("cancel-b");
    let a = start(&net, a_id, tmpdir("cancel-a"));
    let b = start(&net, b_id.clone(), b_dir.clone());
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    // Пир не в сети: заказ лежит, отмена его снимает.
    let fake_blob = hex::encode([0x66u8; 32]);
    b.download_media(&[0x5au8; 32], &fake_blob);
    assert!(b.cancel_download(&fake_blob));
    assert!(wanted(&b).is_empty(), "заказ снят");

    // Идущая докачка: отмена на первом проценте — блоб не доезжает.
    let content: Vec<u8> = (0..24 * 1024 * 1024)
        .map(|i| (i * 7 ^ 0x5a) as u8)
        .collect();
    a.send_media(&b_key, content, "video/mp4", "big.mp4")
        .expect("send_media");
    let deadline = Instant::now() + Duration::from_secs(10);
    let blob_id = loop {
        let hist: serde_json::Value = serde_json::from_str(&b.history_json(&a_key)).unwrap();
        if let Some(m) = hist
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["kind"] == "file" || m["kind"] == "video")
        {
            break m["blob_id"].as_str().unwrap().to_string();
        }
        assert!(Instant::now() < deadline, "B не получил карточку вложения");
        std::thread::sleep(Duration::from_millis(30));
    };
    while b.poll_event().is_some() {}
    assert!(b.download_media(&a_key, &blob_id));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let progress =
            std::iter::from_fn(|| b.poll_event()).any(|e| e.contains("\"blob_progress\""));
        if progress {
            break;
        }
        assert!(Instant::now() < deadline, "докачка не пошла");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(b.cancel_download(&blob_id));
    assert!(wanted(&b).is_empty(), "заказ снят");
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        b.blob_read(&blob_id).is_none(),
        "брошенная докачка не доехала"
    );
    let parts = walk(&b_dir)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "part"))
        .count();
    assert_eq!(parts, 0, "недособранный файл удалён");

    a.shutdown();
    b.shutdown();
    drop(b);
    let b2 = start(&net, b_id, b_dir);
    assert!(
        wanted(&b2).is_empty(),
        "снятый заказ не вернулся после перезапуска"
    );
    b2.shutdown();
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
