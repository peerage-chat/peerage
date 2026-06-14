//! Шифрование данных на диске (at-rest). Ключ шифрования хранится в системном
//! хранилище секретов ОС (keyring: Windows Credential Manager / macOS Keychain /
//! Linux Secret Service) — НЕ рядом с данными, поэтому доступ к диску не выдаёт
//! ни ключ идентичности, ни историю.
//!
//! Формат: [marker][nonce(24)][ciphertext]. marker=1 — зашифровано; иначе данные
//! считаются открытым текстом (миграция старых файлов и режим без keyring).
//!
//! Если хранилище секретов недоступно (напр. headless без D-Bus), [`init`] вернёт
//! ошибку, и стораджи пишут открытым текстом — приложение должно предупредить.

use std::sync::OnceLock;

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng as AeadRng};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};

const NONCE_LEN: usize = 24;
const SERVICE: &str = "dev.p2p.messenger";
const ENTRY: &str = "vault-key";

static KEY: OnceLock<[u8; 32]> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("хранилище секретов ОС недоступно: {0}")]
    Keyring(String),
    #[error("неверный ключ в хранилище")]
    BadKey,
    #[error("расшифровка at-rest не удалась")]
    Decrypt,
}

/// Инициализировать шифрование at-rest: получить/создать ключ в keyring ОС.
pub fn init() -> Result<(), VaultError> {
    if KEY.get().is_some() {
        return Ok(());
    }
    let key = key_from_keyring()?;
    let _ = KEY.set(key);
    Ok(())
}

/// Инициализировать фиксированным ключом (для тестов; без обращения к ОС).
pub fn init_with_key(key: [u8; 32]) {
    let _ = KEY.set(key);
}

/// Включено ли шифрование at-rest.
pub fn enabled() -> bool {
    KEY.get().is_some()
}

fn key_from_keyring() -> Result<[u8; 32], VaultError> {
    let entry = keyring::Entry::new(SERVICE, ENTRY).map_err(|e| VaultError::Keyring(e.to_string()))?;
    match entry.get_password() {
        Ok(h) => {
            let bytes = hex::decode(&h).map_err(|_| VaultError::BadKey)?;
            bytes.as_slice().try_into().map_err(|_| VaultError::BadKey)
        }
        Err(keyring::Error::NoEntry) => {
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            entry
                .set_password(&hex::encode(key))
                .map_err(|e| VaultError::Keyring(e.to_string()))?;
            Ok(key)
        }
        Err(e) => Err(VaultError::Keyring(e.to_string())),
    }
}

/// Зашифровать байты для записи на диск. Без инициализированного ключа —
/// возвращает данные с маркером «открытый текст».
pub fn seal(plain: &[u8]) -> Vec<u8> {
    match KEY.get() {
        Some(k) => {
            let cipher = XChaCha20Poly1305::new(Key::from_slice(k));
            let nonce = XChaCha20Poly1305::generate_nonce(&mut AeadRng);
            let ct = cipher.encrypt(&nonce, plain).expect("at-rest encrypt");
            let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
            out.push(1);
            out.extend_from_slice(nonce.as_slice());
            out.extend_from_slice(&ct);
            out
        }
        None => {
            let mut out = Vec::with_capacity(1 + plain.len());
            out.push(0);
            out.extend_from_slice(plain);
            out
        }
    }
}

/// Расшифровать прочитанные с диска байты. Понимает зашифрованный формат,
/// маркер «открытый текст» и legacy-файлы без маркера.
pub fn open(data: &[u8]) -> Result<Vec<u8>, VaultError> {
    match data.first() {
        Some(1) => {
            let key = KEY.get().ok_or(VaultError::Decrypt)?;
            let rest = &data[1..];
            if rest.len() < NONCE_LEN {
                return Err(VaultError::Decrypt);
            }
            let (n, ct) = rest.split_at(NONCE_LEN);
            let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
            cipher.decrypt(XNonce::from_slice(n), ct).map_err(|_| VaultError::Decrypt)
        }
        Some(0) => Ok(data[1..].to_vec()),
        _ => Ok(data.to_vec()), // legacy: весь файл — открытый текст
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip_with_key() {
        init_with_key([7u8; 32]);
        let pt = b"secret identity seed and messages";
        let sealed = seal(pt);
        assert_eq!(sealed[0], 1, "должно быть зашифровано");
        assert_ne!(&sealed[1..], &pt[..]);
        assert_eq!(open(&sealed).unwrap(), pt);
    }

    #[test]
    fn open_handles_legacy_plaintext() {
        // Старый файл без маркера (например, JSON, начинается с '{' = 0x7b).
        let legacy = b"{\"version\":1}";
        assert_eq!(open(legacy).unwrap(), legacy);
    }
}
