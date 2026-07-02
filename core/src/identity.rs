//! Идентичность узла.
//!
//! Одна пара ключей ed25519 — корень идентичности:
//!   * публичный ключ ed25519 → детерминированный адрес Yggdrasil IPv6;
//!   * тот же ключ конвертируется в Curve25519 для шифрования сообщений (NaCl Box);
//!   * SHA-256(публичный ключ) → 8 слов BIP39 (человекочитаемый fingerprint).
//!
//! Yggdrasil (v0.4+) использует именно ed25519, поэтому ed25519 — первичен.
//! Curve25519 выводится из него по стандартной формуле birational map
//! (как `crypto_sign_ed25519_pk_to_curve25519` в libsodium).

use std::net::Ipv6Addr;

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand_core::OsRng;
use sha2::{Digest, Sha256, Sha512};

/// Размер seed-а ed25519 (он же приватный материал, который храним на диске).
pub const SEED_LEN: usize = 32;
/// Сколько слов BIP39 в fingerprint.
pub const FINGERPRINT_WORDS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("неверная длина seed: ожидалось {SEED_LEN}, получено {0}")]
    BadSeedLen(usize),
    #[error("не удалось сконвертировать ключ ed25519 в curve25519")]
    KeyConversion,
}

/// Идентичность узла. Хранит только seed; всё остальное выводится из него.
#[derive(Clone)]
pub struct Identity {
    signing: SigningKey,
}

impl Identity {
    /// Сгенерировать новую идентичность из системного CSPRNG.
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        Self { signing }
    }

    /// Восстановить идентичность из сохранённого seed-а (32 байта).
    pub fn from_seed(seed: &[u8]) -> Result<Self, IdentityError> {
        let arr: [u8; SEED_LEN] = seed
            .try_into()
            .map_err(|_| IdentityError::BadSeedLen(seed.len()))?;
        Ok(Self {
            signing: SigningKey::from_bytes(&arr),
        })
    }

    /// Seed для сохранения на диск (32 байта). Это секрет — храните защищённо.
    pub fn seed(&self) -> [u8; SEED_LEN] {
        self.signing.to_bytes()
    }

    /// Публичный ключ ed25519.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// Публичный ключ ed25519 в виде байтов (32).
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// Адрес Yggdrasil IPv6, детерминированно выведенный из публичного ключа.
    pub fn yggdrasil_address(&self) -> Ipv6Addr {
        addr_for_key(&self.public_key_bytes())
    }

    /// Публичный ключ Curve25519 (для шифрования NaCl Box).
    pub fn curve25519_public(&self) -> Result<[u8; 32], IdentityError> {
        ed25519_pub_to_curve25519(&self.public_key_bytes()).ok_or(IdentityError::KeyConversion)
    }

    /// Секретный ключ Curve25519 (X25519) для NaCl Box.
    ///
    /// Эквивалент `crypto_sign_ed25519_sk_to_curve25519`: первые 32 байта
    /// SHA-512(seed). Клэмпинг X25519 применяет при умножении (идемпотентно),
    /// поэтому возвращаем «сырое» значение — как libsodium.
    pub fn x25519_secret(&self) -> [u8; 32] {
        let hash = Sha512::digest(self.signing.to_bytes());
        let mut sk = [0u8; 32];
        sk.copy_from_slice(&hash[..32]);
        sk
    }

    /// Доменно-разделённый под-ключ (32 байта) из seed по контекстной метке
    /// (SEC-5). BLAKE3 в режиме KDF: `context` — уникальная незасекреченная строка-
    /// домен (напр. `"dev.p2p.blob-at-rest.v1"`), одна метка → один детерминированный
    /// ключ, независимый от seed и от ключей других доменов.
    ///
    /// ПОЛИТИКА: **любой новый симметричный ключ** (шифрование блобов at-rest, ключи
    /// медиа/группового звонка и т.п.) выводить ЧЕРЕЗ этот метод, а не переиспользуя
    /// seed/идентичность/Box-ключ. Существующие ключи (ed25519-идентичность =
    /// транспорт+MLS-credential; X25519 Box из seed — стандарт libsodium) не трогаем.
    pub fn derive_subkey(&self, context: &str) -> [u8; 32] {
        blake3::derive_key(context, &self.signing.to_bytes())
    }

    /// Fingerprint — 8 слов BIP39 из SHA-256(публичный ключ).
    pub fn fingerprint_words(&self) -> [&'static str; FINGERPRINT_WORDS] {
        fingerprint_words(&self.public_key_bytes())
    }

    /// Fingerprint одной строкой через пробел.
    pub fn fingerprint(&self) -> String {
        self.fingerprint_words().join(" ")
    }
}

/// Вывод адреса Yggdrasil из публичного ключа ed25519.
///
/// Портирование алгоритма из yggdrasil-go (`src/address/address.go`, `AddrForKey`):
///   1. инвертировать все биты публичного ключа;
///   2. посчитать число ведущих единиц (`ones`);
///   3. отбросить ведущие единицы и первый ноль;
///   4. адрес = [0x02, ones, <оставшиеся биты, упакованные в байты>] (16 байт).
pub fn addr_for_key(public_key: &[u8; 32]) -> Ipv6Addr {
    // Инвертируем биты ключа.
    let mut buf = [0u8; 32];
    for (i, b) in public_key.iter().enumerate() {
        buf[i] = !b;
    }

    let mut temp: Vec<u8> = Vec::with_capacity(32);
    let mut done = false;
    let mut ones: u8 = 0;
    let mut bits: u8 = 0;
    let mut n_bits: u32 = 0;

    for idx in 0..(8 * buf.len()) {
        let bit = (buf[idx / 8] >> (7 - (idx % 8))) & 1;
        if !done && bit != 0 {
            ones = ones.wrapping_add(1);
            continue;
        }
        if !done && bit == 0 {
            done = true;
            continue; // пропускаем первый ведущий ноль
        }
        bits = (bits << 1) | bit;
        n_bits += 1;
        if n_bits == 8 {
            n_bits = 0;
            temp.push(bits);
        }
    }

    let mut addr = [0u8; 16];
    addr[0] = 0x02; // префикс Yggdrasil (200::/7)
    addr[1] = ones;
    let take = core::cmp::min(temp.len(), 14);
    addr[2..2 + take].copy_from_slice(&temp[..take]);

    Ipv6Addr::from(addr)
}

/// Конвертация публичного ключа ed25519 → Curve25519 (Montgomery u-coordinate).
/// Эквивалент `crypto_sign_ed25519_pk_to_curve25519`.
pub fn ed25519_pub_to_curve25519(ed_pub: &[u8; 32]) -> Option<[u8; 32]> {
    use curve25519_dalek::edwards::CompressedEdwardsY;
    let compressed = CompressedEdwardsY(*ed_pub);
    let point = compressed.decompress()?;
    Some(point.to_montgomery().to_bytes())
}

/// 8 слов BIP39 из первых 88 бит SHA-256(публичный ключ).
pub fn fingerprint_words(public_key: &[u8; 32]) -> [&'static str; FINGERPRINT_WORDS] {
    let hash = Sha256::digest(public_key);
    // 8 слов × 11 бит = 88 бит = первые 11 байт хеша.
    let wordlist = bip39::Language::English.word_list();
    let mut words: [&'static str; FINGERPRINT_WORDS] = [""; FINGERPRINT_WORDS];
    for (i, slot) in words.iter_mut().enumerate() {
        let idx = extract_11_bits(&hash, i);
        *slot = wordlist[idx as usize];
    }
    words
}

/// Извлечь 11-битный индекс №`word_index` из big-endian битового потока.
fn extract_11_bits(bytes: &[u8], word_index: usize) -> u16 {
    let start_bit = word_index * 11;
    let mut value: u16 = 0;
    for offset in 0..11 {
        let bit_pos = start_bit + offset;
        let byte = bytes[bit_pos / 8];
        let bit = (byte >> (7 - (bit_pos % 8))) & 1;
        value = (value << 1) | bit as u16;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_roundtrip() {
        let id = Identity::generate();
        let seed = id.seed();
        let restored = Identity::from_seed(&seed).unwrap();
        assert_eq!(id.public_key_bytes(), restored.public_key_bytes());
        assert_eq!(id.yggdrasil_address(), restored.yggdrasil_address());
        assert_eq!(id.fingerprint(), restored.fingerprint());
    }

    #[test]
    fn yggdrasil_address_has_correct_prefix() {
        // Адрес Yggdrasil всегда в диапазоне 0200::/7, т.е. первый байт 0x02 или 0x03.
        let id = Identity::generate();
        let octets = id.yggdrasil_address().octets();
        assert_eq!(octets[0], 0x02, "первый байт должен быть префиксом 0x02");
    }

    #[test]
    fn address_is_deterministic() {
        let id = Identity::generate();
        assert_eq!(id.yggdrasil_address(), id.yggdrasil_address());
    }

    #[test]
    fn curve25519_conversion_works() {
        let id = Identity::generate();
        let c = id.curve25519_public().expect("конвертация должна пройти");
        assert_ne!(c, [0u8; 32]);
    }

    #[test]
    fn derive_subkey_is_domain_separated_and_deterministic() {
        let id = Identity::from_seed(&[5u8; 32]).unwrap();
        let a1 = id.derive_subkey("dev.p2p.blob-at-rest.v1");
        let a2 = id.derive_subkey("dev.p2p.blob-at-rest.v1");
        let b = id.derive_subkey("dev.p2p.media-key.v1");
        // Детерминирован по (seed, context).
        assert_eq!(a1, a2);
        // Разные домены → разные ключи.
        assert_ne!(a1, b);
        // Не равен ни seed, ни X25519-секрету (не переиспользование).
        assert_ne!(a1, id.seed());
        assert_ne!(a1, id.x25519_secret());
        // Разные идентичности → разные под-ключи для того же домена.
        let other = Identity::from_seed(&[6u8; 32]).unwrap();
        assert_ne!(a1, other.derive_subkey("dev.p2p.blob-at-rest.v1"));
    }

    #[test]
    fn fingerprint_is_8_words() {
        let id = Identity::generate();
        let words = id.fingerprint_words();
        assert_eq!(words.len(), 8);
        for w in words {
            assert!(!w.is_empty());
        }
    }

    /// Известный вектор: для ключа из всех 0x01 проверяем,
    /// что вывод адреса детерминирован и попадает в диапазон Yggdrasil.
    #[test]
    fn addr_for_known_key_is_stable() {
        let key = [0x01u8; 32];
        let a = addr_for_key(&key);
        let b = addr_for_key(&key);
        assert_eq!(a, b);
        assert_eq!(a.octets()[0], 0x02);
    }

    /// Эталонный вектор, сверенный с yggdrasil-go v0.5.13 (`address.AddrForKey`).
    /// Гарантирует бит-в-бит совместимость нашего вывода адреса с реальным узлом.
    #[test]
    fn addr_matches_yggdrasil_go_reference() {
        let pub_hex = "73e311c3c79391f3fb4322913487ff50d7040e0f1228b116a9f71861097ed279";
        let bytes = hex::decode(pub_hex).unwrap();
        let key: [u8; 32] = bytes.as_slice().try_into().unwrap();
        let addr = addr_for_key(&key);
        assert_eq!(addr.to_string(), "201:3073:b8f0:e1b1:b830:12f3:75bb:2de0");
    }
}
