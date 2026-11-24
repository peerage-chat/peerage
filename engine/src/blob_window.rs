//! Окно докачки блоба по задержке (п. 10 списка с телефона).
//!
//! Докачка — фоновый поток, и уступать она должна переписке. Фиксированное окно в 64 чанка
//! (≈770 КБ) на медленном меше стоит в очереди канала секундами, и текст с подтверждениями
//! ждут за ним. Поэтому окно ведётся по задержке, как у фоновых передач в LEDBAT (RFC 6817)
//! и uTP: базовая задержка — минимум RTT чанка за последние минуты; всё, что сверх неё, —
//! очередь, которую создали мы сами. Очередь меньше цели — окно растёт, больше — сжимается;
//! потеря (таймаут чанка) режет окно вдвое. Пустой канал докачка заполняет как раньше.
//!
//! Чистая логика без часов: время приходит аргументом, проверяется юнит-тестами.

use std::time::{Duration, Instant};

/// Окно не меньше: даже на перегруженном канале докачка идёт, а не замирает.
const MIN: f64 = 2.0;
/// И не больше прежнего фиксированного: больше отдающий всё равно режет.
const MAX: f64 = 64.0;
/// Начало: 16 чанков ≈ 190 КБ — сразу полная скорость на быстром канале, без залпа на
/// медленном.
const START: f64 = 16.0;
/// Сколько своей очереди терпим поверх базовой задержки.
const TARGET: Duration = Duration::from_millis(100);
/// Очередь сверх цели во столько раз и больше — сжатие уже не сильнее.
const MAX_BACKOFF: f64 = 4.0;
/// Сжатие на чанк — доля превышения: за окно это множитель (очередь вдвое сверх цели —
/// ×0,875 за RTT, предел — вдвое). Рост — на чанк в окно, как у LEDBAT: аддитивное сжатие
/// сбрасывало бы очередь в 64 чанка десятки RTT.
const DECREASE: f64 = 8.0;
/// Базовую задержку помним столько (две корзины по столько): маршрут меша меняется, и
/// старый минимум перестаёт быть правдой.
const BASE_EPOCH: Duration = Duration::from_secs(60);
/// Таймауты одной волны — одна потеря: окно режем не чаще раза за это время.
const LOSS_HOLD: Duration = Duration::from_secs(1);

pub(crate) struct Window {
    cwnd: f64,
    /// Минимум RTT в текущей и прошлой корзине.
    base_cur: Option<Duration>,
    base_prev: Option<Duration>,
    epoch: Instant,
    last_cut: Option<Instant>,
}

impl Window {
    pub(crate) fn new(now: Instant) -> Self {
        Window {
            cwnd: START,
            base_cur: None,
            base_prev: None,
            epoch: now,
            last_cut: None,
        }
    }

    /// Сколько чанков держать запрошенными.
    pub(crate) fn size(&self) -> usize {
        self.cwnd as usize
    }

    /// Пришёл чанк, спрошенный один раз (переспрошенный не годится — неизвестно, на какой
    /// запрос ответ): `rtt` от запроса до ответа.
    pub(crate) fn on_sample(&mut self, rtt: Duration, now: Instant) {
        if now.duration_since(self.epoch) >= BASE_EPOCH {
            self.epoch = now;
            self.base_prev = self.base_cur.take();
        }
        let cur = self.base_cur.map_or(rtt, |b| b.min(rtt));
        self.base_cur = Some(cur);
        let base = self.base_prev.map_or(cur, |p| p.min(cur));
        let queuing = rtt.saturating_sub(base).as_secs_f64();
        let target = TARGET.as_secs_f64();
        let off = ((target - queuing) / target).clamp(-MAX_BACKOFF, 1.0);
        let step = if off >= 0.0 {
            off / self.cwnd
        } else {
            off / DECREASE
        };
        self.cwnd = (self.cwnd + step).clamp(MIN, MAX);
    }

    /// Чанк не пришёл за таймаут: потеря — окно вдвое.
    pub(crate) fn on_loss(&mut self, now: Instant) {
        if self
            .last_cut
            .is_some_and(|t| now.duration_since(t) < LOSS_HOLD)
        {
            return;
        }
        self.last_cut = Some(now);
        self.cwnd = (self.cwnd / 2.0).max(MIN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Прогнать `n` чанков с задержкой `rtt_ms`, по сотне в секунду.
    fn feed(w: &mut Window, t: &mut Instant, rtt_ms: u64, n: usize) {
        for _ in 0..n {
            *t += Duration::from_millis(10);
            w.on_sample(Duration::from_millis(rtt_ms), *t);
        }
    }

    #[test]
    fn empty_channel_opens_to_the_old_window() {
        let mut t = Instant::now();
        let mut w = Window::new(t);
        assert_eq!(w.size(), 16);
        feed(&mut w, &mut t, 50, 3000);
        assert_eq!(w.size(), 64, "очереди нет — окно как прежде");
    }

    #[test]
    fn own_queue_shrinks_the_window() {
        let mut t = Instant::now();
        let mut w = Window::new(t);
        feed(&mut w, &mut t, 50, 3000);
        // Канал забит: RTT вырос на 400 мс сверх базы.
        feed(&mut w, &mut t, 450, 300);
        assert_eq!(w.size(), 2, "очередь сверх цели — докачка уступает");
    }

    #[test]
    fn queue_at_target_holds_the_window() {
        let mut t = Instant::now();
        let mut w = Window::new(t);
        feed(&mut w, &mut t, 50, 1);
        feed(&mut w, &mut t, 150, 2000);
        assert_eq!(w.size(), 16, "ровно в цель — ни роста, ни сжатия");
    }

    #[test]
    fn loss_halves_once_per_wave() {
        let mut t = Instant::now();
        let mut w = Window::new(t);
        w.on_loss(t);
        w.on_loss(t + Duration::from_millis(100));
        assert_eq!(w.size(), 8, "таймауты одной волны — одна потеря");
        w.on_loss(t + Duration::from_secs(2));
        assert_eq!(w.size(), 4);
        t += Duration::from_secs(10);
        for _ in 0..10 {
            t += Duration::from_secs(2);
            w.on_loss(t);
        }
        assert_eq!(w.size(), 2, "не ниже минимума");
    }

    #[test]
    fn old_base_is_forgotten_when_the_route_changes() {
        let mut t = Instant::now();
        let mut w = Window::new(t);
        feed(&mut w, &mut t, 20, 100);
        // Маршрут сменился: честная задержка теперь 300 мс. Пока помнится старая база, это
        // похоже на очередь, но через две корзины база — новая, и окно снова растёт.
        t += BASE_EPOCH * 2;
        feed(&mut w, &mut t, 300, 10_000);
        assert_eq!(w.size(), 64);
    }
}
