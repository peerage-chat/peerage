//! Интеграционный тест передачи блоба (M2) через loopback-транспорт: отдача →
//! докачка чанков → сборка с проверкой BLAKE3, в т.ч. под потерями пакетов. Без
//! устройств. Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::sync::Arc;
use std::time::{Duration, Instant};

use p2p_blob::Limits;
use p2p_core::Identity;
use p2p_engine::blobs::{BlobManager, Completed, CompletedData};
use p2p_messenger::{loopback::LoopbackNet, Config, Event, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

/// Передать `content` от A к B через loopback; `drop_every` эмулирует потери.
/// Возвращает собранный на стороне B блоб.
fn transfer(content: &[u8], drop_every: u32) -> Completed {
    transfer_with(content, drop_every, Limits::default())
}

fn transfer_with(content: &[u8], drop_every: u32, limits: Limits) -> Completed {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(30), id(40));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config::default();
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);
    // Потери включаются, когда манифест уже дошёл: тест — про докачку ЧАНКОВ
    // сквозь потери. Манифест здесь уходит один раз и без страховки (в движке его
    // страхуют надёжное `Media` и запрос манифеста по хэшу), и его потеря
    // проверяла бы не докачку, а везение.
    let lossy = |net: &LoopbackNet| {
        if drop_every > 0 {
            net.set_drop_every(drop_every);
        }
    };
    // Каталоги для блобов, собираемых сразу на диск (крупные); мелкие идут через RAM.
    let dir = std::env::temp_dir().join(format!(
        "p2p-blobflow-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let mut a = BlobManager::new(Arc::new(a_msgr), limits, dir.join("a"));
    let mut b = BlobManager::new(Arc::new(b_msgr), limits, dir.join("b"));

    // A предлагает блоб B (шлёт манифест).
    a.offer(b_key, content, "application/octet-stream");

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        // Сторона A: обслуживает запросы чанков.
        while let Ok(ev) = a_rx.try_recv() {
            if let Event::BlobRequest { id, index, peer } = ev {
                a.on_request(&id, index, peer);
            }
        }
        // Сторона B: манифест → докачка; чанки → сборка.
        while let Ok(ev) = b_rx.try_recv() {
            match ev {
                Event::BlobManifest { peer, manifest } => {
                    lossy(&net);
                    if let Some(done) = b.on_manifest(peer, p2p_blob::Manifest::from_json(&manifest).unwrap()) {
                        return done;
                    }
                }
                // Манифест теперь едет частями (снят потолок 4 МБ).
                Event::BlobManifestPart { peer, id, mime, size, part, total, hashes } => {
                    lossy(&net);
                    if let Some(done) =
                        b.on_manifest_part(peer, &id, &mime, size, part, total, &hashes)
                    {
                        return done;
                    }
                }
                Event::BlobChunk { peer, id, index, data } => {
                    if let Some(done) = b.on_chunk(peer, &id, index, &data) {
                        return done;
                    }
                }
                _ => {}
            }
        }
        b.tick(); // переспросить потерянные чанки
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("блоб не собрался за таймаут (drop_every={drop_every})");
}

/// Содержимое собранного в ПАМЯТИ блоба (мелкие блобы; крупные лежат на диске).
fn in_memory(done: &Completed) -> Vec<u8> {
    match &done.data {
        CompletedData::InMemory(v) => v.clone(),
        CompletedData::OnDisk(p) => panic!("ожидали сборку в памяти, а блоб ушёл на диск: {p:?}"),
    }
}

/// Многочанковый блоб (несколько CHUNK_SIZE + хвост) доходит целым, id совпадает.
#[test]
fn blob_transfer_over_loopback() {
    let content: Vec<u8> = (0..p2p_blob::CHUNK_SIZE * 2 + 777).map(|i| (i * 7) as u8).collect();
    let done = transfer(&content, 0);
    assert_eq!(in_memory(&done), content, "содержимое восстановлено байт-в-байт");
    assert_eq!(done.id, p2p_blob::blob_id(&content), "id блоба совпал");
}

/// Тот же блоб под потерями: получатель-драйвен pull переспрашивает и дожимает.
#[test]
fn blob_transfer_survives_packet_loss() {
    let content: Vec<u8> = (0..p2p_blob::CHUNK_SIZE * 3 + 5).map(|i| (i ^ 0x5a) as u8).collect();
    let done = transfer(&content, 3);
    assert_eq!(in_memory(&done), content, "докачка дожала блоб сквозь потери");
}

/// Крупный блоб собирается СРАЗУ НА ДИСК и в память целиком не поднимается —
/// иначе файл на N МБ стоил бы N МБ RAM (это и держало прежний потолок размера).
/// Манифест при этом едет многими частями (>8 МБ ⇒ сотни хэшей).
#[test]
fn large_blob_streams_to_disk() {
    // Порог занижаем, чтобы тест проверял ЛОГИКУ дисковой сборки, а не пропускную
    // способность: чанки идут по CHAN_CHAT и упираются в ingress-бюджет (~60 пак/с),
    // поэтому реальные 9 МБ качались бы больше минуты.
    // Без ключа vault пишет «открытым текстом» — инициализируем, чтобы проверялся
    // настоящий путь (чанки на диске зашифрованы).
    p2p_core::vault::init_with_key([7u8; 32]);
    let limits = Limits::default().with_disk_threshold(64 * 1024);
    let content: Vec<u8> = (0..200 * 1024).map(|i| (i * 31 ^ 0xA5) as u8).collect();
    let done = transfer_with(&content, 0, limits);
    assert_eq!(done.id, p2p_blob::blob_id(&content), "id блоба совпал");

    let path = match &done.data {
        CompletedData::OnDisk(p) => p.clone(),
        CompletedData::InMemory(_) => panic!("крупный блоб не должен собираться в памяти"),
    };
    assert!(path.exists(), "файл блоба на месте");
    // Файл лежит зашифрованным по чанкам: сырое содержимое в нём не найти.
    let raw = std::fs::read(&path).unwrap();
    assert!(
        raw.windows(64).all(|w| w != &content[..64]),
        "содержимое не должно лежать открытым текстом"
    );
    // И читается обратно байт-в-байт (потоково, по чанкам).
    let open = |i: usize, b: &[u8]| p2p_core::vault::open(&p2p_core::vault::chunk_purpose(&done.id, i), b).ok();
    let restored = p2p_blob::disk::read_all(&path, open).unwrap();
    assert_eq!(restored, content, "потоково собранный блоб восстановлен точно");
}

/// Пара менеджеров в общей сети без обслуживания: тест сам решает, когда отвечать.
fn pair(limits: Limits) -> (BlobManager, std::sync::mpsc::Receiver<Event>, BlobManager, std::sync::mpsc::Receiver<Event>, [u8; 32]) {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(50), id(60));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), Config::default());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), Config::default());
    let dir = std::env::temp_dir().join(format!(
        "p2p-blobpair-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::mem::forget(net);
    let a = BlobManager::new(Arc::new(a_msgr), limits, dir.join("a"));
    let b = BlobManager::new(Arc::new(b_msgr), limits, dir.join("b"));
    (a, a_rx, b, b_rx, b_key)
}

/// Передать B все части манифеста, пришедшие ему за `secs`.
fn feed_manifest(b: &mut BlobManager, b_rx: &std::sync::mpsc::Receiver<Event>, secs: u64) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        while let Ok(ev) = b_rx.try_recv() {
            if let Event::BlobManifestPart { peer, id, mime, size, part, total, hashes } = ev {
                b.on_manifest_part(peer, &id, &mime, size, part, total, &hashes);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// R-021: докачка запрашивает окно чанков, а не все разом.
#[test]
fn download_asks_one_window_at_a_time() {
    let (mut a, a_rx, mut b, b_rx, b_key) = pair(Limits::default());
    let content: Vec<u8> = (0..p2p_blob::CHUNK_SIZE * 150).map(|i| (i * 3) as u8).collect();
    a.offer(b_key, &content, "application/octet-stream");
    feed_manifest(&mut b, &b_rx, 1);
    // A молчит — считаем, сколько B попросил.
    std::thread::sleep(Duration::from_millis(300));
    let mut asked = std::collections::HashSet::new();
    while let Ok(ev) = a_rx.try_recv() {
        if let Event::BlobRequest { index, .. } = ev {
            asked.insert(index);
        }
    }
    assert_eq!(asked.len(), 64, "в полёте ровно окно, а не все 150 чанков");
}

/// R-025: блоб больше предела не качается молча — менеджер сообщает почему.
#[test]
fn oversized_blob_reports_failure() {
    let (mut a, _a_rx, mut b, b_rx, b_key) = pair(Limits::with_max_size(100_000));
    let content = vec![1u8; 200_000];
    let id = a.offer(b_key, &content, "video/mp4");
    feed_manifest(&mut b, &b_rx, 1);
    let failures = b.take_failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, id);
    assert_eq!(failures[0].1, p2p_engine::blobs::Failure::TooLarge);
}
