//! Шифрование данных на диске (at-rest). Ключ шифрования хранится в системном
//! хранилище секретов ОС (keyring: Windows Credential Manager / macOS Keychain /
//! Linux Secret Service) — НЕ рядом с данными, поэтому доступ к диску не выдаёт
//! ни ключ идентичности, ни историю.
//!
//! Формат: [marker][nonce(24)][ciphertext]. marker=1 — зашифровано, marker=0 —
//! открытый текст; последний пишется и читается ТОЛЬКО без ключа (тесты, режим без
//! keyring). При заданном ключе открытый текст не принимается: иначе файл,
//! подложенный в каталог данных, читался бы как свой (R-038).
//!
//! Шифртекст привязан к назначению файла (AAD — см. [`purpose`]): вид данных, id и
//! номер чанка. Блоб, переложенный под чужой контент-адрес, или чанк, переставленный
//! на чужое место, не расшифруются.
//!
//! Если хранилище секретов недоступно (напр. headless без D-Bus), [`init`] вернёт
//! ошибку, и стораджи пишут открытым текстом — приложение должно предупредить.

use std::sync::OnceLock;

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng as AeadRng, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};

const NONCE_LEN: usize = 24;
const SERVICE: &str = "chat.peerage";
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

/// Инициализировать ключом из файла (Android и прочие платформы без надёжного
/// keyring). Ключ — 32 случайных байта в приватном каталоге приложения (sandbox).
/// При первом запуске создаётся и пишется с правами 0600; далее читается.
///
/// Слабее аппаратного хранилища (Android Keystore — дальнейшее усиление), но
/// детерминирован между перезапусками, в отличие от `keyring` на Android.
pub fn init_with_keyfile(path: &std::path::Path) -> Result<(), VaultError> {
    if KEY.get().is_some() {
        return Ok(());
    }
    let key: [u8; 32] = match std::fs::read(path) {
        Ok(bytes) => bytes.as_slice().try_into().map_err(|_| VaultError::BadKey)?,
        Err(_) => {
            let mut k = [0u8; 32];
            OsRng.fill_bytes(&mut k);
            write_keyfile(path, &k)?;
            k
        }
    };
    let _ = KEY.set(key);
    Ok(())
}

/// Записать ключ атомарно с ограничением прав (0600 на unix).
fn write_keyfile(path: &std::path::Path, key: &[u8; 32]) -> Result<(), VaultError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VaultError::Keyring(e.to_string()))?;
    }
    crate::fsutil::write_atomic_private(path, key).map_err(|e| VaultError::Keyring(e.to_string()))
}

/// Включено ли шифрование at-rest.
pub fn enabled() -> bool {
    KEY.get().is_some()
}

/// Копия текущего ключа (для SQLCipher PRAGMA key).
pub fn key_bytes() -> Option<[u8; 32]> {
    KEY.get().copied()
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

/// Назначение запечатанного — AAD: вид данных, id и номер чанка (0 у цельных
/// файлов). Разные виды не пересекаются: `kind` отделён нулевым байтом, id — длиной.
pub fn purpose(kind: &str, id: &[u8], index: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(kind.len() + 1 + 4 + id.len() + 8);
    out.extend_from_slice(kind.as_bytes());
    out.push(0);
    out.extend_from_slice(&(id.len() as u32).to_be_bytes());
    out.extend_from_slice(id);
    out.extend_from_slice(&index.to_be_bytes());
    out
}

/// AAD блоба, лежащего одним буфером.
pub fn blob_purpose(id: &[u8]) -> Vec<u8> {
    purpose("blob", id, 0)
}

/// AAD чанка крупного блоба.
pub fn chunk_purpose(id: &[u8], index: usize) -> Vec<u8> {
    purpose("blob-chunk", id, index as u64)
}

/// Зашифровать байты для записи на диск, привязав их к назначению `aad` (см.
/// [`purpose`]). Без инициализированного ключа — данные с маркером «открытый текст».
pub fn seal(aad: &[u8], plain: &[u8]) -> Vec<u8> {
    match KEY.get() {
        Some(k) => {
            let cipher = XChaCha20Poly1305::new(Key::from_slice(k));
            let nonce = XChaCha20Poly1305::generate_nonce(&mut AeadRng);
            let ct = cipher
                .encrypt(&nonce, Payload { msg: plain, aad })
                .expect("at-rest encrypt");
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

/// Расшифровать прочитанные с диска байты назначения `aad`. При заданном ключе
/// принимается только зашифрованный формат, без ключа — только маркер «открытый
/// текст»; всё прочее — ошибка.
pub fn open(aad: &[u8], data: &[u8]) -> Result<Vec<u8>, VaultError> {
    match (KEY.get(), data.split_first()) {
        (Some(key), Some((1, rest))) => {
            if rest.len() < NONCE_LEN {
                return Err(VaultError::Decrypt);
            }
            let (n, ct) = rest.split_at(NONCE_LEN);
            let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
            cipher
                .decrypt(XNonce::from_slice(n), Payload { msg: ct, aad })
                .map_err(|_| VaultError::Decrypt)
        }
        (None, Some((0, rest))) => Ok(rest.to_vec()),
        _ => Err(VaultError::Decrypt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ключ процесса — OnceLock: тесты ниже работают с ключом [7; 32].

    #[test]
    fn seal_open_roundtrip_with_key() {
        init_with_key([7u8; 32]);
        let pt = b"secret identity seed and messages";
        let aad = purpose("profile", &[], 0);
        let sealed = seal(&aad, pt);
        assert_eq!(sealed[0], 1, "должно быть зашифровано");
        assert_ne!(&sealed[1..], &pt[..]);
        assert_eq!(open(&aad, &sealed).unwrap(), pt);
    }

    /// R-038: при заданном ключе открытый текст — и с маркером, и без — не читается.
    #[test]
    fn plaintext_is_rejected_when_key_is_set() {
        init_with_key([7u8; 32]);
        let aad = purpose("profile", &[], 0);
        assert!(open(&aad, b"\x00{\"seed\":\"x\"}").is_err(), "маркер «открытый текст»");
        assert!(open(&aad, b"{\"seed\":\"x\"}").is_err(), "файл без маркера");
    }

    /// R-038: шифртекст привязан к назначению — блоб под чужим адресом, чанк на
    /// чужом месте и файл другого вида не расшифровываются.
    #[test]
    fn ciphertext_is_bound_to_its_purpose() {
        init_with_key([7u8; 32]);
        let sealed = seal(&blob_purpose(&[1; 32]), b"photo");
        assert!(open(&blob_purpose(&[2; 32]), &sealed).is_err(), "другой адрес");
        assert!(open(&chunk_purpose(&[1; 32], 0), &sealed).is_err(), "другой вид");
        let chunk = seal(&chunk_purpose(&[1; 32], 3), b"part");
        assert!(open(&chunk_purpose(&[1; 32], 4), &chunk).is_err(), "другое место");
        assert_eq!(open(&chunk_purpose(&[1; 32], 3), &chunk).unwrap(), b"part");
    }
}
