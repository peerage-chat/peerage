//! Зачистка метаданных ISO BMFF — MP4/MOV/3GP (видео, аудио) и HEIC/HEIF/AVIF (фото) — БЕЗ
//! перекодирования и без сдвига байтов (шаг беты 7m).
//!
//! Почему на месте, а не вырезанием: в MP4 таблицы `stco`/`co64` хранят абсолютные смещения
//! кадров в файле. Убрать коробку из `moov`, лежащего перед `mdat`, — сдвинуть все кадры и
//! сломать видео. Поэтому коробка с метаданными перемечается в `free` (её пропускает любой
//! проигрыватель), а содержимое зануляется: размер файла и все смещения прежние. Это же
//! позволяет чистить файл на диске, не поднимая гигабайт видео в память.
//!
//! Что снимается:
//! - `udta` (в т. ч. `©xyz` — геометка камеры, `©day`, имена), `meta` в `moov`/`trak`
//!   (ключи `mdta`: `com.android.capture.*`, `com.apple.quicktime.location.ISO6709`), XMP в `uuid`;
//! - время создания и изменения в `mvhd`/`tkhd`/`mdhd` — нули («неизвестно»);
//! - в HEIF коробку `meta` трогать нельзя: в ней само изображение (`iinf`/`iloc`/`iprp`).
//!   Там зануляются только данные элементов `Exif` и XMP (`mime` `application/rdf+xml`).
//!
//! Битый или чужой файл не роняет разбор: коробка, вылезающая за границы, обрывает обход —
//! дальше ничего не трогаем.

use std::io::{self, Read, Seek, SeekFrom, Write};

/// Файл с произвольным доступом: [`std::fs::File`] или `Cursor<Vec<u8>>` для байтов в памяти.
pub trait Io: Read + Write + Seek {}
impl<T: Read + Write + Seek> Io for T {}

/// XMP в коробке `uuid` (Adobe): BE7ACFCB-97A9-42E8-9C71-999491E3AFAC.
const XMP_UUID: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

/// Глубже настоящих файлов не бывает; предел — от рекурсии на подложных вложенностях.
const MAX_DEPTH: u32 = 8;

/// Похоже ли начало на ISO BMFF: первая коробка — из тех, с которых такие файлы начинаются.
pub fn looks_like(head: &[u8]) -> bool {
    head.len() >= 8
        && matches!(
            &head[4..8],
            b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip"
        )
}

/// Зачистить контейнер длиной `len`. Сколько коробок занулено — для тестов и лога.
pub fn scrub<F: Io>(f: &mut F, len: u64) -> io::Result<usize> {
    let mut n = 0;
    walk(f, 0, len, 0, &mut n)?;
    Ok(n)
}

#[derive(Clone, Copy)]
struct BoxHdr {
    start: u64,
    /// Длина заголовка: 8, 16 (64-битный размер); у FullBox версия и флаги — уже в теле.
    hdr: u64,
    size: u64,
    kind: [u8; 4],
}

impl BoxHdr {
    fn body(&self) -> u64 {
        self.start + self.hdr
    }
    fn end(&self) -> u64 {
        self.start + self.size
    }
}

fn read_exact_at<F: Io>(f: &mut F, pos: u64, buf: &mut [u8]) -> io::Result<()> {
    f.seek(SeekFrom::Start(pos))?;
    f.read_exact(buf)
}

/// Заголовок коробки в `pos`; `None` — дальше нет целой коробки (конец или мусор).
fn read_hdr<F: Io>(f: &mut F, pos: u64, end: u64) -> io::Result<Option<BoxHdr>> {
    if pos.checked_add(8).is_none_or(|p| p > end) {
        return Ok(None);
    }
    let mut h = [0u8; 8];
    read_exact_at(f, pos, &mut h)?;
    let small = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
    let kind = [h[4], h[5], h[6], h[7]];
    let (size, hdr) = match small {
        1 => {
            if pos + 16 > end {
                return Ok(None);
            }
            let mut l = [0u8; 8];
            read_exact_at(f, pos + 8, &mut l)?;
            (u64::from_be_bytes(l), 16)
        }
        0 => (end - pos, 8), // до конца родителя
        n => (n, 8),
    };
    if size < hdr || pos.checked_add(size).is_none_or(|e| e > end) {
        return Ok(None);
    }
    Ok(Some(BoxHdr {
        start: pos,
        hdr,
        size,
        kind,
    }))
}

fn walk<F: Io>(f: &mut F, start: u64, end: u64, depth: u32, n: &mut usize) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let mut pos = start;
    while let Some(b) = read_hdr(f, pos, end)? {
        match &b.kind {
            b"moov" | b"trak" | b"mdia" => walk(f, b.body(), b.end(), depth + 1, n)?,
            b"udta" => free(f, &b, n)?,
            b"meta" => {
                if is_picture_meta(f, &b)? {
                    heif_items(f, &b, n)?;
                } else {
                    free(f, &b, n)?;
                }
            }
            b"uuid" => {
                let mut id = [0u8; 16];
                if b.size >= b.hdr + 16 {
                    read_exact_at(f, b.body(), &mut id)?;
                    if id == XMP_UUID {
                        free(f, &b, n)?;
                    }
                }
            }
            b"mvhd" | b"tkhd" | b"mdhd" => zero_times(f, &b)?,
            _ => {}
        }
        pos = b.end();
    }
    Ok(())
}

/// Перемечать коробку в `free` и занулить её содержимое: размер и смещения — прежние.
fn free<F: Io>(f: &mut F, b: &BoxHdr, n: &mut usize) -> io::Result<()> {
    f.seek(SeekFrom::Start(b.start + 4))?;
    f.write_all(b"free")?;
    zero(f, b.body(), b.end() - b.body())?;
    *n += 1;
    Ok(())
}

fn zero<F: Io>(f: &mut F, from: u64, len: u64) -> io::Result<()> {
    const CHUNK: usize = 64 * 1024;
    let zeros = [0u8; CHUNK];
    f.seek(SeekFrom::Start(from))?;
    let mut left = len;
    while left > 0 {
        let k = left.min(CHUNK as u64) as usize;
        f.write_all(&zeros[..k])?;
        left -= k as u64;
    }
    Ok(())
}

/// Время создания и изменения в `mvhd`/`tkhd`/`mdhd` — сразу за версией и флагами:
/// версия 1 — два 64-битных поля, версия 0 — два 32-битных. Ноль — «неизвестно».
fn zero_times<F: Io>(f: &mut F, b: &BoxHdr) -> io::Result<()> {
    let mut v = [0u8; 1];
    if b.size < b.hdr + 4 {
        return Ok(());
    }
    read_exact_at(f, b.body(), &mut v)?;
    let len = if v[0] == 1 { 16 } else { 8 };
    if b.body() + 4 + len <= b.end() {
        zero(f, b.body() + 4, len)?;
    }
    Ok(())
}

/// Дети FullBox `meta` начинаются за 4 байтами версии и флагов.
fn meta_children(b: &BoxHdr) -> u64 {
    b.body() + 4
}

/// `meta` с обработчиком `pict` — это HEIF-изображение, а не метаданные видео.
fn is_picture_meta<F: Io>(f: &mut F, b: &BoxHdr) -> io::Result<bool> {
    let mut pos = meta_children(b);
    while let Some(c) = read_hdr(f, pos, b.end())? {
        if &c.kind == b"hdlr" && c.size >= c.hdr + 12 {
            let mut t = [0u8; 4];
            read_exact_at(f, c.body() + 8, &mut t)?; // версия/флаги (4) + pre_defined (4)
            return Ok(&t == b"pict");
        }
        pos = c.end();
    }
    Ok(false)
}

/// HEIF: найти элементы EXIF и XMP в `iinf` и занулить их данные по `iloc`.
fn heif_items<F: Io>(f: &mut F, meta: &BoxHdr, n: &mut usize) -> io::Result<()> {
    let (mut iinf, mut iloc, mut idat) = (None, None, None);
    let mut pos = meta_children(meta);
    while let Some(c) = read_hdr(f, pos, meta.end())? {
        match &c.kind {
            b"iinf" => iinf = Some(c),
            b"iloc" => iloc = Some(c),
            b"idat" => idat = Some(c),
            _ => {}
        }
        pos = c.end();
    }
    let (Some(iinf), Some(iloc)) = (iinf, iloc) else {
        return Ok(());
    };
    let ids = private_items(f, &iinf)?;
    if ids.is_empty() {
        return Ok(());
    }
    let file_end = f.seek(SeekFrom::End(0))?;
    for (id, method, start, len) in item_extents(f, &iloc)? {
        if !ids.contains(&id) || len == 0 {
            continue;
        }
        let at = match (method, idat) {
            (0, _) => Some(start),
            (1, Some(d)) => d.body().checked_add(start),
            _ => None, // по ссылке на другой элемент — данных здесь нет
        };
        if let Some(at) = at.filter(|a| a.checked_add(len).is_some_and(|e| e <= file_end)) {
            zero(f, at, len)?;
            *n += 1;
        }
    }
    Ok(())
}

/// Чтение полей заданной ширины (0, 2, 4, 8 байт) — как их описывает `iloc`.
struct Cur<'a> {
    d: &'a [u8],
    i: usize,
}

impl Cur<'_> {
    fn take(&mut self, k: usize) -> Option<&[u8]> {
        let s = self.d.get(self.i..self.i.checked_add(k)?)?;
        self.i += k;
        Some(s)
    }
    fn uint(&mut self, k: usize) -> Option<u64> {
        let s = self.take(k)?;
        Some(s.iter().fold(0u64, |a, &x| (a << 8) | x as u64))
    }
}

/// Тело коробки в память — только для маленьких служебных (`iinf`, `iloc`), не больше 1 МБ.
fn body_bytes<F: Io>(f: &mut F, b: &BoxHdr) -> io::Result<Vec<u8>> {
    let len = (b.end() - b.body()).min(1 << 20) as usize;
    let mut v = vec![0u8; len];
    read_exact_at(f, b.body(), &mut v)?;
    Ok(v)
}

/// id элементов EXIF и XMP из `iinf` (записи `infe` версий 2 и 3).
fn private_items<F: Io>(f: &mut F, iinf: &BoxHdr) -> io::Result<Vec<u64>> {
    let body = body_bytes(f, iinf)?;
    let mut ids = Vec::new();
    let Some(&ver) = body.first() else {
        return Ok(ids);
    };
    let first = 4 + if ver == 0 { 2 } else { 4 };
    let mut pos = iinf.body() + first as u64;
    while let Some(e) = read_hdr(f, pos, iinf.end())? {
        if &e.kind == b"infe" {
            if let Some(id) = infe_private(&body_bytes(f, &e)?) {
                ids.push(id);
            }
        }
        pos = e.end();
    }
    Ok(ids)
}

fn infe_private(b: &[u8]) -> Option<u64> {
    let mut c = Cur { d: b, i: 0 };
    let ver = c.uint(1)?;
    c.take(3)?;
    let id = match ver {
        2 => c.uint(2)?,
        3 => c.uint(4)?,
        _ => return None,
    };
    c.take(2)?; // item_protection_index
    let kind = c.take(4)?.to_vec();
    let rest = c.d.get(c.i..)?;
    let name_end = rest.iter().position(|&x| x == 0)?;
    let content_type = rest.get(name_end + 1..).unwrap_or(&[]);
    let ct = content_type.split(|&x| x == 0).next().unwrap_or(&[]);
    match kind.as_slice() {
        b"Exif" => Some(id),
        b"mime" if ct == b"application/rdf+xml" => Some(id),
        _ => None,
    }
}

/// Экстенты элементов из `iloc`: (id, способ построения, смещение, длина).
fn item_extents<F: Io>(f: &mut F, iloc: &BoxHdr) -> io::Result<Vec<(u64, u64, u64, u64)>> {
    let body = body_bytes(f, iloc)?;
    Ok(parse_iloc(&body).unwrap_or_default())
}

fn parse_iloc(b: &[u8]) -> Option<Vec<(u64, u64, u64, u64)>> {
    let mut c = Cur { d: b, i: 0 };
    let ver = c.uint(1)?;
    c.take(3)?;
    let s = c.uint(1)?;
    let (off_size, len_size) = ((s >> 4) as usize, (s & 0xF) as usize);
    let s = c.uint(1)?;
    let (base_size, idx_size) = ((s >> 4) as usize, (s & 0xF) as usize);
    let count = if ver < 2 { c.uint(2)? } else { c.uint(4)? };
    let mut out = Vec::new();
    for _ in 0..count {
        let id = if ver < 2 { c.uint(2)? } else { c.uint(4)? };
        let method = if ver == 1 || ver == 2 {
            c.uint(2)? & 0xF
        } else {
            0
        };
        c.take(2)?; // data_reference_index
        let base = c.uint(base_size)?;
        let extents = c.uint(2)?;
        for _ in 0..extents {
            if (ver == 1 || ver == 2) && idx_size > 0 {
                c.take(idx_size)?;
            }
            let off = c.uint(off_size)?;
            let len = c.uint(len_size)?;
            out.push((id, method, base.checked_add(off)?, len));
        }
    }
    Some(out)
}
