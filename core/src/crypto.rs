//! E2E-шифрование сообщений — NaCl Box (Curve25519 + XSalsa20 + Poly1305).
//!
//! Бинарно совместимо с PyNaCl `Box` / libsodium `crypto_box`. Ключи Curve25519
//! детерминированно выводятся из ed25519-идентичности (см. [`crate::identity`]),
//! поэтому отдельного обмена ключами шифрования не нужно — публичный ключ
//! контакта (он же его идентификатор) однозначно задаёт ключ для Box.

use crypto_box::aead::{Aead, AeadCore, OsRng};
use crypto_box::{Nonce, PublicKey, SalsaBox, SecretKey};

use crate::identity::{ed25519_pub_to_curve25519, Identity};

/// Длина nonce XSalsa20 (24 байта), которым префиксуется шифртекст.
pub const NONCE_LEN: usize = 24;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("не удалось сконвертировать ключ контакта ed25519 → curve25519")]
    KeyConversion,
    #[error("шифрование не удалось")]
    Encrypt,
    #[error("расшифровка/проверка подлинности не удалась")]
    Decrypt,
    #[error("сообщение короче nonce")]
    TooShort,
}

/// Построить общий Box между нашей идентичностью и контактом (по его ed25519
/// публичному ключу). Box симметричен: одинаков для (мы→он) и (он→мы).
fn shared_box(me: &Identity, peer_ed_pub: &[u8; 32]) -> Result<SalsaBox, CryptoError> {
    let my_secret = SecretKey::from_bytes(me.x25519_secret());
    let peer_curve =
        ed25519_pub_to_curve25519(peer_ed_pub).ok_or(CryptoError::KeyConversion)?;
    let peer_pub = PublicKey::from_bytes(peer_curve);
    Ok(SalsaBox::new(&peer_pub, &my_secret))
}

/// Зашифровать `plaintext` для контакта `peer_ed_pub`.
/// Возвращает `nonce(24) || ciphertext` (включая тег Poly1305).
pub fn seal(
    me: &Identity,
    peer_ed_pub: &[u8; 32],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let b = shared_box(me, peer_ed_pub)?;
    let nonce = SalsaBox::generate_nonce(&mut OsRng);
    let ct = b.encrypt(&nonce, plaintext).map_err(|_| CryptoError::Encrypt)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Расшифровать сообщение `nonce(24) || ciphertext` от контакта `peer_ed_pub`.
/// Проверяет тег аутентичности — подделка/чужой ключ дадут ошибку.
pub fn open(
    me: &Identity,
    peer_ed_pub: &[u8; 32],
    data: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if data.len() < NONCE_LEN {
        return Err(CryptoError::TooShort);
    }
    let (nonce_bytes, ct) = data.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    let b = shared_box(me, peer_ed_pub)?;
    b.decrypt(nonce, ct).map_err(|_| CryptoError::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_between_two_identities() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let msg = b"hello bob, this is alice";

        // Alice шифрует для Bob, Bob расшифровывает от Alice.
        let sealed = seal(&alice, &bob.public_key_bytes(), msg).unwrap();
        let opened = open(&bob, &alice.public_key_bytes(), &sealed).unwrap();
        assert_eq!(opened, msg);
    }

    #[test]
    fn box_is_symmetric_both_directions() {
        let alice = Identity::generate();
        let bob = Identity::generate();

        let a2b = seal(&alice, &bob.public_key_bytes(), b"ping").unwrap();
        assert_eq!(open(&bob, &alice.public_key_bytes(), &a2b).unwrap(), b"ping");

        let b2a = seal(&bob, &alice.public_key_bytes(), b"pong").unwrap();
        assert_eq!(open(&alice, &bob.public_key_bytes(), &b2a).unwrap(), b"pong");
    }

    #[test]
    fn wrong_recipient_cannot_open() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let eve = Identity::generate();

        let sealed = seal(&alice, &bob.public_key_bytes(), b"secret").unwrap();
        // Eve пытается прочитать сообщение, адресованное Bob — должно упасть.
        assert!(open(&eve, &alice.public_key_bytes(), &sealed).is_err());
    }

    #[test]
    fn tampering_is_detected() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let mut sealed = seal(&alice, &bob.public_key_bytes(), b"important").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff; // портим тег/шифртекст
        assert!(open(&bob, &alice.public_key_bytes(), &sealed).is_err());
    }

    #[test]
    fn nonce_makes_ciphertexts_unique() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let a = seal(&alice, &bob.public_key_bytes(), b"same").unwrap();
        let b = seal(&alice, &bob.public_key_bytes(), b"same").unwrap();
        assert_ne!(a, b, "случайный nonce должен давать разный шифртекст");
    }
}
