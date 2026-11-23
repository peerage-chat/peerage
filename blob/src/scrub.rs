//! Снятие приватных метаданных с вложений перед отправкой (веха M2, PET-ось; шаг беты 7m).
//!
//! Работает **без перекодирования** (без потери качества, без тяжёлых зависимостей):
//! вырезает или зануляет известные места с метаданными. Снимает стандартный приватный слой —
//! **EXIF/GPS, XMP, IPTC, текстовые комментарии, время**:
//! - JPEG, PNG, WebP, GIF — вырезанием сегментов, чанков и блоков (у GIF — комментарии и
//!   XMP; петля анимации и кадры остаются);
//! - ISO BMFF (MP4/MOV/3GP, HEIC/HEIF/AVIF) — на месте, без сдвига байтов ([`crate::bmff`]):
//!   видео и HEIC так можно чистить и файлом на диске ([`scrub_file`]).
//!
//! Честный предел: это не полный ре-энкод. Стандартные геометки уходят гарантированно;
//! экзотический эмбеддер в нестандартном месте может проскочить. Полная зачистка
//! перекодированием — после беты. Неизвестные форматы возвращаются без изменений.

use std::io::Cursor;
use std::path::Path;

/// Снять метаданные по содержимому (mime не нужен: формат виден по сигнатуре). Возвращает
/// очищенные байты (или исходные, если формат не распознан). Вызывать ДО [`crate::build`].
pub fn scrub(_mime: &str, data: &[u8]) -> Vec<u8> {
    if is_jpeg(data) {
        scrub_jpeg(data)
    } else if is_png(data) {
        scrub_png(data)
    } else if is_webp(data) {
        scrub_webp(data)
    } else if is_gif(data) {
        scrub_gif(data)
    } else if crate::bmff::looks_like(data) {
        let mut c = Cursor::new(data.to_vec());
        let len = data.len() as u64;
        // Ошибка ввода-вывода у курсора — только обрыв разбора: что успели, то занулено.
        let _ = crate::bmff::scrub(&mut c, len);
        c.into_inner()
    } else {
        data.to_vec()
    }
}

/// Зачистить файл НА МЕСТЕ, не поднимая в память (видео на гигабайт). Только ISO BMFF —
/// у него зачистка не меняет длину; прочее не трогаем (`Ok(0)`). Файл переписывается — звать
/// на своей копии, а не на оригинале пользователя. Возвращает число зачищенных мест.
pub fn scrub_file(path: &Path) -> std::io::Result<usize> {
    use std::io::Read;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let mut head = [0u8; 12];
    let got = f.read(&mut head)?;
    if !crate::bmff::looks_like(&head[..got]) {
        return Ok(0);
    }
    let len = f.metadata()?.len();
    let n = crate::bmff::scrub(&mut f, len)?;
    f.sync_all()?;
    Ok(n)
}

fn is_jpeg(d: &[u8]) -> bool {
    d.len() >= 2 && d[0] == 0xFF && d[1] == 0xD8
}

fn is_webp(d: &[u8]) -> bool {
    d.len() >= 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP"
}

fn is_png(d: &[u8]) -> bool {
    d.len() >= 8 && d[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
}

fn is_gif(d: &[u8]) -> bool {
    d.len() >= 13 && (&d[..6] == b"GIF87a" || &d[..6] == b"GIF89a")
}

/// Длина палитры по упакованному байту дескриптора (флаг 0x80, размер 2^(n+1) цветов × 3).
fn gif_palette_len(packed: u8) -> usize {
    if packed & 0x80 != 0 {
        3 << ((packed & 7) + 1)
    } else {
        0
    }
}

/// Конец цепочки подблоков, начатой в `i`: позиция после нулевого подблока. None — обрыв.
fn gif_sub_blocks_end(d: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let n = *d.get(i)? as usize;
        i += 1 + n;
        if n == 0 {
            return Some(i);
        }
        if i > d.len() {
            return None;
        }
    }
}

/// Какие расширения GIF оставить: управление кадром (0xF9), текстовый слой (0x01 — это
/// изображение) и петлю анимации (NETSCAPE2.0 / ANIMEXTS1.0). Комментарии (0xFE) и прочие
/// расширения приложений (так в GIF лежит XMP) — нет.
fn gif_keep_extension(label: u8, body: &[u8]) -> bool {
    match label {
        0xF9 | 0x01 => true,
        0xFF => {
            body.len() >= 12
                && body[0] == 11
                && matches!(&body[1..12], b"NETSCAPE2.0" | b"ANIMEXTS1.0")
        }
        _ => false,
    }
}

/// GIF: пройти по блокам, вырезать комментарии и расширения приложений (кроме петли
/// анимации), всё после трейлера отбросить. Кадры копируются как есть. На обрыве остаток
/// отдаётся без изменений — как у PNG и JPEG.
fn scrub_gif(d: &[u8]) -> Vec<u8> {
    let mut i = 13 + gif_palette_len(d[10]); // заголовок, дескриптор экрана, палитра
    if i > d.len() {
        return d.to_vec();
    }
    let mut out = Vec::with_capacity(d.len());
    out.extend_from_slice(&d[..i]);
    while i < d.len() {
        let end = match d[i] {
            0x3B => {
                out.push(0x3B); // трейлер: хвост за ним декодеру не нужен — его не отдаём
                break;
            }
            0x21 if i + 1 < d.len() => gif_sub_blocks_end(d, i + 2).inspect(|&end| {
                if gif_keep_extension(d[i + 1], &d[i + 2..end]) {
                    out.extend_from_slice(&d[i..end]);
                }
            }),
            // Кадр: дескриптор (10 байт с 0x2C), локальная палитра, байт LZW, подблоки.
            0x2C if i + 10 <= d.len() => {
                let data = i + 10 + gif_palette_len(d[i + 9]) + 1;
                (data <= d.len())
                    .then(|| gif_sub_blocks_end(d, data))
                    .flatten()
                    .inspect(|&end| out.extend_from_slice(&d[i..end]))
            }
            _ => None,
        };
        match end {
            Some(end) => i = end,
            None => {
                out.extend_from_slice(&d[i..]);
                break;
            }
        }
    }
    out
}

/// JPEG: пройти по маркерам, вырезать APP1 (EXIF/XMP), APP13 (IPTC) и COM. APP0
/// (JFIF), APP2 (ICC-цветопрофиль) и прочее — сохранить. После SOS (сжатые данные)
/// ничего не трогаем.
fn scrub_jpeg(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(d.len());
    out.extend_from_slice(&d[..2]); // SOI
    let mut i = 2;
    while i + 1 < d.len() {
        if d[i] != 0xFF {
            out.extend_from_slice(&d[i..]); // не маркер — отдать остаток как есть
            break;
        }
        let marker = d[i + 1];
        if marker == 0xDA {
            out.extend_from_slice(&d[i..]); // SOS: дальше сжатый поток — копируем всё
            break;
        }
        if i + 4 > d.len() {
            out.extend_from_slice(&d[i..]);
            break;
        }
        let len = ((d[i + 2] as usize) << 8) | (d[i + 3] as usize);
        let seg_end = i + 2 + len;
        if len < 2 || seg_end > d.len() {
            out.extend_from_slice(&d[i..]);
            break;
        }
        // APP1 (EXIF/XMP), APP13 (IPTC/Photoshop), COM (комментарий) — вырезаем. От EXIF
        // остаётся поворот кадра: без него снимок, пиксели которого лежат боком, приходит
        // повёрнутым, а сам по себе он ничего о снимавшем не говорит.
        let strip = matches!(marker, 0xE1 | 0xED | 0xFE);
        if !strip {
            out.extend_from_slice(&d[i..seg_end]);
        } else if marker == 0xE1 {
            if let Some(o) = exif_orientation(&d[i + 4..seg_end]).filter(|&o| o != 1) {
                out.extend_from_slice(&orientation_app1(o));
            }
        }
        i = seg_end;
    }
    out
}

/// Поворот кадра (тег 0x0112 в IFD0) из полезной нагрузки APP1; None — не EXIF, тега нет
/// или он вне 1..=8.
fn exif_orientation(p: &[u8]) -> Option<u16> {
    let t = p.strip_prefix(b"Exif\0\0")?;
    let le = match t.get(..4)? {
        b"II\x2A\x00" => true,
        b"MM\x00\x2A" => false,
        _ => return None,
    };
    let u16_at = |i: usize| -> Option<u16> {
        let b: [u8; 2] = t.get(i..i.checked_add(2)?)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |i: usize| -> Option<u32> {
        let b: [u8; 4] = t.get(i..i.checked_add(4)?)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let ifd = u32_at(4)? as usize;
    let n = u16_at(ifd)? as usize;
    (0..n)
        .find_map(|k| {
            let e = ifd.checked_add(2 + k * 12)?;
            (u16_at(e)? == 0x0112 && u16_at(e + 2)? == 3).then(|| u16_at(e + 8))?
        })
        .filter(|o| (1..=8).contains(o))
}

/// APP1 с одним тегом — поворотом кадра (TIFF big-endian, IFD0 из одной записи).
fn orientation_app1(o: u16) -> Vec<u8> {
    let mut s = vec![0xFF, 0xE1, 0x00, 34]; // длина: 2 + 32 байта нагрузки
    s.extend_from_slice(b"Exif\0\0MM\x00\x2A\x00\x00\x00\x08"); // заголовок, IFD0 по смещению 8
    s.extend_from_slice(&[0x00, 0x01, 0x01, 0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01]); // 1 запись: 0x0112, SHORT, 1
    s.extend_from_slice(&o.to_be_bytes());
    s.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // добивка значения и «следующего IFD нет»
    s
}

/// PNG: пройти по чанкам, вырезать метаданные (tEXt/zTXt/iTXt/eXIf/tIME). Критичные
/// и рендер-значимые чанки (IHDR/PLTE/IDAT/IEND/gAMA/sRGB/…) — сохранить.
fn scrub_png(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(d.len());
    out.extend_from_slice(&d[..8]); // сигнатура
    let mut i = 8;
    while i + 8 <= d.len() {
        let len = u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
        let ctype = &d[i + 4..i + 8];
        let chunk_end = i + 12 + len; // len(4) + type(4) + data(len) + crc(4)
        if chunk_end > d.len() {
            out.extend_from_slice(&d[i..]);
            break;
        }
        let strip = matches!(ctype, b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"tIME");
        if !strip {
            out.extend_from_slice(&d[i..chunk_end]);
        }
        let is_end = ctype == b"IEND";
        i = chunk_end;
        if is_end {
            break;
        }
    }
    out
}

/// WebP: RIFF-чанки, вырезать `EXIF` и `XMP `, в `VP8X` снять флаги их наличия (иначе
/// декодер ищет то, чего нет) и пересчитать размер RIFF.
fn scrub_webp(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(d.len());
    out.extend_from_slice(&d[..12]);
    let mut i = 12;
    while i + 8 <= d.len() {
        let kind = &d[i..i + 4];
        let len = u32::from_le_bytes([d[i + 4], d[i + 5], d[i + 6], d[i + 7]]) as usize;
        let end = (i + 8).saturating_add(len).saturating_add(len & 1); // чётное выравнивание
        if end > d.len() {
            out.extend_from_slice(&d[i..]); // обрыв — хвост как есть
            break;
        }
        match kind {
            b"EXIF" | b"XMP " => {}
            b"VP8X" if len >= 1 => {
                let at = out.len() + 8;
                out.extend_from_slice(&d[i..end]);
                out[at] &= !(0x08 | 0x04); // флаги EXIF и XMP
            }
            _ => out.extend_from_slice(&d[i..end]),
        }
        i = end;
    }
    let riff = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JPEG: APP1 (EXIF) вырезается, SOI/SOS/поток сохраняются.
    #[test]
    fn jpeg_strips_exif_app1() {
        let mut jpeg = vec![0xFF, 0xD8]; // SOI
        // APP1: FFE1, len=0x0008 (2 + "Exif\0\0")
        jpeg.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0]);
        // APP0 JFIF (должен сохраниться): FFE0, len=0x0004, 2 байта данных
        jpeg.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46]);
        // SOS + сжатые данные + EOI
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34, 0xFF, 0xD9]);

        let out = scrub("image/jpeg", &jpeg);
        assert_eq!(&out[..2], &[0xFF, 0xD8], "SOI на месте");
        // APP1/EXIF ушёл:
        assert!(!contains(&out, &[0xFF, 0xE1]), "APP1 должен быть вырезан");
        assert!(!contains(&out, b"Exif"), "EXIF-полезная нагрузка ушла");
        // APP0/JFIF и SOS сохранились:
        assert!(contains(&out, &[0xFF, 0xE0]), "APP0/JFIF сохранён");
        assert!(contains(&out, &[0xFF, 0xDA]), "SOS сохранён");
        assert!(contains(&out, &[0xFF, 0xD9]), "EOI сохранён");
        assert!(out.len() < jpeg.len());
    }

    /// JPEG: от EXIF остаётся только поворот кадра — иначе снимок с камеры, у которого
    /// пиксели лежат боком, а поворот записан в EXIF, приходит повёрнутым. Модель,
    /// GPS-указатель и всё прочее уходят.
    #[test]
    fn jpeg_keeps_only_orientation_from_exif() {
        // TIFF little-endian, IFD0 по смещению 8: Make="Xiaomi", Orientation=6, GPS IFD.
        let mut tiff = b"II\x2A\x00\x08\x00\x00\x00".to_vec();
        tiff.extend_from_slice(&3u16.to_le_bytes());
        let make_at = 8 + 2 + 3 * 12 + 4;
        for (tag, typ, count, value) in [
            (0x010Fu16, 2u16, 7u32, make_at as u32),
            (0x0112, 3, 1, 6),
            (0x8825, 4, 1, 0),
        ] {
            tiff.extend_from_slice(&tag.to_le_bytes());
            tiff.extend_from_slice(&typ.to_le_bytes());
            tiff.extend_from_slice(&count.to_le_bytes());
            tiff.extend_from_slice(&value.to_le_bytes());
        }
        tiff.extend_from_slice(&0u32.to_le_bytes()); // следующего IFD нет
        tiff.extend_from_slice(b"Xiaomi\0");
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);

        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&app1);
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34, 0xFF, 0xD9]);

        let out = scrub("image/jpeg", &jpeg);
        assert!(!contains(&out, b"Xiaomi"), "модель телефона ушла");
        let mut want = vec![0xFF, 0xD8];
        want.extend_from_slice(&orientation_app1(6));
        want.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34, 0xFF, 0xD9]);
        assert_eq!(out, want, "вместо EXIF — один тег поворота");
        assert_eq!(jpeg_orientation(&out), Some(6), "поворот читается обратно");
    }

    /// PNG: tEXt вырезается, IHDR/IEND сохраняются.
    #[test]
    fn png_strips_text_chunk() {
        let sig = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let mut png = sig.to_vec();
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0u8; 13]);
        png.extend_from_slice(&[0, 0, 0, 0]); // crc (не проверяется)
        png.extend_from_slice(&5u32.to_be_bytes());
        png.extend_from_slice(b"tEXt");
        png.extend_from_slice(b"hello");
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0, 0, 0, 0]);

        let out = scrub("image/png", &png);
        assert_eq!(&out[..8], &sig, "сигнатура на месте");
        assert!(!contains(&out, b"tEXt"), "tEXt вырезан");
        assert!(!contains(&out, b"hello"), "текст метаданных ушёл");
        assert!(contains(&out, b"IHDR"), "IHDR сохранён");
        assert!(contains(&out, b"IEND"), "IEND сохранён");
        assert_eq!(out.len(), png.len() - (5 + 12), "убран ровно чанк tEXt");
    }

    #[test]
    fn non_image_passthrough() {
        let data = b"just a text file, not an image".to_vec();
        assert_eq!(scrub("text/plain", &data), data);
    }

    fn chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut c = kind.to_vec();
        c.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        c.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            c.push(0);
        }
        c
    }

    /// WebP: EXIF и XMP вырезаны, флаги в VP8X сняты, кадр цел, размер RIFF верный.
    #[test]
    fn webp_strips_exif_and_xmp() {
        let mut body = b"WEBP".to_vec();
        body.extend(chunk(b"VP8X", &[0x0C, 0, 0, 0, 15, 0, 0, 15, 0, 0]));
        body.extend(chunk(b"VP8 ", b"frame-bytes"));
        body.extend(chunk(b"EXIF", b"GPS 55.75N 37.61E"));
        body.extend(chunk(b"XMP ", b"<x:xmpmeta/>"));
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&(body.len() as u32).to_le_bytes());
        webp.extend(body);

        let out = scrub("image/webp", &webp);
        assert!(
            !contains(&out, b"EXIF") && !contains(&out, b"GPS"),
            "EXIF ушёл"
        );
        assert!(
            !contains(&out, b"XMP ") && !contains(&out, b"xmpmeta"),
            "XMP ушёл"
        );
        assert!(contains(&out, b"frame-bytes"), "кадр на месте");
        assert_eq!(out[20], 0, "флаги EXIF/XMP в VP8X сняты");
        let riff = u32::from_le_bytes([out[4], out[5], out[6], out[7]]) as usize;
        assert_eq!(riff, out.len() - 8, "размер RIFF пересчитан");
    }

    /// GIF: комментарий, XMP (расширение приложения) и хвост после трейлера уходят; петля
    /// анимации (NETSCAPE2.0), управление кадром и сам кадр — на месте, байт в байт.
    #[test]
    fn gif_strips_comment_and_xmp_keeps_animation() {
        let head: &[u8] = b"GIF89a\x02\x00\x02\x00\x80\x00\x00"; // 2×2, глобальная палитра 2 цвета
        let palette: &[u8] = &[0, 0, 0, 255, 255, 255];
        let looping: &[u8] = b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00";
        let xmp: &[u8] = b"\x21\xFF\x0BXMP DataXMP\x0C<x:xmpmeta/>\x00";
        let comment: &[u8] = b"\x21\xFE\x11GPS 55.75N 37.61E\x00";
        let control: &[u8] = &[0x21, 0xF9, 0x04, 0x04, 0x0A, 0x00, 0x00, 0x00];
        // Кадр: дескриптор без локальной палитры, затем LZW (минимальный код, подблок, конец).
        let descriptor: &[u8] = &[0x2C, 0, 0, 0, 0, 2, 0, 2, 0, 0x00];
        let lzw: &[u8] = &[0x02, 0x02, 0x44, 0x01, 0x00];
        let frame = [descriptor, lzw].concat();
        let frame = frame.as_slice();
        let mut gif = [head, palette, looping, xmp, comment, control, frame].concat();
        gif.push(0x3B);
        gif.extend_from_slice(b"hidden tail");

        let out = scrub("image/gif", &gif);
        assert!(!contains(&out, b"GPS"), "комментарий ушёл");
        assert!(
            !contains(&out, b"xmpmeta") && !contains(&out, b"XMP DataXMP"),
            "XMP ушёл"
        );
        assert!(!contains(&out, b"hidden tail"), "хвост после трейлера ушёл");
        let want = [head, palette, looping, control, frame, &[0x3B]].concat();
        assert_eq!(out, want, "петля, управление кадром и кадр — байт в байт");
    }

    /// Битые входы любой формы не роняют разбор и не меняют длину ISO BMFF.
    #[test]
    fn garbage_never_panics() {
        use proptest::prelude::*;
        let mut runner = proptest::test_runner::TestRunner::default();
        let heads: [&[u8]; 5] = [b"ftyp", b"moov", b"RIFF", b"\xff\xd8", b"GIF8"];
        runner
            .run(
                &(0usize..5, proptest::collection::vec(any::<u8>(), 0..400)),
                |(h, tail)| {
                    let mut d = vec![0, 0, 0, 24];
                    d.extend_from_slice(heads[h]);
                    if h == 2 {
                        d = b"RIFF\0\0\0\0WEBP".to_vec();
                    }
                    if h == 4 {
                        d = b"GIF89a".to_vec();
                    }
                    d.extend(tail);
                    let out = scrub("x", &d);
                    if crate::bmff::looks_like(&d) {
                        prop_assert_eq!(out.len(), d.len());
                    }
                    Ok(())
                },
            )
            .unwrap();
    }

    /// Поворот из первого APP1 файла (до SOS) — для проверки, что он читается обратно.
    fn jpeg_orientation(d: &[u8]) -> Option<u16> {
        let mut i = 2;
        while i + 4 <= d.len() && d[i] == 0xFF && d[i + 1] != 0xDA {
            let end = i + 2 + (((d[i + 2] as usize) << 8) | d[i + 3] as usize);
            if d[i + 1] == 0xE1 {
                return exif_orientation(d.get(i + 4..end)?);
            }
            i = end;
        }
        None
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }
}
