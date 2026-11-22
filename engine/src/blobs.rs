//! Передача блобов по мешу (веха M2): отдача и получатель-драйвен докачка.
//!
//! Получатель сам просит манифест (`request_blob_manifest`) — блобов без спроса не
//! бывает: какие id ждём и от кого, помнит движок. Отдающий отвечает манифестом и
//! запоминает, КОМУ его отдал: чанки получает только он. Получатель по манифесту
//! создаёт сборщик и тянет чанки окном, проверяя каждый по BLAKE3; потерянные
//! переспрашивает по таймауту конкретного чанка.
//!
//! Транспорт-агностичная логика — тестируется через loopback без устройств
//! (`engine/tests/blob_flow.rs`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use std::path::{Path, PathBuf};

use peerage_blob::disk::DiskAssembler;
use peerage_blob::{Hash, Limits, Manifest, ManifestAssembler, ManifestPart, Reassembler};
use peerage_core::vault;
use peerage_messenger::{Messenger, PeerKey};

/// Сколько чанков одной докачки держим запрошенными. Больше — отдающий режет
/// всплеск и половина запросов уходит впустую; меньше — простаивает канал.
const WINDOW: usize = 64;

/// Чанк не пришёл за это время — переспрашиваем ИМЕННО его.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(2);

/// Докачка без единого чанка за это время брошена: пир ушёл. Намерение «скачать»
/// живёт у движка (`wanted`) и возобновится, когда пир вернётся.
const STALL_AFTER: Duration = Duration::from_secs(120);

/// Отдаваемый блоб, который никто не трогал столько, забываем: иначе каждое
/// отданное вложение висело бы в памяти до перезапуска.
const OUTGOING_IDLE: Duration = Duration::from_secs(600);

/// Насколько поднимаем ingress-бюджет собеседника по блобу: этот поток мы ждём
/// сами — как отдающий (его запросы чанков), так и получающий (его чанки).
const BLOB_BOOST: Duration = Duration::from_secs(120);

/// Запас свободного места сверх самого блоба: не забивать диск телефона под ноль.
const DISK_RESERVE: u64 = 64 * 1024 * 1024;

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

/// Почему докачка не началась — чтобы сказать человеку, а не молчать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Блоб больше предела приёма.
    TooLarge,
    /// На диске не хватает места под блоб.
    NoSpace,
}

impl Failure {
    pub fn as_str(self) -> &'static str {
        match self {
            Failure::TooLarge => "too_large",
            Failure::NoSpace => "no_space",
        }
    }
}

/// Откуда берём чанки при отдаче.
enum Source {
    /// Мелкое (аватар, голосовое, миниатюра) — держим в памяти, так проще.
    Memory(Vec<Vec<u8>>),
    /// Блоб из нашего хранилища в чанкованном формате: читаем и расшифровываем
    /// ровно запрошенный чанк. Файл на гигабайт не стоит гигабайта RAM.
    Stored(PathBuf),
}

struct Outgoing {
    manifest: Manifest,
    source: Source,
    /// Кому отдан манифест — только они и получают чанки.
    peers: HashSet<PeerKey>,
    last_used: Instant,
}

/// Куда складываем приходящие чанки.
enum Sink {
    Ram(Reassembler),
    Disk(DiskAssembler),
}

impl Sink {
    fn manifest(&self) -> &Manifest {
        match self {
            Sink::Ram(r) => r.manifest(),
            Sink::Disk(d) => d.manifest(),
        }
    }

    fn has(&self, index: usize) -> bool {
        match self {
            Sink::Ram(r) => r.has(index),
            Sink::Disk(d) => d.has(index),
        }
    }

    fn progress(&self) -> (usize, usize) {
        match self {
            Sink::Ram(r) => r.progress(),
            Sink::Disk(d) => d.progress(),
        }
    }
}

struct Incoming {
    peer: PeerKey,
    sink: Sink,
    /// Запрошенные и ещё не пришедшие чанки: индекс → когда спросили.
    inflight: HashMap<usize, Instant>,
    /// Следующий ни разу не запрошенный индекс.
    next: usize,
    last_chunk: Instant,
    /// Последний отданный наверх процент — чтобы не сыпать событием на каждый чанк.
    last_pct: u8,
}

pub struct BlobManager {
    messenger: Arc<Messenger>,
    limits: Limits,
    /// Куда складывать блобы, собираемые сразу на диск (крупные).
    blob_dir: PathBuf,
    /// Блобы, которые мы отдаём.
    outgoing: HashMap<Hash, Outgoing>,
    /// Блобы, которые мы качаем: id → состояние докачки.
    incoming: HashMap<Hash, Incoming>,
    /// Манифесты крупных блобов, которые ещё собираются из частей: id → сборщик.
    partial_manifests: HashMap<Hash, ManifestAssembler>,
    /// Докачки, которые не начались, — для события в UI.
    failures: Vec<(Hash, Failure)>,
}

fn to_id(bytes: &[u8]) -> Option<Hash> {
    <[u8; 32]>::try_from(bytes).ok()
}

/// Свободно на разделе с `dir`, байт. `None` — узнать не удалось (не мешаем).
#[cfg(unix)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c — валидная C-строка, st — выделенная структура нужного типа.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(not(unix))]
fn free_space(_dir: &Path) -> Option<u64> {
    None
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
            failures: Vec::new(),
        }
    }

    /// Пришла ЧАСТЬ манифеста. Звать только для блобов, которые мы ждём от этого
    /// пира (решает движок). Возвращает `Some`, если блоб оказался пустым и уже
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
            self.failures.push((id, Failure::TooLarge));
            return None;
        }
        let p = ManifestPart {
            id,
            mime: mime.to_string(),
            size,
            part,
            total,
            hashes: hashes
                .chunks(32)
                .filter_map(|c| <[u8; 32]>::try_from(c).ok())
                .collect(),
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
            let need = manifest.size + manifest.chunks.len() as u64 * 45 + DISK_RESERVE;
            if free_space(&self.blob_dir).is_some_and(|free| free < need) {
                self.failures.push((id, Failure::NoSpace));
                return None;
            }
            let path = self.blob_dir.join(hex::encode(id));
            Sink::Disk(DiskAssembler::create(manifest, &path).ok()?)
        } else {
            let re = Reassembler::new(manifest, &self.limits).ok()?;
            if re.is_complete() {
                // Пустой блоб — готов сразу.
                let manifest = re.manifest().clone();
                return re.finish().ok().map(|content| Completed {
                    id,
                    manifest,
                    data: CompletedData::InMemory(content),
                });
            }
            Sink::Ram(re)
        };
        let now = Instant::now();
        let mut inc = Incoming {
            peer,
            sink,
            inflight: HashMap::new(),
            next: 0,
            last_chunk: now,
            last_pct: 0,
        };
        self.fill_window(&id, &mut inc);
        self.incoming.insert(id, inc);
        None
    }

    /// Дозапросить чанки, пока в полёте меньше [`WINDOW`].
    fn fill_window(&self, id: &Hash, inc: &mut Incoming) {
        let n = inc.sink.manifest().chunks.len();
        let now = Instant::now();
        while inc.inflight.len() < WINDOW && inc.next < n {
            let index = inc.next;
            inc.next += 1;
            if inc.sink.has(index) {
                continue;
            }
            let _ = self
                .messenger
                .send_blob_request(&inc.peer, id, index as u32);
            inc.inflight.insert(index, now);
        }
    }

    /// Разослать манифест пиру частями (у крупного блоба список хэшей не влезает в
    /// одну датаграмму).
    fn send_manifest(&self, peer: &PeerKey, manifest: &Manifest) {
        for p in manifest.split() {
            let hashes: Vec<u8> = p.hashes.iter().flat_map(|h| h.iter().copied()).collect();
            let _ = self
                .messenger
                .send_blob_manifest_part(peer, &p.id, &p.mime, p.size, p.part, p.total, &hashes);
        }
    }

    fn remember(&mut self, peer: PeerKey, manifest: Manifest, source: Source) {
        let id = manifest.id;
        let entry = self.outgoing.entry(id).or_insert_with(|| Outgoing {
            manifest,
            source,
            peers: HashSet::new(),
            last_used: Instant::now(),
        });
        entry.peers.insert(peer);
        entry.last_used = Instant::now();
        // Следом придут его запросы чанков — окном сразу; это заказанный нами же
        // поток, резать его анти-флуд бюджетом незачем.
        self.messenger.boost_peer(&peer, BLOB_BOOST);
    }

    /// Отдать пиру мелкий блоб из памяти: нарезать, запомнить, отправить манифест.
    pub fn offer(&mut self, peer: PeerKey, content: &[u8], mime: &str) -> Hash {
        let (manifest, chunks) = peerage_blob::build(content, mime);
        let id = manifest.id;
        self.send_manifest(&peer, &manifest);
        self.remember(peer, manifest, Source::Memory(chunks));
        id
    }

    /// Отдать пиру блоб из хранилища в чанкованном формате. Манифест считается
    /// потоково (один раз — дальше берётся из `outgoing`).
    pub fn offer_stored(&mut self, peer: PeerKey, manifest: Manifest, path: PathBuf) {
        self.send_manifest(&peer, &manifest);
        self.remember(peer, manifest, Source::Stored(path));
    }

    /// Блоб уже отдаётся — переслать манифест ещё одному (или тому же) пиру, не
    /// трогая диск. `false` — такого в отдаче нет.
    pub fn reoffer(&mut self, peer: PeerKey, id: &Hash) -> bool {
        let Some(out) = self.outgoing.get_mut(id) else {
            return false;
        };
        out.peers.insert(peer);
        out.last_used = Instant::now();
        let manifest = out.manifest.clone();
        self.messenger.boost_peer(&peer, BLOB_BOOST);
        self.send_manifest(&peer, &manifest);
        true
    }

    /// Пришёл манифест — начать докачку. Звать только для ожидаемых блобов.
    pub fn on_manifest(&mut self, peer: PeerKey, manifest: Manifest) -> Option<Completed> {
        self.start_download(peer, manifest)
    }

    /// Пир запросил чанк нашего блоба — отдать, если манифест отдавали именно ему.
    pub fn on_request(&mut self, id_bytes: &[u8], index: u32, peer: PeerKey) {
        let Some(id) = to_id(id_bytes) else { return };
        let Some(out) = self.outgoing.get_mut(&id) else {
            return;
        };
        if !out.peers.contains(&peer) {
            return;
        }
        out.last_used = Instant::now();
        let chunk = match &out.source {
            Source::Memory(chunks) => chunks.get(index as usize).cloned(),
            Source::Stored(path) => peerage_blob::disk::read_chunk(path, index as usize, |i, b| {
                vault::open(&vault::chunk_purpose(&id, i), b).ok()
            }),
        };
        if let Some(chunk) = chunk {
            let _ = self.messenger.send_blob_chunk(&peer, &id, index, &chunk);
            // Его запросы чанков — ожидаемый поток, не флуд.
            self.messenger.boost_peer(&peer, BLOB_BOOST);
        }
    }

    /// Пришёл чанк. Проверяет его (по хэшу в манифесте) и добавляет; при полноте —
    /// собирает и возвращает [`Completed`]. Битый чанк и чанк не от того пира
    /// отбрасываются.
    pub fn on_chunk(
        &mut self,
        peer: PeerKey,
        id_bytes: &[u8],
        index: u32,
        data: &[u8],
    ) -> Option<Completed> {
        let id = to_id(id_bytes)?;
        let mut inc = self.incoming.remove(&id)?;
        if inc.peer != peer {
            self.incoming.insert(id, inc);
            return None;
        }
        let index = index as usize;
        let accepted = match &mut inc.sink {
            Sink::Ram(re) => re.add(index, data).is_ok(),
            // Шифруем чанк тем же at-rest ключом, что и остальные данные, и сразу
            // пишем на диск — в памяти он не задерживается.
            Sink::Disk(d) => d
                .add(index, data, |i, b| {
                    vault::seal(&vault::chunk_purpose(&id, i), b)
                })
                .is_ok(),
        };
        if !accepted {
            self.incoming.insert(id, inc);
            return None;
        }
        inc.inflight.remove(&index);
        inc.last_chunk = Instant::now();
        self.messenger.boost_peer(&peer, BLOB_BOOST);
        let complete = match &inc.sink {
            Sink::Ram(re) => re.is_complete(),
            Sink::Disk(d) => d.is_complete(),
        };
        if !complete {
            self.fill_window(&id, &mut inc);
            self.incoming.insert(id, inc);
            return None;
        }
        match inc.sink {
            Sink::Ram(re) => {
                let manifest = re.manifest().clone();
                re.finish().ok().map(|content| Completed {
                    id,
                    manifest,
                    data: CompletedData::InMemory(content),
                })
            }
            Sink::Disk(d) => {
                let manifest = d.manifest().clone();
                let path = d.finish().ok()?;
                Some(Completed {
                    id,
                    manifest,
                    data: CompletedData::OnDisk(path),
                })
            }
        }
    }

    /// Периодический тик: переспросить чанки, которые не пришли за
    /// [`CHUNK_TIMEOUT`], бросить заглохшие докачки, забыть давно не нужные отдачи.
    pub fn tick(&mut self) {
        let now = Instant::now();
        self.incoming
            .retain(|_, inc| now.duration_since(inc.last_chunk) < STALL_AFTER);
        for (id, inc) in self.incoming.iter_mut() {
            for (index, asked) in inc.inflight.iter_mut() {
                if now.duration_since(*asked) >= CHUNK_TIMEOUT {
                    *asked = now;
                    let _ = self
                        .messenger
                        .send_blob_request(&inc.peer, id, *index as u32);
                }
            }
        }
        self.outgoing
            .retain(|_, out| now.duration_since(out.last_used) < OUTGOING_IDLE);
    }

    /// Прогресс докачек, изменившийся с прошлого опроса: (id, получено, всего).
    /// Отдаём только при смене процента — UI рисует кружок, ему хватает.
    pub fn take_progress(&mut self) -> Vec<(Hash, u32, u32)> {
        let mut out = Vec::new();
        for (id, inc) in self.incoming.iter_mut() {
            let (done, total) = inc.sink.progress();
            if total == 0 {
                continue;
            }
            let pct = (done * 100 / total) as u8;
            if pct != inc.last_pct {
                inc.last_pct = pct;
                out.push((*id, done as u32, total as u32));
            }
        }
        out
    }

    /// Докачки, которые не начались с прошлого опроса.
    pub fn take_failures(&mut self) -> Vec<(Hash, Failure)> {
        std::mem::take(&mut self.failures)
    }

    /// Сколько блобов сейчас в отдаче (диагностика и тесты).
    /// Бросить одну докачку («Отмена» в уведомлении о передаче): продолжать её некому,
    /// поэтому недособранный файл удаляется сразу.
    pub fn cancel_download(&mut self, id: &Hash) {
        self.partial_manifests.remove(id);
        if let Some(inc) = self.incoming.remove(id) {
            let disk = matches!(inc.sink, Sink::Disk(_));
            drop(inc); // файл закрыт до удаления
            if disk {
                let part = self.blob_dir.join(hex::encode(id)).with_extension("part");
                let _ = std::fs::remove_file(part);
            }
        }
    }

    /// Бросить все идущие докачки (очистка вложений): их недособранные файлы
    /// сейчас удалят, и дописывать в них нечего.
    pub fn cancel_downloads(&mut self) {
        self.incoming.clear();
        self.partial_manifests.clear();
    }

    pub fn outgoing_len(&self) -> usize {
        self.outgoing.len()
    }
}
