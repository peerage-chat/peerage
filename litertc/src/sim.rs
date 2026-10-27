//! Симулятор сети для тестов: однонаправленный канал на виртуальном времени.
//!
//! Моделирует то, что в звонке проверяли руками на двух телефонах (ADR-014 §7):
//! задержку с разбросом (отсюда и переупорядочивание), случайные потери и потери
//! пачками (модель Гилберта–Эллиотта), узкое место по полосе с очередью ограниченного
//! размера (переполнение — выброс хвоста, как у роутера). Никаких часов и сна: время
//! передаёт тест, случайность — детерминированная от зерна, прогон воспроизводим.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

/// Потери пачками: канал переходит между «хорошим» и «плохим» состоянием.
#[derive(Clone, Copy, Debug)]
pub struct BurstLoss {
    /// Вероятность на пакет перейти из хорошего состояния в плохое.
    pub enter: f64,
    /// Вероятность на пакет вернуться в хорошее.
    pub leave: f64,
    /// Доля потерь в плохом состоянии.
    pub loss_in_burst: f64,
}

/// Параметры канала.
#[derive(Clone, Copy, Debug)]
pub struct LinkConfig {
    /// Базовая односторонняя задержка.
    pub delay: Duration,
    /// Разброс задержки: к базовой прибавляется равномерно от 0 до `jitter`.
    /// Пакеты с разной задержкой обгоняют друг друга.
    pub jitter: Duration,
    /// Доля случайных потерь (0.0…1.0) в хорошем состоянии.
    pub loss: f64,
    /// Потери пачками поверх случайных.
    pub burst: Option<BurstLoss>,
    /// Полоса узкого места, бит/с; `None` — без ограничения.
    pub bandwidth_bps: Option<u64>,
    /// Сколько байт помещается в очередь узкого места; сверх — выброс.
    pub queue_bytes: usize,
}

impl Default for LinkConfig {
    fn default() -> Self {
        LinkConfig {
            delay: Duration::from_millis(20),
            jitter: Duration::ZERO,
            loss: 0.0,
            burst: None,
            bandwidth_bps: None,
            queue_bytes: 256 * 1024,
        }
    }
}

/// Счётчики канала.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkStats {
    pub sent: u64,
    pub delivered: u64,
    /// Потеряно моделью потерь (случайные и пачками).
    pub lost: u64,
    /// Выброшено переполненной очередью узкого места.
    pub queue_drops: u64,
}

/// Однонаправленный канал. Двусторонний звонок — два канала.
pub struct Link {
    cfg: LinkConfig,
    rng: Rng,
    in_burst: bool,
    /// Когда узкое место освободится (последний пакет полностью «вытолкнут»).
    busy_until: Option<Instant>,
    /// В пути: (момент доставки, порядковый номер для устойчивости, данные).
    in_flight: BinaryHeap<Reverse<(Instant, u64, Vec<u8>)>>,
    next_id: u64,
    stats: LinkStats,
}

impl Link {
    pub fn new(cfg: LinkConfig, seed: u64) -> Self {
        Link {
            cfg,
            rng: Rng::new(seed),
            in_burst: false,
            busy_until: None,
            in_flight: BinaryHeap::new(),
            next_id: 0,
            stats: LinkStats::default(),
        }
    }

    /// Поменять параметры на ходу (например, просадка полосы посреди звонка).
    /// Пакеты, уже находящиеся в пути, доезжают по старым.
    pub fn set_config(&mut self, cfg: LinkConfig) {
        self.cfg = cfg;
    }

    /// Отправить пакет в момент `now`. Потерянный или выброшенный пакет просто
    /// не будет доставлен — как в настоящей сети, отправитель об этом не знает.
    pub fn send(&mut self, now: Instant, packet: Vec<u8>) {
        self.stats.sent += 1;
        if self.lose() {
            self.stats.lost += 1;
            return;
        }
        // Узкое место: пакет выходит, когда освободится канал, и занимает его на
        // время передачи своих байт.
        let leaves_at = match self.cfg.bandwidth_bps {
            Some(bps) if bps > 0 => {
                let start = self.busy_until.map_or(now, |b| b.max(now));
                let backlog = start.saturating_duration_since(now);
                let backlog_bytes = (backlog.as_secs_f64() * bps as f64 / 8.0) as usize;
                if backlog_bytes + packet.len() > self.cfg.queue_bytes {
                    self.stats.queue_drops += 1;
                    return;
                }
                let tx = Duration::from_secs_f64(packet.len() as f64 * 8.0 / bps as f64);
                let done = start + tx;
                self.busy_until = Some(done);
                done
            }
            _ => now,
        };
        let jitter = self.cfg.jitter.mul_f64(self.rng.unit());
        let at = leaves_at + self.cfg.delay + jitter;
        self.in_flight.push(Reverse((at, self.next_id, packet)));
        self.next_id += 1;
    }

    /// Забрать следующий пакет, доставленный к моменту `now`.
    pub fn poll(&mut self, now: Instant) -> Option<Vec<u8>> {
        let Reverse((at, _, _)) = self.in_flight.peek()?;
        if *at > now {
            return None;
        }
        let Reverse((_, _, packet)) = self.in_flight.pop()?;
        self.stats.delivered += 1;
        Some(packet)
    }

    /// Когда доставится ближайший пакет (для продвижения виртуального времени).
    pub fn next_delivery(&self) -> Option<Instant> {
        self.in_flight.peek().map(|Reverse((at, _, _))| *at)
    }

    pub fn stats(&self) -> LinkStats {
        self.stats
    }

    fn lose(&mut self) -> bool {
        if let Some(b) = self.cfg.burst {
            let flip = if self.in_burst { b.leave } else { b.enter };
            if self.rng.unit() < flip {
                self.in_burst = !self.in_burst;
            }
            if self.in_burst {
                return self.rng.unit() < b.loss_in_burst;
            }
        }
        self.rng.unit() < self.cfg.loss
    }
}

/// Детерминированный генератор (xorshift64*): тестам нужна воспроизводимость, а не
/// криптостойкость, и без внешней зависимости.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Зерно перемешиваем (splitmix64): соседние зёрна должны давать разные прогоны.
        // Нулевое состояние xorshift не покидает — его заменяем.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng(if z == 0 { 1 } else { z })
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Равномерно в [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Прогнать время до момента, когда всё в пути доставлено.
    fn drain(link: &mut Link, mut now: Instant) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(at) = link.next_delivery() {
            now = now.max(at);
            while let Some(p) = link.poll(now) {
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn delivers_after_delay_not_before() {
        let t0 = Instant::now();
        let mut l = Link::new(
            LinkConfig {
                delay: ms(50),
                ..Default::default()
            },
            1,
        );
        l.send(t0, vec![1]);
        assert_eq!(l.poll(t0 + ms(49)), None);
        assert_eq!(l.poll(t0 + ms(50)), Some(vec![1]));
    }

    #[test]
    fn jitter_reorders_packets() {
        let t0 = Instant::now();
        let mut l = Link::new(
            LinkConfig {
                jitter: ms(40),
                ..Default::default()
            },
            7,
        );
        for i in 0..100u8 {
            l.send(t0 + ms(i as u64), vec![i]);
        }
        let got: Vec<u8> = drain(&mut l, t0).into_iter().map(|p| p[0]).collect();
        assert_eq!(got.len(), 100);
        assert!(
            got.windows(2).any(|w| w[0] > w[1]),
            "разброс задержки должен переставлять пакеты"
        );
    }

    #[test]
    fn random_loss_matches_rate() {
        let t0 = Instant::now();
        let mut l = Link::new(
            LinkConfig {
                loss: 0.1,
                ..Default::default()
            },
            3,
        );
        for _ in 0..10_000 {
            l.send(t0, vec![0]);
        }
        let lost = l.stats().lost;
        assert!(
            (800..1200).contains(&lost),
            "потеряно {lost} из 10000 при 10 %"
        );
    }

    #[test]
    fn burst_loss_comes_in_runs() {
        let t0 = Instant::now();
        let burst = BurstLoss {
            enter: 0.01,
            leave: 0.2,
            loss_in_burst: 1.0,
        };
        let mut l = Link::new(
            LinkConfig {
                burst: Some(burst),
                ..Default::default()
            },
            5,
        );
        for i in 0..10_000u32 {
            l.send(t0, i.to_be_bytes().to_vec());
        }
        let got: Vec<u32> = drain(&mut l, t0)
            .iter()
            .map(|p| u32::from_be_bytes([p[0], p[1], p[2], p[3]]))
            .collect();
        // Средняя длина пропуска заметно больше одного пакета.
        let gaps: Vec<u32> = got
            .windows(2)
            .map(|w| w[1] - w[0] - 1)
            .filter(|&g| g > 0)
            .collect();
        let mean = gaps.iter().sum::<u32>() as f64 / gaps.len() as f64;
        assert!(mean > 3.0, "средний пропуск {mean:.1} — потери не пачками");
    }

    #[test]
    fn bandwidth_paces_and_queue_overflows() {
        let t0 = Instant::now();
        // 1 Мбит/с = 125 байт/мс; очередь 10 КБ.
        let cfg = LinkConfig {
            delay: Duration::ZERO,
            bandwidth_bps: Some(1_000_000),
            queue_bytes: 10_000,
            ..Default::default()
        };
        let mut l = Link::new(cfg, 1);
        // 1000 байт выходят за 8 мс.
        l.send(t0, vec![0; 1000]);
        assert_eq!(l.poll(t0 + ms(7)), None);
        assert!(l.poll(t0 + ms(8)).is_some());
        // Пачка 20 КБ разом: половина не влезает в очередь.
        for _ in 0..20 {
            l.send(t0 + ms(8), vec![0; 1000]);
        }
        assert!(
            l.stats().queue_drops >= 9,
            "переполнение очереди: {:?}",
            l.stats()
        );
    }

    #[test]
    fn same_seed_same_run() {
        let run = |seed| {
            let t0 = Instant::now();
            let cfg = LinkConfig {
                loss: 0.2,
                jitter: ms(30),
                ..Default::default()
            };
            let mut l = Link::new(cfg, seed);
            for i in 0..200u8 {
                l.send(t0 + ms(i as u64), vec![i]);
            }
            drain(&mut l, t0)
        };
        assert_eq!(run(42), run(42));
        assert_ne!(run(42), run(43));
    }
}
