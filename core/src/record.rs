//! Подписанная запись v2 (ADR-015 F2, ADR-016 E3) — один формат для всего, что личность
//! заявляет о себе: адрес в сети, список почтовых ящиков, список устройств.
//!
//! ```text
//! подписывается:  "peerage:rec:v2" ‖ запись
//! запись:         type(1) ‖ signer(1) ‖ len(2, BE) ‖ body
//! тело адреса:    kind(1) ‖ epoch(4, BE) ‖ len(2, BE) ‖ addr
//! ```
//!
//! Вид записи и вид сети — внутри подписанного: адрес, заявленный для одной сети, не
//! проверяется для другой, а запись одного вида — как запись другого. `signer` говорит, чем
//! подписано: сейчас только ключом личности; ключ устройства под списком устройств
//! (ADR-010) ляжет сюда же без новой поломки формата.
//!
//! Разбор строгий: длины обязаны сходиться, хвост — отказ. Неизвестный вид записи или
//! подписанта — отказ при проверке: такую запись нечем осмысленно применить.

use crate::identity::Identity;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Домен подписи записей v2.
const DOMAIN: &[u8] = b"peerage:rec:v2";

/// Что заявляет запись.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordType {
    /// Адрес в сети ([`AddressRecord`]).
    Address = 1,
    // 2 — список почтовых ящиков (ADR-016), 3 — список устройств (ADR-010): номера заняты.
}

impl RecordType {
    fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Address),
            _ => None,
        }
    }
}

/// Чем подписано: 0 — ключом личности.
const SIGNER_IDENTITY: u8 = 0;

/// Вид сети (ADR-015 §2): один профиль — одна сеть.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetKind {
    Yggdrasil = 1,
}

impl NetKind {
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Yggdrasil),
            _ => None,
        }
    }
}

/// Запись в проводном виде и её подпись — то, что едет в объявлении и в коде обмена.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRecord {
    pub bytes: Vec<u8>,
    pub sig: [u8; 64],
}

/// Адрес личности в сети вида `kind` с эпохи `epoch` (ADR-007: эпохи только растут).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressRecord {
    pub kind: NetKind,
    pub epoch: u32,
    pub addr: Vec<u8>,
}

impl AddressRecord {
    fn body(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(7 + self.addr.len());
        b.push(self.kind as u8);
        b.extend_from_slice(&self.epoch.to_be_bytes());
        b.extend_from_slice(&(self.addr.len() as u16).to_be_bytes());
        b.extend_from_slice(&self.addr);
        b
    }

    fn parse(body: &[u8]) -> Option<Self> {
        let (&kind, rest) = body.split_first()?;
        let (epoch, rest) = rest.split_first_chunk::<4>()?;
        let (len, addr) = rest.split_first_chunk::<2>()?;
        (addr.len() == u16::from_be_bytes(*len) as usize).then_some(())?;
        Some(Self {
            kind: NetKind::from_u8(kind)?,
            epoch: u32::from_be_bytes(*epoch),
            addr: addr.to_vec(),
        })
    }

    /// Запись в проводном виде (без подписи; подписана — ключом личности).
    pub fn bytes(&self) -> Vec<u8> {
        encode(RecordType::Address, SIGNER_IDENTITY, &self.body())
    }

    /// Подписать ключом личности.
    pub fn sign(&self, identity: &Identity) -> SignedRecord {
        let bytes = self.bytes();
        let sig = identity.sign_record(&signed(&bytes));
        SignedRecord { bytes, sig }
    }

    /// Проверить запись адреса от личности `identity_pub`: подпись, вид записи, строгий
    /// разбор. Монотонность эпохи проверяет вызывающий — он знает, какая уже известна.
    pub fn verify(identity_pub: &[u8; 32], rec: &[u8], sig: &[u8; 64]) -> Option<Self> {
        let body = verify(identity_pub, RecordType::Address, rec, sig)?;
        Self::parse(body)
    }
}

/// `type ‖ signer ‖ len ‖ body`.
fn encode(ty: RecordType, signer: u8, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(4 + body.len());
    b.push(ty as u8);
    b.push(signer);
    b.extend_from_slice(&(body.len() as u16).to_be_bytes());
    b.extend_from_slice(body);
    b
}

fn signed(rec: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(DOMAIN.len() + rec.len());
    m.extend_from_slice(DOMAIN);
    m.extend_from_slice(rec);
    m
}

/// Тело записи вида `want`, если подпись личности сходится и длины верны.
fn verify<'a>(identity_pub: &[u8; 32], want: RecordType, rec: &'a [u8], sig: &[u8; 64]) -> Option<&'a [u8]> {
    let [ty, signer, l0, l1, body @ ..] = rec else { return None };
    (RecordType::from_u8(*ty)? == want && *signer == SIGNER_IDENTITY).then_some(())?;
    (body.len() == u16::from_be_bytes([*l0, *l1]) as usize).then_some(())?;
    let vk = VerifyingKey::from_bytes(identity_pub).ok()?;
    vk.verify(&signed(rec), &Signature::from_bytes(sig)).ok()?;
    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(epoch: u32, addr: &[u8]) -> AddressRecord {
        AddressRecord { kind: NetKind::Yggdrasil, epoch, addr: addr.to_vec() }
    }

    #[test]
    fn address_record_verifies_for_its_author_only() {
        let alice = Identity::generate();
        let mallory = Identity::generate();
        let r = rec(7, &[9u8; 32]);
        let s = r.sign(&alice);
        assert_eq!(AddressRecord::verify(&alice.public_key_bytes(), &s.bytes, &s.sig), Some(r));
        assert_eq!(AddressRecord::verify(&mallory.public_key_bytes(), &s.bytes, &s.sig), None);
    }

    /// Порча любого байта записи или подписи — отказ (вид, подписант, длины, эпоха, адрес).
    #[test]
    fn any_flipped_byte_is_rejected() {
        let alice = Identity::generate();
        let k = alice.public_key_bytes();
        let s = rec(3, &[1u8; 32]).sign(&alice);
        for i in 0..s.bytes.len() {
            let mut b = s.bytes.clone();
            b[i] ^= 1;
            assert_eq!(AddressRecord::verify(&k, &b, &s.sig), None, "байт {i}");
        }
        let mut sig = s.sig;
        sig[10] ^= 1;
        assert_eq!(AddressRecord::verify(&k, &s.bytes, &sig), None);
    }

    /// Длины обязаны сходиться: хвост или недостача — отказ даже с «верной» подписью.
    #[test]
    fn lengths_must_match() {
        let alice = Identity::generate();
        let k = alice.public_key_bytes();
        // Подписываем заведомо кривое тело — проверка формата идёт и после подписи.
        let mut body = rec(1, &[2u8; 32]).body();
        body.push(0);
        let bytes = encode(RecordType::Address, SIGNER_IDENTITY, &body);
        let sig = alice.sign_record(&signed(&bytes));
        assert_eq!(AddressRecord::verify(&k, &bytes, &sig), None, "хвост в теле");
        let mut bytes = rec(1, &[2u8; 32]).sign(&alice).bytes;
        bytes.push(0);
        let sig = alice.sign_record(&signed(&bytes));
        assert_eq!(AddressRecord::verify(&k, &bytes, &sig), None, "хвост в записи");
        assert_eq!(AddressRecord::verify(&k, &[], &sig), None, "пусто");
    }

    /// Неизвестный вид записи и чужой подписант — не адрес.
    #[test]
    fn foreign_type_or_signer_is_not_an_address() {
        let alice = Identity::generate();
        let k = alice.public_key_bytes();
        let body = rec(1, &[2u8; 32]).body();
        for (ty, signer) in [(2u8, SIGNER_IDENTITY), (1u8, 1u8)] {
            let mut bytes = vec![ty, signer];
            bytes.extend_from_slice(&(body.len() as u16).to_be_bytes());
            bytes.extend_from_slice(&body);
            let sig = alice.sign_record(&signed(&bytes));
            assert_eq!(AddressRecord::verify(&k, &bytes, &sig), None, "type {ty} signer {signer}");
        }
    }

    /// Адрес переменной длины (I2P — сотни байт) проходит как есть.
    #[test]
    fn variable_length_address_roundtrips() {
        let alice = Identity::generate();
        let r = rec(u32::MAX, &[5u8; 391]);
        let s = r.sign(&alice);
        assert_eq!(AddressRecord::verify(&alice.public_key_bytes(), &s.bytes, &s.sig), Some(r));
    }
}
