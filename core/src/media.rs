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

/// Лимит payload-а медиа-датаграммы. Yggdrasil сам фрагментирует на IP-уровне,
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

use std::collections::HashMap;

/// Длина видео-суб-заголовка.
pub const VIDEO_HDR: usize = 9;
/// Флаг ключевого кадра.
pub const VIDEO_KEYFRAME: u8 = 0x01;

/// Максимум данных кадра в одном фрагменте. Yggdrasil сам фрагментирует на IP-уровне,
/// поэтому берём крупные куски для снижения cgo-оверхеда. 8000 байт ≈ 6-7 IP-пакетов.
pub const MAX_VIDEO_CHUNK: usize = 8_000;

/// Больше фрагментов в кадре не принимаем: 8 МБ при [`MAX_VIDEO_CHUNK`] — с запасом
/// на ключевой кадр 4K. Без предела участник звонка объявил бы кадр в 65 535
/// фрагментов, и мы копили бы его до полугигабайта.
pub const MAX_VIDEO_FRAGS: u16 = 1024;

/// Сколько байт недособранных кадров держим всего: дальше выбрасываем самые старые.
const MAX_PENDING_BYTES: usize = 16 << 20;

/// Номер кадра позади последнего доставленного не дальше этого окна — опоздавший
/// кадр, его выбрасываем. Дальше — отправитель начал счёт заново (перезашёл в
/// комнату) или счётчик обернулся: это новый поток, его принимаем. Раньше
/// сравнение было простым `<=`, и после обнуления счётчика у отправителя (65 536
/// кадров — 36 минут при 30 fps — или перезаход) видео замерзало до конца звонка.
const STALE_WINDOW: u32 = 1024;

/// Кадр `id` уже доставлен или опоздал относительно последнего доставленного `last`.
fn is_stale(id: u32, last: u32) -> bool {
    last.wrapping_sub(id) < STALE_WINDOW
}

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

/// Собранный целый видео-кадр.
pub struct CompleteFrame {
    pub frame_id: u32,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

struct FrameAcc {
    count: u16,
    keyframe: bool,
    parts: Vec<Option<Vec<u8>>>,
    received: u16,
}

/// Собирает целые кадры из фрагментов: переупорядочивает, отбрасывает поздние и
/// неполные (когда приходит более новый кадр). Без фоновых потоков.
#[derive(Default)]
pub struct VideoReassembler {
    last_delivered: Option<u32>,
    frames: HashMap<u32, FrameAcc>,
    /// Байты во всех недособранных кадрах (для [`MAX_PENDING_BYTES`]).
    pending: usize,
}

impl VideoReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Принять фрагмент. Возвращает кадр, когда он полностью собран.
    pub fn push(&mut self, frag: VideoFrag<'_>) -> Option<CompleteFrame> {
        // Уже доставленные/опоздавшие кадры игнорируем.
        if self.last_delivered.is_some_and(|last| is_stale(frag.frame_id, last)) {
            return None;
        }
        if frag.frag_count == 0 || frag.frag_count > MAX_VIDEO_FRAGS || frag.frag_idx >= frag.frag_count {
            return None;
        }

        let acc = self.frames.entry(frag.frame_id).or_insert_with(|| FrameAcc {
            count: frag.frag_count,
            keyframe: frag.keyframe,
            parts: vec![None; frag.frag_count as usize],
            received: 0,
        });
        // Тот же номер кадра с другим числом фрагментов — мусор, не наш кадр.
        if acc.count != frag.frag_count {
            return None;
        }
        let slot = &mut acc.parts[frag.frag_idx as usize];
        if slot.is_none() {
            *slot = Some(frag.data.to_vec());
            acc.received += 1;
            self.pending += frag.data.len();
        }

        if acc.received == acc.count {
            let acc = self.take(frag.frame_id)?;
            let mut data = Vec::with_capacity(acc.parts.iter().flatten().map(Vec::len).sum());
            for part in acc.parts.into_iter().flatten() {
                data.extend_from_slice(&part);
            }
            self.last_delivered = Some(frag.frame_id);
            // Неполные кадры до доставленного уже не соберутся — выбрасываем.
            let stale: Vec<u32> =
                self.frames.keys().copied().filter(|&id| is_stale(id, frag.frame_id)).collect();
            for id in stale {
                self.take(id);
            }
            return Some(CompleteFrame { frame_id: frag.frame_id, keyframe: acc.keyframe, data });
        }
        // Предохранители от роста: по числу кадров и по байтам.
        while self.frames.len() > 16 || self.pending > MAX_PENDING_BYTES {
            let newest = frag.frame_id;
            let Some(&oldest) = self.frames.keys().max_by_key(|&&id| newest.wrapping_sub(id)) else {
                break;
            };
            self.take(oldest);
        }
        None
    }

    /// Убрать кадр из сборки, списав его байты.
    fn take(&mut self, id: u32) -> Option<FrameAcc> {
        let acc = self.frames.remove(&id)?;
        self.pending -= acc.parts.iter().flatten().map(Vec::len).sum::<usize>();
        Some(acc)
    }
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

    fn make_frame(n: usize, fill: u8) -> Vec<u8> {
        vec![fill; n]
    }

    #[test]
    fn video_fragment_reassemble_in_order() {
        let frame = make_frame(MAX_VIDEO_CHUNK * 2 + 100, 0xAB);
        let frags = fragment_video(&frame, 1, true);
        assert!(frags.len() > 1, "большой кадр должен разрезаться");

        let mut r = VideoReassembler::new();
        let mut got = None;
        for f in &frags {
            got = r.push(parse_video_frag(f).unwrap());
        }
        let cf = got.expect("кадр должен собраться");
        assert_eq!(cf.data, frame);
        assert!(cf.keyframe);
        assert_eq!(cf.frame_id, 1);
    }

    #[test]
    fn video_reassemble_out_of_order() {
        let frame = make_frame(MAX_VIDEO_CHUNK * 2, 7);
        let mut frags: Vec<Vec<u8>> = fragment_video(&frame, 10, false);
        assert!(frags.len() > 1, "кадр должен быть многофрагментным для теста");
        frags.reverse(); // обратный порядок прихода

        let mut r = VideoReassembler::new();
        let mut got = None;
        for f in &frags {
            if let Some(c) = r.push(parse_video_frag(f).unwrap()) {
                got = Some(c);
            }
        }
        assert_eq!(got.unwrap().data, frame);
    }

    #[test]
    fn incomplete_frame_dropped_when_newer_completes() {
        let f1 = fragment_video(&make_frame(MAX_VIDEO_CHUNK * 2 + 100, 1), 1, false); // потеряем фрагмент
        let f2 = fragment_video(&make_frame(500, 2), 2, true);

        let mut r = VideoReassembler::new();
        // кадр 1 — пропускаем первый фрагмент (потеря) → не соберётся
        for f in f1.iter().skip(1) {
            assert!(r.push(parse_video_frag(f).unwrap()).is_none());
        }
        // кадр 2 — полный → собирается; кадр 1 отбрасывается
        let mut got = None;
        for f in &f2 {
            if let Some(c) = r.push(parse_video_frag(f).unwrap()) {
                got = Some(c);
            }
        }
        let cf = got.expect("кадр 2 собран");
        assert_eq!(cf.frame_id, 2);
        // запоздавший фрагмент кадра 1 теперь игнорируется
        assert!(r.push(parse_video_frag(&f1[0]).unwrap()).is_none());
    }

    /// Счётчик кадров у отправителя обернулся — видео идёт дальше, а не замерзает.
    #[test]
    fn frame_counter_wrap_keeps_video_flowing() {
        let mut r = VideoReassembler::new();
        for id in [u32::MAX - 1, u32::MAX, 0, 1] {
            let f = fragment_video(&make_frame(100, 1), id, false);
            assert!(r.push(parse_video_frag(&f[0]).unwrap()).is_some(), "кадр {id} принят");
        }
    }

    /// Собеседник перезашёл в комнату и начал счёт кадров с нуля — это новый
    /// поток, а не «старые кадры». Раньше видео стояло до номера, где он вышел.
    #[test]
    fn sender_restart_from_zero_is_accepted() {
        let mut r = VideoReassembler::new();
        let f = fragment_video(&make_frame(100, 1), 50_000, true);
        assert!(r.push(parse_video_frag(&f[0]).unwrap()).is_some());
        let f = fragment_video(&make_frame(100, 2), 0, true);
        assert!(r.push(parse_video_frag(&f[0]).unwrap()).is_some(), "новый поток с нуля принят");
        // А действительно опоздавший кадр по-прежнему выбрасывается.
        let late = fragment_video(&make_frame(100, 3), u32::MAX - 3, false);
        assert!(r.push(parse_video_frag(&late[0]).unwrap()).is_none());
    }

    /// Фрагмент с заголовком из `id`, индекса и числа фрагментов (данные — 8 КБ).
    fn raw_frag(id: u32, idx: u16, count: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(&idx.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out.push(0);
        out.extend_from_slice(&[0u8; MAX_VIDEO_CHUNK]);
        out
    }

    /// Участник звонка не может заставить нас копить память: кадр «на 65 535
    /// фрагментов» не принимается, а недособранное ограничено по байтам.
    #[test]
    fn reassembler_memory_is_bounded() {
        let mut r = VideoReassembler::new();
        assert!(r.push(parse_video_frag(&raw_frag(1, 0, u16::MAX)).unwrap()).is_none());
        assert_eq!(r.pending, 0, "кадр сверх MAX_VIDEO_FRAGS не копится");

        // Много больших кадров, ни один не дособирается.
        for id in 0..16u32 {
            for idx in 0..MAX_VIDEO_FRAGS - 1 {
                r.push(parse_video_frag(&raw_frag(id, idx, MAX_VIDEO_FRAGS)).unwrap());
            }
        }
        assert!(r.pending <= MAX_PENDING_BYTES, "недособранного {} байт", r.pending);
        let real: usize = r.frames.values().flat_map(|a| a.parts.iter().flatten()).map(Vec::len).sum();
        assert_eq!(real, r.pending, "учёт байт сходится");
    }

    #[test]
    fn empty_frame_one_fragment() {
        let frags = fragment_video(&[], 5, false);
        assert_eq!(frags.len(), 1);
        let f = parse_video_frag(&frags[0]).unwrap();
        assert_eq!(f.frag_count, 1);
        assert_eq!(f.data.len(), 0);
    }
}
