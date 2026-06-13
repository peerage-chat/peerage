//! Персистентность: профиль (идентичность) и контакты на диске.
//!
//! Формат — JSON. Seed профиля секретен: на Unix файл создаётся с правами 0600.
//! Кросс-платформенно (std::fs есть и на Desktop, и на Android).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::identity::Identity;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("ввод-вывод: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("неверный hex: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("неверная длина ключа/seed")]
    BadLength,
}

// ---- Профиль (идентичность) ----

#[derive(Serialize, Deserialize)]
struct ProfileFile {
    version: u8,
    /// seed ed25519 (32 байта, hex). СЕКРЕТ.
    seed: String,
}

/// Профиль пользователя: хранит идентичность.
pub struct Profile {
    pub identity: Identity,
}

impl Profile {
    /// Загрузить профиль из файла, либо создать новый (сгенерировать
    /// идентичность и сохранить). Идемпотентно для запуска приложения.
    pub fn load_or_create(path: &Path) -> Result<Profile, StoreError> {
        if path.exists() {
            Self::load(path)
        } else {
            let profile = Profile {
                identity: Identity::generate(),
            };
            profile.save(path)?;
            Ok(profile)
        }
    }

    /// Загрузить профиль из файла.
    pub fn load(path: &Path) -> Result<Profile, StoreError> {
        let data = std::fs::read(path)?;
        let pf: ProfileFile = serde_json::from_slice(&data)?;
        let seed = hex::decode(&pf.seed)?;
        let identity = Identity::from_seed(&seed).map_err(|_| StoreError::BadLength)?;
        Ok(Profile { identity })
    }

    /// Сохранить профиль (с правами 0600 на Unix).
    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let pf = ProfileFile {
            version: 1,
            seed: hex::encode(self.identity.seed()),
        };
        let json = serde_json::to_vec_pretty(&pf)?;
        std::fs::write(path, &json)?;
        restrict_permissions(path)?;
        Ok(())
    }
}

// ---- Контакты (TOFU) ----

/// Контакт: идентифицируется публичным ключом ed25519 (он же адрес/идентичность).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Contact {
    /// Публичный ключ ed25519 (hex).
    pub key: String,
    /// Отображаемое имя.
    pub name: String,
    /// Fingerprint (8 слов BIP39) для верификации.
    pub fingerprint: String,
    /// Когда добавлен (unix-секунды).
    pub added: i64,
}

#[derive(Serialize, Deserialize, Default)]
struct ContactsFile {
    version: u8,
    contacts: Vec<Contact>,
}

/// Результат добавления контакта (TOFU).
#[derive(Debug, PartialEq, Eq)]
pub enum AddOutcome {
    /// Новый контакт сохранён.
    Added,
    /// Контакт с этим ключом уже есть (TOFU — доверяем сохранённому).
    AlreadyKnown,
}

/// Хранилище контактов с персистентностью.
pub struct ContactStore {
    path: PathBuf,
    by_key: HashMap<[u8; 32], Contact>,
}

impl ContactStore {
    /// Загрузить хранилище из файла (или создать пустое, если файла нет).
    pub fn load(path: &Path) -> Result<ContactStore, StoreError> {
        let mut by_key = HashMap::new();
        if path.exists() {
            let data = std::fs::read(path)?;
            let cf: ContactsFile = serde_json::from_slice(&data)?;
            for c in cf.contacts {
                let key = decode_key(&c.key)?;
                by_key.insert(key, c);
            }
        }
        Ok(ContactStore {
            path: path.to_path_buf(),
            by_key,
        })
    }

    /// Добавить контакт по публичному ключу (TOFU). Если ключ уже известен —
    /// возвращает [`AddOutcome::AlreadyKnown`] и НЕ перезаписывает имя.
    pub fn add(
        &mut self,
        key: &[u8; 32],
        name: &str,
        added: i64,
    ) -> Result<AddOutcome, StoreError> {
        if self.by_key.contains_key(key) {
            return Ok(AddOutcome::AlreadyKnown);
        }
        let contact = Contact {
            key: hex::encode(key),
            name: name.to_string(),
            fingerprint: crate::identity::fingerprint_words(key).join(" "),
            added,
        };
        self.by_key.insert(*key, contact);
        self.save()?;
        Ok(AddOutcome::Added)
    }

    /// Контакт по ключу.
    pub fn get(&self, key: &[u8; 32]) -> Option<&Contact> {
        self.by_key.get(key)
    }

    /// Все контакты.
    pub fn list(&self) -> Vec<&Contact> {
        self.by_key.values().collect()
    }

    /// Знаем ли мы этот ключ.
    pub fn contains(&self, key: &[u8; 32]) -> bool {
        self.by_key.contains_key(key)
    }

    /// Записать хранилище на диск.
    pub fn save(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let cf = ContactsFile {
            version: 1,
            contacts: self.by_key.values().cloned().collect(),
        };
        let json = serde_json::to_vec_pretty(&cf)?;
        std::fs::write(&self.path, &json)?;
        Ok(())
    }
}

fn decode_key(hex_str: &str) -> Result<[u8; 32], StoreError> {
    let bytes = hex::decode(hex_str)?;
    bytes.as_slice().try_into().map_err(|_| StoreError::BadLength)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("p2p-store-test-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn profile_load_or_create_then_reload_is_stable() {
        let dir = tmp_dir();
        let path = dir.join("profile.json");

        let p1 = Profile::load_or_create(&path).unwrap();
        let key1 = p1.identity.public_key_bytes();

        // Повторная загрузка возвращает ту же идентичность.
        let p2 = Profile::load_or_create(&path).unwrap();
        assert_eq!(p2.identity.public_key_bytes(), key1);
        assert_eq!(p2.identity.seed(), p1.identity.seed());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contacts_add_persist_reload() {
        let dir = tmp_dir();
        let path = dir.join("contacts.json");

        let alice = Identity::generate();
        let bob = Identity::generate();

        let mut store = ContactStore::load(&path).unwrap();
        assert_eq!(store.add(&alice.public_key_bytes(), "Alice", 100).unwrap(), AddOutcome::Added);
        assert_eq!(store.add(&bob.public_key_bytes(), "Bob", 200).unwrap(), AddOutcome::Added);
        // Повторное добавление — TOFU, уже известен.
        assert_eq!(store.add(&alice.public_key_bytes(), "Alice2", 300).unwrap(), AddOutcome::AlreadyKnown);

        // Перезагрузка с диска.
        let store2 = ContactStore::load(&path).unwrap();
        assert!(store2.contains(&alice.public_key_bytes()));
        assert!(store2.contains(&bob.public_key_bytes()));
        assert_eq!(store2.list().len(), 2);
        let c = store2.get(&alice.public_key_bytes()).unwrap();
        assert_eq!(c.name, "Alice"); // имя не перезаписалось
        assert_eq!(c.fingerprint, alice.fingerprint());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contacts_empty_load_ok() {
        let dir = tmp_dir();
        let store = ContactStore::load(&dir.join("nope.json")).unwrap();
        assert_eq!(store.list().len(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
