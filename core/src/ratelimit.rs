//! Приёмный (receiver-enforced) ограничитель входящего трафика — token-bucket
//! на источник. КЛЮЧЕВОЕ свойство: лимит навязывает ПРИНИМАЮЩИЙ, а не отправитель,
//! поэтому модификация исходников отправителя его не обходит. Ключ источника —
//! transport src key Yggdrasil (аутентифицирован на IP-уровне, спуфить дорого).
//!
//! M1 — механизм: per-source бакет с burst-ёмкостью и дозаправкой; при исчерпании
//! пакеты источника дропаются на входе (до расшифровки) — это и есть «разрыв
//! сессии» на прикладном уровне для меш-источника, которого нельзя отключить на
//! линк-уровне. Градуировка по доверию (чужие → общая мелкая корзина, Sybil-
//! устойчивость) и интеграция с репутацией — M3 (см. v1.0-scope «Анти-abuse»).
//!
//! Чистая логика (время инъектируется) → юнит-тестируется без сети/устройства.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Бакет одного источника: дробные токены + момент последней дозаправки.
#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

/// Token-bucket лимитер по источнику ([u8; 32] = ключ пира).
pub struct RateLimiter {
    buckets: HashMap<[u8; 32], Bucket>,
    /// Максимум накопленных токенов (допустимый всплеск).
    capacity: f64,
    /// Скорость дозаправки, токенов/сек (устойчивый разрешённый темп).
    refill_per_sec: f64,
    /// Идле-бакеты старше этого удаляются при обслуживании (анти-рост памяти).
    idle_evict: Duration,
}

impl RateLimiter {
    /// `capacity` — burst (сколько пакетов подряд можно принять «в долг»),
    /// `refill_per_sec` — устойчивый темп приёма от одного источника.
    pub fn new(capacity: f64, refill_per_sec: f64) -> RateLimiter {
        RateLimiter {
            buckets: HashMap::new(),
            capacity: capacity.max(1.0),
            refill_per_sec: refill_per_sec.max(0.0001),
            idle_evict: Duration::from_secs(120),
        }
    }

    /// Принять один пакет от `src` в момент `now`. `true` — в пределах бюджета,
    /// `false` — источник превысил лимит (пакет надо дропнуть).
    pub fn allow(&mut self, src: &[u8; 32], now: Instant) -> bool {
        let cap = self.capacity;
        let refill = self.refill_per_sec;
        let b = self.buckets.entry(*src).or_insert(Bucket { tokens: cap, last: now });
        // Дозаправка пропорционально прошедшему времени.
        let elapsed = now.saturating_duration_since(b.last).as_secs_f64();
        b.tokens = (b.tokens + elapsed * refill).min(cap);
        b.last = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Подчистить давно неактивные бакеты (звать редко, напр. раз в N секунд).
    pub fn evict_idle(&mut self, now: Instant) {
        let idle = self.idle_evict;
        self.buckets.retain(|_, b| now.saturating_duration_since(b.last) < idle);
    }

    /// Число отслеживаемых источников (для диагностики/тестов).
    pub fn tracked(&self) -> usize {
        self.buckets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> [u8; 32] { [b; 32] }

    #[test]
    fn allows_burst_up_to_capacity_then_denies() {
        let mut rl = RateLimiter::new(5.0, 1.0);
        let now = Instant::now();
        // 5 пакетов всплеска проходят...
        for _ in 0..5 {
            assert!(rl.allow(&key(1), now));
        }
        // ...шестой в тот же момент — отказ (бюджет исчерпан).
        assert!(!rl.allow(&key(1), now));
    }

    #[test]
    fn refills_over_time() {
        let mut rl = RateLimiter::new(2.0, 10.0); // 10 токенов/сек
        let t0 = Instant::now();
        assert!(rl.allow(&key(1), t0));
        assert!(rl.allow(&key(1), t0));
        assert!(!rl.allow(&key(1), t0)); // исчерпан
        // через 100мс при 10/сек дозаправится ~1 токен.
        let t1 = t0 + Duration::from_millis(100);
        assert!(rl.allow(&key(1), t1));
        assert!(!rl.allow(&key(1), t1));
    }

    #[test]
    fn refill_capped_at_capacity() {
        let mut rl = RateLimiter::new(3.0, 100.0);
        let t0 = Instant::now();
        // Долгое молчание не даёт накопить больше capacity.
        let t1 = t0 + Duration::from_secs(10);
        for _ in 0..3 {
            assert!(rl.allow(&key(1), t1));
        }
        assert!(!rl.allow(&key(1), t1)); // только 3, не 1000
    }

    #[test]
    fn sources_are_independent() {
        let mut rl = RateLimiter::new(1.0, 0.1);
        let now = Instant::now();
        assert!(rl.allow(&key(1), now));
        assert!(!rl.allow(&key(1), now)); // источник 1 исчерпан
        assert!(rl.allow(&key(2), now)); // источник 2 — свой бюджет
    }

    #[test]
    fn evict_idle_drops_stale_buckets() {
        let mut rl = RateLimiter::new(1.0, 1.0);
        let t0 = Instant::now();
        rl.allow(&key(1), t0);
        rl.allow(&key(2), t0);
        assert_eq!(rl.tracked(), 2);
        rl.evict_idle(t0 + Duration::from_secs(200));
        assert_eq!(rl.tracked(), 0);
    }
}
