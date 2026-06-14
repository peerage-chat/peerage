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
        write_secret(path, &json)?;
        Ok(())
    }
}

/// Записать секретный файл сразу с ограниченными правами (без окна TOCTOU).
#[cfg(unix)]
fn write_secret(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(data)
}

#[cfg(not(unix))]
fn write_secret(path: &Path, data: &[u8]) -> std::io::Result<()> {
    // На не-Unix защита прав — задача ОС/хранилища; см. M1 (шифрование at-rest).
    std::fs::write(path, data)
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

    /// Переименовать контакт. Возвращает false, если контакта нет.
    pub fn rename(&mut self, key: &[u8; 32], name: &str) -> Result<bool, StoreError> {
        match self.by_key.get_mut(key) {
            Some(c) => {
                c.name = name.to_string();
                self.save()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Удалить контакт. Возвращает false, если контакта не было.
    pub fn remove(&mut self, key: &[u8; 32]) -> Result<bool, StoreError> {
        let removed = self.by_key.remove(key).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
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

// ---- История сообщений ----

/// Одно сохранённое сообщение чата.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMessage {
    /// Идентификатор сообщения (от отправителя).
    pub id: u64,
    /// Текст.
    pub text: String,
    /// true — исходящее (наше), false — входящее.
    pub mine: bool,
    /// Unix-время (секунды).
    pub time: i64,
    /// Статус: "sent" | "delivered" | "failed" | "" (для входящих).
    pub status: String,
}

#[derive(Serialize, Deserialize, Default)]
struct MessagesFile {
    version: u8,
    /// peer (hex ключа) → лог сообщений.
    log: std::collections::BTreeMap<String, Vec<StoredMessage>>,
}

/// Персистентная история сообщений по контактам.
pub struct MessageStore {
    path: PathBuf,
    log: HashMap<[u8; 32], Vec<StoredMessage>>,
}

impl MessageStore {
    /// Загрузить (или создать пустую, если файла нет).
    pub fn load(path: &Path) -> Result<MessageStore, StoreError> {
        let mut log = HashMap::new();
        if path.exists() {
            let data = std::fs::read(path)?;
            let mf: MessagesFile = serde_json::from_slice(&data)?;
            for (k, msgs) in mf.log {
                let key = decode_key(&k)?;
                log.insert(key, msgs);
            }
        }
        Ok(MessageStore {
            path: path.to_path_buf(),
            log,
        })
    }

    /// Добавить сообщение в лог пира и сохранить.
    pub fn append(&mut self, peer: &[u8; 32], msg: StoredMessage) -> Result<(), StoreError> {
        self.log.entry(*peer).or_default().push(msg);
        self.save()
    }

    /// Обновить статус исходящего сообщения с данным id. Возвращает true, если найдено.
    pub fn set_status(
        &mut self,
        peer: &[u8; 32],
        id: u64,
        status: &str,
    ) -> Result<bool, StoreError> {
        let found = self
            .log
            .get_mut(peer)
            .and_then(|msgs| msgs.iter_mut().find(|m| m.id == id && m.mine))
            .map(|m| m.status = status.to_string())
            .is_some();
        if found {
            self.save()?;
        }
        Ok(found)
    }

    /// История сообщений с пиром (пустой срез, если нет).
    pub fn history(&self, peer: &[u8; 32]) -> &[StoredMessage] {
        self.log.get(peer).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Удалить весь лог с пиром (например, при удалении контакта).
    pub fn clear(&mut self, peer: &[u8; 32]) -> Result<(), StoreError> {
        if self.log.remove(peer).is_some() {
            self.save()?;
        }
        Ok(())
    }

    fn save(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mf = MessagesFile {
            version: 1,
            log: self
                .log
                .iter()
                .map(|(k, v)| (hex::encode(k), v.clone()))
                .collect(),
        };
        std::fs::write(&self.path, serde_json::to_vec_pretty(&mf)?)?;
        Ok(())
    }
}

fn decode_key(hex_str: &str) -> Result<[u8; 32], StoreError> {
    let bytes = hex::decode(hex_str)?;
    bytes.as_slice().try_into().map_err(|_| StoreError::BadLength)
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

    #[test]
    fn contacts_rename_and_remove() {
        let dir = tmp_dir();
        let path = dir.join("contacts.json");
        let k = Identity::generate().public_key_bytes();

        let mut store = ContactStore::load(&path).unwrap();
        store.add(&k, "Old", 0).unwrap();
        assert!(store.rename(&k, "New").unwrap());
        assert_eq!(store.get(&k).unwrap().name, "New");
        assert!(store.remove(&k).unwrap());
        assert!(!store.contains(&k));

        // Перезагрузка отражает изменения.
        let store2 = ContactStore::load(&path).unwrap();
        assert!(!store2.contains(&k));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn messages_append_status_persist() {
        let dir = tmp_dir();
        let path = dir.join("messages.json");
        let peer = Identity::generate().public_key_bytes();

        let mut store = MessageStore::load(&path).unwrap();
        store
            .append(&peer, StoredMessage { id: 1, text: "hi".into(), mine: true, time: 10, status: "sent".into() })
            .unwrap();
        store
            .append(&peer, StoredMessage { id: 5, text: "yo".into(), mine: false, time: 11, status: "".into() })
            .unwrap();
        assert!(store.set_status(&peer, 1, "delivered").unwrap());

        let store2 = MessageStore::load(&path).unwrap();
        let h = store2.history(&peer);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].status, "delivered");
        assert_eq!(h[1].text, "yo");
        assert!(!h[1].mine);
        std::fs::remove_dir_all(&dir).ok();
    }
}
