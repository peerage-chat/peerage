//! Формат медиа-пакета (внутри запечатанной датаграммы) и видео-фрагмента.
//!
//! Шифрование, аутентичность отправителя и байт канала снаружи — забота
//! вызывающего (в Peerage — `messenger` и сессия 1:1, SEC-4). Сюда приходит уже
//! расшифрованный пакет:
//! ```text
//! байт 0      : MEDIA_V1 (0x02) — маркер формата; в v2 станет байтом версии и типа
//! байт 1      : track (тип потока: аудио/видео/экран)
//! байты 2..4  : seq   (u16, big-endian) — для детекции потерь/переупорядочивания
//! байты 4..8  : ts    (u32, big-endian) — метка времени в единицах кодека
//! байты 8..   : payload (например, кадр Opus)
//! ```
//!
//! Формат зафиксирован тестами `golden_*` байт в байт: старые и новые сборки
//! должны слышать друг друга, пока не введён v2 (ADR-014, шаг 4).

/// Маркер формата v1 — байт 0 медиа-пакета. Исторически это байт канала
/// `CHAN_MEDIA`, продублированный внутри шифротекста; совпадение значения
/// сохраняем ради совместимости по проводу.
pub const MEDIA_V1: u8 = 0x02;

/// Длина медиа-заголовка (включая маркер формата).
pub const MEDIA_HEADER_LEN: usize = 8;

/// Лимит payload-а медиа-пакета. Сеть (в Peerage — Yggdrasil) сама фрагментирует на IP-уровне,
/// поэтому мы можем отдавать крупные куски и снижать cgo-оверхед на пакет.
/// 60000 байт = ~15-25 фрагментов на FHD-кадр вместо ~150+ при старом MTU.
pub const MAX_MEDIA_PAYLOAD: usize = 60_000;

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
    out[0] = MEDIA_V1;
    out[1] = track;
    out[2..4].copy_from_slice(&seq.to_be_bytes());
    out[4..8].copy_from_slice(&ts.to_be_bytes());
}

/// Разобрать медиа-пакет. Возвращает None, если маркер не тот или пакет
/// короче заголовка.
#[inline]
pub fn parse_media(dg: &[u8]) -> Option<MediaFrame<'_>> {
    if dg.len() < MEDIA_HEADER_LEN || dg[0] != MEDIA_V1 {
        return None;
    }
    Some(MediaFrame {
        track: dg[1],
        seq: u16::from_be_bytes([dg[2], dg[3]]),
        ts: u32::from_be_bytes([dg[4], dg[5], dg[6], dg[7]]),
        payload: &dg[MEDIA_HEADER_LEN..],
    })
}

// ---- Видео: фрагментация кадров (кадр > MTU) ----
//
// Видео-кадр (H.264) обычно больше одной датаграммы, поэтому режется на чанки.
// Каждый чанк передаётся как медиа-payload (track = VIDEO/SCREEN) со своим
// суб-заголовком, описывающим принадлежность к кадру и порядок:
// ```text
// байты 0..4 : frame_id (u32 BE) — номер кадра (группирует фрагменты)
// байты 4..6 : frag_idx (u16 BE) — индекс фрагмента в кадре
// байты 6..8 : frag_count (u16 BE) — всего фрагментов в кадре
// байт  8    : flags (бит 0 — ключевой кадр)
// байты 9..  : кусок данных кадра
// ```

/// Длина видео-суб-заголовка.
pub const VIDEO_HDR: usize = 9;
/// Флаг ключевого кадра.
pub const VIDEO_KEYFRAME: u8 = 0x01;

/// Максимум данных кадра в одном фрагменте. Сеть сама фрагментирует на IP-уровне,
/// поэтому берём крупные куски для снижения cgo-оверхеда. 8000 байт ≈ 6-7 IP-пакетов.
pub const MAX_VIDEO_CHUNK: usize = 8_000;

/// Больше фрагментов в кадре не принимаем: 8 МБ при [`MAX_VIDEO_CHUNK`] — с запасом
/// на ключевой кадр 4K. Без предела участник звонка объявил бы кадр в 65 535
/// фрагментов, и мы копили бы его до полугигабайта.
pub const MAX_VIDEO_FRAGS: u16 = 1024;

/// Разбить кадр на медиа-payload-ы (суб-заголовок + кусок), готовые к отправке
/// как медиа-датаграммы (track = VIDEO/SCREEN). Пустой кадр → один пустой фрагмент.
pub fn fragment_video(frame: &[u8], frame_id: u32, keyframe: bool) -> Vec<Vec<u8>> {
    let flags = if keyframe { VIDEO_KEYFRAME } else { 0 };
    let chunks: Vec<&[u8]> = if frame.is_empty() {
        vec![&[]]
    } else {
        frame.chunks(MAX_VIDEO_CHUNK).collect()
    };
    let count = chunks.len() as u16;
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            let mut out = Vec::with_capacity(VIDEO_HDR + chunk.len());
            out.extend_from_slice(&frame_id.to_be_bytes());
            out.extend_from_slice(&(i as u16).to_be_bytes());
            out.extend_from_slice(&count.to_be_bytes());
            out.push(flags);
            out.extend_from_slice(chunk);
            out
        })
        .collect()
}

/// Разобранный видео-фрагмент (из медиа-payload).
#[derive(Debug)]
pub struct VideoFrag<'a> {
    pub frame_id: u32,
    pub frag_idx: u16,
    pub frag_count: u16,
    pub keyframe: bool,
    pub data: &'a [u8],
}

/// Разобрать видео-фрагмент из медиа-payload (то, что вернул [`parse_media`] как payload).
pub fn parse_video_frag(payload: &[u8]) -> Option<VideoFrag<'_>> {
    if payload.len() < VIDEO_HDR {
        return None;
    }
    let frame_id = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let frag_idx = u16::from_be_bytes([payload[4], payload[5]]);
    let frag_count = u16::from_be_bytes([payload[6], payload[7]]);
    let keyframe = payload[8] & VIDEO_KEYFRAME != 0;
    Some(VideoFrag {
        frame_id,
        frag_idx,
        frag_count,
        keyframe,
        data: &payload[VIDEO_HDR..],
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
        assert!(parse_media(&[0x01, 1, 2, 3, 4, 5, 6, 7, 8]).is_none()); // байт чата, не медиа
        assert!(parse_media(&[MEDIA_V1, 0, 0]).is_none()); // короче заголовка
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

    #[test]
    fn empty_frame_one_fragment() {
        let frags = fragment_video(&[], 5, false);
        assert_eq!(frags.len(), 1);
        let f = parse_video_frag(&frags[0]).unwrap();
        assert_eq!(f.frag_count, 1);
        assert_eq!(f.data.len(), 0);
    }

    /// Формат v1 на проводе — байт в байт. Меняется только вместе с версией
    /// (ADR-014, шаг 4); иначе старые и новые сборки перестанут слышать друг друга.
    #[test]
    fn golden_media_header() {
        let mut buf = [0u8; MEDIA_HEADER_LEN];
        write_media_header(&mut buf, TRACK_VIDEO, 0x0102, 0x0A0B_0C0D);
        assert_eq!(buf, [0x02, 0x01, 0x01, 0x02, 0x0A, 0x0B, 0x0C, 0x0D]);
    }

    #[test]
    fn golden_video_fragments() {
        let frame: Vec<u8> = (0..MAX_VIDEO_CHUNK + 3).map(|i| i as u8).collect();
        let frags = fragment_video(&frame, 0x0102_0304, true);
        assert_eq!(frags.len(), 2);
        assert_eq!(
            frags[0][..VIDEO_HDR],
            [0x01, 0x02, 0x03, 0x04, 0, 0, 0, 2, VIDEO_KEYFRAME]
        );
        assert_eq!(frags[0].len(), VIDEO_HDR + MAX_VIDEO_CHUNK);
        assert_eq!(
            frags[1][..VIDEO_HDR],
            [0x01, 0x02, 0x03, 0x04, 0, 1, 0, 2, VIDEO_KEYFRAME]
        );
        assert_eq!(&frags[1][VIDEO_HDR..], &frame[MAX_VIDEO_CHUNK..]);
        assert_eq!(
            (MAX_VIDEO_CHUNK, MAX_MEDIA_PAYLOAD, VIDEO_HDR),
            (8_000, 60_000, 9)
        );
    }
}
