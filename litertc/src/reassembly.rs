//! Сборка видео-кадров из фрагментов [`crate::wire`] и выдача их декодеру по порядку.
//!
//! Декодер H.264 опирается на прошлый кадр: P-кадр без предшественника даёт кашу до
//! следующего ключевого. Поэтому сборщик отдаёт кадры строго подряд. Если следующего
//! нет, а более новые уже собраны, он ждёт недостающий (его фрагменты повторяются по
//! NACK) не дольше `max_wait`; не дождался — декодер потерял опору, и до ключевого
//! кадра отдавать нечего ([`VideoReassembler::wants_keyframe`]: просим его у
//! отправителя). Собранный ключевой кадр новее ожидаемого отдаётся сразу: всё, что
//! до него, уже не нужно.
//!
//! Вход — от участника звонка, который может быть враждебен: память ограничена по
//! кадрам и по байтам.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::wire::{VideoFrag, VideoInfo, MAX_VIDEO_FRAGS};

/// Сколько байт несобранных и ждущих очереди кадров держим всего: дальше выбрасываем
/// самые старые.
const MAX_PENDING_BYTES: usize = 16 << 20;

/// Сколько кадров держим: за время ожидания недостающего (до полсекунды) при 60 fps
/// набегает 30 собранных следом.
const MAX_FRAMES: usize = 64;

/// `a` раньше `b` по кругу u32 (половина круга вперёд — новее): счётчик кадров может
/// обернуться. Перезапуск отправителя сюда не попадает — это новый поток (новый
/// `stream_id`), и сессия заводит для него новый сборщик.
fn before(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

/// Собранный целый видео-кадр.
pub struct CompleteFrame {
    pub frame_id: u32,
    /// Метка времени захвата (из медиа-заголовка фрагментов).
    pub ts: u32,
    pub info: VideoInfo,
    pub data: Vec<u8>,
}

struct FrameAcc {
    count: u16,
    ts: u32,
    info: VideoInfo,
    parts: Vec<Option<Vec<u8>>>,
    received: u16,
}

impl FrameAcc {
    fn complete(&self) -> bool {
        self.received == self.count
    }
}

/// Собирает кадры из фрагментов и выдаёт их по порядку. Без своих потоков и часов:
/// время приходит в [`VideoReassembler::pop`].
pub struct VideoReassembler {
    /// Номер кадра, который декодер ждёт следующим; `None` — ещё ничего не отдали.
    next: Option<u32>,
    /// Опоры у декодера нет (начало потока или кадр потерян): годится только ключевой.
    need_key: bool,
    /// Без ключевого кадра дальше не пойти: собранный кадр пришлось выбросить или
    /// недостающий не дождались. Пока поток только начался и ключевой ещё в пути —
    /// не застряли, просить нечего.
    stuck: bool,
    /// Ключевой кадр ждём, но не просим ([`Self::skip_to_keyframe`] без запроса).
    quiet: bool,
    frames: HashMap<u32, FrameAcc>,
    /// Байты во всех кадрах, что держим (для [`MAX_PENDING_BYTES`]).
    pending: usize,
    /// С какого момента следующего кадра нет, а более новый уже собран.
    waiting_since: Option<Instant>,
}

impl Default for VideoReassembler {
    fn default() -> Self {
        VideoReassembler {
            next: None,
            need_key: true,
            stuck: false,
            quiet: false,
            frames: HashMap::new(),
            pending: 0,
            waiting_since: None,
        }
    }
}

impl VideoReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Принять фрагмент. Кадры забираются [`Self::pop`].
    /// Фрагмент кадра; `ts` — метка времени из его медиа-заголовка.
    pub fn push(&mut self, frag: VideoFrag<'_>, ts: u32) {
        // Кадр уже отдан или пропущен — опоздал.
        if self.next.is_some_and(|next| before(frag.frame_id, next)) {
            return;
        }
        if frag.frag_count == 0
            || frag.frag_count > MAX_VIDEO_FRAGS
            || frag.frag_idx >= frag.frag_count
        {
            return;
        }
        let acc = self
            .frames
            .entry(frag.frame_id)
            .or_insert_with(|| FrameAcc {
                count: frag.frag_count,
                ts,
                info: frag.info,
                parts: vec![None; frag.frag_count as usize],
                received: 0,
            });
        // Тот же номер кадра с другим числом фрагментов — мусор, не наш кадр.
        if acc.count != frag.frag_count {
            return;
        }
        let slot = &mut acc.parts[frag.frag_idx as usize];
        if slot.is_none() {
            *slot = Some(frag.data.to_vec());
            acc.received += 1;
            self.pending += frag.data.len();
        }
        // Предохранители от роста: по числу кадров и по байтам — выбрасываем самые
        // старые. Если среди них ожидаемый, его не дождаться: это решит `pop` по
        // таймауту ожидания.
        while self.frames.len() > MAX_FRAMES || self.pending > MAX_PENDING_BYTES {
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
    }

    /// Следующий кадр для декодера, если его пора отдать. `max_wait` — сколько ждать
    /// недостающий кадр, когда более новые уже собраны (около двух RTT: столько идёт
    /// повтор по NACK).
    pub fn pop(&mut self, now: Instant, max_wait: Duration) -> Option<CompleteFrame> {
        loop {
            let Some(next) = self.next.filter(|_| !self.need_key) else {
                // Опоры нет: годится только ключевой кадр, самый свежий из собранных.
                if let Some(key) = self.newest(|a| a.complete() && a.info.keyframe) {
                    return self.deliver(key);
                }
                // Ключевой ещё собирается (его фрагменты повторяются по NACK): кадры за
                // ним держим — с ним они станут декодируемы. Что до него и что собрано
                // без него — выбрасываем.
                let pending = self.newest(|a| a.info.keyframe);
                let useless: Vec<u32> = self
                    .frames
                    .iter()
                    .filter(|(&id, a)| pending.map_or(a.complete(), |k| before(id, k)))
                    .map(|(&id, _)| id)
                    .collect();
                for id in useless {
                    if self.take(id).is_some_and(|a| a.complete()) {
                        self.stuck |= !self.quiet;
                    }
                }
                let Some(pending) = pending else {
                    self.waiting_since = None;
                    return None;
                };
                // Ждём его не дольше `max_wait`; не дождались — он и всё, что за ним
                // собрано, не пригодится, просим новый.
                let since = *self.waiting_since.get_or_insert(now);
                if now.saturating_duration_since(since) < max_wait {
                    return None;
                }
                let lost: Vec<u32> = self
                    .frames
                    .iter()
                    .filter(|(&id, a)| id == pending || before(pending, id) && !a.info.keyframe)
                    .map(|(&id, _)| id)
                    .collect();
                for id in lost {
                    self.take(id);
                }
                self.stuck |= !self.quiet;
                self.waiting_since = None;
                continue;
            };
            if self.frames.get(&next).is_some_and(FrameAcc::complete) {
                return self.deliver(next);
            }
            // Собранный ключевой кадр новее ожидаемого: ждать незачем, перескакиваем.
            if let Some(key) = self.oldest_complete_after(next, |a| a.info.keyframe) {
                return self.deliver(key);
            }
            // Следующего нет, а за ним уже собранные — ждём повтора, но не вечно.
            if self.oldest_complete_after(next, |_| true).is_none() {
                self.waiting_since = None;
                return None;
            }
            let since = *self.waiting_since.get_or_insert(now);
            if now.saturating_duration_since(since) < max_wait {
                return None;
            }
            self.need_key = true;
            self.stuck = true;
            self.waiting_since = None;
        }
    }

    /// Когда [`Self::pop`] перестанет ждать недостающий кадр — для таймера сессии.
    pub fn deadline(&self, max_wait: Duration) -> Option<Instant> {
        self.waiting_since.map(|t| t + max_wait)
    }

    /// Декодеру не на что опереться — нужен ключевой кадр от отправителя.
    pub fn wants_keyframe(&self) -> bool {
        self.stuck
    }

    /// Дальше — только с ключевого кадра (декодер начат заново или его очередь
    /// пришлось выбросить). `request` — попросить его у отправителя; без запроса
    /// ждём очередного.
    pub fn skip_to_keyframe(&mut self, request: bool) {
        self.need_key = true;
        self.stuck = request;
        self.quiet = !request;
        self.waiting_since = None;
    }

    fn newest(&self, pred: impl Fn(&FrameAcc) -> bool) -> Option<u32> {
        self.frames
            .iter()
            .filter(|(_, a)| pred(a))
            .map(|(&id, _)| id)
            .reduce(|a, b| if before(a, b) { b } else { a })
    }

    fn oldest_complete_after(&self, next: u32, pred: impl Fn(&FrameAcc) -> bool) -> Option<u32> {
        self.frames
            .iter()
            .filter(|(&id, a)| before(next, id) && a.complete() && pred(a))
            .map(|(&id, _)| id)
            .reduce(|a, b| if before(a, b) { a } else { b })
    }

    /// Отдать собранный кадр `id`; всё, что до него, выбросить.
    fn deliver(&mut self, id: u32) -> Option<CompleteFrame> {
        let acc = self.take(id)?;
        let stale: Vec<u32> = self
            .frames
            .keys()
            .copied()
            .filter(|&f| before(f, id))
            .collect();
        for f in stale {
            self.take(f);
        }
        self.next = Some(id.wrapping_add(1));
        self.need_key = false;
        self.stuck = false;
        self.quiet = false;
        self.waiting_since = None;
        let mut data = Vec::with_capacity(acc.parts.iter().flatten().map(Vec::len).sum());
        for part in acc.parts.into_iter().flatten() {
            data.extend_from_slice(&part);
        }
        Some(CompleteFrame {
            frame_id: id,
            ts: acc.ts,
            info: acc.info,
            data,
        })
    }

    /// Убрать кадр, списав его байты.
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

    const WAIT: Duration = Duration::from_millis(300);

    fn make_frame(n: usize, fill: u8) -> Vec<u8> {
        vec![fill; n]
    }

    fn key() -> VideoInfo {
        VideoInfo {
            keyframe: true,
            ..Default::default()
        }
    }

    fn plain() -> VideoInfo {
        VideoInfo::default()
    }

    /// Положить все фрагменты, кроме `skip`, и забрать всё, что готово к `now`.
    fn feed(r: &mut VideoReassembler, frags: &[Vec<u8>], skip: &[usize], now: Instant) -> Vec<u32> {
        for (i, f) in frags.iter().enumerate() {
            if !skip.contains(&i) {
                r.push(parse_video_frag(f).unwrap(), 0);
            }
        }
        drain(r, now)
    }

    fn drain(r: &mut VideoReassembler, now: Instant) -> Vec<u32> {
        std::iter::from_fn(|| r.pop(now, WAIT))
            .map(|f| f.frame_id)
            .collect()
    }

    fn frame(id: u32, info: VideoInfo) -> Vec<Vec<u8>> {
        fragment_video(&make_frame(MAX_VIDEO_CHUNK * 2 + 100, id as u8), id, info)
    }

    #[test]
    fn video_fragment_reassemble_in_order() {
        let frame = make_frame(MAX_VIDEO_CHUNK * 2 + 100, 0xAB);
        let frags = fragment_video(&frame, 1, key());
        assert!(frags.len() > 1, "большой кадр должен разрезаться");

        let mut r = VideoReassembler::new();
        for f in &frags {
            r.push(parse_video_frag(f).unwrap(), 0);
        }
        let cf = r.pop(Instant::now(), WAIT).expect("кадр должен собраться");
        assert_eq!(cf.data, frame);
        assert!(cf.info.keyframe);
        assert_eq!(cf.frame_id, 1);
    }

    /// Ключевой кадр ещё собирается — это не повод его просить.
    #[test]
    fn keyframe_in_flight_is_not_requested() {
        let now = Instant::now();
        let mut r = VideoReassembler::new();
        assert!(feed(&mut r, &frame(1, key()), &[2], now).is_empty());
        assert!(!r.wants_keyframe());
    }

    #[test]
    fn video_reassemble_out_of_order() {
        let frame = make_frame(MAX_VIDEO_CHUNK * 2, 7);
        let mut frags: Vec<Vec<u8>> = fragment_video(&frame, 10, key());
        assert!(
            frags.len() > 1,
            "кадр должен быть многофрагментным для теста"
        );
        frags.reverse(); // обратный порядок прихода

        let mut r = VideoReassembler::new();
        for f in &frags {
            r.push(parse_video_frag(f).unwrap(), 0);
        }
        assert_eq!(r.pop(Instant::now(), WAIT).unwrap().data, frame);
    }

    /// Поток начинается с P-кадров (зашли посреди звонка): их не на что опереть,
    /// просим ключевой и отдаём с него.
    #[test]
    fn stream_starts_at_a_keyframe() {
        let now = Instant::now();
        let mut r = VideoReassembler::new();
        assert!(feed(&mut r, &frame(5, plain()), &[], now).is_empty());
        assert!(r.wants_keyframe());
        assert_eq!(feed(&mut r, &frame(6, key()), &[], now), [6]);
        assert!(!r.wants_keyframe());
        assert_eq!(feed(&mut r, &frame(7, plain()), &[], now), [7]);
    }

    /// Фрагмент потерян: более новые кадры ждут, пока его повторят, а потом идут
    /// все по порядку.
    #[test]
    fn newer_frames_wait_for_a_retransmitted_fragment() {
        let t0 = Instant::now();
        let mut r = VideoReassembler::new();
        assert_eq!(feed(&mut r, &frame(1, key()), &[], t0), [1]);
        let f2 = frame(2, plain());
        assert!(feed(&mut r, &f2, &[1], t0).is_empty());
        assert!(
            feed(&mut r, &frame(3, plain()), &[], t0).is_empty(),
            "3 ждёт 2"
        );
        assert_eq!(r.deadline(WAIT), Some(t0 + WAIT));
        // Повтор пришёл до срока.
        let later = t0 + WAIT / 2;
        r.push(parse_video_frag(&f2[1]).unwrap(), 0);
        assert_eq!(drain(&mut r, later), [2, 3]);
        assert!(!r.wants_keyframe());
        assert_eq!(r.deadline(WAIT), None);
    }

    /// Ключевой кадр ждёт повтора фрагмента — P-кадры за ним не выбрасываются, а
    /// идут следом, когда он соберётся.
    #[test]
    fn frames_after_a_pending_keyframe_are_kept() {
        let t0 = Instant::now();
        let mut r = VideoReassembler::new();
        let k = frame(1, key());
        assert!(feed(&mut r, &k, &[0], t0).is_empty());
        assert!(feed(&mut r, &frame(2, plain()), &[], t0).is_empty());
        assert!(!r.wants_keyframe());
        r.push(parse_video_frag(&k[0]).unwrap(), 0);
        assert_eq!(drain(&mut r, t0 + WAIT / 2), [1, 2]);
    }

    /// Ключевой так и не собрался — просим новый.
    #[test]
    fn pending_keyframe_that_never_completes_is_requested_again() {
        let t0 = Instant::now();
        let mut r = VideoReassembler::new();
        assert!(feed(&mut r, &frame(1, key()), &[0], t0).is_empty());
        assert!(feed(&mut r, &frame(2, plain()), &[], t0).is_empty());
        assert_eq!(r.deadline(WAIT), Some(t0 + WAIT));
        assert!(drain(&mut r, t0 + WAIT).is_empty());
        assert!(r.wants_keyframe());
        assert!(r.frames.is_empty());
    }

    /// Не дождались — опоры нет: P-кадры выбрасываются, нужен ключевой.
    #[test]
    fn gives_up_after_max_wait_and_waits_for_keyframe() {
        let t0 = Instant::now();
        let mut r = VideoReassembler::new();
        feed(&mut r, &frame(1, key()), &[], t0);
        feed(&mut r, &frame(2, plain()), &[0], t0);
        assert!(feed(&mut r, &frame(3, plain()), &[], t0).is_empty());
        assert!(drain(&mut r, t0 + WAIT).is_empty());
        assert!(r.wants_keyframe());
        assert!(feed(&mut r, &frame(4, plain()), &[], t0 + WAIT).is_empty());
        assert_eq!(feed(&mut r, &frame(5, key()), &[], t0 + WAIT), [5]);
        assert!(!r.wants_keyframe());
    }

    /// Собранный ключевой кадр новее недостающего — ждать незачем.
    #[test]
    fn keyframe_jumps_over_a_gap() {
        let t0 = Instant::now();
        let mut r = VideoReassembler::new();
        feed(&mut r, &frame(1, key()), &[], t0);
        feed(&mut r, &frame(2, plain()), &[0], t0);
        assert_eq!(feed(&mut r, &frame(3, key()), &[], t0), [3]);
        // Запоздавший фрагмент кадра 2 уже не нужен.
        r.push(parse_video_frag(&frame(2, plain())[0]).unwrap(), 0);
        assert!(drain(&mut r, t0).is_empty());
    }

    /// Тихий переход к ключевому: P-кадры выбрасываются без запроса, громкий — с ним.
    #[test]
    fn skip_to_keyframe_quietly_or_with_request() {
        let now = Instant::now();
        let mut r = VideoReassembler::new();
        feed(&mut r, &frame(1, key()), &[], now);
        r.skip_to_keyframe(false);
        assert!(feed(&mut r, &frame(2, plain()), &[], now).is_empty());
        assert!(!r.wants_keyframe());
        r.skip_to_keyframe(true);
        assert!(r.wants_keyframe());
        assert_eq!(feed(&mut r, &frame(3, key()), &[], now), [3]);
        assert!(!r.wants_keyframe());
    }

    /// Счётчик кадров у отправителя обернулся — видео идёт дальше, а не замерзает.
    #[test]
    fn frame_counter_wrap_keeps_video_flowing() {
        let now = Instant::now();
        let mut r = VideoReassembler::new();
        let ids = [u32::MAX - 1, u32::MAX, 0, 1];
        for (i, id) in ids.into_iter().enumerate() {
            let info = if i == 0 { key() } else { plain() };
            let f = fragment_video(&make_frame(100, 1), id, info);
            assert_eq!(feed(&mut r, &f, &[], now), [id], "кадр {id} отдан");
        }
    }

    /// Опоздавший кадр выбрасывается и сразу за оборотом счётчика.
    #[test]
    fn late_frame_across_wrap_is_dropped() {
        let now = Instant::now();
        let mut r = VideoReassembler::new();
        let f = fragment_video(&make_frame(100, 1), 2, key());
        assert_eq!(feed(&mut r, &f, &[], now), [2]);
        let late = fragment_video(&make_frame(100, 3), u32::MAX - 3, key());
        assert!(feed(&mut r, &late, &[], now).is_empty());
        assert!(r.frames.is_empty());
    }

    /// Фрагмент с заголовком из `id`, индекса и числа фрагментов (данные — 8 КБ).
    fn raw_frag(id: u32, idx: u16, count: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(&idx.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&[0, 0, 0]); // флаги, поворот, fps
        out.extend_from_slice(&[0u8; MAX_VIDEO_CHUNK]);
        out
    }

    /// Участник звонка не может заставить нас копить память: кадр «на 65 535
    /// фрагментов» не принимается, а несобранное ограничено по байтам и кадрам.
    #[test]
    fn reassembler_memory_is_bounded() {
        let mut r = VideoReassembler::new();
        r.push(parse_video_frag(&raw_frag(1, 0, u16::MAX)).unwrap(), 0);
        assert_eq!(r.pending, 0, "кадр сверх MAX_VIDEO_FRAGS не копится");

        // Много больших кадров, ни один не собирается.
        for id in 0..16u32 {
            for idx in 0..MAX_VIDEO_FRAGS - 1 {
                r.push(
                    parse_video_frag(&raw_frag(id, idx, MAX_VIDEO_FRAGS)).unwrap(),
                    0,
                );
            }
        }
        assert!(
            r.pending <= MAX_PENDING_BYTES,
            "несобранного {} байт",
            r.pending
        );
        // И много мелких: число кадров тоже ограничено.
        for id in 100..1000u32 {
            r.push(parse_video_frag(&raw_frag(id, 0, 2)).unwrap(), 0);
        }
        assert!(r.frames.len() <= MAX_FRAMES);
        let real: usize = r
            .frames
            .values()
            .flat_map(|a| a.parts.iter().flatten())
            .map(Vec::len)
            .sum();
        assert_eq!(real, r.pending, "учёт байт сходится");
    }
}
