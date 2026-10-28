//! Выпуск пакетов в сеть ровным темпом и по приоритетам (ADR-014 §6).
//!
//! Ключевой кадр — десятки фрагментов разом; пачкой они переполняют очередь узкого
//! места, и теряется всё подряд, включая звук. Pacer выпускает видео и повторы со
//! скоростью в 2,5 раза выше оценки полосы — пачка растягивается на миллисекунды,
//! а не копится в сети. Служебные пакеты (отчёты, NACK, запросы) и звук идут сразу:
//! они мелкие, а опоздание речи слышно. Повторы — раньше нового видео: их ждёт
//! уже собираемый кадр.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::session::Transmit;

/// Во сколько раз темп выпуска выше оценки полосы (как в WebRTC): кодер выдаёт
/// неровно, и pacer не должен сам становиться очередью.
const PACING_FACTOR: f64 = 2.5;
/// Сколько времени выпуска можно накопить про запас, пока очередь была пуста.
const BURST: Duration = Duration::from_millis(5);
/// Дольше в очереди видео не стоит: если не успеваем, темп поднимается — задержка
/// хуже потерь.
const MAX_QUEUE_DELAY: Duration = Duration::from_millis(500);

/// Класс пакета, в порядке приоритета.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Control = 0,
    Audio = 1,
    Retransmit = 2,
    Video = 3,
}

pub(crate) struct Pacer<P> {
    /// По классу: (когда поставлен, пакет).
    queues: [VecDeque<(Instant, Transmit<P>)>; 4],
    /// Сколько байт можно выпустить сейчас; уходит в минус на большом пакете.
    budget: f64,
    last: Option<Instant>,
    /// Оценка полосы на всех участников вместе, бит/с.
    rate_bps: f64,
    /// Байты видео и повторов в очереди.
    paced_bytes: usize,
}

impl<P> Pacer<P> {
    pub fn new(rate_bps: f64) -> Self {
        Pacer {
            queues: Default::default(),
            budget: 0.0,
            last: None,
            rate_bps,
            paced_bytes: 0,
        }
    }

    pub fn set_rate(&mut self, rate_bps: f64) {
        self.rate_bps = rate_bps;
    }

    pub fn push(&mut self, class: Class, now: Instant, t: Transmit<P>) {
        if matches!(class, Class::Retransmit | Class::Video) {
            self.paced_bytes += t.data.len();
        }
        self.queues[class as usize].push_back((now, t));
    }

    /// Темп выпуска, байт/с: от оценки, но так, чтобы самый старый пакет видео не
    /// простоял дольше [`MAX_QUEUE_DELAY`] — тогда вся очередь уходит за оставшееся
    /// ему время.
    fn release_rate(&self, now: Instant) -> f64 {
        let paced = self.rate_bps * PACING_FACTOR / 8.0;
        let oldest = self.queues[2..]
            .iter()
            .filter_map(|q| q.front())
            .map(|(at, _)| *at)
            .min();
        let Some(oldest) = oldest else { return paced };
        let left = MAX_QUEUE_DELAY
            .saturating_sub(now.saturating_duration_since(oldest))
            .max(Duration::from_millis(10));
        paced.max(self.paced_bytes as f64 / left.as_secs_f64())
    }

    fn refill(&mut self, now: Instant) {
        let rate = self.release_rate(now);
        let cap = rate * BURST.as_secs_f64();
        let elapsed = self
            .last
            .map_or(BURST, |l| now.saturating_duration_since(l));
        self.budget = (self.budget + rate * elapsed.as_secs_f64()).min(cap);
        self.last = Some(now);
    }

    /// Следующий пакет, который можно выпустить в `now`.
    pub fn pop(&mut self, now: Instant) -> Option<Transmit<P>> {
        self.refill(now);
        let class = if !self.queues[0].is_empty() {
            0
        } else if !self.queues[1].is_empty() {
            1
        } else if self.budget > 0.0 {
            (2..4).find(|&c| !self.queues[c].is_empty())?
        } else {
            return None;
        };
        let (_, t) = self.queues[class].pop_front()?;
        self.budget -= t.data.len() as f64;
        if class >= Class::Retransmit as usize {
            self.paced_bytes -= t.data.len();
        }
        Some(t)
    }

    /// Когда можно будет выпустить следующий пакет; `None` — очередь пуста.
    pub fn next_release(&self, now: Instant) -> Option<Instant> {
        if !self.queues[0].is_empty() || !self.queues[1].is_empty() {
            return Some(now);
        }
        if self.paced_bytes == 0 && self.queues[2..].iter().all(VecDeque::is_empty) {
            return None;
        }
        if self.budget > 0.0 {
            return Some(now);
        }
        // Бюджет станет положительным через столько.
        let wait = (1.0 - self.budget) / self.release_rate(now);
        Some(self.last.unwrap_or(now) + Duration::from_secs_f64(wait))
    }

    /// Убрать пакеты, которые больше не нужны (ушедшему участнику).
    pub fn retain(&mut self, keep: impl Fn(&Transmit<P>) -> bool) {
        for (c, q) in self.queues.iter_mut().enumerate() {
            q.retain(|(_, t)| {
                let k = keep(t);
                if !k && c >= Class::Retransmit as usize {
                    self.paced_bytes -= t.data.len();
                }
                k
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(len: usize) -> Transmit<u8> {
        Transmit {
            to: 1,
            data: vec![0; len],
        }
    }

    /// Кадр в 100 КБ при оценке 1 Мбит/с выходит за ~0,3 с (темп 2,5 Мбит/с), а не
    /// пачкой; звук, поставленный посреди, уходит сразу.
    #[test]
    fn video_is_spread_and_audio_jumps_ahead() {
        let t0 = Instant::now();
        let mut p = Pacer::new(1_000_000.0);
        for _ in 0..100 {
            p.push(Class::Video, t0, t(1000));
        }
        let mut released = 0;
        let mut now = t0;
        while p.pop(now).is_some() {
            released += 1;
        }
        assert!(released < 5, "пачкой ушло {released}");
        p.push(Class::Audio, now, t(100));
        assert_eq!(p.pop(now).map(|t| t.data.len()), Some(100), "звук сразу");
        let mut last = now;
        while released < 100 {
            now = p.next_release(now).expect("очередь не пуста").max(now);
            while p.pop(now).is_some() {
                released += 1;
                last = now;
            }
        }
        let took = last - t0;
        assert!(
            took > Duration::from_millis(250) && took < Duration::from_millis(400),
            "кадр вышел за {took:?}"
        );
        assert_eq!(p.next_release(now), None);
    }

    /// Повторы — раньше нового видео; служебные — раньше всего.
    #[test]
    fn priorities() {
        let now = Instant::now();
        let mut p = Pacer::new(100_000_000.0);
        p.push(Class::Video, now, t(3));
        p.push(Class::Retransmit, now, t(2));
        p.push(Class::Audio, now, t(1));
        p.push(Class::Control, now, t(0));
        let order: Vec<usize> = std::iter::from_fn(|| p.pop(now))
            .map(|t| t.data.len())
            .collect();
        assert_eq!(order, [0, 1, 2, 3]);
    }

    /// Очередь не копится дольше полсекунды: при заниженной оценке темп растёт.
    #[test]
    fn queue_delay_is_bounded() {
        let t0 = Instant::now();
        let mut p = Pacer::new(100_000.0);
        for _ in 0..1000 {
            p.push(Class::Video, t0, t(1000));
        }
        let mut now = t0;
        let mut released = 0;
        while released < 1000 {
            now = p.next_release(now).unwrap().max(now);
            while p.pop(now).is_some() {
                released += 1;
            }
        }
        assert!(
            now - t0 < Duration::from_millis(600),
            "очередь ушла за {:?}",
            now - t0
        );
    }
}
