//! Единый бюджет ресурсов на пира (M3, анти-abuse) — ОДНА точка, из которой
//! выводятся все per-peer лимиты: ingress-трафик, объём офлайн-очереди и её TTL.
//!
//! Зачем единая точка: раньше политики жили в пяти разных местах со своими
//! константами (ingress token-bucket, cap/TTL outbox, cooldown resync, лимиты
//! блобов, блок-лист), и «сколько ресурсов мы выдаём пиру X» нельзя было ответить,
//! не прочитав их все. Теперь ответ — [`Tier`] пира.
//!
//! Дизайн из `docs/v1.0-scope.md` («Анти-abuse: бюджет ресурсов на пира»):
//! enforcement ТОЛЬКО на приёмнике (самоограничению отправителя доверять нельзя),
//! лимиты градуированы по доверию, а чужие делят ОДНУ общую корзину на весь класс —
//! нагенерил 1000 ключей, все черпают из неё (Sybil обнулён).
//!
//! Честный предел (в threat-model): app-лимит бережёт CPU/батарею/хранилище, но
//! байты уже долетели до радио — резать там значит рвать Ygg-пиринг флудера.

/// Тир доверия пира. Определяется приёмником: блок-лист и контакты — локальное
/// знание, подделать его отправитель не может.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tier {
    /// Заблокирован пользователем — не тратим на него даже расшифровку.
    Blocked,
    /// Не в контактах. Общая мелкая корзина на ВЕСЬ класс (анти-Sybil).
    #[default]
    Stranger,
    /// Контакт (принят через гейт «запрос в контакты»).
    Contact,
    /// Контакт со сверенным вживую fingerprint — самый щедрый бюджет.
    Verified,
}

impl Tier {
    /// Бюджет ingress: `(burst, пакетов/сек)`. Для [`Tier::Stranger`] это параметры
    /// ОБЩЕЙ корзины класса, а не персональной.
    pub fn ingress(self) -> (f64, f64) {
        match self {
            Tier::Blocked => (0.0, 0.0),
            // Чужие: небольшой всплеск на класс — хватает на гейт-запрос и обмен
            // ключами, но не на флуд.
            Tier::Stranger => (30.0, 10.0),
            Tier::Contact => (60.0, 30.0),
            // Сверенный вживую — вдвое щедрее (медиа, блобы, групповые Commit).
            Tier::Verified => (120.0, 60.0),
        }
    }

    /// Сколько сообщений офлайн-очереди храним для этого пира (анти-DoS хранилища).
    pub fn outbox_cap(self) -> usize {
        match self {
            Tier::Blocked => 0,
            Tier::Stranger => 50,
            Tier::Contact => 500,
            Tier::Verified => 1000,
        }
    }

    /// Сколько держим недоставленное, прежде чем выбросить.
    pub fn outbox_ttl_secs(self) -> i64 {
        const DAY: i64 = 24 * 3600;
        match self {
            Tier::Blocked => 0,
            // Незнакомцу не копим неделю: его сообщение — это запрос в контакты.
            Tier::Stranger => DAY,
            Tier::Contact => 7 * DAY,
            Tier::Verified => 14 * DAY,
        }
    }

    /// Тратим ли на пира расшифровку вообще.
    pub fn is_blocked(self) -> bool {
        matches!(self, Tier::Blocked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_gets_nothing() {
        assert_eq!(Tier::Blocked.ingress(), (0.0, 0.0));
        assert_eq!(Tier::Blocked.outbox_cap(), 0);
        assert_eq!(Tier::Blocked.outbox_ttl_secs(), 0);
        assert!(Tier::Blocked.is_blocked());
    }

    /// Бюджет обязан РАСТИ с доверием по всем осям — иначе «градуировка» лишь на словах.
    #[test]
    fn budget_grows_monotonically_with_trust() {
        let tiers = [Tier::Blocked, Tier::Stranger, Tier::Contact, Tier::Verified];
        for pair in tiers.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            assert!(hi.ingress().0 > lo.ingress().0, "burst: {hi:?} должен быть щедрее {lo:?}");
            assert!(hi.ingress().1 > lo.ingress().1, "refill: {hi:?} должен быть щедрее {lo:?}");
            assert!(hi.outbox_cap() > lo.outbox_cap(), "cap: {hi:?} должен быть щедрее {lo:?}");
            assert!(
                hi.outbox_ttl_secs() > lo.outbox_ttl_secs(),
                "ttl: {hi:?} должен быть щедрее {lo:?}"
            );
        }
    }

    /// Незнакомец по умолчанию — не «доверенный по недосмотру».
    #[test]
    fn default_tier_is_stranger() {
        assert_eq!(Tier::default(), Tier::Stranger);
        assert!(!Tier::default().is_blocked());
    }
}
