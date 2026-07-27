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
    if drop_every > 0 {
        net.set_drop_every(drop_every);
    }
    let (a_id, b_id) = (id(30), id(40));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config::default();
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);
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
                    if let Some(done) = b.on_manifest(peer, &manifest) {
                        return done;
                    }
                }
                // Манифест теперь едет частями (снят потолок 4 МБ).
                Event::BlobManifestPart { peer, id, mime, size, part, total, hashes } => {
                    if let Some(done) =
                        b.on_manifest_part(peer, &id, &mime, size, part, total, &hashes)
                    {
                        return done;
                    }
                }
                Event::BlobChunk { id, index, data, .. } => {
                    if let Some(done) = b.on_chunk(&id, index, &data) {
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
    let restored = p2p_blob::disk::read_all(&path, |b| p2p_core::vault::open(b).ok()).unwrap();
    assert_eq!(restored, content, "потоково собранный блоб восстановлен точно");
}
