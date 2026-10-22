//! Потоковая сборка блоба НА ДИСК (без загрузки целиком в память).
//!
//! [`crate::Reassembler`] держит все чанки в RAM — это нормально для аватарки и
//! картинки, но не для видео: файл на N мегабайт стоил ровно N мегабайт памяти, и
//! именно это (а не размер манифеста) ограничивало максимальный блоб после того, как
//! манифест научился ехать частями.
//!
//! Здесь чанк проверяется по BLAKE3 из манифеста, **шифруется вызывающим** и
//! кладётся в файл по фиксированному смещению. В памяти остаётся только карта
//! «получен/нет» (1 байт на чанк). Крейт остаётся чистым: шифрование инъектируется
//! замыканием, поэтому `p2p-blob` по-прежнему не зависит от `p2p-core`.
//!
//! Формат файла (чтобы читать чанк не разбирая весь блоб):
//! ```text
//! "P2PB" | ver:u8 | chunk_size:u32 | n_chunks:u32 | size:u64 | слоты…
//! слот = len:u32 | запечатанные байты чанка | добивка до slot_size
//! ```
//! Первый байт «P» отличает его от формата [`vault::seal`] (там 0 или 1): мелкие
//! блобы лежат одним запечатанным буфером, и читатель различает их по нему.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::{hash_of, BlobError, Hash, Manifest, CHUNK_SIZE};

/// Магия формата чанкованного блоба на диске.
pub const MAGIC: &[u8; 4] = b"P2PB";
/// Версия формата.
pub const VERSION: u8 = 1;
/// Максимальный оверхед шифрования на чанк (`vault::seal`: 1 + nonce 24 + tag 16).
pub const SEAL_OVERHEAD: usize = 41;
/// Заголовок: magic(4) + ver(1) + chunk_size(4) + n_chunks(4) + size(8).
const HEADER: usize = 4 + 1 + 4 + 4 + 8;

fn slot_size(chunk_size: usize) -> usize {
    4 + chunk_size + SEAL_OVERHEAD
}

fn slot_offset(chunk_size: usize, index: usize) -> u64 {
    (HEADER + index * slot_size(chunk_size)) as u64
}

/// Потоковый сборщик: пишет принятые чанки во временный файл, держа в RAM только
/// карту полученных.
pub struct DiskAssembler {
    manifest: Manifest,
    part_path: PathBuf,
    final_path: PathBuf,
    file: File,
    have: Vec<bool>,
    have_count: usize,
}

impl DiskAssembler {
    /// Начать сборку. `final_path` — куда переименовать по завершении; рядом
    /// создаётся `<final_path>.part`.
    pub fn create(manifest: Manifest, final_path: &Path) -> Result<Self, BlobError> {
        let n = manifest.chunks.len();
        let part_path = final_path.with_extension("part");
        if let Some(dir) = part_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&part_path)
            .map_err(|_| BlobError::Io)?;

        let mut header = Vec::with_capacity(HEADER);
        header.extend_from_slice(MAGIC);
        header.push(VERSION);
        header.extend_from_slice(&(CHUNK_SIZE as u32).to_be_bytes());
        header.extend_from_slice(&(n as u32).to_be_bytes());
        header.extend_from_slice(&manifest.size.to_be_bytes());
        file.write_all(&header).map_err(|_| BlobError::Io)?;

        Ok(Self {
            manifest,
            part_path,
            final_path: final_path.to_path_buf(),
            file,
            have: vec![false; n],
            have_count: 0,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn missing(&self) -> Vec<usize> {
        self.have.iter().enumerate().filter(|(_, h)| !**h).map(|(i, _)| i).collect()
    }

    pub fn is_complete(&self) -> bool {
        self.have_count == self.have.len()
    }

    /// Получен ли чанк `index`.
    pub fn has(&self, index: usize) -> bool {
        self.have.get(index).copied().unwrap_or(false)
    }

    /// Прогресс приёма (полученных чанков, всего).
    pub fn progress(&self) -> (usize, usize) {
        (self.have_count, self.have.len())
    }

    /// Принять чанк: проверить BLAKE3 против манифеста, запечатать и записать в слот.
    /// `seal(номер, байты)` — шифрование at-rest (инъекция, чтобы крейт не знал про
    /// vault). Номер чанка нужен ему для привязки шифртекста к месту в файле.
    pub fn add(
        &mut self,
        index: usize,
        data: &[u8],
        seal: impl Fn(usize, &[u8]) -> Vec<u8>,
    ) -> Result<(), BlobError> {
        if index >= self.have.len() {
            return Err(BlobError::BadIndex);
        }
        // Все чанки кроме последнего — ровно CHUNK_SIZE (тот же инвариант, что в RAM-сборке).
        let last = index + 1 == self.have.len();
        if data.len() > CHUNK_SIZE || (!last && data.len() != CHUNK_SIZE) {
            return Err(BlobError::ChunkMismatch);
        }
        if hash_of(data) != self.manifest.chunks[index] {
            return Err(BlobError::ChunkMismatch);
        }
        if self.have[index] {
            return Ok(()); // дубль — уже записан
        }
        let sealed = seal(index, data);
        if sealed.len() > CHUNK_SIZE + SEAL_OVERHEAD {
            return Err(BlobError::Io); // шифрование дало больше, чем рассчитан слот
        }
        self.file
            .seek(SeekFrom::Start(slot_offset(CHUNK_SIZE, index)))
            .map_err(|_| BlobError::Io)?;
        self.file
            .write_all(&(sealed.len() as u32).to_be_bytes())
            .map_err(|_| BlobError::Io)?;
        self.file.write_all(&sealed).map_err(|_| BlobError::Io)?;
        self.have[index] = true;
        self.have_count += 1;
        Ok(())
    }

    /// Завершить сборку: дописать хвост файла и переименовать в финальный путь.
    /// Возвращает путь готового блоба.
    pub fn finish(mut self) -> Result<PathBuf, BlobError> {
        if !self.is_complete() {
            return Err(BlobError::Incomplete);
        }
        // Дотягиваем файл до полного размера — иначе последний слот может быть короче
        // и чтение по смещению упрётся в конец файла.
        let total = slot_offset(CHUNK_SIZE, self.have.len());
        self.file.set_len(total).map_err(|_| BlobError::Io)?;
        self.file.flush().map_err(|_| BlobError::Io)?;
        drop(self.file);
        std::fs::rename(&self.part_path, &self.final_path).map_err(|_| BlobError::Io)?;
        Ok(self.final_path)
    }
}

/// Заголовок готового файла: (chunk_size, n_chunks, size). `None` — не наш формат
/// (старый блоб: файл целиком запечатан одним буфером).
pub fn read_header(path: &Path) -> Option<(usize, usize, u64)> {
    let mut f = File::open(path).ok()?;
    let mut h = [0u8; HEADER];
    f.read_exact(&mut h).ok()?;
    if &h[0..4] != MAGIC || h[4] != VERSION {
        return None;
    }
    let chunk_size = u32::from_be_bytes(h[5..9].try_into().ok()?) as usize;
    let n = u32::from_be_bytes(h[9..13].try_into().ok()?) as usize;
    let size = u64::from_be_bytes(h[13..21].try_into().ok()?);
    Some((chunk_size, n, size))
}

/// Прочитать ОДИН чанк готового блоба, не читая файл целиком. `open(номер, байты)`
/// — расшифровка at-rest.
pub fn read_chunk(
    path: &Path,
    index: usize,
    open: impl Fn(usize, &[u8]) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    let (chunk_size, n, _size) = read_header(path)?;
    if index >= n {
        return None;
    }
    let mut f = File::open(path).ok()?;
    f.seek(SeekFrom::Start(slot_offset(chunk_size, index))).ok()?;
    let mut len_buf = [0u8; 4];
    f.read_exact(&mut len_buf).ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > chunk_size + SEAL_OVERHEAD {
        return None;
    }
    let mut sealed = vec![0u8; len];
    f.read_exact(&mut sealed).ok()?;
    open(index, &sealed)
}

/// Прочитать блоб целиком (для мелких — картинка, аватар). Для крупных файлов
/// вызывающий должен читать по чанкам через [`read_chunk`], иначе смысл потоковой
/// сборки теряется.
pub fn read_all(path: &Path, open: impl Fn(usize, &[u8]) -> Option<Vec<u8>> + Copy) -> Option<Vec<u8>> {
    let (_chunk_size, n, size) = read_header(path)?;
    let mut out = Vec::with_capacity(size as usize);
    for i in 0..n {
        out.extend_from_slice(&read_chunk(path, i, open)?);
    }
    Some(out)
}

/// Манифест готового блоба на диске — чтобы отдать его, не поднимая в память:
/// хэши чанков и id считаются потоково, по одному чанку.
pub fn manifest_of_stored(
    path: &Path,
    mime: impl Into<String>,
    open: impl Fn(usize, &[u8]) -> Option<Vec<u8>> + Copy,
) -> Option<Manifest> {
    let (_cs, n, size) = read_header(path)?;
    let mut total = blake3::Hasher::new();
    let mut chunks = Vec::with_capacity(n);
    for i in 0..n {
        let c = read_chunk(path, i, open)?;
        total.update(&c);
        chunks.push(hash_of(&c));
    }
    Some(Manifest { mime: mime.into(), size, id: *total.finalize().as_bytes(), chunks })
}

/// Проверить контент-адрес готового файла, не загружая его целиком: считаем BLAKE3
/// потоково по чанкам.
pub fn verify_id(path: &Path, expect: Hash, open: impl Fn(usize, &[u8]) -> Option<Vec<u8>> + Copy) -> bool {
    let Some((_cs, n, _size)) = read_header(path) else { return false };
    let mut hasher = blake3::Hasher::new();
    for i in 0..n {
        match read_chunk(path, i, open) {
            Some(c) => {
                hasher.update(&c);
            }
            None => return false,
        }
    }
    *hasher.finalize().as_bytes() == expect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build;

    fn tmp(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("p2p-disk-{tag}-{n}"));
        std::fs::create_dir_all(&d).unwrap();
        d.join("blob")
    }

    /// Имитация vault: маркер + «шифртекст» той же длины (реальный vault добавляет
    /// 41 байт, здесь достаточно проверить, что оверхед учитывается).
    fn seal(_i: usize, p: &[u8]) -> Vec<u8> {
        let mut v = vec![1u8];
        v.extend_from_slice(p);
        v
    }
    fn open(_i: usize, d: &[u8]) -> Option<Vec<u8>> {
        d.split_first().map(|(_, rest)| rest.to_vec())
    }

    fn data(n: usize, fill: u8) -> Vec<u8> {
        (0..n).map(|i| fill.wrapping_add(i as u8)).collect()
    }

    #[test]
    fn assembles_multichunk_blob_on_disk() {
        let content = data(CHUNK_SIZE * 2 + 777, 5);
        let (manifest, chunks) = build(&content, "video/mp4");
        let path = tmp("ok");
        let mut a = DiskAssembler::create(manifest.clone(), &path).unwrap();
        assert!(!a.is_complete());
        // В обратном порядке — сборка не зависит от очерёдности.
        for i in (0..chunks.len()).rev() {
            a.add(i, &chunks[i], seal).unwrap();
        }
        assert!(a.is_complete());
        let done = a.finish().unwrap();

        assert_eq!(read_all(&done, open).unwrap(), content, "собран байт-в-байт");
        assert!(verify_id(&done, manifest.id, open), "контент-адрес сходится");
        // Чанк читается точечно, без чтения всего файла.
        assert_eq!(read_chunk(&done, 1, open).unwrap(), chunks[1]);
    }

    #[test]
    fn rejects_corrupt_chunk_and_bad_index() {
        let content = data(CHUNK_SIZE + 10, 2);
        let (manifest, chunks) = build(&content, "x/y");
        let mut a = DiskAssembler::create(manifest, &tmp("bad")).unwrap();
        assert_eq!(a.add(9, &chunks[0], seal), Err(BlobError::BadIndex));
        let mut bad = chunks[0].clone();
        bad[0] ^= 0xFF;
        assert_eq!(a.add(0, &bad, seal), Err(BlobError::ChunkMismatch));
        assert!(!a.is_complete());
    }

    #[test]
    fn finish_requires_all_chunks() {
        let content = data(CHUNK_SIZE * 2, 4);
        let (manifest, chunks) = build(&content, "x/y");
        let mut a = DiskAssembler::create(manifest, &tmp("part")).unwrap();
        a.add(0, &chunks[0], seal).unwrap();
        assert_eq!(a.finish().unwrap_err(), BlobError::Incomplete);
    }

    /// Дубликат чанка не ломает счётчик (в сети повторы — норма).
    #[test]
    fn duplicate_chunk_is_idempotent() {
        let content = data(CHUNK_SIZE * 2, 6);
        let (manifest, chunks) = build(&content, "x/y");
        let mut a = DiskAssembler::create(manifest, &tmp("dup")).unwrap();
        a.add(0, &chunks[0], seal).unwrap();
        a.add(0, &chunks[0], seal).unwrap();
        assert!(!a.is_complete(), "дубль не должен считаться вторым чанком");
        a.add(1, &chunks[1], seal).unwrap();
        assert!(a.is_complete());
    }

    /// Файл чужого формата (старый блоб) не опознаётся как чанкованный.
    #[test]
    fn foreign_format_not_recognised() {
        let p = tmp("old");
        std::fs::write(&p, [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 0].repeat(4)).unwrap();
        assert!(read_header(&p).is_none());
        assert!(read_all(&p, open).is_none());
    }
}
