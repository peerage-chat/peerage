//! Режим уведомлений чата (notifications.md, «Режим чата как в Discord»): все сообщения,
//! только упоминания или ничего. Свой выбор человека хранится локально (`chat_flags`) со
//! сроком; умолчание группы ставит админ, и оно едет участникам вместе с карточкой группы.
//! Решение «звенеть ли» принимает клиент — здесь только значения и их запись.

use serde::{Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NotifyMode {
    #[default]
    All,
    Mentions,
    Nothing,
}

impl NotifyMode {
    /// Имя на проводе и в JSON клиента.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Mentions => "mentions",
            Self::Nothing => "none",
        }
    }

    /// Разбор имени; неизвестное (пропатченный или новый клиент) — `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "all" => Some(Self::All),
            "mentions" => Some(Self::Mentions),
            "none" => Some(Self::Nothing),
            _ => None,
        }
    }

    /// Число в базе.
    pub fn code(self) -> i64 {
        match self {
            Self::All => 0,
            Self::Mentions => 1,
            Self::Nothing => 2,
        }
    }

    pub fn from_code(c: i64) -> Option<Self> {
        match c {
            0 => Some(Self::All),
            1 => Some(Self::Mentions),
            2 => Some(Self::Nothing),
            _ => None,
        }
    }

    /// Годится ли как умолчание группы: «ничего» навязывать участникам нельзя —
    /// так они пропустили бы и упоминания (в Discord то же).
    pub fn group_default_ok(self) -> bool {
        self != Self::Nothing
    }
}

impl Serialize for NotifyMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// Свой выбор человека для чата: режим и до какого времени (с, Unix; 0 — пока не включит).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChatNotify {
    pub mode: NotifyMode,
    pub until: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_codes_round_trip() {
        for m in [NotifyMode::All, NotifyMode::Mentions, NotifyMode::Nothing] {
            assert_eq!(NotifyMode::parse(m.as_str()), Some(m));
            assert_eq!(NotifyMode::from_code(m.code()), Some(m));
        }
        assert_eq!(NotifyMode::parse("loud"), None);
        assert_eq!(NotifyMode::from_code(-1), None);
    }

    #[test]
    fn group_default_cannot_be_nothing() {
        assert!(NotifyMode::Mentions.group_default_ok());
        assert!(!NotifyMode::Nothing.group_default_ok());
    }
}
