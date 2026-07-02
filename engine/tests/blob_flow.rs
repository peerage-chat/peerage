//! Интеграционный тест передачи блоба (M2) через loopback-транспорт: отдача →
//! докачка чанков → сборка с проверкой BLAKE3, в т.ч. под потерями пакетов. Без
//! устройств. Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::sync::Arc;
use std::time::{Duration, Instant};

use p2p_blob::Limits;
use p2p_core::Identity;
use p2p_engine::blobs::{BlobManager, Completed};
use p2p_messenger::{loopback::LoopbackNet, Config, Event, Messenger};

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

/// Передать `content` от A к B через loopback; `drop_every` эмулирует потери.
/// Возвращает собранный на стороне B блоб.
fn transfer(content: &[u8], drop_every: u32) -> Completed {
    let net = LoopbackNet::new();
    if drop_every > 0 {
        net.set_drop_every(drop_every);
    }
    let (a_id, b_id) = (id(30), id(40));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let cfg = Config::default();
    let (a_msgr, a_rx) = Messenger::start_with_transport(a_id, net.endpoint(a_key), cfg.clone());
    let (b_msgr, b_rx) = Messenger::start_with_transport(b_id, net.endpoint(b_key), cfg);
    let mut a = BlobManager::new(Arc::new(a_msgr), Limits::default());
    let mut b = BlobManager::new(Arc::new(b_msgr), Limits::default());

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

/// Многочанковый блоб (несколько CHUNK_SIZE + хвост) доходит целым, id совпадает.
#[test]
fn blob_transfer_over_loopback() {
    let content: Vec<u8> = (0..p2p_blob::CHUNK_SIZE * 2 + 777).map(|i| (i * 7) as u8).collect();
    let done = transfer(&content, 0);
    assert_eq!(done.content, content, "содержимое восстановлено байт-в-байт");
    assert_eq!(done.id, p2p_blob::blob_id(&content), "id блоба совпал");
}

/// Тот же блоб под потерями: получатель-драйвен pull переспрашивает и дожимает.
#[test]
fn blob_transfer_survives_packet_loss() {
    let content: Vec<u8> = (0..p2p_blob::CHUNK_SIZE * 3 + 5).map(|i| (i ^ 0x5a) as u8).collect();
    let done = transfer(&content, 3);
    assert_eq!(done.content, content, "докачка дожала блоб сквозь потери");
}
