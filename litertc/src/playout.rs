//! Когда показывать видео-кадр (ADR-014, шаг 8).
//!
//! Кадр несёт метку времени захвата на часах отправителя (90 кГц). Задержка доставки
//! у кадров разная: очередь в сети, пачки доставки, большой ключевой кадр, повтор
//! потерянного по NACK. Показывать кадр в момент прихода — значит показывать
//! рывками; показывать «по fps из заголовка» — значит разъехаться с настоящим темпом
//! камеры (она отдаёт меньше заявленного в темноте и на эмуляторе: буфер пустеет,
//! картинка замирает, задержка растёт). Поэтому показ — по метке:
//!
//! `показ = метка + смещение`, где смещение покрывает почти все недавние кадры
//! (99-й перцентиль «приход − метка» за 10 с). Разница часов отправителя и нашей
//! в смещение входит сама, их дрейф отслеживается окном. Смещение растёт сразу
//! (короткая пауза лучше череды опозданий), а убывает не быстрее 50 мс за секунду —
//! видео идёт на 5 % быстрее, глазу это незаметно. После затора, когда лишнего
//! больше четверти секунды, — вдвое быстрее: отставание на секунды в разговоре
//! хуже заметного ускорения.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::wire::VIDEO_CLOCK_HZ;

/// Окно наблюдений «приход − метка».
const WINDOW: Duration = Duration::from_secs(10);
/// Наблюдений в окне не больше (120 fps × 10 с).
const WINDOW_MAX: usize = 1200;
/// Доля кадров, которые должны успевать к показу.
const PERCENTILE: f64 = 0.99;
/// Насколько быстро смещение может убывать: мс за секунду; при большом лишнем —
/// быстрее.
const DECAY_MS_PER_S: f64 = 50.0;
const FAST_DECAY_MS_PER_S: f64 = 100.0;
const FAST_DECAY_ABOVE_MS: f64 = 250.0;
/// Больше этой задержки сверх самого быстрого кадра не ждём — показываем опоздавшим.
const MAX_EXTRA_DELAY_MS: f64 = 1000.0;

/// Расписание показа кадров одного потока.
#[derive(Default)]
pub(crate) struct Playout {
    /// Наши часы: начало отсчёта, мс от него.
    epoch: Option<Instant>,
    /// Метка последнего кадра и её значение без оборота u32.
    last_ts: Option<(u32, i64)>,
    /// (когда пришёл — наши мс, «приход − метка» — мс).
    window: VecDeque<(f64, f64)>,
    /// Текущее смещение показа от метки, мс.
    offset: Option<f64>,
    updated_at: f64,
    last_play: Option<Instant>,
}

impl Playout {
    /// Кадр с меткой `ts` готов к декодированию в `now`: когда его показать.
    /// Время показа не убывает от кадра к кадру.
    pub fn schedule(&mut self, now: Instant, ts: u32) -> Instant {
        let epoch = *self.epoch.get_or_insert(now);
        let t = now.saturating_duration_since(epoch).as_secs_f64() * 1000.0;
        let ts_ms = self.unwrap(ts) as f64 * 1000.0 / VIDEO_CLOCK_HZ as f64;
        let transit = t - ts_ms;

        let window_ms = WINDOW.as_secs_f64() * 1000.0;
        while self
            .window
            .front()
            .is_some_and(|&(at, _)| t - at > window_ms)
            || self.window.len() >= WINDOW_MAX
        {
            self.window.pop_front();
        }
        self.window.push_back((t, transit));
        let target = self.target();

        let offset = match self.offset {
            Some(o) if target < o => {
                let dt = (t - self.updated_at).max(0.0);
                let rate = if o - target > FAST_DECAY_ABOVE_MS {
                    FAST_DECAY_MS_PER_S
                } else {
                    DECAY_MS_PER_S
                };
                (o - rate * dt / 1000.0).max(target)
            }
            _ => target,
        };
        self.offset = Some(offset);
        self.updated_at = t;

        let play_ms = ts_ms + offset;
        let mut play = if play_ms >= 0.0 {
            epoch + Duration::from_secs_f64(play_ms / 1000.0)
        } else {
            epoch
        };
        // Не раньше, чем кадр готов (редкий опоздавший — за пределом перцентиля —
        // показывается сразу), не раньше прошлого кадра (смещение убывает) и не
        // позже, чем через секунду: дальше задержка хуже опоздания.
        play = play.max(now);
        if let Some(last) = self.last_play {
            play = play.max(last);
        }
        let cap = now + Duration::from_secs_f64(MAX_EXTRA_DELAY_MS / 1000.0);
        play = play.min(cap);
        self.last_play = Some(play);
        play
    }

    /// Смещение, которое покрывает [`PERCENTILE`] кадров окна, — но не больше
    /// [`MAX_EXTRA_DELAY_MS`] сверх самого быстрого.
    fn target(&self) -> f64 {
        let mut v: Vec<f64> = self.window.iter().map(|&(_, x)| x).collect();
        let min = v.iter().copied().fold(f64::INFINITY, f64::min);
        let k = ((v.len() - 1) as f64 * PERCENTILE).round() as usize;
        let (_, p, _) = v.select_nth_unstable_by(k, f64::total_cmp);
        p.min(min + MAX_EXTRA_DELAY_MS)
    }

    fn unwrap(&mut self, ts: u32) -> i64 {
        let v = match self.last_ts {
            Some((last, base)) => base + ts.wrapping_sub(last) as i32 as i64,
            None => ts as i64,
        };
        self.last_ts = Some((ts, v));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn ts(ms: u64) -> u32 {
        (ms * VIDEO_CLOCK_HZ as u64 / 1000) as u32
    }

    /// Шаг между показами равен `want` с точностью до микросекунды (округление f64).
    fn step_is(a: Instant, b: Instant, want: Duration) {
        let got = b - a;
        let diff = got.abs_diff(want);
        assert!(
            diff < Duration::from_micros(1),
            "шаг {got:?}, ждали {want:?}"
        );
    }

    /// Кадры с разбросом доставки показываются ровно в темпе захвата.
    #[test]
    fn jittered_frames_play_at_capture_pace() {
        let t0 = Instant::now();
        let mut p = Playout::default();
        let mut plays = Vec::new();
        for i in 0..300u64 {
            let jitter = i * 37 % 41; // 0..40 мс, псевдослучайно
            let at = t0 + ms(i * 33 + 50 + jitter);
            plays.push(p.schedule(at, ts(i * 33)));
        }
        // После первых секунд разгона — шаг показа ровно 33 мс.
        for w in plays[100..].windows(2) {
            step_is(w[0], w[1], ms(33));
        }
    }

    /// Камера отдаёт кадры неровно (темнота, эмулятор): показ следует меткам, а не fps.
    #[test]
    fn variable_capture_rate_is_followed() {
        let t0 = Instant::now();
        let mut p = Playout::default();
        let mut cap = 0u64;
        let mut prev: Option<(u64, Instant)> = None;
        for i in 0..200u64 {
            cap += 33 + (i % 3) * 8; // 33, 41, 49 мс
            let play = p.schedule(t0 + ms(cap + 40), ts(cap));
            if let Some((pc, pp)) = prev {
                if i > 10 {
                    step_is(pp, play, ms(cap - pc));
                }
            }
            prev = Some((cap, play));
        }
    }

    /// Задержка сети выросла — смещение следом сразу; упала — убывает плавно, без
    /// пачки кадров подряд (окно ещё 10 с помнит медленные кадры, потом спуск).
    #[test]
    fn delay_rises_at_once_and_falls_gently() {
        let t0 = Instant::now();
        let mut p = Playout::default();
        let mut plays = Vec::new();
        for i in 0..1500u64 {
            // 10–20 с — сеть на 300 мс медленнее.
            let extra = if (300..600).contains(&i) { 300 } else { 0 };
            let at = t0 + ms(i * 33 + 50 + extra);
            let play = p.schedule(at, ts(i * 33));
            assert!(play >= at, "кадр {i} показан раньше прихода");
            plays.push(play);
        }
        // После спада шаг показа не короче 33 мс × 0,9.
        for w in plays[600..].windows(2) {
            assert!(
                w[1] - w[0] >= Duration::from_micros(29_700),
                "{:?}",
                w[1] - w[0]
            );
        }
        // И через 30 с задержка снова около базовых 50 мс.
        let last = plays.len() - 1;
        let delay = plays[last] - (t0 + ms(last as u64 * 33));
        assert!(delay < ms(80), "задержка {delay:?}");
    }

    /// Часы отправителя спешат на 0,1 %: задержка не уползает.
    #[test]
    fn clock_drift_is_tracked() {
        let t0 = Instant::now();
        let mut p = Playout::default();
        for i in 0..6000u64 {
            let at = t0 + ms(i * 33 * 999 / 1000 + 50);
            let delay = p.schedule(at, ts(i * 33)).saturating_duration_since(at);
            if i > 100 {
                assert!(delay < ms(20), "кадр {i}: ждёт {delay:?}");
            }
        }
    }

    /// Метка проходит через оборот u32 (13 часов при 90 кГц) — показ не прыгает.
    #[test]
    fn timestamp_wrap_is_seamless() {
        let t0 = Instant::now();
        let mut p = Playout::default();
        let start = u32::MAX - 90 * 33 * 5;
        let mut prev = None;
        for i in 0..20u32 {
            let play = p.schedule(t0 + ms(i as u64 * 33 + 50), start.wrapping_add(i * 90 * 33));
            if let Some(pp) = prev {
                step_is(pp, play, ms(33));
            }
            prev = Some(play);
        }
    }
}
