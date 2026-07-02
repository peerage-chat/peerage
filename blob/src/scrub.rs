//! Снятие приватных метаданных с изображений перед отправкой (веха M2, PET-ось).
//!
//! Работает **без перекодирования** (без потери качества, без тяжёлых зависимостей):
//! вырезает сегменты/чанки с метаданными из JPEG и PNG. Снимает стандартный
//! приватный слой — **EXIF/GPS, XMP, IPTC, текстовые комментарии, время**.
//!
//! Честный предел: это не полный ре-энкод. Стандартные EXIF/GPS-геометки уходят
//! гарантированно; экзотический эмбеддер в нестандартном сегменте может проскочить.
//! Полная зачистка ре-энкодом — отдельный опциональный слой (тяжёлая image-зависимость).
//! Неизвестные форматы возвращаются без изменений (best-effort).

/// Снять метаданные с изображения по mime/содержимому. Возвращает очищенные байты
/// (или исходные, если формат не распознан). Вызывать ДО [`crate::build`].
pub fn scrub(_mime: &str, data: &[u8]) -> Vec<u8> {
    if is_jpeg(data) {
        scrub_jpeg(data)
    } else if is_png(data) {
        scrub_png(data)
    } else {
        data.to_vec()
    }
}

fn is_jpeg(d: &[u8]) -> bool {
    d.len() >= 2 && d[0] == 0xFF && d[1] == 0xD8
}

fn is_png(d: &[u8]) -> bool {
    d.len() >= 8 && d[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
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
        // APP1 (EXIF/XMP), APP13 (IPTC/Photoshop), COM (комментарий) — вырезаем.
        let strip = matches!(marker, 0xE1 | 0xED | 0xFE);
        if !strip {
            out.extend_from_slice(&d[i..seg_end]);
        }
        i = seg_end;
    }
    out
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

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }
}
