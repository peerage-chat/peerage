//! p2p-blob — унифицированный контент-адресный примитив (веха M2).
//!
//! Один примитив под аватарки, картинки, файлы, голосовые, стикеры: контент
//! режется на чанки, адресуется по BLAKE3-хэшу, докачивается кусками и проверяется
//! на каждом шаге. Сообщение ссылается на блоб по [`Manifest`] (мелкий, идёт внутри
//! E2E-сообщения, поэтому список хэшей чанков доверенный).
//!
//! Свойства:
//!   * **контент-адресация** — id = BLAKE3(содержимое) → дедуп «бесплатно»;
//!   * **проверяемость** — каждый чанк сверяется с хэшем в манифесте, собранный
//!     блоб — с id; подделка/битый чанк отклоняются;
//!   * **анти-DoS** — [`Limits`] режут гигантские/бесконечные блобы ДО докачки.
//!
//! Здесь только логика адресации/сборки (чистый крейт, тестируется на любом хосте).
//! Сетевая докачка (pull по мешу) и метадата-scrub (EXIF) — отдельные слои поверх.

use serde::{Deserialize, Serialize};

pub mod disk;
pub mod scrub;

/// BLAKE3-хэш (256 бит) — и id блоба, и хэш чанка.
pub type Hash = [u8; 32];

/// Размер чанка (12 KiB). Ограничен сверху датаграммой транспорта (~65535 Б на
/// приёме). Чанк едет E2E-зашифрованным JSON-сообщением, где байты hex-кодируются
/// ДВАЖДЫ (data в payload + весь sealed-конверт), т.е. ×4: 12 КБ → датаграмма ≈
/// 49 КБ, влезает с запасом. Крупнее не влезало и молча обрезалось на приёме.
pub const CHUNK_SIZE: usize = 12 * 1024;

/// BLAKE3 куска данных (для проверки чанка против манифеста).
pub fn hash_of(data: &[u8]) -> Hash {
    hash(data)
}

fn hash(data: &[u8]) -> Hash {
    *blake3::hash(data).as_bytes()
}

/// Хэш-адрес содержимого (id блоба).
pub fn blob_id(content: &[u8]) -> Hash {
    hash(content)
}

/// Лимиты приёма (анти-DoS): защищают от гигантских/бесконечных блобов. Проверяются
/// по манифесту ДО начала докачки, поэтому злоумышленник не заставит качать лишнее.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Максимум байт всего блоба.
    pub max_size: u64,
    /// Максимум чанков (косвенно ограничивает и память под манифест).
    pub max_chunks: usize,
    /// С этого размера блоб собирается сразу НА ДИСК, а не в память. Мелкие
    /// (аватар/картинка) дешевле собрать в RAM — они там же и показываются.
    pub disk_threshold: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // 64 MiB. Прежний потолок 4 МБ держался тем, что манифест ехал ОДНИМ пакетом;
        // теперь он режется на части ([`Manifest::split`]), и это ограничение снято.
        //
        // Оставшийся ограничитель — ПАМЯТЬ: [`Reassembler`] собирает блоб целиком в
        // RAM, поэтому предел выбран из того, сколько не жалко держать в памяти
        // телефона. Для гигабайтных файлов нужна потоковая сборка на диск (отдельно).
        let max_size = 64 * 1024 * 1024;
        Self {
            max_size,
            max_chunks: (max_size as usize / CHUNK_SIZE) + 1,
            disk_threshold: 8 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// Лимит под конкретный максимум размера (число чанков выводится).
    pub fn with_max_size(max_size: u64) -> Self {
        Self { max_size, ..Default::default() }
            .with_chunks_for(max_size)
    }

    fn with_chunks_for(mut self, max_size: u64) -> Self {
        self.max_chunks = (max_size as usize / CHUNK_SIZE) + 1;
        self
    }

    /// Порог сборки на диск (для тестов и настроек хранилища).
    pub fn with_disk_threshold(mut self, bytes: u64) -> Self {
        self.disk_threshold = bytes;
        self
    }
}

/// Манифест блоба — план докачки и проверки + типизированный конверт (mime).
/// Передаётся в аутентифицированном E2E-сообщении, поэтому хэши чанков доверенные.
///
/// Хэши сериализуются в hex (id — строка, chunks — одна конкатенированная строка),
/// а не как JSON-массивы чисел: иначе манифест раздувался бы в ~3–4 раза и не влезал
/// в датаграмму даже для средних блобов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// MIME-тип (image/jpeg, application/octet-stream, audio/opus, …).
    pub mime: String,
    /// Полный размер содержимого в байтах.
    pub size: u64,
    /// BLAKE3 всего содержимого — контент-адрес (id, ключ дедупликации).
    #[serde(with = "hex_hash")]
    pub id: Hash,
    /// BLAKE3-хэши чанков по порядку.
    #[serde(with = "hex_hashes")]
    pub chunks: Vec<Hash>,
}

mod hex_hash {
    use super::Hash;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(h: &Hash, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(h))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Hash, D::Error> {
        let s = String::deserialize(d)?;
        let v = hex::decode(&s).map_err(serde::de::Error::custom)?;
        <[u8; 32]>::try_from(v.as_slice()).map_err(|_| serde::de::Error::custom("плохая длина хэша"))
    }
}

mod hex_hashes {
    use super::Hash;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[Hash], s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(v.len() * 64);
        for h in v {
            out.push_str(&hex::encode(h));
        }
        s.serialize_str(&out)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Hash>, D::Error> {
        let s = String::deserialize(d)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() % 32 != 0 {
            return Err(serde::de::Error::custom("длина хэшей не кратна 32"));
        }
        Ok(bytes.chunks(32).map(|c| <[u8; 32]>::try_from(c).unwrap()).collect())
    }
}

impl Manifest {
    /// Ожидаемое число чанков для данного размера.
    fn expected_chunks(size: u64) -> usize {
        if size == 0 {
            0
        } else {
            ((size as usize) + CHUNK_SIZE - 1) / CHUNK_SIZE
        }
    }

    /// Самопроверка манифеста против лимитов и внутренней согласованности —
    /// ДО докачки (анти-DoS + защита от кривого/злого манифеста).
    pub fn validate(&self, limits: &Limits) -> Result<(), BlobError> {
        if self.size > limits.max_size {
            return Err(BlobError::TooLarge);
        }
        if self.chunks.len() > limits.max_chunks {
            return Err(BlobError::TooManyChunks);
        }
        if self.chunks.len() != Self::expected_chunks(self.size) {
            return Err(BlobError::InconsistentManifest);
        }
        Ok(())
    }

    /// Сериализовать в JSON (для вкладывания в сообщение).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }

    /// Разобрать из JSON.
    pub fn from_json(s: &str) -> Result<Manifest, BlobError> {
        serde_json::from_str(s).map_err(|_| BlobError::BadManifest)
    }

    /// Нарезать манифест на части, каждая из которых влезает в одну датаграмму.
    /// Раньше манифест ехал ОДНИМ пакетом, и это ограничивало блоб 4 МБ: список
    /// хэшей рос линейно с размером файла и упирался в 65535.
    pub fn split(&self) -> Vec<ManifestPart> {
        if self.chunks.is_empty() {
            return vec![ManifestPart {
                id: self.id,
                mime: self.mime.clone(),
                size: self.size,
                part: 0,
                total: 1,
                hashes: Vec::new(),
            }];
        }
        let groups: Vec<&[Hash]> = self.chunks.chunks(HASHES_PER_PART).collect();
        let total = groups.len() as u32;
        groups
            .into_iter()
            .enumerate()
            .map(|(i, g)| ManifestPart {
                id: self.id,
                mime: self.mime.clone(),
                size: self.size,
                part: i as u32,
                total,
                hashes: g.to_vec(),
            })
            .collect()
    }
}

/// Сколько хэшей чанков кладём в одну часть манифеста. 300 × 32 Б = 9.6 КБ сырых →
/// после hex-кодирования (×2 в payload и ×2 в конверте) ≈ 40 КБ < 65535.
pub const HASHES_PER_PART: usize = 300;

/// Часть манифеста: метаданные блоба + кусок списка хэшей. Метаданные дублируются в
/// каждой части намеренно — получатель может проверить лимиты и отказаться от
/// докачки, не дожидаясь всех частей.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestPart {
    pub id: Hash,
    pub mime: String,
    pub size: u64,
    pub part: u32,
    pub total: u32,
    pub hashes: Vec<Hash>,
}

/// Сборщик манифеста из частей. Живёт у получателя между приходом первой части и
/// полного комплекта; проверяет согласованность (один id/size/total у всех частей).
#[derive(Debug)]
pub struct ManifestAssembler {
    id: Hash,
    mime: String,
    size: u64,
    total: u32,
    parts: Vec<Option<Vec<Hash>>>,
    have: usize,
}

impl ManifestAssembler {
    /// Начать сборку по первой пришедшей части.
    pub fn new(first: &ManifestPart) -> Result<Self, BlobError> {
        if first.total == 0 || first.part >= first.total {
            return Err(BlobError::BadManifest);
        }
        let mut a = ManifestAssembler {
            id: first.id,
            mime: first.mime.clone(),
            size: first.size,
            total: first.total,
            parts: vec![None; first.total as usize],
            have: 0,
        };
        a.add(first)?;
        Ok(a)
    }

    /// Принять часть. Части от другого блоба или с иным разбиением отвергаются.
    pub fn add(&mut self, p: &ManifestPart) -> Result<(), BlobError> {
        if p.id != self.id || p.size != self.size || p.total != self.total {
            return Err(BlobError::BadManifest);
        }
        let idx = p.part as usize;
        if idx >= self.parts.len() {
            return Err(BlobError::BadIndex);
        }
        if self.parts[idx].is_none() {
            self.parts[idx] = Some(p.hashes.clone());
            self.have += 1;
        }
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.have == self.parts.len()
    }

    /// Собрать манифест, когда пришли все части.
    pub fn finish(&self) -> Option<Manifest> {
        if !self.is_complete() {
            return None;
        }
        let chunks: Vec<Hash> = self.parts.iter().flat_map(|p| p.clone().unwrap_or_default()).collect();
        Some(Manifest { mime: self.mime.clone(), size: self.size, id: self.id, chunks })
    }
}

/// Нарезать содержимое на чанки и построить манифест. Возвращает манифест и сами
/// чанки (по порядку) — отправитель раздаёт чанки по запросу получателя.
pub fn build(content: &[u8], mime: impl Into<String>) -> (Manifest, Vec<Vec<u8>>) {
    let chunks: Vec<Vec<u8>> = if content.is_empty() {
        Vec::new()
    } else {
        content.chunks(CHUNK_SIZE).map(|c| c.to_vec()).collect()
    };
    let hashes: Vec<Hash> = chunks.iter().map(|c| hash(c)).collect();
    let manifest = Manifest { mime: mime.into(), size: content.len() as u64, id: hash(content), chunks: hashes };
    (manifest, chunks)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BlobError {
    #[error("блоб превышает лимит размера")]
    TooLarge,
    #[error("слишком много чанков")]
    TooManyChunks,
    #[error("манифест не согласован (число чанков ≠ размеру)")]
    InconsistentManifest,
    #[error("не разобрать манифест")]
    BadManifest,
    #[error("индекс чанка вне диапазона")]
    BadIndex,
    #[error("чанк не совпал с хэшем манифеста")]
    ChunkMismatch,
    #[error("блоб собран не полностью")]
    Incomplete,
    #[error("собранное содержимое не совпало с id/размером блоба")]
    IdMismatch,
    #[error("ошибка ввода-вывода при сборке блоба на диск")]
    Io,
}

/// Инкрементальная сборка блоба из чанков с проверкой каждого против манифеста.
/// Порядок прихода чанков любой; дубликаты игнорируются; битые — отвергаются.
pub struct Reassembler {
    manifest: Manifest,
    parts: Vec<Option<Vec<u8>>>,
    have: usize,
}

impl Reassembler {
    /// Создать сборщик по манифесту (валидирует его против лимитов сразу).
    pub fn new(manifest: Manifest, limits: &Limits) -> Result<Self, BlobError> {
        manifest.validate(limits)?;
        let n = manifest.chunks.len();
        Ok(Self { manifest, parts: vec![None; n], have: 0 })
    }

    /// Индексы ещё не полученных чанков (для запроса докачки).
    pub fn missing(&self) -> Vec<usize> {
        self.parts.iter().enumerate().filter(|(_, p)| p.is_none()).map(|(i, _)| i).collect()
    }

    /// Принять чанк по индексу. Проверяет размер и BLAKE3 против манифеста; чужой/
    /// битый чанк отвергается (`ChunkMismatch`), не искажая собранный блоб.
    pub fn add(&mut self, index: usize, data: &[u8]) -> Result<(), BlobError> {
        if index >= self.parts.len() {
            return Err(BlobError::BadIndex);
        }
        // Все чанки, кроме последнего, ровно CHUNK_SIZE; последний — не больше.
        if data.len() > CHUNK_SIZE {
            return Err(BlobError::ChunkMismatch);
        }
        if hash(data) != self.manifest.chunks[index] {
            return Err(BlobError::ChunkMismatch);
        }
        if self.parts[index].is_none() {
            self.parts[index] = Some(data.to_vec());
            self.have += 1;
        }
        Ok(())
    }

    /// Все ли чанки на месте.
    pub fn is_complete(&self) -> bool {
        self.have == self.parts.len()
    }

    /// Прогресс приёма (полученных чанков, всего).
    pub fn progress(&self) -> (usize, usize) {
        (self.have, self.parts.len())
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Собрать содержимое и сверить с id и размером блоба. Финальная гарантия
    /// целостности сверх по-чанковой проверки.
    pub fn finish(self) -> Result<Vec<u8>, BlobError> {
        if !self.is_complete() {
            return Err(BlobError::Incomplete);
        }
        let mut out = Vec::with_capacity(self.manifest.size as usize);
        for p in self.parts {
            out.extend_from_slice(&p.expect("проверено is_complete"));
        }
        if out.len() as u64 != self.manifest.size || blob_id(&out) != self.manifest.id {
            return Err(BlobError::IdMismatch);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(n: usize, fill: u8) -> Vec<u8> {
        (0..n).map(|i| fill.wrapping_add(i as u8)).collect()
    }

    /// Полный цикл: нарезали → собрали (в перемешанном порядке) → совпало.
    #[test]
    fn build_reassemble_roundtrip() {
        let content = data(CHUNK_SIZE * 3 + 123, 7); // 3 полных чанка + хвост
        let (manifest, chunks) = build(&content, "application/octet-stream");
        assert_eq!(manifest.chunks.len(), 4);
        assert_eq!(manifest.size, content.len() as u64);

        let mut r = Reassembler::new(manifest, &Limits::default()).unwrap();
        // добавляем в обратном порядке — сборщик не зависит от порядка
        for i in (0..chunks.len()).rev() {
            r.add(i, &chunks[i]).unwrap();
        }
        assert!(r.is_complete());
        assert_eq!(r.finish().unwrap(), content);
    }

    #[test]
    fn empty_blob_has_no_chunks() {
        let (manifest, chunks) = build(&[], "text/plain");
        assert!(chunks.is_empty());
        assert_eq!(manifest.chunks.len(), 0);
        let r = Reassembler::new(manifest, &Limits::default()).unwrap();
        assert!(r.is_complete());
        assert_eq!(r.finish().unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn same_content_same_id_dedup() {
        let a = build(&data(1000, 1), "image/png").0;
        let b = build(&data(1000, 1), "image/jpeg").0; // тот же контент, иной mime
        assert_eq!(a.id, b.id, "id зависит только от содержимого (дедуп)");
        let c = build(&data(1000, 2), "image/png").0;
        assert_ne!(a.id, c.id);
    }

    #[test]
    fn tampered_chunk_rejected() {
        let content = data(CHUNK_SIZE + 10, 3);
        let (manifest, chunks) = build(&content, "x/y");
        let mut r = Reassembler::new(manifest, &Limits::default()).unwrap();
        let mut bad = chunks[0].clone();
        bad[0] ^= 0xff;
        assert_eq!(r.add(0, &bad), Err(BlobError::ChunkMismatch));
        // правильный чанк принимается
        assert!(r.add(0, &chunks[0]).is_ok());
    }

    #[test]
    fn missing_tracking_and_incomplete_finish() {
        let content = data(CHUNK_SIZE * 2, 5);
        let (manifest, chunks) = build(&content, "x/y");
        let mut r = Reassembler::new(manifest, &Limits::default()).unwrap();
        r.add(0, &chunks[0]).unwrap();
        assert_eq!(r.missing(), vec![1]);
        assert_eq!(r.progress(), (1, 2));
        // финиш до полноты — ошибка
        // (собираем копию, т.к. finish поглощает self)
        let content2 = content.clone();
        let (m2, c2) = build(&content2, "x/y");
        let mut r2 = Reassembler::new(m2, &Limits::default()).unwrap();
        r2.add(0, &c2[0]).unwrap();
        assert_eq!(r2.finish(), Err(BlobError::Incomplete));
    }

    #[test]
    fn limits_reject_oversized_and_inconsistent() {
        // Слишком большой размер.
        let big = Manifest { mime: "x/y".into(), size: 999_999_999, id: [0; 32], chunks: vec![] };
        let small_limit = Limits::with_max_size(1024);
        assert_eq!(big.validate(&small_limit), Err(BlobError::TooLarge));

        // Несогласованный манифест: size говорит про 2 чанка, а список пуст.
        let bad = Manifest { mime: "x/y".into(), size: (CHUNK_SIZE * 2) as u64, id: [0; 32], chunks: vec![] };
        assert_eq!(bad.validate(&Limits::default()), Err(BlobError::InconsistentManifest));
    }

    #[test]
    fn manifest_json_roundtrip() {
        let (manifest, _) = build(&data(200, 9), "audio/opus");
        let s = manifest.to_json();
        assert_eq!(Manifest::from_json(&s).unwrap(), manifest);
        assert!(Manifest::from_json("не json").is_err());
    }

    #[test]
    fn bad_index_rejected() {
        let (manifest, chunks) = build(&data(10, 1), "x/y");
        let mut r = Reassembler::new(manifest, &Limits::default()).unwrap();
        assert_eq!(r.add(5, &chunks[0]), Err(BlobError::BadIndex));
    }

    /// Манифест крупного блоба режется на части и собирается обратно байт-в-байт.
    /// Именно это снимало прежний потолок 4 МБ (список хэшей не влезал в датаграмму).
    #[test]
    fn manifest_splits_and_reassembles() {
        // Столько чанков, что хэши заведомо не влезли бы в один пакет.
        let big = CHUNK_SIZE * (HASHES_PER_PART * 2 + 5);
        let limits = Limits::with_max_size(big as u64);
        let (manifest, _) = build(&data(big, 3), "video/mp4");

        let parts = manifest.split();
        assert!(parts.len() > 2, "манифест должен разбиться на несколько частей");
        assert!(parts.iter().all(|p| p.hashes.len() <= HASHES_PER_PART));

        let mut asm = ManifestAssembler::new(&parts[0]).unwrap();
        for p in &parts[1..] {
            assert!(!asm.is_complete());
            asm.add(p).unwrap();
        }
        assert!(asm.is_complete());
        let got = asm.finish().unwrap();
        assert_eq!(got, manifest, "собранный манифест совпадает с исходным");
        got.validate(&limits).expect("собранный манифест валиден");
    }

    /// Части приходят по сети в произвольном порядке и могут дублироваться.
    #[test]
    fn manifest_parts_tolerate_reorder_and_duplicates() {
        let (manifest, _) = build(&data(CHUNK_SIZE * (HASHES_PER_PART + 3), 7), "x/y");
        let parts = manifest.split();
        let mut asm = ManifestAssembler::new(&parts[parts.len() - 1]).unwrap();
        for p in parts.iter().rev() {
            asm.add(p).unwrap();
            asm.add(p).unwrap(); // дубль не должен ломать счётчик
        }
        assert!(asm.is_complete());
        assert_eq!(asm.finish().unwrap(), manifest);
    }

    /// Часть от ЧУЖОГО блоба (или с иным разбиением) отвергается — иначе можно было бы
    /// подмешать чужие хэши в собираемый манифест.
    #[test]
    fn foreign_manifest_part_rejected() {
        let (m1, _) = build(&data(CHUNK_SIZE * 2, 1), "x/y");
        let (m2, _) = build(&data(CHUNK_SIZE * 2, 2), "x/y");
        let mut asm = ManifestAssembler::new(&m1.split()[0]).unwrap();
        assert_eq!(asm.add(&m2.split()[0]), Err(BlobError::BadManifest));
    }
}
