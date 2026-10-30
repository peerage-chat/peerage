//! Выпуск пакетов в сеть ровным темпом и по приоритетам (ADR-014 §6).
//!
//! Ключевой кадр — десятки фрагментов разом; пачкой они переполняют очередь узкого
//! места, и теряется всё подряд, включая звук. Pacer выпускает видео и повторы со
//! скоростью в 2,5 раза выше оценки полосы — пачка растягивается на миллисекунды,
//! а не копится в сети. Служебные пакеты (отчёты, NACK, запросы) и звук идут сразу:
//! они мелкие, а опоздание речи слышно. Повторы — раньше нового видео: их ждёт
//! уже собираемый кадр. Видео и повторы участнику, чьё окно перегрузки полно
//! ([`crate::bwe`]), ждут; простоявшие сверх [`MAX_QUEUE_DELAY`] выбрасываются.

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

    /// Первое видео или повтор участнику не из `held`: (класс, место в очереди).
    fn first_free(&self, held: &impl Fn(&P) -> bool) -> Option<(usize, usize)> {
        (2..4).find_map(|c| {
            self.queues[c]
                .iter()
                .position(|(_, t)| !held(&t.to))
                .map(|i| (c, i))
        })
    }

    /// Следующий пакет, который можно выпустить в `now`. Видео и повторы участникам
    /// из `held` (их окно перегрузки полно) ждут.
    pub fn pop(&mut self, now: Instant, held: impl Fn(&P) -> bool) -> Option<Transmit<P>> {
        self.refill(now);
        let (class, i) = if !self.queues[0].is_empty() {
            (0, 0)
        } else if !self.queues[1].is_empty() {
            (1, 0)
        } else if self.budget > 0.0 {
            self.first_free(&held)?
        } else {
            return None;
        };
        let (_, t) = self.queues[class].remove(i)?;
        self.budget -= t.data.len() as f64;
        if class >= Class::Retransmit as usize {
            self.paced_bytes -= t.data.len();
        }
        Some(t)
    }

    /// Когда можно будет выпустить следующий пакет; `None` — выпускать нечего (или
    /// всё оставшееся — участникам из `held`: их окно откроет обратная связь).
    pub fn next_release(&self, now: Instant, held: impl Fn(&P) -> bool) -> Option<Instant> {
        if !self.queues[0].is_empty() || !self.queues[1].is_empty() {
            return Some(now);
        }
        self.first_free(&held)?;
        if self.budget > 0.0 {
            return Some(now);
        }
        // Бюджет станет положительным через столько.
        let wait = (1.0 - self.budget) / self.release_rate(now);
        Some(self.last.unwrap_or(now) + Duration::from_secs_f64(wait))
    }

    /// Выбросить видео и повторы участникам из `held`, простоявшие дольше
    /// [`MAX_QUEUE_DELAY`]: пока окно откроется, они опоздают, а потерянное приёмник
    /// попросит заново (ключевой кадр).
    pub fn drop_stale(&mut self, now: Instant, held: impl Fn(&P) -> bool) {
        let paced = &mut self.paced_bytes;
        for q in &mut self.queues[2..] {
            q.retain(|(at, t)| {
                let stale = held(&t.to) && now.saturating_duration_since(*at) > MAX_QUEUE_DELAY;
                if stale {
                    *paced -= t.data.len();
                }
                !stale
            });
        }
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

    fn to(peer: u8, len: usize) -> Transmit<u8> {
        Transmit {
            to: peer,
            data: vec![0; len],
        }
    }

    /// Сколько байт выпущено за `ms` миллисекунд по шагу в 1 мс.
    fn released(p: &mut Pacer<u8>, t0: Instant, ms: u64) -> usize {
        (0..ms)
            .flat_map(|i| {
                let now = t0 + Duration::from_millis(i);
                std::iter::from_fn(move || Some(now)).take(1)
            })
            .map(|now| {
                std::iter::from_fn(|| p.pop(now, |_| false))
                    .map(|t| t.data.len())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Видео выходит темпом в 2,5 оценки: 1 Мбит/с — около 31 КБ за 100 мс.
    #[test]
    fn video_leaves_at_two_and_a_half_times_the_estimate() {
        let t0 = Instant::now();
        let mut p = Pacer::new(1_000_000.0);
        for _ in 0..100 {
            p.push(Class::Video, t0, to(1, 1000));
        }
        let bytes = released(&mut p, t0, 100);
        assert!((29_000..=34_000).contains(&bytes), "{bytes}");
    }

    /// Звук и служебные не ждут бюджета: выпускать их — сейчас.
    #[test]
    fn audio_is_due_at_once() {
        let t0 = Instant::now();
        let mut p = Pacer::new(1_000_000.0);
        p.push(Class::Video, t0, to(1, 100_000));
        assert!(p.pop(t0, |_| false).is_some()); // бюджет ушёл в минус
        p.push(Class::Audio, t0, to(1, 100));
        assert_eq!(p.next_release(t0, |_| false), Some(t0));
    }

    /// Удержанное видео выбрасывается, простояв дольше срока, — не ровно в срок.
    #[test]
    fn held_video_is_dropped_after_its_deadline() {
        let t0 = Instant::now();
        let mut p = Pacer::new(1_000_000.0);
        p.push(Class::Video, t0, to(1, 1000));
        p.drop_stale(t0 + MAX_QUEUE_DELAY, |_| true);
        assert!(p.pop(t0 + MAX_QUEUE_DELAY, |_| false).is_some());
        p.push(Class::Video, t0, to(1, 1000));
        p.drop_stale(t0 + MAX_QUEUE_DELAY + Duration::from_millis(1), |_| true);
        assert!(p.pop(t0 + MAX_QUEUE_DELAY, |_| false).is_none());
    }

    /// Очередь ушедшему участнику убрана — и в темп выпуска оставшимся она больше
    /// не входит: иначе им досталась бы чужая спешка.
    #[test]
    fn departed_peer_queue_does_not_speed_up_others() {
        let t0 = Instant::now();
        let mut p = Pacer::new(100_000.0);
        for _ in 0..100 {
            p.push(Class::Video, t0, to(2, 1000));
        }
        for _ in 0..10 {
            p.push(Class::Video, t0, to(1, 1000));
        }
        p.retain(|t| t.to == 1);
        // 400 мс спустя у старшего пакета осталось 100 мс: 10 КБ за них — 100 КБ/с.
        let now = t0 + Duration::from_millis(400);
        let bytes = released(&mut p, now, 10);
        assert!(bytes <= 3000, "выпущено {bytes}");
        assert_eq!(
            released(&mut p, now + Duration::from_millis(10), 200),
            9000 - bytes + 1000
        );
    }

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
        while p.pop(now, |_| false).is_some() {
            released += 1;
        }
        assert!(released < 5, "пачкой ушло {released}");
        p.push(Class::Audio, now, t(100));
        assert_eq!(
            p.pop(now, |_| false).map(|t| t.data.len()),
            Some(100),
            "звук сразу"
        );
        let mut last = now;
        while released < 100 {
            now = p
                .next_release(now, |_| false)
                .expect("очередь не пуста")
                .max(now);
            while p.pop(now, |_| false).is_some() {
                released += 1;
                last = now;
            }
        }
        let took = last - t0;
        assert!(
            took > Duration::from_millis(250) && took < Duration::from_millis(400),
            "кадр вышел за {took:?}"
        );
        assert_eq!(p.next_release(now, |_| false), None);
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
        let order: Vec<usize> = std::iter::from_fn(|| p.pop(now, |_| false))
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
            now = p.next_release(now, |_| false).unwrap().max(now);
            while p.pop(now, |_| false).is_some() {
                released += 1;
            }
        }
        assert!(
            now - t0 < Duration::from_millis(600),
            "очередь ушла за {:?}",
            now - t0
        );
    }

    /// Окно участника полно: его видео ждёт (звук — нет, видео другим — тоже нет),
    /// таймер ради него не взводится, а простоявшее сверх предела выбрасывается.
    #[test]
    fn held_video_waits_and_goes_stale() {
        let t0 = Instant::now();
        let mut p = Pacer::new(100_000_000.0);
        let to = |to: u8, len: usize| Transmit {
            to,
            data: vec![0; len],
        };
        p.push(Class::Video, t0, to(1, 10));
        p.push(Class::Video, t0, to(2, 20));
        p.push(Class::Audio, t0, to(1, 30));
        let held = |p: &u8| *p == 1;
        let order: Vec<usize> = std::iter::from_fn(|| p.pop(t0, held))
            .map(|t| t.data.len())
            .collect();
        assert_eq!(order, [30, 20]);
        assert_eq!(p.next_release(t0, held), None);
        assert!(p.next_release(t0, |_| false).is_some(), "окно открылось");
        p.drop_stale(t0 + MAX_QUEUE_DELAY / 2, held);
        assert!(p.next_release(t0, |_| false).is_some(), "ещё не устарело");
        p.drop_stale(t0 + MAX_QUEUE_DELAY * 2, held);
        assert_eq!(p.next_release(t0, |_| false), None);
        assert_eq!(p.paced_bytes, 0);
    }
}
