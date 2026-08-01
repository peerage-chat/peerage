//! Персистентность: профиль, контакты и история сообщений.
//!
//! * [`Profile`] — идентичность, хранится в profile.json (секрет, 0600 на Unix).
//! * [`ContactStore`] — TOFU-контакты в SQLite, in-memory кэш для быстрых чтений.
//! * [`MessageStore`] — история сообщений в SQLite, без кэша (прямые запросы).
//!
//! Оба стора принимают [`crate::db::Db`] — общее `Arc<Mutex<Connection>>`.
//! Миграция из JSON-файлов выполняется один раз в `open()` при наличии legacy-файла.

use std::collections::{HashMap, HashSet};
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
    /// Репутация (M3): счётчик «полезных дел» — сколько раз пир потратил на нас свой
    /// трафик, полностью отдав блоб. Повышает его бюджет ресурсов (`budget::Tier`).
    #[serde(default)]
    pub helpful: u32,
    /// hex ТРАНСПОРТНОГО ключа контакта — адреса, по которому он сейчас доступен
    /// (ADR-007). Пусто = эпоха 0, адрес совпадает с ключом личности.
    ///
    /// Личность (`key`) не меняется никогда: на ней fingerprint, верификация и
    /// членство в группах. Адрес — расходник: контакт может сменить его, чтобы
    /// оборвать чужой флуд, и прислать подписанное объявление.
    #[serde(default)]
    pub transport_key: String,
    /// Номер эпохи адреса. Растёт строго — старое объявление не переиграет новое.
    #[serde(default)]
    pub transport_epoch: u32,
}

impl Contact {
    /// Куда слать: транспортный ключ, если контакт объявлял смену адреса, иначе
    /// ключ личности (эпоха 0). Одна точка правды — чтобы «слать по личности»
    /// не размазалось по коду и не пережило первую же ротацию.
    pub fn transport_or_identity(&self) -> String {
        if self.transport_key.is_empty() {
            self.key.clone()
        } else {
            self.transport_key.clone()
        }
    }
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
            "SELECT key, name, fingerprint, added, verified, profile_name, avatar_id, helpful, \
             transport_key, transport_epoch FROM contacts",
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
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })?;
        for row in rows {
            let (kb, name, fp, added, ver, profile_name, avatar_id, helpful, tk, tepoch) = row?;
            if let Ok(key) = <[u8; 32]>::try_from(kb.as_slice()) {
                self.cache.insert(key, Contact {
                    key: hex::encode(key),
                    name,
                    fingerprint: fp,
                    added,
                    verified: ver != 0,
                    profile_name,
                    avatar_id,
                    helpful: helpful.max(0) as u32,
                    transport_key: tk,
                    transport_epoch: tepoch.max(0) as u32,
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
            helpful: 0,
            // Новый контакт живёт в эпохе 0: адрес = ключ личности, пока он сам не
            // объявит смену (ADR-007).
            transport_key: String::new(),
            transport_epoch: 0,
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

    /// Применить объявление контакта о смене адреса (ADR-007).
    ///
    /// Возвращает `true`, только если объявление принято. Три условия, и все
    /// обязательны:
    ///   * контакт известен — смена адреса имеет смысл лишь для того, чью личность
    ///     мы уже знаем; для незнакомца объявление ничем не подкреплено;
    ///   * подпись проверяется ключом ЛИЧНОСТИ контакта — иначе «привет, я новый
    ///     адрес Алисы» станет способом увести переписку;
    ///   * эпоха строго новее известной — иначе перехваченное старое объявление
    ///     можно переиграть и вернуть жертву на брошенный (возможно, флудимый) адрес.
    pub fn apply_transport_rotate(
        &mut self,
        key: &[u8; 32],
        epoch: u32,
        transport_pub: &[u8; 32],
        sig: &[u8; 64],
    ) -> Result<bool, StoreError> {
        let Some(c) = self.cache.get_mut(key) else { return Ok(false) };
        if epoch <= c.transport_epoch {
            return Ok(false);
        }
        if !crate::identity::verify_transport_rotate(key, epoch, transport_pub, sig) {
            return Ok(false);
        }
        c.transport_key = hex::encode(transport_pub);
        c.transport_epoch = epoch;
        db::lock(&self.db).execute(
            "UPDATE contacts SET transport_key=?1, transport_epoch=?2 WHERE key=?3",
            rusqlite::params![&c.transport_key, epoch as i64, key.as_slice()],
        )?;
        Ok(true)
    }

    /// Куда слать этому контакту: его текущий транспортный ключ (адрес). Для эпохи 0
    /// это сам ключ личности. `None` — контакт неизвестен.
    pub fn transport_of(&self, key: &[u8; 32]) -> Option<[u8; 32]> {
        let c = self.cache.get(key)?;
        if c.transport_key.is_empty() {
            return Some(*key);
        }
        let raw = hex::decode(&c.transport_key).ok()?;
        <[u8; 32]>::try_from(raw.as_slice()).ok()
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

    /// Засчитать пиру «полезное дело» (M3, репутация): он потратил на нас свой трафик,
    /// полностью отдав блоб. Растит его бюджет ресурсов (см. `budget::reputation_boost`).
    /// Для не-контакта ничего не делает — заслуги копятся только после гейта (анти-Sybil).
    pub fn bump_helpful(&mut self, key: &[u8; 32]) -> Result<u32, StoreError> {
        let Some(c) = self.cache.get_mut(key) else { return Ok(0) };
        c.helpful = c.helpful.saturating_add(1);
        let n = c.helpful;
        db::lock(&self.db).execute(
            "UPDATE contacts SET helpful=?1 WHERE key=?2",
            rusqlite::params![n as i64, key.as_slice()],
        )?;
        Ok(n)
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

// ---- Запрошенные вложения («скачать») --------------------------------------

/// Намерение скачать вложение: пользователь нажал «скачать», и мы обязаны довести
/// это до конца САМИ — дождаться пира, пережить перезапуск. Следить, когда
/// собеседник появится в сети, — не работа пользователя.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedBlob {
    pub blob_id: String,
    pub peer: [u8; 32],
    pub requested: i64,
}

/// SQLite-список запрошенных вложений.
pub struct WantedStore {
    db: Db,
}

impl WantedStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Запомнить намерение (идемпотентно: повторное «скачать» ничего не портит).
    pub fn add(&self, blob_id: &str, peer: &[u8; 32], now: i64) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT OR REPLACE INTO wanted_blobs (blob_id, peer, requested) VALUES (?1, ?2, ?3)",
            rusqlite::params![blob_id, peer.as_slice(), now],
        )?;
        Ok(())
    }

    /// Снять намерение (блоб пришёл или пользователь передумал).
    pub fn remove(&self, blob_id: &str) -> Result<(), StoreError> {
        db::lock(&self.db)
            .execute("DELETE FROM wanted_blobs WHERE blob_id=?1", rusqlite::params![blob_id])?;
        Ok(())
    }

    /// Что мы ждём от конкретного пира (он появился в сети — пора продолжить).
    pub fn for_peer(&self, peer: &[u8; 32]) -> Vec<WantedBlob> {
        self.query(Some(peer))
    }

    /// Всё запрошенное (на старте приложения).
    pub fn all(&self) -> Vec<WantedBlob> {
        self.query(None)
    }

    fn query(&self, peer: Option<&[u8; 32]>) -> Vec<WantedBlob> {
        let conn = db::lock(&self.db);
        let map = |row: &rusqlite::Row| -> rusqlite::Result<WantedBlob> {
            let p: Vec<u8> = row.get(1)?;
            Ok(WantedBlob {
                blob_id: row.get(0)?,
                peer: <[u8; 32]>::try_from(p.as_slice()).unwrap_or([0u8; 32]),
                requested: row.get(2)?,
            })
        };
        const COLS: &str = "SELECT blob_id, peer, requested FROM wanted_blobs";
        match peer {
            Some(p) => conn
                .prepare(&format!("{COLS} WHERE peer=?1"))
                .and_then(|mut s| {
                    s.query_map(rusqlite::params![p.as_slice()], map)?.collect::<Result<Vec<_>, _>>()
                })
                .unwrap_or_default(),
            None => conn
                .prepare(COLS)
                .and_then(|mut s| s.query_map([], map)?.collect::<Result<Vec<_>, _>>())
                .unwrap_or_default(),
        }
    }
}

// ---- Гейт контактов: входящие запросы + блок-лист (анти-abuse) --------------

/// Входящий «запрос в контакты» от незнакомца. Первое сообщение приходит НЕ в чат,
/// а карточкой на одобрение — замена тихого TOFU-автодобавления. `preview` — текст
/// первого сообщения (или «[image]»/«[file]»/«[voice]» для медиа), чтобы получатель
/// решал осознанно.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactRequest {
    pub key: String,
    pub fingerprint: String,
    #[serde(default)]
    pub preview: String,
    pub time: i64,
}

/// SQLite-хранилище гейта: `contact_requests` (pending) + `blocklist` (тихий дроп).
/// In-memory кэши — приём сообщений проверяет гейт на горячем пути.
pub struct GateStore {
    db: Db,
    requests: HashMap<[u8; 32], ContactRequest>,
    blocked: HashSet<[u8; 32]>,
}

impl GateStore {
    pub fn open(db: Db) -> Result<Self, StoreError> {
        let mut store = GateStore { db, requests: HashMap::new(), blocked: HashSet::new() };
        store.load_cache()?;
        Ok(store)
    }

    fn load_cache(&mut self) -> Result<(), StoreError> {
        let conn = db::lock(&self.db);
        {
            let mut stmt =
                conn.prepare("SELECT key, fingerprint, preview, time FROM contact_requests")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (kb, fp, preview, time) = row?;
                if let Ok(key) = <[u8; 32]>::try_from(kb.as_slice()) {
                    self.requests.insert(key, ContactRequest {
                        key: hex::encode(key), fingerprint: fp, preview, time,
                    });
                }
            }
        }
        {
            let mut stmt = conn.prepare("SELECT key FROM blocklist")?;
            let rows = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
            for row in rows {
                if let Ok(key) = <[u8; 32]>::try_from(row?.as_slice()) {
                    self.blocked.insert(key);
                }
            }
        }
        Ok(())
    }

    // — Запросы —

    /// Записать/обновить входящий запрос. Первый `preview` сохраняем; повторные
    /// сообщения до одобрения НЕ копим (анти-DoS) — только двигаем `time`.
    /// Возвращает `true`, если запрос новый (UI показать уведомление).
    pub fn upsert_request(&mut self, key: &[u8; 32], preview: &str, time: i64) -> Result<bool, StoreError> {
        if let Some(existing) = self.requests.get_mut(key) {
            existing.time = time;
            db::lock(&self.db).execute(
                "UPDATE contact_requests SET time=?1 WHERE key=?2",
                rusqlite::params![time, key.as_slice()],
            )?;
            return Ok(false);
        }
        let fingerprint = crate::identity::fingerprint_words(key).join(" ");
        db::lock(&self.db).execute(
            "INSERT OR IGNORE INTO contact_requests (key, fingerprint, preview, time) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![key.as_slice(), &fingerprint, preview, time],
        )?;
        self.requests.insert(*key, ContactRequest {
            key: hex::encode(key), fingerprint, preview: preview.to_string(), time,
        });
        Ok(true)
    }

    pub fn has_request(&self, key: &[u8; 32]) -> bool {
        self.requests.contains_key(key)
    }

    pub fn list_requests(&self) -> Vec<&ContactRequest> {
        self.requests.values().collect()
    }

    /// Снять запрос из очереди (при одобрении/отклонении/блоке).
    pub fn take_request(&mut self, key: &[u8; 32]) -> Option<ContactRequest> {
        let r = self.requests.remove(key);
        if r.is_some() {
            let _ = db::lock(&self.db).execute(
                "DELETE FROM contact_requests WHERE key=?1",
                rusqlite::params![key.as_slice()],
            );
        }
        r
    }

    // — Блоки —

    pub fn is_blocked(&self, key: &[u8; 32]) -> bool {
        self.blocked.contains(key)
    }

    /// Все заблокированные ключи (hex) — для единого бюджета: заблокированный
    /// режется на ingress ещё до расшифровки (см. `p2p_core::budget::Tier`).
    pub fn list_blocked(&self) -> Vec<String> {
        self.blocked.iter().map(hex::encode).collect()
    }

    /// Заблокировать пира: будущие сообщения тихо дропаются. Снимает висящий запрос.
    pub fn block(&mut self, key: &[u8; 32], time: i64) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT OR REPLACE INTO blocklist (key, blocked) VALUES (?1, ?2)",
            rusqlite::params![key.as_slice(), time],
        )?;
        self.blocked.insert(*key);
        let _ = self.take_request(key);
        Ok(())
    }

    pub fn unblock(&mut self, key: &[u8; 32]) -> Result<bool, StoreError> {
        if !self.blocked.remove(key) { return Ok(false); }
        db::lock(&self.db).execute(
            "DELETE FROM blocklist WHERE key=?1",
            rusqlite::params![key.as_slice()],
        )?;
        Ok(true)
    }
}

// ---- История сообщений -----------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMessage {
    pub id: u64,
    pub text: String,
    pub mine: bool,
    pub time: i64,
    pub status: String,
    /// Тип сообщения (M2): "text" (по умолчанию) | "image" | "file". Пустой = "text".
    #[serde(default)]
    pub kind: String,
    /// hex-хэш блоба вложения (M2); пусто для текста. Байты — через blob_read.
    #[serde(default)]
    pub blob_id: String,
    /// MIME вложения (M2).
    #[serde(default)]
    pub mime: String,
    /// Исходное имя файла вложения (M2; для не-картинок).
    #[serde(default)]
    pub name: String,
    /// Размер вложения в байтах — показывается в карточке ДО скачивания.
    #[serde(default)]
    pub size: u64,
    /// Длительность аудио/видео в мс (0 — не применимо).
    #[serde(default)]
    pub duration_ms: u32,
    /// Размеры кадра (0 — неизвестно).
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// hex-хэш блоба-миниатюры (пусто — нет превью).
    #[serde(default)]
    pub thumb_id: String,
    /// Скачано ли содержимое. Крупные вложения не тянутся автоматически: сначала
    /// приходит карточка (имя/размер/превью), байты — по кнопке.
    #[serde(default)]
    pub downloaded: bool,
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
            "INSERT OR REPLACE INTO messages
                 (peer, id, mine, text, time, status, kind, blob_id, mime, name,
                  size, duration_ms, width, height, thumb_id, downloaded)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params![
                peer.as_slice(), msg.id as i64, msg.mine as i64,
                &msg.text, msg.time, &msg.status,
                &msg.kind, &msg.blob_id, &msg.mime, &msg.name,
                msg.size as i64, msg.duration_ms as i64, msg.width as i64, msg.height as i64,
                &msg.thumb_id, msg.downloaded as i64,
            ],
        )?;
        Ok(())
    }

    /// Пометить вложение скачанным (байты блоба доехали). Ищем по контент-адресу, а
    /// не по (peer,id): один и тот же файл может быть в нескольких чатах — дедуп
    /// контент-адресации означает, что скачан он сразу для всех.
    pub fn mark_downloaded(&self, blob_id: &str) -> Result<usize, StoreError> {
        let n = db::lock(&self.db).execute(
            "UPDATE messages SET downloaded=1 WHERE blob_id=?1",
            rusqlite::params![blob_id],
        )?;
        Ok(n)
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

    /// Удалить одно сообщение (нужно «Сохранённому»: заметку выкидывают точечно).
    pub fn delete_one(&self, peer: &[u8; 32], id: u64, mine: bool) -> Result<(), StoreError> {
        let conn = db::lock(&self.db);
        conn.execute(
            "DELETE FROM messages WHERE peer = ?1 AND id = ?2 AND mine = ?3",
            rusqlite::params![peer.as_slice(), id as i64, mine as i64],
        )?;
        Ok(())
    }

    /// Вложения всех чатов: (пир, blob_id, kind, размер). Нужна экрану «Хранилище»,
    /// чтобы показать, ЧТО занимает место и в каком чате, а не только общую цифру.
    pub fn attachments(&self) -> Vec<([u8; 32], String, String, i64)> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT peer, blob_id, kind, size FROM messages WHERE blob_id <> '' ORDER BY time DESC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |r| {
            let peer: Vec<u8> = r.get(0)?;
            Ok((
                <[u8; 32]>::try_from(peer.as_slice()).unwrap_or([0u8; 32]),
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    pub fn history(&self, peer: &[u8; 32]) -> Vec<StoredMessage> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT id, text, mine, time, status, kind, blob_id, mime, name,
                    size, duration_ms, width, height, thumb_id, downloaded
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
                kind: row.get(5)?,
                blob_id: row.get(6)?,
                mime: row.get(7)?,
                name: row.get(8)?,
                size: row.get::<_, i64>(9)? as u64,
                duration_ms: row.get::<_, i64>(10)? as u32,
                width: row.get::<_, i64>(11)? as u32,
                height: row.get::<_, i64>(12)? as u32,
                thumb_id: row.get(13)?,
                downloaded: row.get::<_, i64>(14)? != 0,
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

/// Запись журнала звонков.
#[derive(Debug, Clone)]
pub struct CallRecord {
    pub id: i64,
    pub peer: [u8; 32],
    /// id комнаты; пусто у личного звонка.
    pub gid: Vec<u8>,
    pub incoming: bool,
    /// answered | missed | declined | unanswered
    pub outcome: String,
    pub started: i64,
    pub duration: i64,
}

/// Журнал звонков. ЛОКАЛЬНЫЙ: никуда не отправляется и ни с кем не синхронизируется.
pub struct CallStore {
    db: Db,
}

impl CallStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Завести запись о начавшемся звонке. Возвращает её id — по нему исход и
    /// длительность дописываются позже, когда звонок закончится.
    pub fn start(&self, peer: &[u8; 32], gid: &[u8], incoming: bool, outcome: &str, now: i64) -> i64 {
        let conn = db::lock(&self.db);
        let ok = conn.execute(
            "INSERT INTO calls (peer, gid, incoming, outcome, started) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![peer.as_slice(), gid, incoming as i64, outcome, now],
        );
        if ok.is_err() {
            return -1;
        }
        conn.last_insert_rowid()
    }

    /// Обновить исход (например «пропущенный» → «отвечен»).
    pub fn set_outcome(&self, id: i64, outcome: &str) {
        let conn = db::lock(&self.db);
        let _ = conn.execute("UPDATE calls SET outcome = ?1 WHERE id = ?2", rusqlite::params![outcome, id]);
    }

    /// Закрыть звонок: длительность в секундах.
    pub fn finish(&self, id: i64, duration: i64) {
        let conn = db::lock(&self.db);
        let _ = conn.execute(
            "UPDATE calls SET duration = ?1 WHERE id = ?2",
            rusqlite::params![duration.max(0), id],
        );
    }

    /// Последние записи, свежие первыми.
    pub fn list(&self, limit: usize) -> Vec<CallRecord> {
        let conn = db::lock(&self.db);
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, peer, gid, incoming, outcome, started, duration              FROM calls ORDER BY started DESC LIMIT ?1",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map([limit as i64], |r| {
            let peer: Vec<u8> = r.get(1)?;
            Ok(CallRecord {
                id: r.get(0)?,
                peer: <[u8; 32]>::try_from(peer.as_slice()).unwrap_or([0u8; 32]),
                gid: r.get(2)?,
                incoming: r.get::<_, i64>(3)? != 0,
                outcome: r.get(4)?,
                started: r.get(5)?,
                duration: r.get(6)?,
            })
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    /// Очистить журнал (явное действие пользователя).
    pub fn clear(&self) {
        let conn = db::lock(&self.db);
        let _ = conn.execute("DELETE FROM calls", []);
    }
}

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
    fn transport_rotate_applies_persists_and_resists_replay() {
        use crate::Identity;
        let alice = Identity::from_seed(&[31u8; 32]).unwrap();
        let k = alice.public_key_bytes();
        let shared = db();
        let mut store = ContactStore::open(shared.clone(), None).unwrap();
        store.add(&k, "Алиса", 0).unwrap();

        // Эпоха 0: адрес совпадает с личностью — старые записи так и живут.
        assert_eq!(store.transport_of(&k), Some(k));
        assert_eq!(store.get(&k).unwrap().transport_epoch, 0);

        // Подписанное объявление принимается, адрес меняется.
        let tk1 = alice.transport_public(1);
        let sig1 = alice.sign_transport_rotate(1, &tk1);
        assert!(store.apply_transport_rotate(&k, 1, &tk1, &sig1).unwrap());
        assert_eq!(store.transport_of(&k), Some(tk1));

        // Повтор той же (и более старой) эпохи отвергается: иначе перехваченное
        // объявление вернуло бы жертву на брошенный адрес.
        assert!(!store.apply_transport_rotate(&k, 1, &tk1, &sig1).unwrap());
        let tk0_sig = alice.sign_transport_rotate(0, &k);
        assert!(!store.apply_transport_rotate(&k, 0, &k, &tk0_sig).unwrap());
        assert_eq!(store.transport_of(&k), Some(tk1), "адрес не откатился");

        // Чужая подпись не проходит, даже с новой эпохой.
        let mallory = Identity::from_seed(&[32u8; 32]).unwrap();
        let fake = mallory.transport_public(2);
        let fake_sig = mallory.sign_transport_rotate(2, &fake);
        assert!(!store.apply_transport_rotate(&k, 2, &fake, &fake_sig).unwrap());
        assert_eq!(store.transport_of(&k), Some(tk1), "захват адреса не прошёл");

        // Персист через переоткрытие: адрес и эпоха переживают перезапуск.
        let s2 = ContactStore::open(shared, None).unwrap();
        assert_eq!(s2.transport_of(&k), Some(tk1));
        assert_eq!(s2.get(&k).unwrap().transport_epoch, 1);
    }

    #[test]
    fn transport_rotate_ignores_unknown_contact() {
        // Для незнакомца объявление ничем не подкреплено: его личность мы не знаем,
        // а значит и «смена адреса» — просто чужой ключ.
        let stranger = crate::Identity::from_seed(&[33u8; 32]).unwrap();
        let k = stranger.public_key_bytes();
        let tk = stranger.transport_public(1);
        let sig = stranger.sign_transport_rotate(1, &tk);
        let mut store = ContactStore::open(db(), None).unwrap();
        assert!(!store.apply_transport_rotate(&k, 1, &tk, &sig).unwrap());
        assert_eq!(store.transport_of(&k), None);
    }

    #[test]
    fn messages_append_status_history() {
        let peer = key();
        let store = MessageStore::open(db(), None).unwrap();
        store.append(&peer, StoredMessage { id: 1, text: "hi".into(), mine: true,  time: 10, status: "sent".into(), ..Default::default() }).unwrap();
        store.append(&peer, StoredMessage { id: 5, text: "yo".into(), mine: false, time: 11, status: "".into(), ..Default::default() }).unwrap();
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
        store.append(&peer, StoredMessage { id: 1, text: "x".into(), mine: true, time: 0, status: "".into(), ..Default::default() }).unwrap();
        store.clear(&peer).unwrap();
        assert!(store.history(&peer).is_empty());
    }

    /// Репутация копится только у контактов и переживает переоткрытие.
    #[test]
    fn helpful_counter_persists_and_ignores_unknown() {
        let k = key();
        let shared = db();
        let mut s = ContactStore::open(shared.clone(), None).unwrap();
        s.add(&k, "Helper", 0).unwrap();
        assert_eq!(s.get(&k).unwrap().helpful, 0);
        assert_eq!(s.bump_helpful(&k).unwrap(), 1);
        assert_eq!(s.bump_helpful(&k).unwrap(), 2);
        // Не-контакт заслуг не копит (анти-Sybil: бонус только после гейта).
        assert_eq!(s.bump_helpful(&key()).unwrap(), 0);

        let s2 = ContactStore::open(shared, None).unwrap();
        assert_eq!(s2.get(&k).unwrap().helpful, 2, "репутация пережила переоткрытие");
    }

    #[test]
    fn gate_request_upsert_keeps_first_preview() {
        let k = key();
        let mut g = GateStore::open(db()).unwrap();
        assert!(g.upsert_request(&k, "привет", 100).unwrap()); // новый
        assert!(!g.upsert_request(&k, "ещё раз", 200).unwrap()); // повтор → не новый
        assert!(g.has_request(&k));
        let r = g.list_requests();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].preview, "привет"); // первый превью сохранён (анти-DoS)
        assert_eq!(r[0].time, 200); // время подвинуто
    }

    #[test]
    fn gate_request_persists_and_take_removes() {
        let k = key();
        let shared = db();
        {
            let mut g = GateStore::open(shared.clone()).unwrap();
            g.upsert_request(&k, "[image]", 1).unwrap();
        }
        let mut g2 = GateStore::open(shared).unwrap();
        assert!(g2.has_request(&k));
        let taken = g2.take_request(&k).unwrap();
        assert_eq!(taken.preview, "[image]");
        assert!(!g2.has_request(&k));
        assert!(g2.take_request(&k).is_none()); // повторный take — пусто
    }

    #[test]
    fn gate_block_drops_request_and_persists() {
        let k = key();
        let shared = db();
        {
            let mut g = GateStore::open(shared.clone()).unwrap();
            g.upsert_request(&k, "спам", 1).unwrap();
            g.block(&k, 2).unwrap();
            assert!(g.is_blocked(&k));
            assert!(!g.has_request(&k)); // блок снял запрос
        }
        let mut g2 = GateStore::open(shared).unwrap();
        assert!(g2.is_blocked(&k)); // блок пережил переоткрытие
        assert!(g2.unblock(&k).unwrap());
        assert!(!g2.is_blocked(&k));
        assert!(!g2.unblock(&k).unwrap()); // повторный unblock — уже нет
    }
}
