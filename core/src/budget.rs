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

/// Сколько «полезных дел» пира даёт максимальную прибавку к бюджету. Полезное дело —
/// пир потратил на нас свой трафик: полностью отдал блоб (картинка/файл/аватар).
pub const REPUTATION_FULL_AT: u32 = 20;
/// Во сколько раз максимум может вырасти бюджет заслуженного пира.
pub const REPUTATION_MAX_BOOST: f64 = 2.0;

/// Множитель бюджета за заслуги (способ 1 из scope: «помогавшие → бóльшие лимиты»).
///
/// ВАЖНО (анти-Sybil): бонус получают ТОЛЬКО те, кто уже прошёл гейт — контакты.
/// Иначе схема ломается: нагенерил 1000 ключей, каждый «помог» разок — и общая
/// корзина чужих обойдена. Незнакомцу заслуги не копятся вовсе.
pub fn reputation_boost(tier: Tier, helpful: u32) -> f64 {
    match tier {
        Tier::Contact | Tier::Verified => {
            let frac = (helpful as f64 / REPUTATION_FULL_AT as f64).min(1.0);
            1.0 + frac * (REPUTATION_MAX_BOOST - 1.0)
        }
        Tier::Blocked | Tier::Stranger => 1.0,
    }
}

impl Tier {
    /// Бюджет ingress с учётом заслуг пира (`helpful` — счётчик полезных дел).
    pub fn ingress_with_reputation(self, helpful: u32) -> (f64, f64) {
        let k = reputation_boost(self, helpful);
        let (cap, refill) = self.ingress();
        (cap * k, refill * k)
    }

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

    /// Сколько держим недоставленное. `None` — без срока.
    ///
    /// Очередь состоит из СОБСТВЕННЫХ сообщений пользователя, поэтому её не режем ни
    /// по объёму, ни (для контактов) по времени: сервера, который сохранит письмо за
    /// нас, нет, и «протухло» здесь означает «потеряно навсегда». О разросшейся
    /// очереди UI ПРЕДУПРЕЖДАЕТ (`OutboxStats::should_warn`), решение — за человеком.
    pub fn outbox_ttl_secs(self) -> Option<i64> {
        const DAY: i64 = 24 * 3600;
        match self {
            Tier::Blocked => Some(0),
            // Незнакомцу копить незачем: его сообщение — это запрос в контакты.
            Tier::Stranger => Some(DAY),
            // Контактам держим до победного (вытеснит только cap — старейшими).
            Tier::Contact | Tier::Verified => None,
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
        assert_eq!(Tier::Blocked.outbox_ttl_secs(), Some(0));
        assert!(Tier::Blocked.is_blocked());
    }

    /// Бюджет обязан РАСТИ с доверием по всем осям — иначе «градуировка» лишь на словах.
    /// TTL сравниваем как «срок хранения»: `None` (бессрочно) — самый щедрый вариант.
    #[test]
    fn budget_grows_monotonically_with_trust() {
        let tiers = [Tier::Blocked, Tier::Stranger, Tier::Contact, Tier::Verified];
        let keep = |t: Tier| t.outbox_ttl_secs().unwrap_or(i64::MAX);
        for pair in tiers.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            assert!(hi.ingress().0 > lo.ingress().0, "burst: {hi:?} должен быть щедрее {lo:?}");
            assert!(hi.ingress().1 > lo.ingress().1, "refill: {hi:?} должен быть щедрее {lo:?}");
            assert!(keep(hi) >= keep(lo), "срок хранения: {hi:?} не должен быть меньше {lo:?}");
        }
    }

    /// Сообщение контакту НЕ должно молча пропасть: сервера, который сохранит его за
    /// нас, нет — значит срок хранения не ограничиваем, объём держит cap.
    #[test]
    fn contacts_have_no_expiry() {
        assert_eq!(Tier::Contact.outbox_ttl_secs(), None);
        assert_eq!(Tier::Verified.outbox_ttl_secs(), None);
        // А незнакомцу копить незачем — его сообщение это запрос в контакты.
        assert!(Tier::Stranger.outbox_ttl_secs().is_some());
    }

    /// Незнакомец по умолчанию — не «доверенный по недосмотру».
    #[test]
    fn default_tier_is_stranger() {
        assert_eq!(Tier::default(), Tier::Stranger);
        assert!(!Tier::default().is_blocked());
    }

    /// Заслуги повышают бюджет контакта — но не безгранично.
    #[test]
    fn reputation_raises_contact_budget_up_to_cap() {
        let base = Tier::Contact.ingress().0;
        let some = Tier::Contact.ingress_with_reputation(REPUTATION_FULL_AT / 2).0;
        let full = Tier::Contact.ingress_with_reputation(REPUTATION_FULL_AT).0;
        let over = Tier::Contact.ingress_with_reputation(REPUTATION_FULL_AT * 100).0;
        assert!(some > base, "заслуги должны повышать бюджет");
        assert!(full > some);
        assert_eq!(full, base * REPUTATION_MAX_BOOST, "потолок = MAX_BOOST");
        assert_eq!(over, full, "сверх потолка бюджет не растёт");
    }

    /// АНТИ-SYBIL: заслуги не работают до гейта. Иначе 1000 сгенерированных ключей,
    /// каждый «помог» разок, обошли бы общую корзину чужих.
    #[test]
    fn reputation_does_not_help_strangers_or_blocked() {
        assert_eq!(reputation_boost(Tier::Stranger, 10_000), 1.0);
        assert_eq!(reputation_boost(Tier::Blocked, 10_000), 1.0);
        assert_eq!(Tier::Stranger.ingress_with_reputation(10_000), Tier::Stranger.ingress());
        assert_eq!(Tier::Blocked.ingress_with_reputation(10_000), (0.0, 0.0));
    }
}
