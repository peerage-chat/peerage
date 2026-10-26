//! Идентичность узла.
//!
//! Одна пара ключей ed25519 — корень идентичности:
//!   * публичный ключ ed25519 → детерминированный адрес Yggdrasil IPv6;
//!   * ключи для остального — доменно-разделённые под-ключи seed-а
//!     ([`Identity::derive_subkey`]), связанные с личностью подписью, где нужно
//!     (статический ключ сессий 1:1, ADR-012);
//!   * SHA-256(публичный ключ) → 8 слов BIP39 (человекочитаемый fingerprint).
//!
//! Yggdrasil (v0.4+) использует именно ed25519, поэтому ed25519 — первичен.

use std::net::Ipv6Addr;

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand_core::OsRng;
use sha2::{Digest, Sha256};

/// Размер seed-а ed25519 (он же приватный материал, который храним на диске).
pub const SEED_LEN: usize = 32;
/// Сколько слов BIP39 в fingerprint.
pub const FINGERPRINT_WORDS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("неверная длина seed: ожидалось {SEED_LEN}, получено {0}")]
    BadSeedLen(usize),
    #[error("неверная сид-фраза BIP39")]
    BadPhrase,
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

    /// Секретная сид-фраза (24 слова BIP39) — ПОЛНЫЙ бэкап личности: 32-байтный seed
    /// (256 бит) как мнемоника. СЕКРЕТ. Восстанавливает ЛИЧНОСТЬ (не историю — серверов
    /// нет). НЕ путать с [`fingerprint_words`] (8 слов из ПУБЛИЧНОГО ключа — сверка
    /// контакта, не секрет). Для онбординга M4 «Показать фразу». См. app-ui.md.
    pub fn seed_phrase(&self) -> String {
        bip39::Mnemonic::from_entropy(&self.signing.to_bytes())
            .expect("32 байта seed → валидная 24-словная мнемоника BIP39")
            .to_string()
    }

    /// Восстановить личность из секретной сид-фразы (24 слова BIP39). Пробелы/регистр
    /// нормализуются. Обратна [`seed_phrase`].
    pub fn from_phrase(phrase: &str) -> Result<Self, IdentityError> {
        // Английский wordlist — строчный; понижаем регистр, чтобы ввод/копипаст с
        // заглавными не отвергался (неоднозначности это не создаёт).
        let normalized = phrase.trim().to_lowercase();
        let mnemonic = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &normalized)
            .map_err(|_| IdentityError::BadPhrase)?;
        Self::from_seed(&mnemonic.to_entropy())
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

    /// Доменно-разделённый под-ключ (32 байта) из seed по контекстной метке
    /// (SEC-5). BLAKE3 в режиме KDF: `context` — уникальная незасекреченная строка-
    /// домен (напр. `"peerage:blob-at-rest:v1"`), одна метка → один детерминированный
    /// ключ, независимый от seed и от ключей других доменов.
    ///
    /// ПОЛИТИКА: **любой новый ключ** (шифрование блобов at-rest, ключи медиа,
    /// статический ключ сессий и т.п.) выводить ЧЕРЕЗ этот метод, а не переиспользуя
    /// seed или ключ личности (ed25519 = транспорт эпохи 0 + MLS-credential).
    pub fn derive_subkey(&self, context: &str) -> [u8; 32] {
        blake3::derive_key(context, &self.signing.to_bytes())
    }

    // ── Транспортный ключ (ADR-007) ─────────────────────────────────────────
    //
    // Адрес Yggdrasil = функция ключа узла, поэтому «сменить адрес» = «сменить
    // ключ». Раньше это означало сменить ЛИЧНОСТЬ: тот же ed25519 — и fingerprint,
    // и MLS-credential, и запись в контактах. Здесь личность (IK) отделена от
    // транспортного ключа (TK): IK не меняется никогда, TK — расходник по эпохам.
    //
    // Зачем это нужно: ротация адреса — единственная мера, реально обрывающая
    // чужой флуд (проверено на публичном меше, ADR-007), но она обязана стоить
    // дёшево. Здесь она стоит рассылки одного подписанного объявления.

    /// Seed транспортного ключа для эпохи `epoch` (узел Yggdrasil поднимается на нём).
    ///
    /// **Эпоха 0 — сама идентичность** (`TK_0 == IK`): пока пользователь не менял
    /// адрес, всё в точности как раньше, миграция не нужна. Дальше — доменный
    /// под-ключ, то есть детерминированно из того же seed: восстановление из
    /// мнемоники продолжает работать (seed + номер эпохи → тот же адрес), отдельный
    /// секрет хранить не надо.
    pub fn transport_seed(&self, epoch: u32) -> [u8; SEED_LEN] {
        if epoch == 0 {
            return self.seed();
        }
        self.derive_subkey(&format!("peerage:transport:v1/{epoch}"))
    }

    /// Публичный транспортный ключ эпохи — то, что видит сеть как наш адрес.
    pub fn transport_public(&self, epoch: u32) -> [u8; 32] {
        SigningKey::from_bytes(&self.transport_seed(epoch)).verifying_key().to_bytes()
    }

    /// Подписать объявление «мой новый адрес — вот этот» ключом ЛИЧНОСТИ.
    ///
    /// Подпись обязательна: без неё объявление превращается в вектор захвата
    /// («привет, я новый адрес Алисы»). Домен в подписываемых байтах не даёт
    /// переиспользовать её в другом контексте, номер эпохи — переиграть старую.
    pub fn sign_transport_rotate(&self, epoch: u32, transport_pub: &[u8; 32]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        self.signing.sign(&rotate_msg(epoch, transport_pub)).to_bytes()
    }

    // ── Статический ключ сессий 1:1 (ADR-012) ───────────────────────────────
    //
    // Отдельный X25519-ключ рукопожатия, а не Box-ключ, переделанный из ed25519
    // личности (SEC-5): один ключ — одна роль. Связь с личностью даёт подпись,
    // которая едет внутри рукопожатия (как в libp2p noise).

    /// Секрет статического ключа сессий 1:1. Детерминирован из seed — после
    /// восстановления из мнемоники он тот же, хранить отдельно нечего.
    pub fn session_static_secret(&self) -> [u8; 32] {
        self.derive_subkey("peerage:session-static:v1")
    }

    /// Подписать свой статический ключ сессий ключом ЛИЧНОСТИ.
    pub fn sign_session_static(&self, static_pub: &[u8; 32]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        self.signing.sign(&session_static_msg(static_pub)).to_bytes()
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

/// Домен подписи объявления о смене адреса. Входит в подписываемые байты, чтобы
/// подпись из одного контекста не годилась в другом (SEC-5, доменное разделение).
const ROTATE_DOMAIN: &[u8] = b"peerage:transport-rotate:v1";

/// Байты, которые подписывает владелец личности при смене адреса.
fn rotate_msg(epoch: u32, transport_pub: &[u8; 32]) -> Vec<u8> {
    let mut m = Vec::with_capacity(ROTATE_DOMAIN.len() + 4 + 32);
    m.extend_from_slice(ROTATE_DOMAIN);
    m.extend_from_slice(&epoch.to_be_bytes());
    m.extend_from_slice(transport_pub);
    m
}

/// Проверить объявление о смене адреса: действительно ли владелец `identity_pub`
/// заявил, что его адрес теперь `transport_pub` в эпохе `epoch`.
///
/// Это единственное, что связывает личность с новым адресом, поэтому проверка
/// обязана быть строгой: сломанная подпись, чужая личность или подменённый номер
/// эпохи — всё это `false`. Монотонность эпохи проверяет вызывающий: здесь нет
/// знания о том, какая эпоха уже известна.
pub fn verify_transport_rotate(
    identity_pub: &[u8; 32],
    epoch: u32,
    transport_pub: &[u8; 32],
    sig: &[u8; 64],
) -> bool {
    use ed25519_dalek::Verifier;
    let Ok(vk) = VerifyingKey::from_bytes(identity_pub) else { return false };
    vk.verify(&rotate_msg(epoch, transport_pub), &ed25519_dalek::Signature::from_bytes(sig))
        .is_ok()
}

/// Домен подписи статического ключа сессий (ADR-012).
const SESSION_STATIC_DOMAIN: &[u8] = b"peerage:session-static:v1";

fn session_static_msg(static_pub: &[u8; 32]) -> Vec<u8> {
    [SESSION_STATIC_DOMAIN, static_pub.as_slice()].concat()
}

/// Проверить, что статический ключ сессий `static_pub` принадлежит `identity_pub`.
/// Строгая проверка (`verify_strict`): ключ малого порядка и неканоничная подпись
/// не проходят — проверяется подпись, пришедшая от кого угодно.
pub fn verify_session_static(identity_pub: &[u8; 32], static_pub: &[u8; 32], sig: &[u8; 64]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(identity_pub) else { return false };
    vk.verify_strict(&session_static_msg(static_pub), &ed25519_dalek::Signature::from_bytes(sig))
        .is_ok()
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

/// Сколько знаков в визуальном отпечатке (сетка 6×2 в UI).
pub const VISUAL_FP_LEN: usize = 12;

/// Версия НАБОРА знаков и правила отображения. Сверка идёт глазами по картинке,
/// поэтому смена набора обязана менять и сам отпечаток: иначе двое с разными
/// версиями приложения увидят разные картинки для одного ключа и решат, что их
/// подслушивают. Домен-сепаратор ниже включает версию — бампнули набор, старые
/// сверки честно инвалидируются.
pub const VISUAL_FP_VERSION: u8 = 1;

/// Визуальный отпечаток публичного ключа: 12 байт = 12 знаков по 8 бит = 96 бит
/// энтропии (столько же, сколько у 8 слов BIP39 с их 88 битами, и заметно больше,
/// чем у коротких «эмодзи-хешей» на 4-6 символов).
///
/// НЕ секрет — выводится из ПУБЛИЧНОГО ключа и служит для сверки «та ли это связь».
/// Не путать с сид-фразой (она восстанавливает личность и секретна).
pub fn visual_fingerprint(public_key: &[u8; 32]) -> [u8; VISUAL_FP_LEN] {
    // Домен-сепаратор с версией: тот же ключ в другой роли (или в другой версии
    // набора) обязан давать другой отпечаток.
    let mut hasher = Sha256::new();
    hasher.update(b"peerage:visual-fingerprint:v");
    hasher.update([VISUAL_FP_VERSION]);
    hasher.update(public_key);
    let hash = hasher.finalize();
    let mut out = [0u8; VISUAL_FP_LEN];
    out.copy_from_slice(&hash[..VISUAL_FP_LEN]);
    out
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
    fn derive_subkey_is_domain_separated_and_deterministic() {
        let id = Identity::from_seed(&[5u8; 32]).unwrap();
        let a1 = id.derive_subkey("peerage:blob-at-rest:v1");
        let a2 = id.derive_subkey("peerage:blob-at-rest:v1");
        let b = id.derive_subkey("peerage:media-key:v1");
        // Детерминирован по (seed, context).
        assert_eq!(a1, a2);
        // Разные домены → разные ключи.
        assert_ne!(a1, b);
        // Не равен seed-у (не переиспользование).
        assert_ne!(a1, id.seed());
        // Разные идентичности → разные под-ключи для того же домена.
        let other = Identity::from_seed(&[6u8; 32]).unwrap();
        assert_ne!(a1, other.derive_subkey("peerage:blob-at-rest:v1"));
    }

    #[test]
    fn transport_epoch_zero_is_the_identity_itself() {
        // Обратная совместимость: пока адрес не меняли, узел поднимается на том же
        // ключе, что и раньше. Иначе существующие установки «переехали» бы молча.
        let id = Identity::from_seed(&[7u8; 32]).unwrap();
        assert_eq!(id.transport_seed(0), id.seed());
        assert_eq!(id.transport_public(0), id.public_key_bytes());
    }

    #[test]
    fn transport_keys_differ_per_epoch_and_survive_restore() {
        let id = Identity::from_seed(&[7u8; 32]).unwrap();
        let (t1, t2) = (id.transport_public(1), id.transport_public(2));
        assert_ne!(t1, id.public_key_bytes(), "новая эпоха — новый адрес");
        assert_ne!(t1, t2, "разные эпохи не пересекаются");
        // Детерминизм: восстановление из той же мнемоники даёт тот же адрес эпохи,
        // поэтому отдельный секрет для транспорта хранить не нужно.
        let restored = Identity::from_seed(&id.seed()).unwrap();
        assert_eq!(restored.transport_public(1), t1);
        // Чужая личность — чужие транспортные ключи.
        let other = Identity::from_seed(&[8u8; 32]).unwrap();
        assert_ne!(other.transport_public(1), t1);
    }

    #[test]
    fn rotate_announcement_verifies_only_for_its_author_epoch_and_key() {
        let alice = Identity::from_seed(&[9u8; 32]).unwrap();
        let mallory = Identity::from_seed(&[10u8; 32]).unwrap();
        let tk = alice.transport_public(3);
        let sig = alice.sign_transport_rotate(3, &tk);

        assert!(verify_transport_rotate(&alice.public_key_bytes(), 3, &tk, &sig));
        // Чужая личность не может выдать свой адрес за адрес Алисы — иначе
        // объявление становилось бы вектором захвата контакта.
        assert!(!verify_transport_rotate(&mallory.public_key_bytes(), 3, &tk, &sig));
        // Ни номер эпохи, ни объявляемый ключ подменить нельзя: оба под подписью.
        assert!(!verify_transport_rotate(&alice.public_key_bytes(), 4, &tk, &sig));
        assert!(!verify_transport_rotate(
            &alice.public_key_bytes(),
            3,
            &mallory.transport_public(3),
            &sig,
        ));
        // Битая подпись — не «почти верно», а нет.
        let mut broken = sig;
        broken[0] ^= 1;
        assert!(!verify_transport_rotate(&alice.public_key_bytes(), 3, &tk, &broken));
    }

    #[test]
    fn visual_fingerprint_is_stable_and_key_dependent() {
        let a = Identity::generate();
        let b = Identity::generate();
        let fa = visual_fingerprint(&a.public_key_bytes());
        assert_eq!(fa, visual_fingerprint(&a.public_key_bytes()), "детерминирован");
        assert_ne!(fa, visual_fingerprint(&b.public_key_bytes()), "зависит от ключа");
        assert_eq!(fa.len(), VISUAL_FP_LEN);
    }

    /// Отпечаток обязан отличаться от «просто SHA-256 ключа»: домен-сепаратор
    /// с версией — то, что позволит сменить набор знаков, не сломав доверие молча.
    #[test]
    fn visual_fingerprint_is_domain_separated() {
        let id = Identity::generate();
        let pk = id.public_key_bytes();
        let plain = Sha256::digest(pk);
        assert_ne!(&visual_fingerprint(&pk)[..], &plain[..VISUAL_FP_LEN]);
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

    #[test]
    fn seed_phrase_roundtrip_restores_identity() {
        // Сид-фраза = 24 слова и восстанавливает ТУ ЖЕ личность (тот же seed/pubkey).
        let id = Identity::from_seed(&[7u8; 32]).unwrap();
        let phrase = id.seed_phrase();
        assert_eq!(phrase.split_whitespace().count(), 24);
        let restored = Identity::from_phrase(&phrase).unwrap();
        assert_eq!(restored.seed(), id.seed());
        assert_eq!(restored.public_key_bytes(), id.public_key_bytes());
        // Нормализация пробелов/регистра.
        let messy = format!("  {}  ", phrase.to_uppercase());
        assert_eq!(Identity::from_phrase(&messy).unwrap().seed(), id.seed());
        // Мусор → ошибка, не паника.
        assert!(Identity::from_phrase("not a valid mnemonic phrase").is_err());
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
