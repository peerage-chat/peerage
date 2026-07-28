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

use std::path::PathBuf;

use p2p_blob::disk::DiskAssembler;
use p2p_blob::{Hash, Limits, Manifest, ManifestAssembler, ManifestPart, Reassembler};
use p2p_messenger::{Messenger, PeerKey};

/// Переспрашивать недостающие чанки не чаще этого (лечение потерь без флуда).
const REREQUEST_AFTER: Duration = Duration::from_millis(150);

/// Собранный блоб для верхнего слоя.
pub struct Completed {
    pub id: Hash,
    pub manifest: Manifest,
    /// Где лежит содержимое. Крупный блоб собирается сразу на диск и в память НЕ
    /// поднимается — иначе файл на N МБ стоил бы N МБ RAM.
    pub data: CompletedData,
}

pub enum CompletedData {
    /// Мелкий блоб (картинка/аватар/голосовое) — собран в памяти, ещё не сохранён.
    InMemory(Vec<u8>),
    /// Крупный — уже записан по этому пути в чанкованном зашифрованном формате.
    OnDisk(PathBuf),
}

/// Откуда берём чанки при отдаче.
enum Source {
    /// Мелкое (аватар, голосовое, миниатюра) — держим в памяти, так проще.
    Memory(Vec<Vec<u8>>),
    /// Крупное — читаем с диска по индексу. Файл на гигабайт не должен стоить
    /// гигабайта RAM только потому, что мы его отдаём.
    File(PathBuf),
}

/// Куда складываем приходящие чанки.
enum Sink {
    Ram(Reassembler),
    Disk(DiskAssembler),
}

struct Incoming {
    peer: PeerKey,
    sink: Sink,
    last_req: Instant,
    /// Последний отданный наверх процент — чтобы не сыпать событием на каждый чанк.
    last_pct: u8,
}

pub struct BlobManager {
    messenger: Arc<Messenger>,
    limits: Limits,
    /// Куда складывать блобы, собираемые сразу на диск (крупные).
    blob_dir: PathBuf,
    /// Блобы, которые мы отдаём: id → манифест + откуда брать чанки.
    outgoing: HashMap<Hash, (Manifest, Source)>,
    /// Блобы, которые мы качаем: id → состояние докачки.
    incoming: HashMap<Hash, Incoming>,
    /// Манифесты крупных блобов, которые ещё собираются из частей: id → сборщик.
    partial_manifests: HashMap<Hash, ManifestAssembler>,
}

fn to_id(bytes: &[u8]) -> Option<Hash> {
    <[u8; 32]>::try_from(bytes).ok()
}

impl BlobManager {
    pub fn new(messenger: Arc<Messenger>, limits: Limits, blob_dir: PathBuf) -> Self {
        Self {
            messenger,
            limits,
            blob_dir,
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            partial_manifests: HashMap::new(),
        }
    }

    /// Пришла ЧАСТЬ манифеста. Возвращает `Some`, если блоб оказался пустым и уже
    /// собран; докачка стартует, как только собраны все части.
    #[allow(clippy::too_many_arguments)]
    pub fn on_manifest_part(
        &mut self,
        peer: PeerKey,
        id_bytes: &[u8],
        mime: &str,
        size: u64,
        part: u32,
        total: u32,
        hashes: &[u8],
    ) -> Option<Completed> {
        let id = to_id(id_bytes)?;
        if hashes.len() % 32 != 0 {
            return None; // битая часть — молча игнорируем
        }
        // Отсекаем великанов ДО накопления частей: иначе злоумышленник заставил бы
        // держать в памяти список хэшей блоба, который мы всё равно не примем.
        if size > self.limits.max_size {
            return None;
        }
        let p = ManifestPart {
            id,
            mime: mime.to_string(),
            size,
            part,
            total,
            hashes: hashes.chunks(32).filter_map(|c| <[u8; 32]>::try_from(c).ok()).collect(),
        };
        // Уже качаем этот блоб — части больше не нужны (мог прийти дубль).
        if self.incoming.contains_key(&id) {
            return None;
        }
        let done = match self.partial_manifests.get_mut(&id) {
            Some(asm) => asm.add(&p).is_ok() && asm.is_complete(),
            None => {
                let asm = ManifestAssembler::new(&p).ok()?;
                let complete = asm.is_complete();
                self.partial_manifests.insert(id, asm);
                complete
            }
        };
        if !done {
            return None;
        }
        let manifest = self.partial_manifests.remove(&id)?.finish()?;
        self.start_download(peer, manifest)
    }

    /// Общий старт докачки по готовому манифесту (пришёл целиком или собран из частей).
    fn start_download(&mut self, peer: PeerKey, manifest: Manifest) -> Option<Completed> {
        if manifest.validate(&self.limits).is_err() {
            return None;
        }
        let id = manifest.id;
        // Крупный блоб собираем СРАЗУ НА ДИСК: держать его целиком в RAM нельзя.
        let sink = if manifest.size >= self.limits.disk_threshold {
            let path = self.blob_dir.join(hex::encode(id));
            Sink::Disk(DiskAssembler::create(manifest, &path).ok()?)
        } else {
            let re = Reassembler::new(manifest, &self.limits).ok()?;
            if re.is_complete() {
                // Пустой блоб — готов сразу.
                let manifest = re.manifest().clone();
                return re
                    .finish()
                    .ok()
                    .map(|content| Completed { id, manifest, data: CompletedData::InMemory(content) });
            }
            Sink::Ram(re)
        };
        let missing = match &sink {
            Sink::Ram(r) => r.missing(),
            Sink::Disk(d) => d.missing(),
        };
        self.incoming.insert(id, Incoming { peer, sink, last_req: Instant::now(), last_pct: 0 });
        for index in missing {
            let _ = self.messenger.send_blob_request(&peer, &id, index as u32);
        }
        None
    }

    /// Предложить блоб пиру: нарезать, запомнить для отдачи, отправить манифест.
    /// Возвращает id блоба (контент-адрес).
    pub fn offer(&mut self, peer: PeerKey, content: &[u8], mime: &str) -> Hash {
        let (manifest, chunks) = p2p_blob::build(content, mime);
        let id = manifest.id;
        // Манифест едет ЧАСТЯМИ: у крупного блоба список хэшей не влезает в одну
        // датаграмму (это и держало прежний потолок 4 МБ). Мелкий блоб = одна часть.
        for p in manifest.split() {
            let hashes: Vec<u8> = p.hashes.iter().flat_map(|h| h.iter().copied()).collect();
            let _ = self.messenger.send_blob_manifest_part(
                &peer, &p.id, &p.mime, p.size, p.part, p.total, &hashes,
            );
        }
        self.outgoing.insert(id, (manifest, Source::Memory(chunks)));
        id
    }

    /// Предложить пиру блоб, лежащий ФАЙЛОМ: манифест уже посчитан потоково, чанки
    /// читаются с диска по запросу. Путь к файлу должен пережить отдачу.
    pub fn offer_file(&mut self, peer: PeerKey, manifest: Manifest, path: PathBuf) -> Hash {
        let id = manifest.id;
        for p in manifest.split() {
            let hashes: Vec<u8> = p.hashes.iter().flat_map(|h| h.iter().copied()).collect();
            let _ = self.messenger.send_blob_manifest_part(
                &peer, &p.id, &p.mime, p.size, p.part, p.total, &hashes,
            );
        }
        self.outgoing.insert(id, (manifest, Source::File(path)));
        id
    }

    /// Пришёл манифест — начать докачку. Возвращает `Some`, если блоб пустой (0
    /// чанков) и уже «готов». Невалидный/превышающий лимиты манифест игнорируется.
    pub fn on_manifest(&mut self, peer: PeerKey, manifest_json: &str) -> Option<Completed> {
        let manifest = Manifest::from_json(manifest_json).ok()?;
        self.start_download(peer, manifest)
    }

    /// Пир запросил чанк нашего блоба — отдать, если он у нас есть.
    pub fn on_request(&self, id_bytes: &[u8], index: u32, peer: PeerKey) {
        let Some(id) = to_id(id_bytes) else { return };
        let Some((_m, src)) = self.outgoing.get(&id) else { return };
        match src {
            Source::Memory(chunks) => {
                if let Some(chunk) = chunks.get(index as usize) {
                    let _ = self.messenger.send_blob_chunk(&peer, &id, index, chunk);
                }
            }
            Source::File(path) => {
                // Читаем ровно один чанк с диска — так отдача крупного файла не
                // зависит от его размера по памяти.
                if let Ok(chunk) = p2p_blob::read_file_chunk(path, index as usize) {
                    if !chunk.is_empty() {
                        let _ = self.messenger.send_blob_chunk(&peer, &id, index, &chunk);
                    }
                }
            }
        }
    }

    /// Пришёл чанк. Проверяет его (по хэшу в манифесте) и добавляет; при полноте —
    /// собирает и возвращает [`Completed`]. Битый чанк отбрасывается.
    pub fn on_chunk(&mut self, id_bytes: &[u8], index: u32, data: &[u8]) -> Option<Completed> {
        let id = to_id(id_bytes)?;
        let inc = self.incoming.get_mut(&id)?;
        let complete = match &mut inc.sink {
            Sink::Ram(re) => {
                if re.add(index as usize, data).is_err() {
                    return None; // чужой/битый чанк — игнор
                }
                re.is_complete()
            }
            Sink::Disk(d) => {
                // Шифруем чанк тем же at-rest ключом, что и остальные данные, и сразу
                // пишем на диск — в памяти он не задерживается.
                if d.add(index as usize, data, p2p_core::vault::seal).is_err() {
                    return None;
                }
                d.is_complete()
            }
        };
        if !complete {
            return None;
        }
        let inc = self.incoming.remove(&id)?;
        match inc.sink {
            Sink::Ram(re) => {
                let manifest = re.manifest().clone();
                re.finish().ok().map(|content| Completed { id, manifest, data: CompletedData::InMemory(content) })
            }
            Sink::Disk(d) => {
                let manifest = d.manifest().clone();
                let path = d.finish().ok()?;
                Some(Completed { id, manifest, data: CompletedData::OnDisk(path) })
            }
        }
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
            let (id, missing) = match &inc.sink {
                Sink::Ram(r) => (r.manifest().id, r.missing()),
                Sink::Disk(d) => (d.manifest().id, d.missing()),
            };
            for index in missing {
                let _ = self.messenger.send_blob_request(&inc.peer, &id, index as u32);
            }
        }
    }

    /// Прогресс докачек, изменившийся с прошлого опроса: (id, получено, всего).
    /// Отдаём только при смене процента — UI рисует кружок, ему хватает.
    pub fn take_progress(&mut self) -> Vec<(Hash, u32, u32)> {
        let mut out = Vec::new();
        for (id, inc) in self.incoming.iter_mut() {
            let (total, missing) = match &inc.sink {
                Sink::Ram(r) => (r.manifest().chunks.len(), r.missing().len()),
                Sink::Disk(d) => (d.manifest().chunks.len(), d.missing().len()),
            };
            if total == 0 {
                continue;
            }
            let done = total - missing;
            let pct = (done * 100 / total) as u8;
            if pct != inc.last_pct {
                inc.last_pct = pct;
                out.push((*id, done as u32, total as u32));
            }
        }
        out
    }

    /// Забыть отдаваемый блоб (когда доставку можно считать завершённой).
    pub fn drop_outgoing(&mut self, id: &Hash) {
        self.outgoing.remove(id);
    }
}
