//! Передача блобов по мешу (веха M2): отдача и получатель-драйвен докачка.
//!
//! Отдающий кладёт блоб в [`BlobManager::offer`] и шлёт манифест; получатель на
//! манифесте создаёт [`Reassembler`], запрашивает недостающие чанки и собирает,
//! проверяя каждый по BLAKE3. Потери лечит `tick` (переспрашивает застрявшие).
//!
//! Транспорт-агностичная логика — тестируется через loopback без устройств
//! (`engine/tests/blob_flow.rs`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use p2p_blob::{Hash, Limits, Manifest, Reassembler};
use p2p_messenger::{Messenger, PeerKey};

/// Переспрашивать недостающие чанки не чаще этого (лечение потерь без флуда).
const REREQUEST_AFTER: Duration = Duration::from_millis(150);

/// Собранный блоб для верхнего слоя.
pub struct Completed {
    pub id: Hash,
    pub manifest: Manifest,
    pub content: Vec<u8>,
}

struct Incoming {
    peer: PeerKey,
    re: Reassembler,
    last_req: Instant,
}

pub struct BlobManager {
    messenger: Arc<Messenger>,
    limits: Limits,
    /// Блобы, которые мы отдаём: id → (manifest, чанки по порядку).
    outgoing: HashMap<Hash, (Manifest, Vec<Vec<u8>>)>,
    /// Блобы, которые мы качаем: id → состояние докачки.
    incoming: HashMap<Hash, Incoming>,
}

fn to_id(bytes: &[u8]) -> Option<Hash> {
    <[u8; 32]>::try_from(bytes).ok()
}

impl BlobManager {
    pub fn new(messenger: Arc<Messenger>, limits: Limits) -> Self {
        Self { messenger, limits, outgoing: HashMap::new(), incoming: HashMap::new() }
    }

    /// Предложить блоб пиру: нарезать, запомнить для отдачи, отправить манифест.
    /// Возвращает id блоба (контент-адрес).
    pub fn offer(&mut self, peer: PeerKey, content: &[u8], mime: &str) -> Hash {
        let (manifest, chunks) = p2p_blob::build(content, mime);
        let id = manifest.id;
        let _ = self.messenger.send_blob_manifest(&peer, &manifest.to_json());
        self.outgoing.insert(id, (manifest, chunks));
        id
    }

    /// Пришёл манифест — начать докачку. Возвращает `Some`, если блоб пустой (0
    /// чанков) и уже «готов». Невалидный/превышающий лимиты манифест игнорируется.
    pub fn on_manifest(&mut self, peer: PeerKey, manifest_json: &str) -> Option<Completed> {
        let manifest = Manifest::from_json(manifest_json).ok()?;
        let re = Reassembler::new(manifest, &self.limits).ok()?;
        let id = re.manifest().id;
        if re.is_complete() {
            // Пустой блоб — собираем сразу.
            let manifest = re.manifest().clone();
            return re.finish().ok().map(|content| Completed { id, manifest, content });
        }
        let missing = re.missing();
        self.incoming.insert(id, Incoming { peer, re, last_req: Instant::now() });
        for index in missing {
            let _ = self.messenger.send_blob_request(&peer, &id, index as u32);
        }
        None
    }

    /// Пир запросил чанк нашего блоба — отдать, если он у нас есть.
    pub fn on_request(&self, id_bytes: &[u8], index: u32, peer: PeerKey) {
        let Some(id) = to_id(id_bytes) else { return };
        if let Some((_m, chunks)) = self.outgoing.get(&id) {
            if let Some(chunk) = chunks.get(index as usize) {
                let _ = self.messenger.send_blob_chunk(&peer, &id, index, chunk);
            }
        }
    }

    /// Пришёл чанк. Проверяет его (по хэшу в манифесте) и добавляет; при полноте —
    /// собирает и возвращает [`Completed`]. Битый чанк отбрасывается.
    pub fn on_chunk(&mut self, id_bytes: &[u8], index: u32, data: &[u8]) -> Option<Completed> {
        let id = to_id(id_bytes)?;
        let inc = self.incoming.get_mut(&id)?;
        if inc.re.add(index as usize, data).is_err() {
            return None; // чужой/битый чанк — игнор
        }
        if inc.re.is_complete() {
            let inc = self.incoming.remove(&id)?;
            let manifest = inc.re.manifest().clone();
            return inc.re.finish().ok().map(|content| Completed { id, manifest, content });
        }
        None
    }

    /// Периодический тик: переспросить застрявшие (потерянные) чанки. Зовётся
    /// таймером/циклом. Дёшево, идемпотентно.
    pub fn tick(&mut self) {
        let now = Instant::now();
        for inc in self.incoming.values_mut() {
            if now.duration_since(inc.last_req) < REREQUEST_AFTER {
                continue;
            }
            inc.last_req = now;
            let id = inc.re.manifest().id;
            for index in inc.re.missing() {
                let _ = self.messenger.send_blob_request(&inc.peer, &id, index as u32);
            }
        }
    }

    /// Забыть отдаваемый блоб (когда доставку можно считать завершённой).
    pub fn drop_outgoing(&mut self, id: &Hash) {
        self.outgoing.remove(id);
    }
}
