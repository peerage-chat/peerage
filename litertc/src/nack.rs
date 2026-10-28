//! Повторы видео по NACK (ADR-014 §5): приёмник помнит пропущенные номера пакетов и
//! просит их повторить, отправитель держит недавнюю историю отправленного.
//!
//! Звук не повторяется: повтор пришёл бы позже, чем кадр пора играть. Его потери
//! закрывает FEC Opus.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Пропущенных номеров на поток помним не больше: дыру шире дешевле закрыть
/// ключевым кадром, чем сотней повторов. Столько же влезает в один NACK
/// ([`crate::wire::MAX_NACK_ITEMS`]), даже если номера идут вразброс.
const MAX_MISSING: usize = 128;

/// Сколько раз просим повторить один пакет.
const MAX_NACK_TRIES: u8 = 3;

/// Дыру в номерах просим не сразу: фрагменты кадра уходят разом, и разброс задержки
/// их переставляет. Без запаса каждая перестановка — ложный NACK и лишний повтор из
/// бюджета, которого потом не хватает на настоящие потери.
const REORDER_GRACE: Duration = Duration::from_millis(20);

/// История для повторов: дольше повтор уже не нужен — приёмник ждёт недостающий
/// кадр не больше 0,8 с, и его просьба идёт к нам ещё полпути.
const HISTORY_AGE: Duration = Duration::from_millis(1500);

/// И не больше стольких байт (8 Мбит/с видео — четыре секунды).
const HISTORY_BYTES: usize = 4 << 20;

struct Missing {
    since: Instant,
    tries: u8,
    last_sent: Option<Instant>,
}

impl Missing {
    /// Когда просить: первый раз — переждав перестановку, дальше — через `retry`.
    fn due(&self, retry: Duration) -> Instant {
        self.last_sent
            .map_or(self.since + REORDER_GRACE, |t| t + retry)
    }
}

/// Пропущенные номера одного входящего потока.
#[derive(Default)]
pub(crate) struct NackList {
    max_seq: Option<u16>,
    missing: HashMap<u16, Missing>,
}

impl NackList {
    /// Учесть пришедший пакет (первичный или повтор). Номера между прошлым старшим и
    /// этим — пропущены.
    pub fn on_packet(&mut self, now: Instant, seq: u16) {
        let Some(max) = self.max_seq else {
            self.max_seq = Some(seq);
            return;
        };
        let d = seq.wrapping_sub(max) as i16;
        if d <= 0 {
            self.missing.remove(&seq);
            return;
        }
        let gap = (d - 1) as usize;
        if self.missing.len() + gap > MAX_MISSING {
            // Дыра шире, чем стоит чинить повтором: ключевой кадр запросит сборщик,
            // не дождавшись кадров.
            self.missing.clear();
        } else {
            for i in 1..d as u16 {
                self.missing.insert(
                    max.wrapping_add(i),
                    Missing {
                        since: now,
                        tries: 0,
                        last_sent: None,
                    },
                );
            }
        }
        self.max_seq = Some(seq);
    }

    /// Номера, которые пора попросить (старые первыми); отмечаются отправленными.
    /// Старше `max_age` или исчерпавшие попытки — забываются.
    pub fn due(&mut self, now: Instant, retry: Duration, max_age: Duration) -> Vec<u16> {
        let Some(max) = self.max_seq else {
            return Vec::new();
        };
        self.missing.retain(|_, m| {
            m.tries < MAX_NACK_TRIES && now.saturating_duration_since(m.since) <= max_age
        });
        let mut seqs: Vec<u16> = self
            .missing
            .iter_mut()
            .filter(|(_, m)| now >= m.due(retry))
            .map(|(&seq, m)| {
                m.tries += 1;
                m.last_sent = Some(now);
                seq
            })
            .collect();
        seqs.sort_by_key(|&seq| std::cmp::Reverse(max.wrapping_sub(seq)));
        seqs
    }

    /// Когда следующий номер станет пора просить.
    pub fn next_due(&self, retry: Duration) -> Option<Instant> {
        self.missing
            .values()
            .filter(|m| m.tries < MAX_NACK_TRIES)
            .map(|m| m.due(retry))
            .min()
    }
}

struct Sent {
    seq: u16,
    at: Instant,
    ts: u32,
    payload: Vec<u8>,
}

/// Недавно отправленные пакеты трека — по номеру. Одна на всех участников: пакет
/// один и тот же, у каждого свой только сквозной номер.
#[derive(Default)]
pub(crate) struct TxHistory {
    packets: VecDeque<Sent>,
    bytes: usize,
}

impl TxHistory {
    pub fn push(&mut self, now: Instant, seq: u16, ts: u32, payload: &[u8]) {
        // Номера идут подряд — поиск по смещению от первого. Дыра (не бывает, но)
        // сломала бы смещения: тогда начинаем историю заново.
        if self
            .packets
            .back()
            .is_some_and(|b| b.seq.wrapping_add(1) != seq)
        {
            self.packets.clear();
            self.bytes = 0;
        }
        self.packets.push_back(Sent {
            seq,
            at: now,
            ts,
            payload: payload.to_vec(),
        });
        self.bytes += payload.len();
        while let Some(front) = self.packets.front() {
            let old = now.saturating_duration_since(front.at) > HISTORY_AGE;
            if !old && self.bytes <= HISTORY_BYTES {
                break;
            }
            self.bytes -= front.payload.len();
            self.packets.pop_front();
        }
    }

    /// Пакет `seq`: его метка времени и payload.
    pub fn get(&self, seq: u16) -> Option<(u32, &[u8])> {
        let first = self.packets.front()?.seq;
        let p = self.packets.get(seq.wrapping_sub(first) as usize)?;
        (p.seq == seq).then_some((p.ts, p.payload.as_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RETRY: Duration = Duration::from_millis(100);
    const AGE: Duration = Duration::from_millis(500);

    #[test]
    fn gap_is_requested_then_retried_then_forgotten() {
        let t0 = Instant::now();
        let mut n = NackList::default();
        for seq in [10, 11, 14] {
            n.on_packet(t0, seq);
        }
        assert!(n.due(t0, RETRY, AGE).is_empty(), "ждём перестановку");
        assert_eq!(n.next_due(RETRY), Some(t0 + REORDER_GRACE));
        let t1 = t0 + REORDER_GRACE;
        assert_eq!(n.due(t1, RETRY, AGE), [12, 13]);
        assert_eq!(n.due(t1, RETRY, AGE), [] as [u16; 0], "не чаще retry");
        assert_eq!(n.next_due(RETRY), Some(t1 + RETRY));
        n.on_packet(t1, 12); // повтор пришёл
        assert_eq!(n.due(t1 + RETRY, RETRY, AGE), [13]);
        assert_eq!(n.due(t1 + RETRY * 2, RETRY, AGE), [13]);
        assert_eq!(
            n.due(t1 + RETRY * 3, RETRY, AGE),
            [] as [u16; 0],
            "попытки кончились"
        );
        assert_eq!(n.next_due(RETRY), None);
    }

    #[test]
    fn old_and_reordered_are_not_requested() {
        let t0 = Instant::now();
        let mut n = NackList::default();
        n.on_packet(t0, 1);
        n.on_packet(t0, 3);
        n.on_packet(t0, 2); // просто переставился
        assert!(n.due(t0, RETRY, AGE).is_empty());
        n.on_packet(t0, 5);
        assert!(n.due(t0 + AGE * 2, RETRY, AGE).is_empty(), "устарел");
    }

    #[test]
    fn wrap_and_huge_gap() {
        let t0 = Instant::now();
        let mut n = NackList::default();
        n.on_packet(t0, 65_534);
        n.on_packet(t0, 1);
        let t1 = t0 + REORDER_GRACE;
        assert_eq!(n.due(t1, RETRY, AGE), [65_535, 0]);
        n.on_packet(t1, 1 + MAX_MISSING as u16 + 5);
        assert!(
            n.due(t1 + REORDER_GRACE, RETRY, AGE).is_empty(),
            "широкую дыру не чиним повтором"
        );
    }

    #[test]
    fn history_is_bounded_and_found_by_seq() {
        let t0 = Instant::now();
        let mut h = TxHistory::default();
        for seq in 0..10u16 {
            h.push(t0, 65_530u16.wrapping_add(seq), seq as u32, &[seq as u8]);
        }
        assert_eq!(h.get(65_535), Some((5, &[5u8][..])));
        assert_eq!(h.get(2), Some((8, &[8u8][..])));
        assert_eq!(h.get(4), None);
        // Через секунду старое забыто.
        h.push(t0 + HISTORY_AGE * 2, 4, 10, &[10]);
        assert_eq!(h.get(3), None);
        assert_eq!(h.get(4), Some((10, &[10u8][..])));
        // По байтам.
        let big = vec![0u8; HISTORY_BYTES / 2 + 1];
        for seq in 5..8u16 {
            h.push(t0 + HISTORY_AGE * 2, seq, 0, &big);
        }
        assert!(h.bytes <= HISTORY_BYTES);
        assert_eq!(h.get(5), None);
        assert!(h.get(7).is_some());
    }
}
