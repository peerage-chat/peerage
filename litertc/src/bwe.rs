//! Оценка полосы до участника (ADR-014 §6) — упрощённый GCC
//! (draft-ietf-rmcat-gcc-02, как в WebRTC).
//!
//! Два сигнала. **Задержка:** пакеты группируются по 5 мс отправки, и если группы
//! приходят всё дальше друг от друга, чем ушли, — очередь в сети растёт. Для нас это
//! главный сигнал: линки Yggdrasil идут поверх TCP/TLS/QUIC, перегрузка там сначала
//! копит очередь и задержку и только потом теряет пакеты. По задержке — AIMD:
//! прибавляем понемногу, сбавляем сразу до 85 % того, что реально дошло. **Потери:**
//! своя оценка — больше 10 % сбавляем, меньше 2 % прибавляем, между — держим. Итог —
//! меньшая из двух. Оценки раздельные (как в WebRTC): общая при постоянных
//! неперегрузочных потерях (радио) только спускалась бы — сбросы по задержке есть,
//! а прибавлять потери не дают. По той же причине пороги потерь считаются сверх
//! собственных потерь канала — постоянной доли, которая есть при любой нашей
//! скорости (радио, захлёбывающийся CPU узла).
//!
//! Сведения идут из транспортной обратной связи ([`crate::wire::TransportFeedback`]):
//! приёмник сообщает, когда пришёл каждый пакет, отправитель знает, когда его отправил.
//! Враждебный участник может солгать — и снизить только то, что мы шлём (ADR-014 §8).

use std::collections::VecDeque;

use crate::wire::{TransportFeedback, FEEDBACK_TICK_US, MAX_FEEDBACK_PACKETS};

/// Пакеты, ушедшие в пределах этого интервала, — одна группа (пачка фрагментов кадра).
const GROUP_SPAN_MS: f64 = 5.0;
/// Пакет, пришедший не позже этого после предыдущего и догнавший его, — из той же
/// пачки доставки: сеть копила и отдала разом (меш поверх TCP, Wi-Fi). Иначе каждая
/// такая пачка выглядела бы как растущая очередь.
const BURST_GAP_MS: f64 = 5.0;
/// Длиннее пачка доставки не бывает.
const MAX_BURST_MS: f64 = 100.0;
/// Точек в линейной регрессии тренда задержки.
const TREND_WINDOW: usize = 20;
/// Сглаживание накопленной задержки.
const SMOOTHING: f64 = 0.9;
/// Усиление тренда перед сравнением с порогом.
const TREND_GAIN: f64 = 4.0;
/// Перегрузка признаётся, если тренд выше порога дольше этого.
const OVERUSE_TIME_MS: f64 = 10.0;
/// Адаптивный порог: начальный, пределы и скорость подстройки (вверх медленно,
/// вниз быстро — как в GCC).
const THRESHOLD_INIT: f64 = 12.5;
const THRESHOLD_MIN: f64 = 6.0;
const THRESHOLD_MAX: f64 = 600.0;
const K_UP: f64 = 0.0087;
const K_DOWN: f64 = 0.039;

/// Сбавляем до этой доли дошедшего.
const BACKOFF: f64 = 0.85;
/// Прибавка вдали от последней перегрузки — за секунду.
const INCREASE_PER_SEC: f64 = 1.08;
/// Потери сверх собственных выше — сбавляем, выше нижней — не прибавляем.
const LOSS_HIGH: f64 = 0.10;
const LOSS_LOW: f64 = 0.02;
/// Собственные потери канала: падают сразу до замеренных, растут на эту долю
/// разницы за окно (≈ 10 с) — перегрузка, на которую мы сразу отвечаем, их почти не
/// сдвигает, а постоянные потери за несколько секунд становятся фоном. Не выше
/// предела: канал, теряющий больше, плох при любой скорости.
const INHERENT_LOSS_RISE: f64 = 0.1;
const INHERENT_LOSS_MAX: f64 = 0.1;
/// Окно подсчёта потерь и дошедшего битрейта.
const LOSS_WINDOW_MS: f64 = 1000.0;
const ACKED_WINDOW_MS: f64 = 500.0;
/// Журнал отправленного: столько пакетов и столько времени. Время — с запасом на
/// раздутую очередь: обратная связь о пакете приходит через RTT плюс очередь, и если
/// журнал забыл его раньше, оценка слепнет как раз тогда, когда нужнее всего.
const SEND_LOG_MAX: usize = 4096;
const SEND_LOG_AGE_MS: f64 = 10_000.0;

/// Что говорит задержка.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Usage {
    Normal,
    /// Очередь растёт — канал перегружен.
    Over,
    /// Очередь рассасывается — держим, пока не опустеет.
    Under,
}

/// Отправленные пакеты участнику по сквозному номеру: когда и сколько байт.
#[derive(Default)]
struct SendLog {
    first: u16,
    sent: VecDeque<(f64, u32)>,
}

impl SendLog {
    fn push(&mut self, seq: u16, at_ms: f64, size: u32) {
        if self.first.wrapping_add(self.sent.len() as u16) != seq {
            self.sent.clear();
        }
        if self.sent.is_empty() {
            self.first = seq;
        }
        self.sent.push_back((at_ms, size));
        while self.sent.len() > SEND_LOG_MAX
            || self
                .sent
                .front()
                .is_some_and(|&(t, _)| at_ms - t > SEND_LOG_AGE_MS)
        {
            self.sent.pop_front();
            self.first = self.first.wrapping_add(1);
        }
    }

    fn get(&self, seq: u16) -> Option<(f64, u32)> {
        self.sent
            .get(seq.wrapping_sub(self.first) as usize)
            .copied()
    }
}

struct Group {
    first_send: f64,
    last_send: f64,
    first_arrival: f64,
    last_arrival: f64,
}

impl Group {
    /// Пакет — из той же пачки доставки (GCC, `BelongsToBurst`).
    fn in_burst(&self, send_ms: f64, arrival_ms: f64) -> bool {
        let arrival_delta = arrival_ms - self.last_arrival;
        let send_delta = send_ms - self.last_send;
        send_delta == 0.0
            || arrival_delta - send_delta < 0.0
                && arrival_delta <= BURST_GAP_MS
                && arrival_ms - self.first_arrival < MAX_BURST_MS
    }
}

/// Тренд задержки между группами пакетов и решение «перегрузка / норма / разгрузка».
pub(crate) struct DelayDetector {
    cur: Option<Group>,
    prev: Option<Group>,
    acc: f64,
    smoothed: f64,
    first_arrival: Option<f64>,
    window: VecDeque<(f64, f64)>,
    deltas: u32,
    threshold: f64,
    threshold_at: Option<f64>,
    overuse_ms: f64,
    overuse_count: u32,
    prev_trend: f64,
    usage: Usage,
}

impl Default for DelayDetector {
    fn default() -> Self {
        DelayDetector {
            cur: None,
            prev: None,
            acc: 0.0,
            smoothed: 0.0,
            first_arrival: None,
            window: VecDeque::new(),
            deltas: 0,
            threshold: THRESHOLD_INIT,
            threshold_at: None,
            overuse_ms: 0.0,
            overuse_count: 0,
            prev_trend: 0.0,
            usage: Usage::Normal,
        }
    }
}

impl DelayDetector {
    /// Пакет, отправленный в `send_ms` (наши часы) и пришедший в `arrival_ms` (часы
    /// получателя: важны только разности). Пакеты — в порядке отправки.
    pub fn on_packet(&mut self, send_ms: f64, arrival_ms: f64) {
        if let Some(g) = &mut self.cur {
            if send_ms < g.first_send {
                return; // ушёл раньше группы — переставился, не учитываем
            }
            if send_ms - g.first_send <= GROUP_SPAN_MS || g.in_burst(send_ms, arrival_ms) {
                g.last_send = g.last_send.max(send_ms);
                g.last_arrival = g.last_arrival.max(arrival_ms);
                return;
            }
        }
        if let (Some(prev), Some(cur)) = (&self.prev, &self.cur) {
            let send_delta = cur.last_send - prev.last_send;
            let arrival_delta = cur.last_arrival - prev.last_arrival;
            let at = cur.last_arrival;
            self.update(arrival_delta - send_delta, send_delta, at);
        }
        self.prev = self.cur.take();
        self.cur = Some(Group {
            first_send: send_ms,
            last_send: send_ms,
            first_arrival: arrival_ms,
            last_arrival: arrival_ms,
        });
    }

    pub fn usage(&self) -> Usage {
        self.usage
    }

    fn update(&mut self, delay_delta: f64, send_delta: f64, arrival: f64) {
        self.deltas = (self.deltas + 1).min(1000);
        self.acc += delay_delta;
        self.smoothed = SMOOTHING * self.smoothed + (1.0 - SMOOTHING) * self.acc;
        let first = *self.first_arrival.get_or_insert(arrival);
        self.window.push_back((arrival - first, self.smoothed));
        if self.window.len() > TREND_WINDOW {
            self.window.pop_front();
        }
        let trend = if self.window.len() == TREND_WINDOW {
            slope(&self.window).unwrap_or(self.prev_trend)
        } else {
            self.prev_trend
        };
        let modified = self.deltas.min(60) as f64 * trend * TREND_GAIN;
        if modified > self.threshold {
            self.overuse_ms += send_delta;
            self.overuse_count += 1;
            if self.overuse_ms > OVERUSE_TIME_MS
                && self.overuse_count > 1
                && trend >= self.prev_trend
            {
                self.overuse_ms = 0.0;
                self.overuse_count = 0;
                self.usage = Usage::Over;
            }
        } else {
            self.overuse_ms = 0.0;
            self.overuse_count = 0;
            self.usage = if modified < -self.threshold {
                Usage::Under
            } else {
                Usage::Normal
            };
        }
        self.prev_trend = trend;
        self.adapt_threshold(modified, arrival);
    }

    /// Порог следует за |трендом|: иначе на канале с постоянным разбросом задержки
    /// (или в конкуренции с TCP) мы бы сбавляли без конца.
    fn adapt_threshold(&mut self, modified: f64, now: f64) {
        let last = *self.threshold_at.get_or_insert(now);
        self.threshold_at = Some(now);
        if modified.abs() > self.threshold + 15.0 {
            return; // всплеск — не повод двигать порог
        }
        let k = if modified.abs() < self.threshold {
            K_DOWN
        } else {
            K_UP
        };
        let dt = (now - last).clamp(0.0, 100.0);
        self.threshold += k * (modified.abs() - self.threshold) * dt;
        self.threshold = self.threshold.clamp(THRESHOLD_MIN, THRESHOLD_MAX);
    }
}

/// Наклон прямой по точкам (метод наименьших квадратов).
fn slope(points: &VecDeque<(f64, f64)>) -> Option<f64> {
    let n = points.len() as f64;
    let (sx, sy) = points
        .iter()
        .fold((0.0, 0.0), |(sx, sy), &(x, y)| (sx + x, sy + y));
    let (mx, my) = (sx / n, sy / n);
    let (num, den) = points.iter().fold((0.0, 0.0), |(num, den), &(x, y)| {
        (num + (x - mx) * (y - my), den + (x - mx) * (x - mx))
    });
    (den > 0.0).then(|| num / den)
}

/// Оценка полосы до одного участника.
pub(crate) struct Bwe {
    sends: SendLog,
    delay: DelayDetector,
    /// Итог, бит/с: меньшая из оценок по задержке и по потерям.
    rate: f64,
    delay_rate: f64,
    loss_rate: f64,
    min: f64,
    max: f64,
    /// Когда последний раз сбавляли (наши мс) — не чаще раза в RTT.
    decreased_at: Option<f64>,
    /// Дошедший битрейт при последнем сбросе — около него прибавляем осторожно.
    capacity: Option<f64>,
    updated_at: Option<f64>,
    /// Дошедшие пакеты по времени прихода (мс получателя, байт).
    acked: VecDeque<(f64, u32)>,
    /// Потери в текущем окне: (потеряно, всего, начало окна).
    loss_window: (u32, u32, Option<f64>),
    /// Потери в последнем окне сверх собственных.
    loss: f64,
    inherent_loss: f64,
    /// Развёрнутое время получателя: (последнее `ref_time`, его значение без оборота).
    ref_clock: Option<(u32, i64)>,
    /// Сколько раз сбавляли из-за роста задержки и из-за потерь — для статистики.
    pub cuts_delay: u64,
    pub cuts_loss: u64,
}

impl Bwe {
    /// Начинаем с `max`: до первой перегрузки причин сбавлять нет, а разгон с малого
    /// по 8 % в секунду занял бы десятки секунд плохой картинки.
    pub fn new(min: f64, max: f64) -> Self {
        Bwe {
            sends: SendLog::default(),
            delay: DelayDetector::default(),
            rate: max,
            delay_rate: max,
            loss_rate: max,
            min,
            max,
            decreased_at: None,
            capacity: None,
            updated_at: None,
            acked: VecDeque::new(),
            loss_window: (0, 0, None),
            loss: 0.0,
            inherent_loss: 0.0,
            ref_clock: None,
            cuts_delay: 0,
            cuts_loss: 0,
        }
    }

    pub fn set_bounds(&mut self, min: f64, max: f64) {
        self.min = min;
        self.max = max;
        // Сведений о канале ещё нет — начинаем с нового потолка, как в [`Self::new`].
        if self.updated_at.is_none() {
            (self.delay_rate, self.loss_rate) = (max, max);
        }
        self.delay_rate = self.delay_rate.clamp(min, max);
        self.loss_rate = self.loss_rate.clamp(min, max);
        self.rate = self.delay_rate.min(self.loss_rate);
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    pub fn on_sent(&mut self, seq: u16, at_ms: f64, size: u32) {
        self.sends.push(seq, at_ms, size);
    }

    /// Обратная связь пришла в `now_ms` (наши часы); `rtt_ms` — текущий RTT.
    pub fn on_feedback(&mut self, now_ms: f64, fb: &TransportFeedback, rtt_ms: f64) {
        let ref_ms = self.unwrap_ref(fb.ref_time) as f64 * FEEDBACK_TICK_US as f64 / 1000.0;
        let (mut lost, mut total) = (0u32, 0u32);
        for (i, a) in fb.arrivals.iter().enumerate() {
            let seq = fb.base_seq.wrapping_add(i as u16);
            let Some((send_ms, size)) = self.sends.get(seq) else {
                continue;
            };
            total += 1;
            match a {
                Some(d) => {
                    let arrival = ref_ms + *d as f64 * FEEDBACK_TICK_US as f64 / 1000.0;
                    self.delay.on_packet(send_ms, arrival);
                    self.acked.push_back((arrival, size));
                }
                None => lost += 1,
            }
        }
        self.count_loss(now_ms, lost, total);
        self.update(now_ms, rtt_ms);
    }

    fn unwrap_ref(&mut self, t: u32) -> i64 {
        let v = match self.ref_clock {
            Some((last, base)) => base + t.wrapping_sub(last) as i32 as i64,
            None => t as i64,
        };
        self.ref_clock = Some((t, v));
        v
    }

    fn count_loss(&mut self, now_ms: f64, lost: u32, total: u32) {
        let (l, t, since) = &mut self.loss_window;
        *l += lost;
        *t += total;
        let start = *since.get_or_insert(now_ms);
        if now_ms - start >= LOSS_WINDOW_MS && *t >= 10 {
            let loss = *l as f64 / *t as f64;
            self.loss = (loss - self.inherent_loss).max(0.0);
            self.inherent_loss = if loss < self.inherent_loss {
                loss
            } else {
                self.inherent_loss + INHERENT_LOSS_RISE * (loss - self.inherent_loss)
            }
            .min(INHERENT_LOSS_MAX);
            self.loss_window = (0, 0, Some(now_ms));
        }
    }

    /// Дошедший битрейт за последние полсекунды прихода.
    fn acked_bps(&mut self) -> Option<f64> {
        let last = self.acked.back()?.0;
        while self
            .acked
            .front()
            .is_some_and(|&(t, _)| last - t > ACKED_WINDOW_MS)
        {
            self.acked.pop_front();
        }
        let span = last - self.acked.front()?.0;
        if span < ACKED_WINDOW_MS / 2.0 {
            return None;
        }
        let bytes: u64 = self.acked.iter().map(|&(_, s)| s as u64).sum();
        Some(bytes as f64 * 8.0 * 1000.0 / span)
    }

    fn update(&mut self, now_ms: f64, rtt_ms: f64) {
        let dt = self
            .updated_at
            .map_or(0.0, |t| (now_ms - t).clamp(0.0, 1000.0));
        self.updated_at = Some(now_ms);
        let acked = self.acked_bps();
        let may_decrease = self
            .decreased_at
            .is_none_or(|t| now_ms - t >= rtt_ms.max(100.0) + 100.0);
        let grow = INCREASE_PER_SEC.powf(dt / 1000.0);
        // Потери: сбавляем от того, что шлём сейчас, — иначе от далёкого потолка
        // сброс ничего бы не значил.
        if self.loss > LOSS_HIGH && may_decrease {
            self.loss_rate = self.loss_rate.min(self.rate) * (1.0 - 0.5 * self.loss);
            self.decreased_at = Some(now_ms);
            self.cuts_loss += 1;
        } else if self.loss < LOSS_LOW {
            self.loss_rate *= grow;
        }
        let usage = self.delay.usage();
        if usage == Usage::Over && may_decrease {
            let base = acked.map_or(self.rate, |a| a.min(self.rate));
            self.delay_rate = BACKOFF * base;
            self.capacity = Some(base);
            self.decreased_at = Some(now_ms);
            self.cuts_delay += 1;
        } else if usage == Usage::Normal {
            // Дошедшее заметно выше прошлой ёмкости — канал стал шире, она устарела.
            if let (Some(c), Some(a)) = (self.capacity, acked) {
                if a > c * 1.5 {
                    self.capacity = None;
                }
            }
            let near_capacity = self.capacity.is_some_and(|c| self.delay_rate > c * 0.8);
            if near_capacity {
                // Около известной ёмкости — по пакету за время отклика.
                let response_s = (rtt_ms + 100.0) / 1000.0;
                self.delay_rate += 1200.0 * 8.0 / response_s * dt / 1000.0;
            } else {
                self.delay_rate *= grow;
            }
        }
        self.delay_rate = self.delay_rate.clamp(self.min, self.max);
        self.loss_rate = self.loss_rate.clamp(self.min, self.max);
        self.rate = self.delay_rate.min(self.loss_rate);
    }
}

/// Когда пришли пакеты участника — для обратной связи ему (сторона получателя).
#[derive(Default)]
pub(crate) struct ArrivalLog {
    /// Первый сквозной номер, о котором ещё не сообщали.
    next: Option<u16>,
    /// (сквозной номер, приход в единицах [`FEEDBACK_TICK_US`]) с прошлой обратной связи.
    got: Vec<(u16, u64)>,
}

impl ArrivalLog {
    pub fn on_packet(&mut self, seq: u16, ticks: u64) {
        // Предел — на случай, если таймер долго не зовут: больше в одну обратную
        // связь всё равно не войдёт.
        if self.got.len() < 4 * MAX_FEEDBACK_PACKETS {
            self.got.push((seq, ticks));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.got.is_empty()
    }

    /// Обратная связь о пришедшем с прошлого раза: подряд от первого не
    /// сообщённого номера до самого нового, с пропусками на месте непришедших.
    /// Опоздавшие к уже сообщённому диапазону не учитываются.
    pub fn take(&mut self) -> Option<TransportFeedback> {
        let got = std::mem::take(&mut self.got);
        let newer = |a: u16, b: u16| (a.wrapping_sub(b) as i16) > 0;
        let end = got
            .iter()
            .map(|&(s, _)| s)
            .reduce(|a, b| if newer(b, a) { b } else { a })?;
        let oldest = got
            .iter()
            .map(|&(s, _)| s)
            .reduce(|a, b| if newer(a, b) { b } else { a })?;
        let mut start = self.next.unwrap_or(oldest);
        if newer(start, end) {
            return None; // всё пришедшее — опоздавшее
        }
        let mut count = end.wrapping_sub(start) as usize + 1;
        if count > MAX_FEEDBACK_PACKETS {
            start = end.wrapping_sub(MAX_FEEDBACK_PACKETS as u16 - 1);
            count = MAX_FEEDBACK_PACKETS;
        }
        self.next = Some(end.wrapping_add(1));
        let mut at: Vec<Option<u64>> = vec![None; count];
        for (seq, t) in got {
            let i = seq.wrapping_sub(start) as usize;
            if let Some(slot) = at.get_mut(i) {
                *slot = Some(slot.map_or(t, |old: u64| old.min(t)));
            }
        }
        let first = at.iter().flatten().min().copied()?;
        Some(TransportFeedback {
            base_seq: start,
            ref_time: first as u32,
            arrivals: at
                .into_iter()
                .map(|t| t.map(|t| (t - first).min(u16::MAX as u64 - 1) as u16))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ровный поток без очереди — норма; задержка каждой группы растёт — перегрузка;
    /// очередь рассасывается — разгрузка.
    #[test]
    fn delay_trend_detects_queue_growth() {
        let mut d = DelayDetector::default();
        for i in 0..100 {
            let t = i as f64 * 10.0;
            d.on_packet(t, t + 50.0);
        }
        assert_eq!(d.usage(), Usage::Normal);
        for i in 100..200 {
            let t = i as f64 * 10.0;
            let queue = (i - 100) as f64 * 2.0; // +2 мс на каждую группу
            d.on_packet(t, t + 50.0 + queue);
        }
        assert_eq!(d.usage(), Usage::Over);
        for i in 200..260 {
            let t = i as f64 * 10.0;
            let queue = (200.0 - (i - 200) as f64 * 3.0).max(0.0);
            d.on_packet(t, t + 50.0 + queue);
        }
        assert_ne!(d.usage(), Usage::Over);
    }

    /// Сеть доставляет пачками (меш поверх TCP: копит и отдаёт разом) — это не
    /// очередь, которая растёт.
    #[test]
    fn bursty_delivery_is_not_overuse() {
        let mut d = DelayDetector::default();
        let mut overuse = 0;
        for i in 0..3000 {
            let send = i as f64 * 2.0; // пакет раз в 2 мс
                                       // Всё, что ушло за 60 мс, приходит одной пачкой в конце окна.
            let arrival = (send / 60.0).floor() * 60.0 + 60.0 + 50.0 + (i % 30) as f64 * 0.05;
            d.on_packet(send, arrival);
            overuse += (d.usage() == Usage::Over) as u32;
        }
        assert_eq!(overuse, 0, "ложных перегрузок {overuse}");
    }

    /// Случайный разброс задержки без роста — не перегрузка.
    #[test]
    fn jitter_alone_is_not_overuse() {
        let mut d = DelayDetector::default();
        let mut seed = 7u32;
        let mut overuse = 0;
        for i in 0..2000 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let jitter = (seed >> 16) as f64 % 20.0;
            let t = i as f64 * 10.0;
            d.on_packet(t, t + 50.0 + jitter);
            overuse += (d.usage() == Usage::Over) as u32;
        }
        assert!(overuse < 20, "ложных перегрузок {overuse}");
    }

    fn feedback(base: u16, arrivals: &[Option<f64>]) -> TransportFeedback {
        let first = arrivals.iter().flatten().fold(f64::MAX, |a, &b| a.min(b));
        TransportFeedback {
            base_seq: base,
            ref_time: (first * 4.0) as u32,
            arrivals: arrivals
                .iter()
                .map(|a| a.map(|t| ((t - first) * 4.0) as u16))
                .collect(),
        }
    }

    /// Высокие потери — сбавляем, но не чаще раза в RTT; без потерь — прибавляем.
    #[test]
    fn loss_backs_off_and_clean_link_ramps_up() {
        let mut b = Bwe::new(100_000.0, 4_000_000.0);
        (b.rate, b.delay_rate, b.loss_rate) = (2_000_000.0, 2_000_000.0, 2_000_000.0);
        let mut seq = 0u16;
        let mut now = 0.0;
        // 20 % потерь в течение 3 с.
        for _ in 0..30 {
            let mut arr = Vec::new();
            for i in 0..10 {
                b.on_sent(seq, now + i as f64 * 10.0, 1000);
                seq = seq.wrapping_add(1);
                arr.push((i % 5 != 0).then_some(now + 50.0 + i as f64 * 10.0));
            }
            now += 100.0;
            b.on_feedback(now, &feedback(seq.wrapping_sub(10), &arr), 100.0);
        }
        let after_loss = b.rate();
        assert!(after_loss < 1_000_000.0, "после потерь {after_loss}");
        // Чисто 8 с; первую секунду окно потерь ещё помнит плохое, и оценка падает.
        let mut lowest = after_loss;
        for _ in 0..80 {
            let mut arr = Vec::new();
            for i in 0..10 {
                b.on_sent(seq, now + i as f64 * 10.0, 1000);
                seq = seq.wrapping_add(1);
                arr.push(Some(now + 50.0 + i as f64 * 10.0));
            }
            now += 100.0;
            b.on_feedback(now, &feedback(seq.wrapping_sub(10), &arr), 100.0);
            lowest = lowest.min(b.rate());
        }
        assert!(b.rate() > lowest * 1.5, "разгон {lowest} → {}", b.rate());
    }

    /// Пропуски — на своих местах, опоздавшие к сообщённому — мимо.
    #[test]
    fn arrival_log_covers_gaps_and_ignores_late() {
        let mut a = ArrivalLog::default();
        a.on_packet(65_535, 100);
        a.on_packet(1, 104);
        a.on_packet(2, 103);
        let f = a.take().unwrap();
        assert_eq!(f.base_seq, 65_535);
        assert_eq!(f.ref_time, 100);
        assert_eq!(f.arrivals, [Some(0), None, Some(4), Some(3)]);
        a.on_packet(0, 110); // опоздал: о нём уже сообщили как о потерянном
        assert_eq!(a.take(), None);
        a.on_packet(4, 120);
        let f = a.take().unwrap();
        assert_eq!((f.base_seq, f.arrivals), (3, vec![None, Some(0)]));
    }

    #[test]
    fn send_log_finds_by_seq_and_forgets_old() {
        let mut l = SendLog::default();
        for i in 0..10u16 {
            l.push(65_530u16.wrapping_add(i), i as f64, i as u32);
        }
        assert_eq!(l.get(65_535), Some((5.0, 5)));
        assert_eq!(l.get(3), Some((9.0, 9)));
        assert_eq!(l.get(4), None);
        l.push(4, 20_000.0, 1);
        assert_eq!(l.get(3), None, "старше десяти секунд — забыто");
        assert_eq!(l.get(4), Some((20_000.0, 1)));
    }
}
