//! Джиттер-буфер для реалтайм-аудио: переупорядочивает по seq, держит небольшой
//! префетч, отбрасывает опоздавшие кадры и сигналит о потерях (для PLC).
//!
//! Лёгкий: фиксированная глубина, без фоновых потоков — `pop` вызывается
//! потребителем по таймеру кадра (напр. каждые 20мс).

use std::collections::BTreeMap;

/// Результат [`JitterBuffer::pop`].
#[derive(Debug, PartialEq, Eq)]
pub enum JitterOut {
    /// Готовый кадр (payload пакета).
    Frame(Vec<u8>),
    /// Кадр потерян — потребитель должен сделать PLC (decode(None)).
    Lost,
}

/// seq `a` строго раньше `b` (с учётом обёртки u16).
fn seq_lt(a: u16, b: u16) -> bool {
    (a.wrapping_sub(b) as i16) < 0
}

pub struct JitterBuffer {
    depth: usize,
    next: Option<u16>,
    buf: BTreeMap<u16, Vec<u8>>,
}

impl JitterBuffer {
    /// `depth` — сколько кадров префетчить перед началом воспроизведения
    /// (компромисс задержка/устойчивость; напр. 3 кадра по 20мс = 60мс).
    pub fn new(depth: usize) -> Self {
        JitterBuffer { depth: depth.max(1), next: None, buf: BTreeMap::new() }
    }

    /// Принять пакет. Слишком старые (уже воспроизведённые) отбрасываются.
    pub fn push(&mut self, seq: u16, data: Vec<u8>) {
        if let Some(next) = self.next {
            if seq_lt(seq, next) {
                return; // опоздал
            }
        }
        self.buf.insert(seq, data);
    }

    /// Извлечь следующий кадр для воспроизведения.
    /// `None` — буфер ещё набирается/пуст (играть тишину).
    pub fn pop(&mut self) -> Option<JitterOut> {
        // Префетч: ждём заполнения до глубины перед стартом.
        if self.next.is_none() {
            if self.buf.len() < self.depth {
                return None;
            }
            self.next = self.buf.keys().next().copied();
        }
        let next = self.next?;
        if let Some(data) = self.buf.remove(&next) {
            self.next = Some(next.wrapping_add(1));
            return Some(JitterOut::Frame(data));
        }
        // Ожидаемого кадра нет. Если позади него уже копятся другие (буфер сверх
        // глубины) — объявляем потерю и идём дальше; иначе ждём (underrun).
        if self.buf.len() > self.depth {
            self.next = Some(next.wrapping_add(1));
            return Some(JitterOut::Lost);
        }
        None
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: u8) -> Vec<u8> {
        vec![n]
    }

    #[test]
    fn prefetch_then_in_order() {
        let mut j = JitterBuffer::new(2);
        j.push(0, f(0));
        assert_eq!(j.pop(), None); // ещё набирается (depth=2)
        j.push(1, f(1));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(0))));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(1))));
        assert_eq!(j.pop(), None); // пусто
    }

    #[test]
    fn reorders_out_of_order_arrivals() {
        let mut j = JitterBuffer::new(1);
        j.push(2, f(2));
        j.push(0, f(0));
        j.push(1, f(1));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(0))));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(1))));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(2))));
    }

    #[test]
    fn reports_loss_when_gap_and_buffer_grows() {
        let mut j = JitterBuffer::new(1);
        j.push(0, f(0));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(0))));
        // seq 1 потерян; приходят 2 и 3 → буфер сверх глубины → объявляем потерю 1
        j.push(2, f(2));
        j.push(3, f(3));
        assert_eq!(j.pop(), Some(JitterOut::Lost));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(2))));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(3))));
    }

    #[test]
    fn drops_late_packets() {
        let mut j = JitterBuffer::new(1);
        j.push(5, f(5));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(5)))); // next=6
        j.push(3, f(3)); // опоздал — отброшен
        assert!(j.is_empty());
    }
}
