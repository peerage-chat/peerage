//! Персистентность: профиль, контакты и история сообщений.
//!
//! * [`Profile`] — идентичность, хранится в profile.json (секрет, 0600 на Unix).
//! * [`ContactStore`] — TOFU-контакты в SQLite, in-memory кэш для быстрых чтений.
//! * [`MessageStore`] — история сообщений в SQLite, без кэша (прямые запросы).
//!
//! Оба стора принимают [`crate::db::Db`] — общее `Arc<Mutex<Connection>>`.
//! Миграция из JSON-файлов выполняется один раз в `open()` при наличии legacy-файла.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::{self, Db};
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
    #[error("at-rest: {0}")]
    Vault(#[from] crate::vault::VaultError),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
}

// ---- Профиль (идентичность) ------------------------------------------------

#[derive(Serialize, Deserialize)]
struct ProfileFile {
    version: u8,
    seed: String,
}

/// Профиль пользователя — хранит идентичность в profile.json.
pub struct Profile {
    pub identity: Identity,
}

impl Profile {
    pub fn load_or_create(path: &Path) -> Result<Profile, StoreError> {
        if path.exists() { Self::load(path) } else {
            let profile = Profile { identity: Identity::generate() };
            profile.save(path)?;
            Ok(profile)
        }
    }

    pub fn load(path: &Path) -> Result<Profile, StoreError> {
        let data = crate::vault::open(&std::fs::read(path)?)?;
        let pf: ProfileFile = serde_json::from_slice(&data)?;
        let seed = hex::decode(&pf.seed)?;
        let identity = Identity::from_seed(&seed).map_err(|_| StoreError::BadLength)?;
        Ok(Profile { identity })
    }

    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let pf = ProfileFile { version: 1, seed: hex::encode(self.identity.seed()) };
        let json = serde_json::to_vec_pretty(&pf)?;
        write_secret(path, &crate::vault::seal(&json))?;
        Ok(())
    }
}

#[cfg(unix)]
fn write_secret(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(data)
}

#[cfg(not(unix))]
fn write_secret(path: &Path, data: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, data)
}

// ---- Контакты (TOFU) -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Contact {
    pub key: String,
    /// Локальное имя — как ЭТОТ пользователь назвал контакт (TOFU-заметка).
    pub name: String,
    pub fingerprint: String,
    pub added: i64,
    #[serde(default)]
    pub verified: bool,
    /// Самоназвание контакта (M2): как он назвал СЕБЯ. Приоритет отображения:
    /// локальное `name` → `profile_name` → hex. Приходит в `ProfileUpdate`.
    #[serde(default)]
    pub profile_name: String,
    /// hex-хэш аватара контакта (M2). Байты тянутся блобом по хэшу; показ — blob_read.
    #[serde(default)]
    pub avatar_id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddOutcome {
    Added,
    AlreadyKnown,
}

/// SQLite-хранилище контактов с in-memory кэшем для быстрых чтений.
pub struct ContactStore {
    db: Db,
    cache: HashMap<[u8; 32], Contact>,
}

impl ContactStore {
    /// Открыть хранилище. Если `legacy` указывает на существующий contacts.json,
    /// импортирует оттуда данные и переименовывает файл в .bak.
    pub fn open(db: Db, legacy: Option<&Path>) -> Result<Self, StoreError> {
        let mut store = ContactStore { db: db.clone(), cache: HashMap::new() };
        store.load_cache()?;

        if let Some(path) = legacy {
            if path.exists() {
                store.import_json(path)?;
                let _ = std::fs::rename(path, path.with_extension("json.bak"));
            }
        }
        Ok(store)
    }

    fn load_cache(&mut self) -> Result<(), StoreError> {
        let conn = db::lock(&self.db);
        let mut stmt = conn.prepare(
            "SELECT key, name, fingerprint, added, verified, profile_name, avatar_id FROM contacts",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        for row in rows {
            let (kb, name, fp, added, ver, profile_name, avatar_id) = row?;
            if let Ok(key) = <[u8; 32]>::try_from(kb.as_slice()) {
                self.cache.insert(key, Contact {
                    key: hex::encode(key),
                    name,
                    fingerprint: fp,
                    added,
                    verified: ver != 0,
                    profile_name,
                    avatar_id,
                });
            }
        }
        Ok(())
    }

    fn import_json(&mut self, path: &Path) -> Result<(), StoreError> {
        #[derive(Deserialize)]
        struct LegacyFile { contacts: Vec<Contact> }

        let raw = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return Ok(()),
        };
        let data = crate::vault::open(&raw).unwrap_or(raw);
        let lf: LegacyFile = match serde_json::from_slice(&data) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };
        let conn = db::lock(&self.db);
        for c in lf.contacts {
            let key = match decode_key(&c.key) { Ok(k) => k, Err(_) => continue };
            if self.cache.contains_key(&key) { continue; }
            conn.execute(
                "INSERT OR IGNORE INTO contacts (key, name, fingerprint, added, verified)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![key.as_slice(), &c.name, &c.fingerprint, c.added, c.verified as i64],
            )?;
            self.cache.insert(key, c);
        }
        Ok(())
    }

    pub fn add(&mut self, key: &[u8; 32], name: &str, added: i64) -> Result<AddOutcome, StoreError> {
        if self.cache.contains_key(key) {
            return Ok(AddOutcome::AlreadyKnown);
        }
        let contact = Contact {
            key: hex::encode(key),
            name: name.to_string(),
            fingerprint: crate::identity::fingerprint_words(key).join(" "),
            added,
            verified: false,
            profile_name: String::new(),
            avatar_id: String::new(),
        };
        db::lock(&self.db).execute(
            "INSERT OR IGNORE INTO contacts (key, name, fingerprint, added, verified)
             VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![key.as_slice(), &contact.name, &contact.fingerprint, added],
        )?;
        self.cache.insert(*key, contact);
        Ok(AddOutcome::Added)
    }

    pub fn get(&self, key: &[u8; 32]) -> Option<&Contact> {
        self.cache.get(key)
    }

    pub fn list(&self) -> Vec<&Contact> {
        self.cache.values().collect()
    }

    pub fn contains(&self, key: &[u8; 32]) -> bool {
        self.cache.contains_key(key)
    }

    pub fn set_verified(&mut self, key: &[u8; 32], verified: bool) -> Result<bool, StoreError> {
        match self.cache.get_mut(key) {
            Some(c) => {
                c.verified = verified;
                db::lock(&self.db).execute(
                    "UPDATE contacts SET verified=?1 WHERE key=?2",
                    rusqlite::params![verified as i64, key.as_slice()],
                )?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn rename(&mut self, key: &[u8; 32], name: &str) -> Result<bool, StoreError> {
        match self.cache.get_mut(key) {
            Some(c) => {
                c.name = name.to_string();
                db::lock(&self.db).execute(
                    "UPDATE contacts SET name=?1 WHERE key=?2",
                    rusqlite::params![name, key.as_slice()],
                )?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Обновить профиль контакта (M2): его самоназвание + hex-хэш аватара (из
    /// `ProfileUpdate`). Локальное `name` не трогаем (приоритет за пользователем).
    pub fn set_profile(&mut self, key: &[u8; 32], profile_name: &str, avatar_id: &str) -> Result<bool, StoreError> {
        match self.cache.get_mut(key) {
            Some(c) => {
                c.profile_name = profile_name.to_string();
                c.avatar_id = avatar_id.to_string();
                db::lock(&self.db).execute(
                    "UPDATE contacts SET profile_name=?1, avatar_id=?2 WHERE key=?3",
                    rusqlite::params![profile_name, avatar_id, key.as_slice()],
                )?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn remove(&mut self, key: &[u8; 32]) -> Result<bool, StoreError> {
        if self.cache.remove(key).is_none() { return Ok(false); }
        db::lock(&self.db).execute(
            "DELETE FROM contacts WHERE key=?1",
            rusqlite::params![key.as_slice()],
        )?;
        Ok(true)
    }
}

// ---- История сообщений -----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMessage {
    pub id: u64,
    pub text: String,
    pub mine: bool,
    pub time: i64,
    pub status: String,
}

/// SQLite-хранилище сообщений без кэша — каждый вызов делает SQL-запрос.
pub struct MessageStore {
    db: Db,
}

impl MessageStore {
    /// Открыть хранилище. Если `legacy` указывает на существующий messages.json,
    /// импортирует оттуда данные и переименовывает файл в .bak.
    pub fn open(db: Db, legacy: Option<&Path>) -> Result<Self, StoreError> {
        let store = MessageStore { db };
        if let Some(path) = legacy {
            if path.exists() {
                store.import_json(path)?;
                let _ = std::fs::rename(path, path.with_extension("json.bak"));
            }
        }
        Ok(store)
    }

    fn import_json(&self, path: &Path) -> Result<(), StoreError> {
        #[derive(Deserialize)]
        struct LegacyFile {
            log: std::collections::BTreeMap<String, Vec<StoredMessage>>,
        }

        let raw = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return Ok(()),
        };
        let data = crate::vault::open(&raw).unwrap_or(raw);
        let lf: LegacyFile = match serde_json::from_slice(&data) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };
        let conn = db::lock(&self.db);
        for (peer_hex, msgs) in &lf.log {
            let peer = match decode_key(peer_hex) { Ok(k) => k, Err(_) => continue };
            for m in msgs {
                conn.execute(
                    "INSERT OR IGNORE INTO messages (peer, id, mine, text, time, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        peer.as_slice(), m.id as i64, m.mine as i64,
                        &m.text, m.time, &m.status,
                    ],
                )?;
            }
        }
        Ok(())
    }

    pub fn append(&self, peer: &[u8; 32], msg: StoredMessage) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT OR REPLACE INTO messages (peer, id, mine, text, time, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                peer.as_slice(), msg.id as i64, msg.mine as i64,
                &msg.text, msg.time, &msg.status,
            ],
        )?;
        Ok(())
    }

    pub fn set_status(&self, peer: &[u8; 32], id: u64, status: &str) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "UPDATE messages SET status=?1 WHERE peer=?2 AND id=?3 AND mine=1",
            rusqlite::params![status, peer.as_slice(), id as i64],
        )?;
        Ok(n > 0)
    }

    /// Есть ли уже сообщение (peer, id, mine) — для дедупа повторной доставки
    /// (офлайн-досылка со стабильным id приходит дважды).
    pub fn contains(&self, peer: &[u8; 32], id: u64, mine: bool) -> bool {
        db::lock(&self.db)
            .query_row(
                "SELECT 1 FROM messages WHERE peer=?1 AND id=?2 AND mine=?3",
                rusqlite::params![peer.as_slice(), id as i64, mine as i64],
                |_| Ok(()),
            )
            .is_ok()
    }

    pub fn history(&self, peer: &[u8; 32]) -> Vec<StoredMessage> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT id, text, mine, time, status
             FROM messages WHERE peer=?1 ORDER BY time, id",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(rusqlite::params![peer.as_slice()], |row| {
            Ok(StoredMessage {
                id: row.get::<_, i64>(0)? as u64,
                text: row.get(1)?,
                mine: row.get::<_, i64>(2)? != 0,
                time: row.get(3)?,
                status: row.get(4)?,
            })
        })
        .and_then(|rows| rows.collect())
        .unwrap_or_default()
    }

    pub fn clear(&self, peer: &[u8; 32]) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM messages WHERE peer=?1",
            rusqlite::params![peer.as_slice()],
        )?;
        Ok(())
    }
}

// ---- Утилиты ---------------------------------------------------------------

fn decode_key(hex_str: &str) -> Result<[u8; 32], StoreError> {
    let bytes = hex::decode(hex_str)?;
    bytes.as_slice().try_into().map_err(|_| StoreError::BadLength)
}

// ---- Тесты -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn db() -> Db { open_in_memory().unwrap() }

    fn key() -> [u8; 32] { Identity::generate().public_key_bytes() }

    #[test]
    fn profile_load_or_create_then_reload_is_stable() {
        let dir = std::env::temp_dir().join(format!("p2p-prof-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profile.json");
        let p1 = Profile::load_or_create(&path).unwrap();
        let key1 = p1.identity.public_key_bytes();
        let p2 = Profile::load_or_create(&path).unwrap();
        assert_eq!(p2.identity.public_key_bytes(), key1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contacts_add_persist() {
        let k1 = key();
        let k2 = key();
        let mut store = ContactStore::open(db(), None).unwrap();
        assert_eq!(store.add(&k1, "Alice", 100).unwrap(), AddOutcome::Added);
        assert_eq!(store.add(&k2, "Bob", 200).unwrap(), AddOutcome::Added);
        assert_eq!(store.add(&k1, "Alice2", 300).unwrap(), AddOutcome::AlreadyKnown);
        assert!(store.contains(&k1));
        assert!(store.contains(&k2));
        assert_eq!(store.list().len(), 2);
        assert_eq!(store.get(&k1).unwrap().name, "Alice");
    }

    #[test]
    fn contacts_reload_from_same_db() {
        let shared = db();
        let k = key();
        {
            let mut s = ContactStore::open(shared.clone(), None).unwrap();
            s.add(&k, "Reload", 0).unwrap();
        }
        let s2 = ContactStore::open(shared, None).unwrap();
        assert!(s2.contains(&k));
        assert_eq!(s2.get(&k).unwrap().name, "Reload");
    }

    #[test]
    fn contacts_rename_and_remove() {
        let k = key();
        let mut store = ContactStore::open(db(), None).unwrap();
        store.add(&k, "Old", 0).unwrap();
        assert!(store.rename(&k, "New").unwrap());
        assert_eq!(store.get(&k).unwrap().name, "New");
        assert!(store.remove(&k).unwrap());
        assert!(!store.contains(&k));
    }

    #[test]
    fn contacts_verified_flag() {
        let k = key();
        let shared = db();
        let mut store = ContactStore::open(shared.clone(), None).unwrap();
        store.add(&k, "Eve", 0).unwrap();
        assert!(!store.get(&k).unwrap().verified);
        assert!(store.set_verified(&k, true).unwrap());
        assert!(store.get(&k).unwrap().verified);

        let s2 = ContactStore::open(shared, None).unwrap();
        assert!(s2.get(&k).unwrap().verified);
    }

    #[test]
    fn contacts_profile_persists() {
        let k = key();
        let shared = db();
        let mut store = ContactStore::open(shared.clone(), None).unwrap();
        store.add(&k, "MyNote", 0).unwrap();
        assert!(store.set_profile(&k, "TheirName", "ab12").unwrap());
        let c = store.get(&k).unwrap();
        assert_eq!(c.name, "MyNote"); // локальное имя не тронуто
        assert_eq!(c.profile_name, "TheirName");
        assert_eq!(c.avatar_id, "ab12");
        // Персист через переоткрытие.
        let s2 = ContactStore::open(shared, None).unwrap();
        assert_eq!(s2.get(&k).unwrap().profile_name, "TheirName");
        assert_eq!(s2.get(&k).unwrap().avatar_id, "ab12");
    }

    #[test]
    fn messages_append_status_history() {
        let peer = key();
        let store = MessageStore::open(db(), None).unwrap();
        store.append(&peer, StoredMessage { id: 1, text: "hi".into(), mine: true,  time: 10, status: "sent".into() }).unwrap();
        store.append(&peer, StoredMessage { id: 5, text: "yo".into(), mine: false, time: 11, status: "".into()     }).unwrap();
        assert!(store.set_status(&peer, 1, "delivered").unwrap());

        let h = store.history(&peer);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].status, "delivered");
        assert_eq!(h[1].text, "yo");
        assert!(!h[1].mine);
    }

    #[test]
    fn messages_clear() {
        let peer = key();
        let store = MessageStore::open(db(), None).unwrap();
        store.append(&peer, StoredMessage { id: 1, text: "x".into(), mine: true, time: 0, status: "".into() }).unwrap();
        store.clear(&peer).unwrap();
        assert!(store.history(&peer).is_empty());
    }
}
