//! E2E-шифрование сообщений — NaCl Box (Curve25519 + XSalsa20 + Poly1305).
//!
//! Бинарно совместимо с PyNaCl `Box` / libsodium `crypto_box`. Ключи Curve25519
//! детерминированно выводятся из ed25519-идентичности (см. [`crate::identity`]),
//! поэтому отдельного обмена ключами шифрования не нужно — публичный ключ
//! контакта (он же его идентификатор) однозначно задаёт ключ для Box.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

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

/// Сколько общих Box держим готовыми. Пиров, с которыми идёт обмен, — десятки;
/// предел нужен лишь против разрастания от потока незнакомых ключей.
const BOX_CACHE: usize = 256;

/// Готовые Box по паре (наш ключ, ключ пира). Конвертация ключа и DH X25519 —
/// скалярные умножения; делать их на каждый ack и каждый чанк файла незачем.
#[derive(Default)]
struct BoxCache {
    map: HashMap<([u8; 32], [u8; 32]), Arc<SalsaBox>>,
    order: VecDeque<([u8; 32], [u8; 32])>,
}

fn box_cache() -> &'static Mutex<BoxCache> {
    static CACHE: OnceLock<Mutex<BoxCache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Построить общий Box между нашей идентичностью и контактом (по его ed25519
/// публичному ключу). Box симметричен: одинаков для (мы→он) и (он→мы).
fn shared_box(me: &Identity, peer_ed_pub: &[u8; 32]) -> Result<Arc<SalsaBox>, CryptoError> {
    let key = (me.public_key_bytes(), *peer_ed_pub);
    if let Some(b) = box_cache().lock().unwrap_or_else(|e| e.into_inner()).map.get(&key) {
        return Ok(b.clone());
    }
    let my_secret = SecretKey::from_bytes(me.x25519_secret());
    let peer_curve =
        ed25519_pub_to_curve25519(peer_ed_pub).ok_or(CryptoError::KeyConversion)?;
    let peer_pub = PublicKey::from_bytes(peer_curve);
    let b = Arc::new(SalsaBox::new(&peer_pub, &my_secret));
    let mut cache = box_cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.map.insert(key, b.clone()).is_none() {
        cache.order.push_back(key);
        while cache.order.len() > BOX_CACHE {
            if let Some(old) = cache.order.pop_front() {
                cache.map.remove(&old);
            }
        }
    }
    Ok(b)
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

    /// R-016: Box кэшируется, но кэш ограничен, а вытесненная пара просто
    /// строится заново.
    #[test]
    fn box_cache_is_bounded_and_transparent() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let first = seal(&alice, &bob.public_key_bytes(), b"one").unwrap();
        for _ in 0..BOX_CACHE + 10 {
            let stranger = Identity::generate();
            seal(&alice, &stranger.public_key_bytes(), b"x").unwrap();
        }
        {
            let cache = box_cache().lock().unwrap();
            assert!(cache.map.len() <= BOX_CACHE);
            assert_eq!(cache.map.len(), cache.order.len());
        }
        // Пара alice↔bob давно вытеснена — расшифровка всё равно работает.
        assert_eq!(open(&bob, &alice.public_key_bytes(), &first).unwrap(), b"one");
    }
}
