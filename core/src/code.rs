//! Код обмена v2 (ADR-015 §7; F3, ADR-016 E4, E2) — то, что показывает QR «Мой код» и что
//! пересылают текстом.
//!
//! ```text
//! "PEERAGE:" ‖ base32(версия(1) ‖ вид сети(1) ‖ TLV*)
//! TLV:       тип(1) ‖ длина(2, BE) ‖ значение
//! ```
//!
//! Base32 (RFC 4648, без паддинга) в верхнем регистре вместе с префиксом целиком входит в
//! алфавитно-цифровой набор QR — код плотнее, чем байтовый режим, и переживает пересылку
//! в любом мессенджере. Регистр и пробелы при разборе не важны.
//!
//! Поля: личность (обязательна), подписанная запись адреса и её подпись (без них — адреса
//! нет, связь будет ждать объявления), самоназвание (не подписано: его перезапишет профиль).
//! Номера 5 (список почтовых ящиков, ADR-016) и 6 (добавки сети, например ключ клиента onion)
//! заняты; неизвестный тип пропускается (E2) — новое поле не ломает старый разбор.

use crate::record::{NetKind, SignedRecord};

pub const PREFIX: &str = "PEERAGE:";
const VERSION: u8 = 2;

const T_IDENTITY: u8 = 1;
const T_ADDR_RECORD: u8 = 2;
const T_ADDR_SIG: u8 = 3;
const T_NAME: u8 = 4;

/// Самое длинное самоназвание в коде, в символах: код приходит от кого угодно.
pub const NAME_MAX: usize = 64;

/// Разобранный код обмена.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactCode {
    pub kind: NetKind,
    pub identity: [u8; 32],
    /// Подписанная запись адреса (её проверяет тот, кто применяет: подпись — личности).
    pub address: Option<SignedRecord>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeError {
    /// Не код Peerage (или испорчен).
    NotACode,
    /// Код более новой версии — обновить приложение.
    Version,
    /// Код профиля другой сети (ADR-015): в этот профиль не ложится.
    ForeignNetwork,
}

impl ContactCode {
    pub fn encode(&self) -> String {
        let mut b = vec![VERSION, self.kind as u8];
        tlv(&mut b, T_IDENTITY, &self.identity);
        if let Some(a) = &self.address {
            tlv(&mut b, T_ADDR_RECORD, &a.bytes);
            tlv(&mut b, T_ADDR_SIG, &a.sig);
        }
        if let Some(n) = self.name.as_deref().map(clean_name).filter(|n| !n.is_empty()) {
            tlv(&mut b, T_NAME, n.as_bytes());
        }
        format!("{PREFIX}{}", base32_encode(&b))
    }

    /// Разобрать код для профиля сети `kind`.
    pub fn parse(s: &str, kind: NetKind) -> Result<Self, CodeError> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
        let body = s.strip_prefix(PREFIX).ok_or(CodeError::NotACode)?;
        let b = base32_decode(body).ok_or(CodeError::NotACode)?;
        let &[version, k, ref fields @ ..] = b.as_slice() else { return Err(CodeError::NotACode) };
        if version != VERSION {
            return Err(if version > VERSION { CodeError::Version } else { CodeError::NotACode });
        }
        if k != kind as u8 {
            return Err(CodeError::ForeignNetwork);
        }
        let (mut identity, mut rec, mut sig, mut name) = (None, None, None, None);
        let mut rest: &[u8] = fields;
        while let &[t, l0, l1, ref tail @ ..] = rest {
            let len = u16::from_be_bytes([l0, l1]) as usize;
            let (v, after) = (tail.get(..len).ok_or(CodeError::NotACode)?, &tail[len..]);
            match t {
                T_IDENTITY => identity = Some(<[u8; 32]>::try_from(v).map_err(|_| CodeError::NotACode)?),
                T_ADDR_RECORD => rec = Some(v.to_vec()),
                T_ADDR_SIG => sig = Some(<[u8; 64]>::try_from(v).map_err(|_| CodeError::NotACode)?),
                // Кривое имя — не повод отказать: ключ разобран, имя просто не берём.
                T_NAME => name = std::str::from_utf8(v).ok().map(clean_name).filter(|n| !n.is_empty()),
                _ => {} // E2: будущее поле
            }
            rest = after;
        }
        if !rest.is_empty() {
            return Err(CodeError::NotACode); // обрывок заголовка поля
        }
        Ok(Self {
            kind,
            identity: identity.ok_or(CodeError::NotACode)?,
            address: rec.zip(sig).map(|(bytes, sig)| SignedRecord { bytes, sig }),
            name,
        })
    }
}

fn tlv(b: &mut Vec<u8>, t: u8, v: &[u8]) {
    b.push(t);
    b.extend_from_slice(&(v.len() as u16).to_be_bytes());
    b.extend_from_slice(v);
}

/// Самоназвание из чужих рук (код, запрос в контакты): без управляющих символов, не
/// длиннее [`NAME_MAX`] символов.
pub fn clean_name(s: &str) -> String {
    let name: String = s.chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    name.trim().to_string()
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &byte in data {
        acc = (acc << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = B32.iter().position(|&x| x == c)? as u32;
        acc = ((acc << 5) | v) & 0xFFFF;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // Хвост — только нулевые биты дополнения (иначе у кода два написания).
    (bits < 5 && acc & ((1 << bits) - 1) == 0).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identity;

    fn code(id: &Identity) -> ContactCode {
        ContactCode {
            kind: NetKind::Yggdrasil,
            identity: id.public_key_bytes(),
            address: Some(id.address_record(1_800_000_000)),
            name: Some("Алиса".into()),
        }
    }

    #[test]
    fn roundtrip_any_case_and_spaces() {
        let c = code(&Identity::generate());
        let s = c.encode();
        assert!(s.starts_with(PREFIX));
        assert_eq!(ContactCode::parse(&s, NetKind::Yggdrasil), Ok(c.clone()));
        let messy = format!("  {} \n", s.to_lowercase().replace("peerage:", "peerage: "));
        assert_eq!(ContactCode::parse(&messy, NetKind::Yggdrasil), Ok(c));
    }

    /// Весь код — в алфавитно-цифровом наборе QR.
    #[test]
    fn code_fits_qr_alphanumeric_mode() {
        let s = code(&Identity::generate()).encode();
        assert!(s.chars().all(|c| "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:".contains(c)), "{s}");
    }

    /// E2: неизвестное поле пропускается, известные разбираются.
    #[test]
    fn unknown_tlv_is_skipped() {
        let c = code(&Identity::generate());
        let mut b = base32_decode(c.encode().strip_prefix(PREFIX).unwrap()).unwrap();
        tlv(&mut b, 5, &[1, 2, 3]); // ящики (ADR-016) — этому разбору неведомы
        tlv(&mut b, 200, &[]);
        let s = format!("{PREFIX}{}", base32_encode(&b));
        assert_eq!(ContactCode::parse(&s, NetKind::Yggdrasil), Ok(c));
    }

    #[test]
    fn foreign_network_newer_version_and_garbage_are_rejected() {
        let c = code(&Identity::generate());
        let b = base32_decode(c.encode().strip_prefix(PREFIX).unwrap()).unwrap();
        let with = |i: usize, v: u8| {
            let mut x = b.clone();
            x[i] = v;
            format!("{PREFIX}{}", base32_encode(&x))
        };
        assert_eq!(ContactCode::parse(&with(1, 2), NetKind::Yggdrasil), Err(CodeError::ForeignNetwork));
        assert_eq!(ContactCode::parse(&with(0, 3), NetKind::Yggdrasil), Err(CodeError::Version));
        assert_eq!(ContactCode::parse(&with(0, 1), NetKind::Yggdrasil), Err(CodeError::NotACode));
        for junk in ["", "PEERAGE:", "PEERAGE:1", "hello", &hex::encode([7u8; 32])] {
            assert!(ContactCode::parse(junk, NetKind::Yggdrasil).is_err(), "{junk:?}");
        }
        // Обрезанный код — не код.
        let s = c.encode();
        assert!(ContactCode::parse(&s[..s.len() - 10], NetKind::Yggdrasil).is_err());
    }

    /// Имя: управляющие символы выбрасываются, длина ограничена; кривое — ключ всё равно ок.
    #[test]
    fn name_is_cleaned_and_bounded() {
        let id = Identity::generate();
        let c = ContactCode { name: Some(format!("A\u{7}{}", "я".repeat(100))), ..code(&id) };
        let got = ContactCode::parse(&c.encode(), NetKind::Yggdrasil).unwrap();
        assert_eq!(got.name.unwrap().chars().count(), NAME_MAX);
        let mut b = base32_decode(ContactCode { name: None, ..code(&id) }.encode().strip_prefix(PREFIX).unwrap()).unwrap();
        tlv(&mut b, T_NAME, &[0xff, 0xfe]);
        let got = ContactCode::parse(&format!("{PREFIX}{}", base32_encode(&b)), NetKind::Yggdrasil).unwrap();
        assert_eq!((got.identity, got.name), (id.public_key_bytes(), None));
    }

    #[test]
    fn base32_roundtrips_every_length() {
        for n in 0..40 {
            let d: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base32_decode(&base32_encode(&d)), Some(d));
        }
    }
}
