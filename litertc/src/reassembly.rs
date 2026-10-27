//! Сборка видео-кадров из фрагментов [`crate::wire`]: переупорядочивание,
//! выброс опоздавших и неполных кадров, жёсткие пределы памяти (вход — от
//! участника звонка, который может быть враждебен).

use std::collections::HashMap;

use crate::wire::{VideoFrag, MAX_VIDEO_FRAGS};

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
        if self
            .last_delivered
            .is_some_and(|last| is_stale(frag.frame_id, last))
        {
            return None;
        }
        if frag.frag_count == 0
            || frag.frag_count > MAX_VIDEO_FRAGS
            || frag.frag_idx >= frag.frag_count
        {
            return None;
        }

        let acc = self
            .frames
            .entry(frag.frame_id)
            .or_insert_with(|| FrameAcc {
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
            let stale: Vec<u32> = self
                .frames
                .keys()
                .copied()
                .filter(|&id| is_stale(id, frag.frame_id))
                .collect();
            for id in stale {
                self.take(id);
            }
            return Some(CompleteFrame {
                frame_id: frag.frame_id,
                keyframe: acc.keyframe,
                data,
            });
        }
        // Предохранители от роста: по числу кадров и по байтам.
        while self.frames.len() > 16 || self.pending > MAX_PENDING_BYTES {
            let newest = frag.frame_id;
            let Some(&oldest) = self
                .frames
                .keys()
                .max_by_key(|&&id| newest.wrapping_sub(id))
            else {
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
    use crate::wire::{fragment_video, parse_video_frag, MAX_VIDEO_CHUNK};

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
        assert!(
            frags.len() > 1,
            "кадр должен быть многофрагментным для теста"
        );
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
            assert!(
                r.push(parse_video_frag(&f[0]).unwrap()).is_some(),
                "кадр {id} принят"
            );
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
        assert!(
            r.push(parse_video_frag(&f[0]).unwrap()).is_some(),
            "новый поток с нуля принят"
        );
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
        assert!(r
            .push(parse_video_frag(&raw_frag(1, 0, u16::MAX)).unwrap())
            .is_none());
        assert_eq!(r.pending, 0, "кадр сверх MAX_VIDEO_FRAGS не копится");

        // Много больших кадров, ни один не дособирается.
        for id in 0..16u32 {
            for idx in 0..MAX_VIDEO_FRAGS - 1 {
                r.push(parse_video_frag(&raw_frag(id, idx, MAX_VIDEO_FRAGS)).unwrap());
            }
        }
        assert!(
            r.pending <= MAX_PENDING_BYTES,
            "недособранного {} байт",
            r.pending
        );
        let real: usize = r
            .frames
            .values()
            .flat_map(|a| a.parts.iter().flatten())
            .map(Vec::len)
            .sum();
        assert_eq!(real, r.pending, "учёт байт сходится");
    }
}
