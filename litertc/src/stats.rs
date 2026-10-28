//! Статистика приёма потока (как в RTP, RFC 3550 §6.4 и приложение A.1): сколько
//! пакетов ожидалось и пришло, доля потерь за интервал отчёта, межпакетный джиттер.
//! Из неё собирается блок отчёта получателя ([`crate::wire::ReportBlock`]).

use crate::wire::ReportBlock;

/// Статистика одного трека: как мы его принимаем или как его принимает собеседник
/// (из его отчёта).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackStats {
    /// Расширенный номер последнего пакета (циклы × 2¹⁶ + seq).
    pub highest_seq: u32,
    /// Потеряно всего с начала потока.
    pub lost: u32,
    /// Доля потерь за последний интервал отчёта, 0.0…1.0.
    pub fraction_lost: f32,
    /// Межпакетный джиттер, мс.
    pub jitter_ms: f32,
}

impl TrackStats {
    /// Из блока чужого отчёта о нашем потоке.
    pub(crate) fn from_block(b: &ReportBlock, clock_hz: u32) -> Self {
        TrackStats {
            highest_seq: b.highest_seq,
            lost: b.cumulative_lost,
            fraction_lost: b.fraction_lost as f32 / 256.0,
            jitter_ms: b.jitter as f32 * 1000.0 / clock_hz as f32,
        }
    }
}

/// Счётчики приёма одного потока (одного `stream_id` одного трека).
pub(crate) struct RxStats {
    pub stream: u32,
    clock_hz: u32,
    /// Расширенный номер первого пакета.
    base: u32,
    max_seq: u16,
    /// Циклы номера × 2¹⁶.
    cycles: u32,
    received: u32,
    expected_prior: u32,
    received_prior: u32,
    fraction_lost: u8,
    /// Время в пути прошлого пакета (приход − метка отправителя) в единицах часов.
    transit: Option<i64>,
    /// Джиттер в единицах часов, ×16 точнее не нужно — храним дробью.
    jitter: f64,
}

impl RxStats {
    pub fn new(stream: u32, seq: u16, clock_hz: u32) -> Self {
        RxStats {
            stream,
            clock_hz,
            base: seq as u32,
            max_seq: seq,
            cycles: 0,
            received: 0,
            expected_prior: 0,
            received_prior: 0,
            fraction_lost: 0,
            transit: None,
            jitter: 0.0,
        }
    }

    /// Учесть пакет. `arrival` — время прихода в единицах часов трека (от любой
    /// своей точки отсчёта: важны только разности).
    pub fn update(&mut self, seq: u16, ts: u32, arrival: i64) {
        // Номер вперёд по кругу — новый максимум; назад — опоздавший или повтор.
        let delta = seq.wrapping_sub(self.max_seq) as i16;
        if delta > 0 {
            if seq < self.max_seq {
                self.cycles = self.cycles.wrapping_add(1 << 16);
            }
            self.max_seq = seq;
        }
        self.received = self.received.wrapping_add(1);
        // RFC 3550 §6.4.1: J += (|D| − J) / 16, D — разница времени в пути соседних пакетов.
        let transit = arrival - ts as i64;
        if let Some(prev) = self.transit {
            // Метка отправителя идёт по кругу u32: разницу берём по кругу.
            let d = (transit - prev) as i32 as f64;
            self.jitter += (d.abs() - self.jitter) / 16.0;
        }
        self.transit = Some(transit);
    }

    fn ext_max(&self) -> u32 {
        self.cycles.wrapping_add(self.max_seq as u32)
    }

    fn expected(&self) -> u32 {
        self.ext_max().wrapping_sub(self.base).wrapping_add(1)
    }

    fn lost(&self) -> u32 {
        self.expected().saturating_sub(self.received)
    }

    /// Блок отчёта; заодно закрывает интервал для доли потерь.
    pub fn report_block(&mut self, track: u8) -> ReportBlock {
        let expected = self.expected();
        let exp_int = expected.wrapping_sub(self.expected_prior);
        let rcv_int = self.received.wrapping_sub(self.received_prior);
        self.expected_prior = expected;
        self.received_prior = self.received;
        let lost_int = exp_int.saturating_sub(rcv_int);
        self.fraction_lost = if exp_int == 0 {
            0
        } else {
            ((lost_int as u64 * 256) / exp_int as u64).min(255) as u8
        };
        ReportBlock {
            track,
            fraction_lost: self.fraction_lost,
            stream_id: self.stream,
            highest_seq: self.ext_max(),
            cumulative_lost: self.lost(),
            jitter: self.jitter as u32,
        }
    }

    pub fn snapshot(&self) -> TrackStats {
        TrackStats {
            highest_seq: self.ext_max(),
            lost: self.lost(),
            fraction_lost: self.fraction_lost as f32 / 256.0,
            jitter_ms: (self.jitter * 1000.0 / self.clock_hz as f64) as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_loss_and_fraction_per_interval() {
        let mut s = RxStats::new(1, 10, 48_000);
        for seq in 10..20u16 {
            if seq != 13 && seq != 17 {
                s.update(seq, 0, 0);
            }
        }
        let b = s.report_block(0);
        assert_eq!(b.highest_seq, 19);
        assert_eq!(b.cumulative_lost, 2);
        assert_eq!(b.fraction_lost, (2 * 256 / 10) as u8);
        // Следующий интервал без потерь.
        for seq in 20..30u16 {
            s.update(seq, 0, 0);
        }
        let b = s.report_block(0);
        assert_eq!((b.cumulative_lost, b.fraction_lost), (2, 0));
    }

    #[test]
    fn seq_wrap_extends_highest() {
        let mut s = RxStats::new(1, 65_534, 48_000);
        for seq in [65_534u16, 65_535, 0, 1] {
            s.update(seq, 0, 0);
        }
        let b = s.report_block(0);
        assert_eq!(b.highest_seq, 65_536 + 1);
        assert_eq!(b.cumulative_lost, 0);
    }

    /// Ровный поток — джиттер ноль; пакеты то раньше, то позже — джиттер растёт.
    #[test]
    fn jitter_follows_arrival_variation() {
        let mut s = RxStats::new(1, 0, 48_000);
        for i in 0..50u16 {
            s.update(i, i as u32 * 960, i as i64 * 960);
        }
        assert_eq!(s.snapshot().jitter_ms, 0.0);
        for i in 50..200u16 {
            let wobble = if i % 2 == 0 { 480 } else { 0 }; // ±10 мс
            s.update(i, i as u32 * 960, i as i64 * 960 + wobble);
        }
        let j = s.snapshot().jitter_ms;
        assert!((8.0..=11.0).contains(&j), "джиттер {j} мс");
    }

    #[test]
    fn duplicates_do_not_make_negative_loss() {
        let mut s = RxStats::new(1, 0, 48_000);
        for _ in 0..3 {
            s.update(0, 0, 0);
        }
        let b = s.report_block(0);
        assert_eq!((b.cumulative_lost, b.fraction_lost), (0, 0));
    }
}
