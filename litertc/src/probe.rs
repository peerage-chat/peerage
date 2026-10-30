//! Пробы полосы (ADR-014 §6) — как в WebRTC (ProbeController и
//! ProbeBitrateEstimator).
//!
//! Оценка стартует снизу ([`crate::bwe`]): начни она с потолка, в узком месте с
//! первой секунды стояла бы очередь, а детектор задержки ловит её рост, не её саму, —
//! и не сработал бы ни разу. Но разгон снизу по 8 % в секунду до пары Мбит/с — полминуты
//! плохой картинки. Поэтому — пробы: короткая пачка пустых пакетов (трек
//! [`crate::wire::TRACK_PADDING`]) ровным темпом выше оценки. По обратной связи видно,
//! с каким темпом пачка пришла: так же, как ушла, — канал держит не меньше, и оценку
//! поднимаем сразу; растянулась — канал уже, и пришедший темп — его мера. В начале
//! звонка две пробы (3× и 6× старта), дальше, пока каждая подтверждается, — вдвое
//! выше новой оценки, до потолка.

use std::collections::VecDeque;

/// Длительность пробы. У WebRTC от 15 мс, у нас дольше: линки Yggdrasil идут поверх
/// TCP и отдают пакеты пачками, и на коротком отрезке темп прихода — шум.
const PROBE_MS: f64 = 60.0;
/// Пакетов в пробе.
const PROBE_PACKETS: usize = 10;
/// Пакет пробы не мельче этого: иначе заголовки дороже самой пробы.
const MIN_PACKET: usize = 200;
/// Пробы в начале звонка — во столько раз выше стартовой оценки.
const INITIAL_SCALES: [f64; 2] = [3.0, 6.0];
/// Подтвердилась — следующая во столько раз выше новой оценки.
const FURTHER_SCALE: f64 = 2.0;
/// Подтвердилась — если показала хотя бы такую долю своего темпа.
const FURTHER_THRESHOLD: f64 = 0.7;
/// Дошло меньше этой доли пакетов — проба ничего не говорит.
const MIN_RECEIVED: f64 = 0.8;
/// Пришла медленнее этой доли ушедшего темпа — канал насыщен, его мера — пришедший
/// темп с запасом.
const SATURATED: f64 = 0.9;
const SATURATED_UTILIZATION: f64 = 0.95;
/// Ничего не показавшую пробу (потери, перестановки) повторяем не больше стольких
/// раз подряд: где проба не проходит раз за разом, канал меряют задержка и потери.
const RETRIES: u32 = 2;
/// Проба без ответа дольше этого забывается (её пакеты забыл журнал отправленного).
const RESULT_TIMEOUT_MS: f64 = 10_000.0;

/// Итог пробы.
#[derive(Debug, PartialEq)]
pub(crate) struct Outcome {
    /// Показанная полоса, бит/с; `None` — проба ничего не показала.
    pub shown: Option<f64>,
    /// Темп пробы, бит/с.
    pub target: f64,
}

struct Cluster {
    /// Темп, бит/с.
    target: f64,
    packet: usize,
    /// Отправлено: (сквозной номер, наши мс, байт).
    sent: Vec<(u16, f64, u32)>,
    /// Дошло: (наши мс отправки, мс прихода у получателя, байт).
    got: Vec<(f64, f64, u32)>,
    /// О скольких пакетах пробы уже сообщили (дошёл или потерян).
    reported: usize,
}

impl Cluster {
    fn new(target: f64) -> Self {
        let packet = (target / 8.0 * PROBE_MS / 1000.0 / PROBE_PACKETS as f64) as usize;
        Cluster {
            target,
            packet: packet.max(MIN_PACKET),
            sent: Vec::with_capacity(PROBE_PACKETS),
            got: Vec::with_capacity(PROBE_PACKETS),
            reported: 0,
        }
    }

    /// Когда следующий пакет: по расписанию от первого, а не от прошлого — таймер
    /// будит не раньше срока, и опоздания, копясь, замедляли бы пробу.
    fn next_at(&self) -> Option<f64> {
        let &(_, first, _) = self.sent.first()?;
        let bytes: u64 = self.sent.iter().map(|&(_, _, s)| s as u64).sum();
        Some(first + bytes as f64 * 8000.0 / self.target)
    }

    fn holds(&self, seq: u16) -> bool {
        self.sent.iter().any(|&(s, _, _)| s == seq)
    }

    /// Какую полосу показала, бит/с; `None` — ничего не показала.
    fn estimate(&self) -> Option<f64> {
        let got = &self.got;
        if got.len() < 2 || (got.len() as f64) < MIN_RECEIVED * self.sent.len() as f64 {
            return None;
        }
        let total: u64 = got.iter().map(|&(_, _, s)| s as u64).sum();
        // Темп — байты между первым и последним: последний ушедший уже не в счёт
        // отправки, первый пришедший — не в счёт прихода.
        let by = |key: fn(&(f64, f64, u32)) -> f64| {
            let first = got.iter().min_by(|a, b| key(a).total_cmp(&key(b)))?;
            let last = got.iter().max_by(|a, b| key(a).total_cmp(&key(b)))?;
            Some((key(last) - key(first), first.2 as u64, last.2 as u64))
        };
        let (send_ms, _, last_sent) = by(|g| g.0)?;
        let (recv_ms, first_got, _) = by(|g| g.1)?;
        if send_ms <= 0.0 {
            return None;
        }
        let send = (total - last_sent) as f64 * 8000.0 / send_ms;
        // Пришла разом (сеть отдала пачкой) — канал не уже ушедшего темпа.
        let recv = if recv_ms > 0.0 {
            (total - first_got) as f64 * 8000.0 / recv_ms
        } else {
            f64::INFINITY
        };
        Some(if recv < SATURATED * send {
            SATURATED_UTILIZATION * recv
        } else {
            send.min(recv)
        })
    }
}

/// Пробы до одного участника.
#[derive(Default)]
pub(crate) struct Prober {
    /// Ждут отправки; первая отправляется.
    queue: VecDeque<Cluster>,
    /// Когда закончилась по расписанию прошлая (наши мс) — раньше следующая не
    /// начинается.
    next_at: Option<f64>,
    /// Отправлены целиком, ждут обратной связи.
    measuring: VecDeque<Cluster>,
    started: bool,
    /// Сколько проб подряд ничего не показали.
    failed: u32,
}

impl Prober {
    /// Начало звонка: пробы выше стартовой оценки `rate`, но не выше `max`. Один раз.
    pub fn start(&mut self, rate: f64, max: f64) {
        if std::mem::replace(&mut self.started, true) {
            return;
        }
        for k in INITIAL_SCALES {
            self.push(rate, k * rate, max);
        }
    }

    fn push(&mut self, rate: f64, target: f64, max: f64) {
        let target = target.min(max);
        if target > rate {
            self.queue.push_back(Cluster::new(target));
        }
    }

    /// Когда отправить следующий пакет пробы (наши мс), какого размера и начата ли
    /// уже эта проба.
    pub fn due(&self) -> Option<(f64, usize, bool)> {
        let c = self.queue.front()?;
        let at = c.next_at().or(self.next_at).unwrap_or(0.0);
        Some((at, c.packet, !c.sent.is_empty()))
    }

    pub fn on_sent(&mut self, seq: u16, at_ms: f64, size: u32) {
        let Some(c) = self.queue.front_mut() else {
            return;
        };
        c.sent.push((seq, at_ms, size));
        if c.sent.len() >= PROBE_PACKETS {
            self.next_at = c.next_at();
            self.measuring.extend(self.queue.pop_front());
        }
    }

    /// Обратная связь о пакете `seq`, ушедшем в `send_ms`: пришёл в `arrival` (мс
    /// получателя) или потерян.
    pub fn on_report(&mut self, seq: u16, send_ms: f64, arrival: Option<f64>, size: u32) {
        let in_flight = self.queue.front_mut();
        let Some(c) = self
            .measuring
            .iter_mut()
            .chain(in_flight)
            .find(|c| c.holds(seq))
        else {
            return;
        };
        c.reported += 1;
        if let Some(a) = arrival {
            c.got.push((send_ms, a, size));
        }
    }

    /// Итоги проб, о каждом пакете которых уже сообщили (или которые пора забыть),
    /// по порядку. Зовущий поднимает по ним оценку и зовёт [`Prober::next`].
    pub fn take_results(&mut self, now_ms: f64) -> Vec<Outcome> {
        let mut out = Vec::new();
        while let Some(c) = self.measuring.front() {
            let done = c.reported >= c.sent.len();
            let stale = c
                .sent
                .last()
                .is_some_and(|&(_, t, _)| now_ms - t > RESULT_TIMEOUT_MS);
            if !done && !stale {
                break;
            }
            let Some(c) = self.measuring.pop_front() else {
                break;
            };
            out.push(Outcome {
                shown: c.estimate().filter(|_| done),
                target: c.target,
            });
        }
        out
    }

    /// Проба закончилась так, оценка теперь `rate`. Если других проб нет:
    /// подтвердилась — следующая вдвое выше оценки; ничего не показала — ещё раз
    /// (не больше [`RETRIES`] подряд); показала меньше — канал уже, хватит.
    pub fn next(&mut self, o: &Outcome, rate: f64, max: f64) {
        if !self.queue.is_empty() || !self.measuring.is_empty() {
            return;
        }
        match o.shown {
            Some(shown) => {
                self.failed = 0;
                if shown >= FURTHER_THRESHOLD * o.target {
                    self.push(rate, FURTHER_SCALE * rate, max);
                }
            }
            None if self.failed < RETRIES => {
                self.failed += 1;
                self.push(rate, o.target, max);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Отправить пробу целиком с её темпом; пакет `i` приходит в `arrive(i)` или
    /// теряется.
    fn run(p: &mut Prober, first_seq: u16, arrive: impl Fn(usize, f64) -> Option<f64>) {
        let mut sent = Vec::new();
        for i in 0..PROBE_PACKETS {
            let (at, size, _) = p.due().unwrap();
            let at = at.max(1000.0);
            let seq = first_seq + i as u16;
            p.on_sent(seq, at, size as u32);
            sent.push((seq, at, size as u32));
        }
        for (i, &(seq, at, size)) in sent.iter().enumerate() {
            p.on_report(seq, at, arrive(i, at), size);
        }
    }

    /// Пришла так же ровно, как ушла, — показала свой темп и зовёт следующую,
    /// вдвое выше новой оценки.
    #[test]
    fn clean_probe_shows_its_rate_and_asks_for_more() {
        let mut p = Prober::default();
        p.start(300_000.0, 10_000_000.0);
        run(&mut p, 0, |_, at| Some(at + 50.0));
        let o = p.take_results(2000.0).pop().unwrap();
        assert_eq!(o.target, 900_000.0);
        let shown = o.shown.unwrap();
        assert!((shown - 900_000.0).abs() < 1000.0, "{shown}");
        run(&mut p, 10, |_, at| Some(at + 50.0));
        let o = p.take_results(2000.0).pop().unwrap();
        p.next(&o, o.shown.unwrap(), 10_000_000.0);
        let (_, size, _) = p.due().unwrap();
        // 3,6 Мбит/с × 60 мс / 10 пакетов
        assert_eq!(size, 2700);
    }

    /// Узкое место растянуло пачку — её мера пришедший темп с запасом, и выше не
    /// пробуем.
    #[test]
    fn stretched_probe_shows_the_bottleneck() {
        let mut p = Prober::default();
        p.start(300_000.0, 10_000_000.0);
        // Пакеты по 675 байт через узкое место 300 кбит/с — 18 мс на пакет.
        run(&mut p, 0, |i, _| Some(1000.0 + i as f64 * 18.0));
        let shown = p.take_results(2000.0)[0].shown.unwrap();
        assert!((shown - 285_000.0).abs() < 1000.0, "{shown}");
        // Вторая (1,8 Мбит/с, по 1350 байт) — тоже.
        run(&mut p, 10, |i, _| Some(2000.0 + i as f64 * 36.0));
        let o = p.take_results(3000.0).pop().unwrap();
        p.next(&o, 300_000.0, 10_000_000.0);
        assert!(p.due().is_none(), "после неподтверждённой проб больше нет");
    }

    /// Дошло мало — проба ничего не говорит; без ответа — со временем забывается.
    /// Не показавшую повторяем, но не без конца.
    #[test]
    fn lossy_or_silent_probe_shows_nothing_and_is_retried_a_little() {
        let mut p = Prober::default();
        p.start(300_000.0, 10_000_000.0);
        run(&mut p, 0, |i, at| (i % 3 != 0).then_some(at + 50.0));
        let nothing = |target| Outcome {
            shown: None,
            target,
        };
        assert_eq!(p.take_results(2000.0), [nothing(900_000.0)]);
        for _ in 0..PROBE_PACKETS {
            let (at, size, _) = p.due().unwrap();
            p.on_sent(0, at, size as u32);
        }
        assert_eq!(p.take_results(5000.0), []);
        assert_eq!(p.measuring.len(), 1);
        let o = p.take_results(20_000.0);
        assert_eq!(o, [nothing(1_800_000.0)]);
        for _ in 0..RETRIES {
            p.next(&o[0], 300_000.0, 10_000_000.0);
            assert_eq!(p.queue.pop_front().map(|c| c.target), Some(1_800_000.0));
        }
        p.next(&o[0], 300_000.0, 10_000_000.0);
        assert!(p.due().is_none());
    }

    /// Старт у потолка — пробовать нечего.
    #[test]
    fn no_probes_above_max() {
        let mut p = Prober::default();
        p.start(1_000_000.0, 1_000_000.0);
        assert!(p.due().is_none());
    }
}
