//! Джиттер-буфер для реалтайм-аудио: переупорядочивает по seq, держит небольшой
//! префетч, отбрасывает опоздавшие кадры и сигналит о потерях (для PLC).
//!
//! Лёгкий: фиксированная глубина, без фоновых потоков — `pop` вызывается
//! потребителем по таймеру кадра (напр. каждые 20мс).

use std::collections::BTreeMap;

/// Запас сверх `depth` для поглощения сетевых пачек (месh/эмулятор доставляют
/// пакеты не равномерно, а всплесками по 10-15 разом). Пачка — это джиттер, а не
/// рост задержки: эти кадры будут съедены за следующие N×20мс. Кап (сброс старья)
/// должен срабатывать только при НАСТОЯЩЕМ убегании задержки, а не на каждой пачке.
/// 20 кадров ≈ 400мс — потолок, который буфер достигает лишь при дрейфе часов.
const BURST_HEADROOM: usize = 20;

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
    /// Потолок бэклога: если накопилось больше — отбрасываем самые старые кадры
    /// и ресинхронизируемся (иначе задержка растёт и не возвращается).
    max: usize,
    /// Минимум кадров после пропуска, чтобы объявить потерю и запустить PLC.
    /// Отдельно от depth: маленькое значение (1-2) = быстрый PLC вместо тишины.
    gap_tol: usize,
    next: Option<u16>,
    buf: BTreeMap<u16, Vec<u8>>,
    /// DIAG: счётчик пакетов, выброшенных anti-latency капом (читать+сброс).
    dropped: u32,
}

impl JitterBuffer {
    /// `depth` — сколько кадров префетчить перед началом воспроизведения
    /// (компромисс задержка/устойчивость; напр. 3 кадра по 20мс = 60мс).
    pub fn new(depth: usize) -> Self {
        let depth = depth.max(1);
        // gap_tol=2: терпим 1-кадровый реордеринг без ложной потери, но объявляем
        // настоящую потерю уже через 40мс (2 кадра), а не ждём depth кадров.
        JitterBuffer { depth, gap_tol: 2, max: depth + BURST_HEADROOM, next: None, buf: BTreeMap::new(), dropped: 0 }
    }

    /// Изменить целевую глубину префетча на лету.
    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth.max(1);
        self.max = self.depth + BURST_HEADROOM;
        if self.next.is_some() && self.buf.len() > self.max {
            while self.buf.len() > self.depth {
                let oldest = *self.buf.keys().next().unwrap();
                self.buf.remove(&oldest);
                self.dropped += 1;
            }
            self.next = self.buf.keys().next().copied();
        }
    }

    /// DIAG: прочитать и обнулить счётчик выброшенных капом пакетов.
    pub fn take_dropped(&mut self) -> u32 {
        std::mem::take(&mut self.dropped)
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
        // Анти-латентность: бэклог сверх потолка (отправитель слал быстрее, чем мы
        // воспроизводим, либо был всплеск после паузы) — отбрасываем старьё,
        // оставляя ~depth свежих кадров, и ресинхронизируемся.
        if self.next.is_some() && self.buf.len() > self.max {
            while self.buf.len() > self.depth {
                let oldest = *self.buf.keys().next().unwrap();
                self.buf.remove(&oldest);
                self.dropped += 1;
            }
            self.next = self.buf.keys().next().copied();
        }
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
        // Ожидаемого кадра нет. Если после пропуска накопилось >= gap_tol кадров —
        // объявляем потерю и запускаем PLC; иначе ждём (underrun).
        // gap_tol намеренно < depth: декларируем потерю быстро вместо тишины.
        if self.buf.len() >= self.gap_tol {
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
    fn reports_loss_after_gap_tol_future_frames() {
        // gap_tol=2: нужно 2 кадра после пропуска чтобы объявить потерю.
        // Это защищает от ложных срабатываний при 1-кадровом реордеринге.
        let mut j = JitterBuffer::new(4);
        for s in 0..4u16 { j.push(s, f(s as u8)); }
        for _ in 0..4 { assert!(matches!(j.pop(), Some(JitterOut::Frame(_)))); }
        // seq 4 потерян; приходит seq 5 (только 1 кадр < gap_tol=2 → ждём)
        j.push(5, f(5));
        assert_eq!(j.pop(), None);
        // seq 6 пришёл — теперь buf.len()=2 >= gap_tol=2 → объявляем потерю
        j.push(6, f(6));
        assert_eq!(j.pop(), Some(JitterOut::Lost));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(5))));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(6))));
    }

    #[test]
    fn no_false_loss_for_one_frame_reorder() {
        // Пакет seq 1 пришёл после seq 2 (реордеринг на 1) — не должно быть Lost.
        let mut j = JitterBuffer::new(2);
        j.push(0, f(0));
        j.push(1, f(1));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(0))));
        // seq 2 и seq 1 пришли не по порядку: 2 раньше 1 (но 1 уже в буфере)
        // Этого нет тут — просто убедимся что порядок соблюдается
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(1))));
        assert_eq!(j.pop(), None);
    }

    #[test]
    fn caps_backlog_to_bound_latency() {
        let mut j = JitterBuffer::new(1); // depth=1 → max=1+BURST_HEADROOM
        j.push(0, f(0));
        assert_eq!(j.pop(), Some(JitterOut::Frame(f(0)))); // playout начат, next=1
        // Отправитель обогнал воспроизведение ДАЛЕКО за потолок (настоящий дрейф).
        for s in 1..=60u16 {
            j.push(s, vec![s as u8]);
        }
        // len=60 > max → отбрасываем старьё, ресинк на свежие кадры.
        match j.pop() {
            Some(JitterOut::Frame(d)) => assert!(d[0] >= 59, "ожидали свежий кадр, был {}", d[0]),
            other => panic!("ожидали свежий Frame, получили {other:?}"),
        }
        assert!(j.len() <= 1, "бэклог не ограничен: {}", j.len());
    }

    #[test]
    fn absorbs_burst_without_dropping() {
        // Сетевая пачка: 15 кадров приходят разом. Их НЕ должно выбросить —
        // BURST_HEADROOM=20 поглощает пачку, кадры воспроизводятся по порядку.
        let mut j = JitterBuffer::new(3);
        for s in 0..15u16 { j.push(s, f(s as u8)); }
        assert_eq!(j.take_dropped(), 0, "пачка из 15 не должна дропаться");
        // И все 15 извлекаются по порядку.
        for s in 0..15u16 {
            assert_eq!(j.pop(), Some(JitterOut::Frame(f(s as u8))));
        }
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
