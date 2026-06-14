//! Лёгкое обрамление медиа-датаграмм (голос/видео/экран).
//!
//! Все датаграммы приложения начинаются с 1 байта канала: [`CHAN_CHAT`] или
//! [`CHAN_MEDIA`] — это позволяет мультиплексировать надёжный чат и реалтайм-медиа
//! по одному пути Yggdrasil без отдельного сокета.
//!
//! Медиа-кадр (минимальный оверхед, без JSON/крипто на пакет — конфиденциальность
//! и аутентичность источника обеспечивает E2E-шифрование транспорта Yggdrasil):
//! ```text
//! байт 0      : CHAN_MEDIA (0x02)
//! байт 1      : track (тип потока: аудио/видео/экран)
//! байты 2..4  : seq   (u16, big-endian) — для детекции потерь/переупорядочивания
//! байты 4..8  : ts    (u32, big-endian) — метка времени в единицах кодека
//! байты 8..   : payload (например, кадр Opus)
//! ```

/// Канал надёжного чата (далее — JSON-конверт протокола).
pub const CHAN_CHAT: u8 = 0x01;
/// Канал реалтайм-медиа (далее — бинарный медиа-кадр).
pub const CHAN_MEDIA: u8 = 0x02;

/// Длина медиа-заголовка (включая байт канала).
pub const MEDIA_HEADER_LEN: usize = 8;

/// Консервативный лимит payload-а медиа-датаграммы, чтобы избегать IP-фрагментации
/// на худшем хопе (IPv6 min MTU 1280 − заголовок).
pub const MAX_MEDIA_PAYLOAD: usize = 1280 - MEDIA_HEADER_LEN;

/// Идентификаторы потоков (track).
pub const TRACK_AUDIO: u8 = 0;
pub const TRACK_VIDEO: u8 = 1;
pub const TRACK_SCREEN: u8 = 2;

/// Разобранный медиа-кадр (заголовок + срез payload без копирования).
#[derive(Debug, PartialEq, Eq)]
pub struct MediaFrame<'a> {
    pub track: u8,
    pub seq: u16,
    pub ts: u32,
    pub payload: &'a [u8],
}

/// Записать 8-байтный медиа-заголовок в начало `out` (нужно `out.len() >= 8`).
#[inline]
pub fn write_media_header(out: &mut [u8], track: u8, seq: u16, ts: u32) {
    out[0] = CHAN_MEDIA;
    out[1] = track;
    out[2..4].copy_from_slice(&seq.to_be_bytes());
    out[4..8].copy_from_slice(&ts.to_be_bytes());
}

/// Разобрать медиа-датаграмму. Возвращает None, если это не медиа-канал или
/// датаграмма короче заголовка.
#[inline]
pub fn parse_media(dg: &[u8]) -> Option<MediaFrame<'_>> {
    if dg.len() < MEDIA_HEADER_LEN || dg[0] != CHAN_MEDIA {
        return None;
    }
    Some(MediaFrame {
        track: dg[1],
        seq: u16::from_be_bytes([dg[2], dg[3]]),
        ts: u32::from_be_bytes([dg[4], dg[5], dg[6], dg[7]]),
        payload: &dg[MEDIA_HEADER_LEN..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let mut buf = [0u8; MEDIA_HEADER_LEN + 4];
        write_media_header(&mut buf, TRACK_AUDIO, 0xBEEF, 0x0102_0304);
        buf[MEDIA_HEADER_LEN..].copy_from_slice(&[1, 2, 3, 4]);

        let f = parse_media(&buf).unwrap();
        assert_eq!(f.track, TRACK_AUDIO);
        assert_eq!(f.seq, 0xBEEF);
        assert_eq!(f.ts, 0x0102_0304);
        assert_eq!(f.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn parse_rejects_non_media_and_short() {
        assert!(parse_media(&[CHAN_CHAT, 1, 2, 3, 4, 5, 6, 7, 8]).is_none());
        assert!(parse_media(&[CHAN_MEDIA, 0, 0]).is_none()); // короче заголовка
        assert!(parse_media(&[]).is_none());
    }

    #[test]
    fn seq_wraps_as_u16() {
        let mut buf = [0u8; MEDIA_HEADER_LEN];
        write_media_header(&mut buf, TRACK_VIDEO, u16::MAX, 0);
        let f = parse_media(&buf).unwrap();
        assert_eq!(f.seq, u16::MAX);
        assert_eq!(f.payload.len(), 0);
    }
}
