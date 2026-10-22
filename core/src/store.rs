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
        crate::fsutil::write_atomic_private(path, &crate::vault::seal(&json))?;
        Ok(())
    }
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

    /// Вставить контакт целиком из резервной копии.
    ///
    /// Существующий НЕ перезаписывается: локальная запись новее той, что в копии, —
    /// восстановление не должно откатывать переименование или (тем более) снимать
    /// верификацию. Возвращает true, если контакт действительно добавлен.
    ///
    /// **Признак верификации НЕ переносится** (решение владельца 2026-08-01).
    /// Верификация — это утверждение «я сверил отпечаток вживую»; восстановленное
    /// из файла утверждение таким не является. Иначе появлялся бы простой обман:
    /// «вот твоя копия, вот пароль» — и ключ постороннего оказывался бы в контактах
    /// уже верифицированным, то есть с высшим тиром бюджета и без предупреждения в
    /// UI. Цена решения — после законного восстановления отпечатки сверяют заново;
    /// это ровно та работа, которую верификация и обозначает.
    ///
    /// Транспортный ключ (адрес, ADR-007) тоже **не** переносится: он мог
    /// протухнуть за время, пока копия лежала, а слать на мёртвый адрес хуже, чем на
    /// ключ личности — с него связь восстановится сама при первом же объявлении.
    pub fn import_contact(&mut self, c: &Contact) -> Result<bool, StoreError> {
        let key = decode_key(&c.key)?;
        if self.cache.contains_key(&key) {
            return Ok(false);
        }
        db::lock(&self.db).execute(
            "INSERT OR IGNORE INTO contacts
                 (key, name, fingerprint, added, verified, profile_name, avatar_id, helpful)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7)",
            rusqlite::params![
                key.as_slice(), &c.name, &c.fingerprint, c.added,
                &c.profile_name, &c.avatar_id, c.helpful as i64,
            ],
        )?;
        self.cache.insert(key, Contact {
            verified: false,
            transport_key: String::new(),
            transport_epoch: 0,
            ..c.clone()
        });
        Ok(true)
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
    /// Первое текстовое сообщение целиком — ляжет в чат при одобрении. Наружу (UI)
    /// не уходит: для решения хватает `preview`.
    #[serde(skip)]
    pub first: Option<FirstMessage>,
}

/// Первое сообщение незнакомца, отложенное до решения по запросу.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstMessage {
    pub id: u64,
    pub text: String,
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
                conn.prepare(
                    "SELECT key, fingerprint, preview, time, first_id, first_text, first_time \
                     FROM contact_requests",
                )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?;
            for row in rows {
                let (kb, fp, preview, time, first_id, first_text, first_time) = row?;
                // Пустой текст — запрос с медиа или из версии до буфера: в чат класть нечего.
                let first = (!first_text.is_empty())
                    .then(|| FirstMessage { id: first_id as u64, text: first_text, time: first_time });
                if let Ok(key) = <[u8; 32]>::try_from(kb.as_slice()) {
                    self.requests.insert(key, ContactRequest {
                        key: hex::encode(key), fingerprint: fp, preview, time, first,
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

    /// Записать/обновить входящий запрос. Первое сообщение сохраняем (превью и, если
    /// это текст, `first` целиком — для чата после одобрения); повторные до одобрения
    /// НЕ копим (анти-DoS) — только двигаем `time`. Так на незнакомца приходится не
    /// больше одного сообщения, а его размер ограничен протоколом.
    /// Возвращает `true`, если запрос новый (UI показать уведомление).
    pub fn upsert_request(
        &mut self,
        key: &[u8; 32],
        preview: &str,
        first: Option<FirstMessage>,
        time: i64,
    ) -> Result<bool, StoreError> {
        if let Some(existing) = self.requests.get_mut(key) {
            existing.time = time;
            db::lock(&self.db).execute(
                "UPDATE contact_requests SET time=?1 WHERE key=?2",
                rusqlite::params![time, key.as_slice()],
            )?;
            return Ok(false);
        }
        let fingerprint = crate::identity::fingerprint_words(key).join(" ");
        let (first_id, first_text, first_time) = match &first {
            Some(m) => (m.id as i64, m.text.as_str(), m.time),
            None => (0, "", 0),
        };
        db::lock(&self.db).execute(
            "INSERT OR IGNORE INTO contact_requests \
             (key, fingerprint, preview, time, first_id, first_text, first_time) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![key.as_slice(), &fingerprint, preview, time, first_id, first_text, first_time],
        )?;
        self.requests.insert(*key, ContactRequest {
            key: hex::encode(key), fingerprint, preview: preview.to_string(), time, first,
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

/// Где живёт сообщение. Личный чат и «Сохранённое» — один и тот же вид (во втором
/// собеседник это ты сам), группа — другой: там авторов много, и строку истории без
/// явного автора не адресовать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chat<'a> {
    Dm(&'a [u8; 32]),
    Group(&'a [u8]),
}

impl<'a> Chat<'a> {
    /// Идентификатор чата в базе: ключ собеседника или gid.
    pub fn id(self) -> &'a [u8] {
        match self {
            Chat::Dm(k) => k.as_slice(),
            Chat::Group(g) => g,
        }
    }

    /// 0 — личный чат, 1 — группа. Хранится отдельно от идентификатора: длина не
    /// признак, и правило «32 байта значит пир» однажды обернётся группой,
    /// притворившейся чатом.
    pub fn scope(self) -> i64 {
        match self {
            Chat::Dm(_) => 0,
            Chat::Group(_) => 1,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMessage {
    pub id: u64,
    /// hex-ключ автора. Ссылка на сообщение — пара «автор + id» (ADR-006), и в
    /// личном чате, и в группе она читается одинаково; `mine` рядом — лишь удобный
    /// для UI ярлык «моё», а не часть ключа.
    #[serde(default)]
    pub sender: String,
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
    /// id сообщения, на которое это — ответ (0 = обычное сообщение). Ссылка, а не
    /// копия цитаты (ADR-006): цитата рисуется из локальной истории, поэтому правка
    /// или удаление оригинала не оставляет расходящегося дубля текста.
    #[serde(default)]
    pub reply_to: u64,
    /// hex-ключ автора цитируемого сообщения (пусто — обычное сообщение).
    #[serde(default)]
    pub reply_sender: String,
    /// Системная строка (смена состава группы) — рисуется не пузырём.
    #[serde(default)]
    pub system: bool,
    /// Пересылка: hex-ключ автора ИСХОДНОГО сообщения (пусто — не переслано).
    /// Хранится ссылкой на человека, а не строкой «переслано от Ани»: имя рисует
    /// клиент по своим контактам, и подделать чужую подпись пересылкой нельзя.
    #[serde(default)]
    pub fwd_from: String,
    /// «Переслано, автора не называем»: источник скрыт пересылающим. Хранится и у
    /// него самого — чтобы то, что он видит, совпадало с тем, что ушло адресату.
    #[serde(default)]
    pub fwd_anon: bool,
    /// Когда сообщение отредактировали (0 — не редактировалось). Показывается
    /// пометкой «изменено»: текст подменился, и молчать об этом нельзя.
    #[serde(default)]
    pub edited_at: i64,
    /// Курсор пагинации: номер строки в ЛОКАЛЬНОЙ базе. Только на чтение — при
    /// записи игнорируется, на другом устройстве он свой.
    #[serde(default)]
    pub row: i64,
}

/// Колонки истории в порядке, который ждёт [`read_msg`]. Один список на все
/// запросы: разъехавшийся порядок здесь — это молча перепутанные поля.
const MSG_COLS: &str = "m.rowid, m.id, m.sender, m.mine, m.text, m.time, m.status, m.kind,
     m.blob_id, m.mime, m.name, m.size, m.duration_ms, m.width, m.height, m.thumb_id,
     m.downloaded, m.reply_to, m.reply_sender, m.system, m.fwd_from, m.fwd_anon,
     m.edited_at";

/// Сколько колонок в [`MSG_COLS`] — с этого места начинаются дописанные к запросу.
/// Держать числом рядом со списком: запрос, который добавит колонку и забудет про
/// сдвиг, прочитает соседнее поле молча.
const MSG_COL_COUNT: usize = 23;

fn read_msg(row: &rusqlite::Row) -> rusqlite::Result<StoredMessage> {
    read_msg_offset(row, 0)
}

/// Строка списка чатов: последнее сообщение и локальные пометки. Непрочитанные и
/// присутствие сюда НЕ входят — они живут не в истории, и клиент подмешивает их сам.
#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub chat: Vec<u8>,
    pub scope: i64,
    pub last: StoredMessage,
    /// 0 — чат не закреплён; иначе время закрепления (позже закрепили — выше).
    pub pinned_at: i64,
    pub muted: bool,
}

/// Вложение из истории: чем занято место и в каком чате.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub chat: Vec<u8>,
    pub scope: i64,
    pub blob_id: String,
    pub kind: String,
    pub size: i64,
}

/// SQLite-хранилище сообщений без кэша — каждый вызов делает SQL-запрос.
/// Личные чаты, «Сохранённое» и группы лежат в ОДНОЙ таблице: разные хранилища
/// означали бы, что каждая фича чата пишется дважды (и однажды — по-разному).
pub struct MessageStore {
    db: Db,
}

impl MessageStore {
    /// Открыть хранилище. `me` нужен, чтобы у старых строк (где автор кодировался
    /// флагом `mine`) появился явный автор. Если `legacy` указывает на существующий
    /// messages.json, импортирует оттуда данные и переименовывает файл в .bak.
    pub fn open(db: Db, legacy: Option<&Path>, me: &[u8; 32]) -> Result<Self, StoreError> {
        let store = MessageStore { db };
        store.migrate_legacy_table(me)?;
        if let Some(path) = legacy {
            if path.exists() {
                store.import_json(path, me)?;
                let _ = std::fs::rename(path, path.with_extension("json.bak"));
            }
        }
        Ok(store)
    }

    /// Перелить историю из таблицы `messages` (личные чаты, автор = флаг `mine`) в
    /// общую `chat_messages` и убрать старую. Разово: после переноса таблицы нет.
    fn migrate_legacy_table(&self, me: &[u8; 32]) -> Result<(), StoreError> {
        let conn = db::lock(&self.db);
        let exists = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='messages'",
                [],
                |_| Ok(()),
            )
            .is_ok();
        if !exists {
            return Ok(());
        }
        let me_hex = hex::encode(me);
        conn.execute(
            "INSERT OR IGNORE INTO chat_messages
                 (chat, scope, id, sender, mine, text, time, status, kind, blob_id, mime, name,
                  size, duration_ms, width, height, thumb_id, downloaded, reply_to, reply_sender, system)
             SELECT peer, 0, id,
                    CASE mine WHEN 1 THEN ?1 ELSE lower(hex(peer)) END,
                    mine, text, time, status, kind, blob_id, mime, name,
                    size, duration_ms, width, height, thumb_id, downloaded, reply_to,
                    CASE WHEN reply_to = 0 THEN ''
                         WHEN reply_mine = 1 THEN ?1
                         ELSE lower(hex(peer)) END,
                    0
             FROM messages",
            rusqlite::params![me_hex],
        )?;
        conn.execute("DROP TABLE messages", [])?;
        Ok(())
    }

    fn import_json(&self, path: &Path, me: &[u8; 32]) -> Result<(), StoreError> {
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
        let me_hex = hex::encode(me);
        let conn = db::lock(&self.db);
        for (peer_hex, msgs) in &lf.log {
            let peer = match decode_key(peer_hex) { Ok(k) => k, Err(_) => continue };
            for m in msgs {
                let sender = if m.mine { &me_hex } else { peer_hex };
                conn.execute(
                    "INSERT OR IGNORE INTO chat_messages
                         (chat, scope, id, sender, mine, text, time, status)
                     VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        peer.as_slice(), m.id as i64, sender, m.mine as i64,
                        &m.text, m.time, &m.status,
                    ],
                )?;
            }
        }
        Ok(())
    }

    /// Записать сообщение. Возвращает номер строки — курсор пагинации, который
    /// клиенту нужен и у сообщения, пришедшего событием.
    pub fn append(&self, chat: Chat, msg: &StoredMessage) -> Result<i64, StoreError> {
        let conn = db::lock(&self.db);
        conn.execute(
            "INSERT OR REPLACE INTO chat_messages
                 (chat, scope, id, sender, mine, text, time, status, kind, blob_id, mime, name,
                  size, duration_ms, width, height, thumb_id, downloaded, reply_to, reply_sender,
                  system, fwd_from, fwd_anon, edited_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            rusqlite::params![
                chat.id(), chat.scope(), msg.id as i64, &msg.sender, msg.mine as i64,
                &msg.text, msg.time, &msg.status,
                &msg.kind, &msg.blob_id, &msg.mime, &msg.name,
                msg.size as i64, msg.duration_ms as i64, msg.width as i64, msg.height as i64,
                &msg.thumb_id, msg.downloaded as i64,
                msg.reply_to as i64, &msg.reply_sender, msg.system as i64, &msg.fwd_from,
                msg.fwd_anon as i64, msg.edited_at,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Пометить вложение скачанным (байты блоба доехали). Ищем по контент-адресу, а
    /// не по ключу строки: один и тот же файл может быть в нескольких чатах — дедуп
    /// контент-адресации означает, что скачан он сразу для всех.
    pub fn mark_downloaded(&self, blob_id: &str) -> Result<usize, StoreError> {
        let n = db::lock(&self.db).execute(
            "UPDATE chat_messages SET downloaded=1 WHERE blob_id=?1",
            rusqlite::params![blob_id],
        )?;
        Ok(n)
    }

    /// Хэши миниатюр всех вложений в истории: их байты нужны карточкам и после
    /// очистки вложений.
    pub fn thumb_ids(&self) -> Vec<String> {
        let conn = db::lock(&self.db);
        let Ok(mut stmt) = conn.prepare("SELECT DISTINCT thumb_id FROM chat_messages WHERE thumb_id != ''") else {
            return Vec::new();
        };
        stmt.query_map([], |r| r.get::<_, String>(0))
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// Снять пометку «скачано» со всех вложений, кроме `keep` (их байты остались).
    /// После очистки вложений карточки должны снова предлагать «Скачать».
    pub fn reset_downloaded(&self, keep: &std::collections::HashSet<String>) -> Result<(), StoreError> {
        let conn = db::lock(&self.db);
        let ids: Vec<String> = {
            let mut stmt = conn.prepare("SELECT DISTINCT blob_id FROM chat_messages WHERE downloaded=1 AND blob_id != ''")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        for id in ids.iter().filter(|id| !keep.contains(*id)) {
            conn.execute("UPDATE chat_messages SET downloaded=0 WHERE blob_id=?1", rusqlite::params![id])?;
        }
        Ok(())
    }

    /// Статус доставки — только у СВОИХ сообщений (у чужих его не бывает).
    pub fn set_status(&self, chat: Chat, id: u64, status: &str) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "UPDATE chat_messages SET status=?1 WHERE chat=?2 AND scope=?3 AND id=?4 AND mine=1",
            rusqlite::params![status, chat.id(), chat.scope(), id as i64],
        )?;
        Ok(n > 0)
    }

    /// Есть ли уже такое сообщение — для дедупа повторной доставки (офлайн-досылка
    /// со стабильным id приходит дважды).
    pub fn contains(&self, chat: Chat, sender: &str, id: u64) -> bool {
        db::lock(&self.db)
            .query_row(
                "SELECT 1 FROM chat_messages WHERE chat=?1 AND scope=?2 AND sender=?3 AND id=?4",
                rusqlite::params![chat.id(), chat.scope(), sender, id as i64],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Удалить одно сообщение (нужно «Сохранённому»: заметку выкидывают точечно).
    pub fn delete_one(&self, chat: Chat, sender: &str, id: u64) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM chat_messages WHERE chat=?1 AND scope=?2 AND sender=?3 AND id=?4",
            rusqlite::params![chat.id(), chat.scope(), sender, id as i64],
        )?;
        Ok(())
    }

    /// Удалить несколько сообщений одним махом: `(id, hex автора)`. Возвращает,
    /// сколько строк удалено.
    ///
    /// Именно пачкой, а не циклом `delete_one`: выделение нескольких — обычный
    /// сценарий, а каждый отдельный `execute` это отдельная транзакция SQLite.
    pub fn delete_many(&self, chat: Chat, refs: &[(u64, String)]) -> Result<usize, StoreError> {
        let mut conn = db::lock(&self.db);
        let tx = conn.transaction()?;
        let mut n = 0;
        for (id, sender) in refs {
            n += tx.execute(
                "DELETE FROM chat_messages WHERE chat=?1 AND scope=?2 AND sender=?3 AND id=?4",
                rusqlite::params![chat.id(), chat.scope(), sender, *id as i64],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Вложения всех чатов. Нужна экрану «Хранилище», чтобы показать, ЧТО занимает
    /// место и в каком чате, а не только общую цифру.
    pub fn attachments(&self) -> Vec<Attachment> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT chat, scope, blob_id, kind, size FROM chat_messages
             WHERE blob_id <> '' ORDER BY time DESC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |r| {
            Ok(Attachment {
                chat: r.get(0)?,
                scope: r.get(1)?,
                blob_id: r.get(2)?,
                kind: r.get(3)?,
                size: r.get(4)?,
            })
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    /// Прочитать сообщения по ссылкам `(id, hex автора)` — в том порядке, в каком
    /// они лежат в чате. Нужно пересылке: она берёт содержимое уже сохранённого.
    pub fn get_many(&self, chat: Chat, refs: &[(u64, String)]) -> Vec<StoredMessage> {
        if refs.is_empty() {
            return Vec::new();
        }
        let conn = db::lock(&self.db);
        let sql = format!(
            "SELECT {MSG_COLS} FROM chat_messages m
             WHERE m.chat=?1 AND m.scope=?2 AND m.sender=?3 AND m.id=?4"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::with_capacity(refs.len());
        for (id, sender) in refs {
            let row = stmt.query_row(
                rusqlite::params![chat.id(), chat.scope(), sender, *id as i64],
                read_msg,
            );
            if let Ok(m) = row {
                out.push(m);
            }
        }
        out.sort_by_key(|m| (m.time, m.row));
        out
    }

    /// Заменить текст сообщения (правка автора). Возвращает true, если строка нашлась.
    ///
    /// Меняется ТОЛЬКО текст: тип, вложение и время остаются прежними — правка это
    /// исправление слов, а не подмена содержимого другим файлом.
    pub fn edit_text(
        &self,
        chat: Chat,
        sender: &str,
        id: u64,
        text: &str,
        at: i64,
    ) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "UPDATE chat_messages SET text=?1, edited_at=?2
             WHERE chat=?3 AND scope=?4 AND sender=?5 AND id=?6",
            rusqlite::params![text, at, chat.id(), chat.scope(), sender, id as i64],
        )?;
        Ok(n > 0)
    }

    /// Закрепить сообщение в чате. Повторный закреп того же — не ошибка (обновляем
    /// время и автора), чтобы «закрепить» дважды не ломалось на гонке двух устройств.
    pub fn pin(&self, chat: Chat, id: u64, sender: &str, at: i64, by: &str) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT INTO pinned (chat, scope, msg_id, sender, pinned_at, by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(chat, scope, msg_id, sender)
             DO UPDATE SET pinned_at = excluded.pinned_at, by = excluded.by",
            rusqlite::params![chat.id(), chat.scope(), id as i64, sender, at, by],
        )?;
        Ok(())
    }

    /// Открепить. Возвращает true, если что-то было закреплено.
    pub fn unpin(&self, chat: Chat, id: u64, sender: &str) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "DELETE FROM pinned WHERE chat=?1 AND scope=?2 AND msg_id=?3 AND sender=?4",
            rusqlite::params![chat.id(), chat.scope(), id as i64, sender],
        )?;
        Ok(n > 0)
    }

    /// Закреплённые сообщения чата вместе с их содержимым, свежие первыми.
    ///
    /// Возвращает именно СООБЩЕНИЯ, а не ссылки: шапка чата показывает текст, и
    /// заставлять клиента искать его в незагруженной истории значит показать пустую
    /// плашку на старом закрепе.
    pub fn pinned(&self, chat: Chat) -> Vec<StoredMessage> {
        let conn = db::lock(&self.db);
        let sql = format!(
            "SELECT {MSG_COLS} FROM chat_messages m
             JOIN pinned p ON p.chat = m.chat AND p.scope = m.scope
                          AND p.msg_id = m.id AND p.sender = m.sender
             WHERE m.chat=?1 AND m.scope=?2
             ORDER BY p.pinned_at DESC"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(rusqlite::params![chat.id(), chat.scope()], read_msg)
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// Убрать все закрепы чата (чат удалён/группа покинута).
    pub fn clear_pins(&self, chat: Chat) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM pinned WHERE chat=?1 AND scope=?2",
            rusqlite::params![chat.id(), chat.scope()],
        )?;
        Ok(())
    }

    /// Поиск по тексту переписки. `chat = None` — по всем чатам; свежие первыми.
    ///
    /// Возвращает пары «где нашлось + сама строка», потому что результат по всем
    /// чатам бесполезен без чата: клиенту нужно и показать, откуда сообщение, и
    /// открыть его.
    ///
    /// Идёт через FTS5, а при его отсутствии — через `LIKE`. Деградация тихая и
    /// намеренная: поиск, который «иногда не собирается», хуже медленного поиска.
    pub fn search(
        &self,
        query: &str,
        chat: Option<Chat>,
        limit: usize,
    ) -> Vec<(Vec<u8>, i64, StoredMessage)> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let conn = db::lock(&self.db);
        let (chat_id, scope) = match chat {
            Some(c) => (c.id().to_vec(), c.scope()),
            None => (Vec::new(), -1),
        };
        // FTS5-запрос: слово целиком в кавычках + префиксный поиск, чтобы «прив»
        // находило «привет». Кавычки внутри удваиваем — иначе ввод ломает синтаксис.
        let fts_query = format!("\"{}\"*", q.replace('"', "\"\""));
        let fts_sql = format!(
            "SELECT m.chat, m.scope, {MSG_COLS} FROM messages_fts f
             JOIN chat_messages m ON m.rowid = f.rowid
             WHERE messages_fts MATCH ?1 AND (?2 < 0 OR (m.chat = ?3 AND m.scope = ?2))
             ORDER BY m.time DESC LIMIT ?4"
        );
        let rows = conn.prepare(&fts_sql).and_then(|mut stmt| {
            stmt.query_map(
                rusqlite::params![fts_query, scope, chat_id, limit as i64],
                |row| {
                    let c: Vec<u8> = row.get(0)?;
                    let s: i64 = row.get(1)?;
                    Ok((c, s, read_msg_offset(row, 2)?))
                },
            )
            .and_then(|it| it.collect::<rusqlite::Result<Vec<_>>>())
        });
        // FTS может отсутствовать в сборке SQLite — тогда молча идём в LIKE.
        if let Ok(found) = rows {
            return found;
        }

        // Фолбэк без FTS: подстрока. `%` и `_` в запросе экранируем, иначе «100%»
        // превратилось бы в «что угодно».
        let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let like_sql = format!(
            "SELECT m.chat, m.scope, {MSG_COLS} FROM chat_messages m
             WHERE m.text LIKE ?1 ESCAPE '\\' AND (?2 < 0 OR (m.chat = ?3 AND m.scope = ?2))
             ORDER BY m.time DESC LIMIT ?4"
        );
        let mut stmt = match conn.prepare(&like_sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let out = stmt
            .query_map(rusqlite::params![like, scope, chat_id, limit as i64], |row| {
                let c: Vec<u8> = row.get(0)?;
                let s: i64 = row.get(1)?;
                Ok((c, s, read_msg_offset(row, 2)?))
            })
            .and_then(|it| it.collect::<rusqlite::Result<Vec<_>>>());
        out.unwrap_or_default()
    }

    /// Запомнить название чата (для групп — имя группы).
    pub fn set_chat_title(&self, chat: Chat, name: &str) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT INTO chat_titles (chat, scope, name) VALUES (?1, ?2, ?3)
             ON CONFLICT(chat, scope) DO UPDATE SET name = excluded.name",
            rusqlite::params![chat.id(), chat.scope(), name],
        )?;
        Ok(())
    }

    /// Все известные названия чатов заданного вида: `(идентификатор, имя)`.
    pub fn chat_titles(&self, scope: i64) -> Vec<(Vec<u8>, String)> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare("SELECT chat, name FROM chat_titles WHERE scope=?1") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(rusqlite::params![scope], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// Забыть название чата (группу удалили у себя).
    pub fn forget_chat_title(&self, chat: Chat) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM chat_titles WHERE chat=?1 AND scope=?2",
            rusqlite::params![chat.id(), chat.scope()],
        )?;
        Ok(())
    }

    /// Чаты заданного вида, где встречается этот блоб. Нужно после докачки: сама
    /// пометка ставится разом по контент-адресу, а перерисовать надо ровно те чаты,
    /// где вложение видно.
    pub fn chats_with_blob(&self, scope: i64, blob_id: &str) -> Vec<Vec<u8>> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT DISTINCT chat FROM chat_messages WHERE scope=?1 AND blob_id=?2",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(rusqlite::params![scope, blob_id], |r| r.get::<_, Vec<u8>>(0))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// Строка списка чатов: последнее сообщение плюс локальные пометки. Собирается
    /// ОДНИМ запросом: список чатов рисуется на каждом событии, и делать по запросу
    /// на чат — значит платить за это тем больше, чем активнее человек переписывается.
    pub fn summaries(&self) -> Vec<ChatSummary> {
        let conn = db::lock(&self.db);
        // Последнее сообщение чата — строка с максимальным курсором `(time, rowid)`,
        // тем же, по которому листается история: иначе «последнее» в списке и
        // «последнее» в чате разъедутся на сообщениях одной секунды.
        //
        // Сначала список чатов (проход по индексу), потом по одному поиску на ЧАТ.
        // Раньше подзапрос шёл на каждую СТРОКУ истории — на сотне тысяч сообщений
        // это сотни миллисекунд на каждое входящее событие (R-039).
        let sql = format!(
            "SELECT {MSG_COLS}, m.chat, m.scope,
                    COALESCE(f.pinned_at, 0), COALESCE(f.muted, 0)
             FROM (SELECT DISTINCT chat, scope FROM chat_messages) c
             JOIN chat_messages m ON m.rowid =
                 (SELECT m2.rowid FROM chat_messages m2
                  WHERE m2.chat = c.chat AND m2.scope = c.scope
                  ORDER BY m2.time DESC, m2.rowid DESC LIMIT 1)
             LEFT JOIN chat_flags f ON f.chat = m.chat AND f.scope = m.scope"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let n = MSG_COL_COUNT;
        stmt.query_map([], |r| {
            Ok(ChatSummary {
                chat: r.get(n)?,
                scope: r.get(n + 1)?,
                pinned_at: r.get(n + 2)?,
                muted: r.get::<_, i64>(n + 3)? != 0,
                last: read_msg(r)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Закрепить чат в списке (`at` — время закрепления) или снять закреп (`at = 0`).
    pub fn set_chat_pinned(&self, chat: Chat, at: i64) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT INTO chat_flags (chat, scope, pinned_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(chat, scope) DO UPDATE SET pinned_at = excluded.pinned_at",
            rusqlite::params![chat.id(), chat.scope(), at],
        )?;
        Ok(())
    }

    /// Выключить/включить звук чата.
    pub fn set_chat_muted(&self, chat: Chat, muted: bool) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT INTO chat_flags (chat, scope, muted) VALUES (?1, ?2, ?3)
             ON CONFLICT(chat, scope) DO UPDATE SET muted = excluded.muted",
            rusqlite::params![chat.id(), chat.scope(), muted as i64],
        )?;
        Ok(())
    }

    /// Пометки всех чатов: `(идентификатор, вид, когда закреплён, без звука)`.
    /// Отдельно от [`summaries`], потому что пометка живёт и у чата без единого
    /// сообщения — закрепить пустой чат можно, и он обязан остаться наверху.
    pub fn chat_flags(&self) -> Vec<(Vec<u8>, i64, i64, bool)> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT chat, scope, pinned_at, muted FROM chat_flags
             WHERE pinned_at != 0 OR muted != 0",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Забыть пометки чата (чат удалён у себя).
    pub fn forget_chat_flags(&self, chat: Chat) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM chat_flags WHERE chat=?1 AND scope=?2",
            rusqlite::params![chat.id(), chat.scope()],
        )?;
        Ok(())
    }

    /// Страница истории чата, старые первыми. `before` — курсор `(time, row)` из
    /// самого верхнего уже показанного сообщения; `None` — самый хвост переписки.
    ///
    /// Страницами, а не «весь чат сразу»: переписка бывает в сотни тысяч строк, и
    /// открытие чата не должно поднимать её в память целиком (и гнать через JNI).
    /// Курсор двойной, потому что время не уникально: сообщения одной секунды при
    /// курсоре только по времени либо потерялись бы, либо пришли дважды.
    pub fn history(&self, chat: Chat, before: Option<(i64, i64)>, limit: usize) -> Vec<StoredMessage> {
        let conn = db::lock(&self.db);
        let sql = format!(
            "SELECT {MSG_COLS} FROM chat_messages m
             WHERE m.chat=?1 AND m.scope=?2 AND (?3 = 0 OR (m.time, m.rowid) < (?4, ?5))
             ORDER BY m.time DESC, m.rowid DESC LIMIT ?6"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let (has_cursor, t, r) = match before {
            Some((t, r)) => (1i64, t, r),
            None => (0i64, 0, 0),
        };
        let mut rows: Vec<StoredMessage> = stmt
            .query_map(
                rusqlite::params![chat.id(), chat.scope(), has_cursor, t, r, limit as i64],
                read_msg,
            )
            .and_then(|rows| rows.collect())
            .unwrap_or_default();
        rows.reverse(); // наружу — как в чате: старые сверху
        rows
    }

    pub fn clear(&self, chat: Chat) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM chat_messages WHERE chat=?1 AND scope=?2",
            rusqlite::params![chat.id(), chat.scope()],
        )?;
        Ok(())
    }

    /// Сколько сообщений всего (для оценки объёма копии до её создания).
    pub fn count(&self) -> u64 {
        db::lock(&self.db)
            .query_row("SELECT COUNT(*) FROM chat_messages", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as u64
    }

    /// Страница истории ВСЕХ чатов для резервной копии: `(rowid, чат, вид, сообщение)`.
    ///
    /// Постранично и по `rowid`, а не «все сразу»: переписка бывает в сотни тысяч
    /// строк, и держать её в памяти целиком на телефоне нельзя. `rowid` растёт
    /// монотонно, поэтому курсор переживает вставки во время обхода.
    pub fn export_page(&self, after: i64, limit: usize) -> Vec<(i64, Vec<u8>, i64, StoredMessage)> {
        let conn = db::lock(&self.db);
        let sql = format!(
            "SELECT m.chat, m.scope, {MSG_COLS} FROM chat_messages m
             WHERE m.rowid > ?1 ORDER BY m.rowid LIMIT ?2"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(rusqlite::params![after, limit as i64], |row| {
            let chat: Vec<u8> = row.get(0)?;
            let scope: i64 = row.get(1)?;
            // Дальше идут MSG_COLS, сдвинутые на две колонки: rowid у них первый.
            let msg = read_msg_offset(row, 2)?;
            Ok((msg.row, chat, scope, msg))
        })
        .and_then(|rows| rows.collect())
        .unwrap_or_default()
    }

    /// Вставить сообщение из копии, НЕ трогая уже существующее (`INSERT OR IGNORE`).
    ///
    /// Восстановление — слияние, а не замена: то, что пришло после снятия копии,
    /// новее и потерять его нельзя. Ключ (чат, вид, автор, id) один и тот же на обоих
    /// концах, поэтому повторный импорт того же файла ничего не задваивает.
    pub fn import_message(&self, chat: Chat, m: &StoredMessage) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "INSERT OR IGNORE INTO chat_messages
                 (chat, scope, id, sender, mine, text, time, status, kind, blob_id, mime, name,
                  size, duration_ms, width, height, thumb_id, downloaded, reply_to, reply_sender,
                  system, fwd_from, fwd_anon, edited_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            rusqlite::params![
                chat.id(), chat.scope(), m.id as i64, &m.sender, m.mine as i64,
                &m.text, m.time, &m.status,
                &m.kind, &m.blob_id, &m.mime, &m.name,
                m.size as i64, m.duration_ms as i64, m.width as i64, m.height as i64,
                &m.thumb_id, m.downloaded as i64,
                m.reply_to as i64, &m.reply_sender, m.system as i64, &m.fwd_from,
                m.fwd_anon as i64, m.edited_at,
            ],
        )?;
        Ok(n > 0)
    }
}

/// То же, что [`read_msg`], но колонки начинаются не с нуля.
fn read_msg_offset(row: &rusqlite::Row, off: usize) -> rusqlite::Result<StoredMessage> {
    Ok(StoredMessage {
        row: row.get(off)?,
        id: row.get::<_, i64>(off + 1)? as u64,
        sender: row.get(off + 2)?,
        mine: row.get::<_, i64>(off + 3)? != 0,
        text: row.get(off + 4)?,
        time: row.get(off + 5)?,
        status: row.get(off + 6)?,
        kind: row.get(off + 7)?,
        blob_id: row.get(off + 8)?,
        mime: row.get(off + 9)?,
        name: row.get(off + 10)?,
        size: row.get::<_, i64>(off + 11)? as u64,
        duration_ms: row.get::<_, i64>(off + 12)? as u32,
        width: row.get::<_, i64>(off + 13)? as u32,
        height: row.get::<_, i64>(off + 14)? as u32,
        thumb_id: row.get(off + 15)?,
        downloaded: row.get::<_, i64>(off + 16)? != 0,
        reply_to: row.get::<_, i64>(off + 17)? as u64,
        reply_sender: row.get(off + 18)?,
        system: row.get::<_, i64>(off + 19)? != 0,
        fwd_from: row.get(off + 20)?,
        fwd_anon: row.get::<_, i64>(off + 21)? != 0,
        edited_at: row.get(off + 22)?,
    })
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

    /// Сколько записей всего (оценка объёма копии).
    pub fn count(&self) -> u64 {
        db::lock(&self.db)
            .query_row("SELECT COUNT(*) FROM calls", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as u64
    }

    /// Вставить запись из резервной копии. Дедуп по (пир, момент начала): `id`
    /// автоинкрементный и на другом устройстве другой, поэтому по нему сверять
    /// нельзя — повторный импорт задвоил бы журнал.
    pub fn import_row(&self, c: &CallRecord) -> bool {
        let conn = db::lock(&self.db);
        let dup: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM calls WHERE peer = ?1 AND started = ?2",
                rusqlite::params![c.peer.as_slice(), c.started],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if dup > 0 {
            return false;
        }
        conn.execute(
            "INSERT INTO calls (peer, gid, incoming, outcome, started, duration)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                c.peer.as_slice(), &c.gid, c.incoming as i64, &c.outcome, c.started, c.duration,
            ],
        )
        .is_ok()
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
    fn summaries_pick_last_message_per_chat() {
        let me = key();
        let store = MessageStore::open(db(), None, &me).unwrap();
        let chats: Vec<[u8; 32]> = (0..30).map(|_| key()).collect();
        let started = std::time::Instant::now();
        for (n, c) in chats.iter().enumerate() {
            for i in 0..2000u64 {
                let m = StoredMessage { id: i, sender: hex::encode(c), text: format!("{n}-{i}"), time: (i / 3) as i64, ..Default::default() };
                store.append(Chat::Dm(c), &m).unwrap();
            }
        }
        eprintln!("заполнение: {:?}", started.elapsed());
        let t = std::time::Instant::now();
        let sums = store.summaries();
        eprintln!("summaries на 60 тыс. строк: {:?}", t.elapsed());
        assert_eq!(sums.len(), chats.len());
        for s in &sums {
            let n = chats.iter().position(|c| c.as_slice() == s.chat.as_slice()).unwrap();
            // Последние три сообщения — в одну «секунду»; побеждает вставленное позже.
            assert_eq!(s.last.text, format!("{n}-1999"));
        }
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

    /// Сообщение в личном чате: автор проставлен явно, статус меняется у своего.
    #[test]
    fn messages_append_status_history() {
        let peer = key();
        let me = key();
        let store = MessageStore::open(db(), None, &me).unwrap();
        let (me_hex, peer_hex) = (hex::encode(me), hex::encode(peer));
        store.append(Chat::Dm(&peer), &StoredMessage { id: 1, sender: me_hex.clone(), text: "hi".into(), mine: true,  time: 10, status: "sent".into(), ..Default::default() }).unwrap();
        store.append(Chat::Dm(&peer), &StoredMessage { id: 5, sender: peer_hex, text: "yo".into(), mine: false, time: 11, status: "".into(), ..Default::default() }).unwrap();
        assert!(store.set_status(Chat::Dm(&peer), 1, "delivered").unwrap());

        let h = store.history(Chat::Dm(&peer), None, 50);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].status, "delivered");
        assert_eq!(h[0].sender, me_hex);
        assert_eq!(h[1].text, "yo");
        assert!(!h[1].mine);
    }

    #[test]
    fn messages_clear() {
        let peer = key();
        let me = key();
        let store = MessageStore::open(db(), None, &me).unwrap();
        store.append(Chat::Dm(&peer), &StoredMessage { id: 1, sender: hex::encode(me), text: "x".into(), mine: true, time: 0, status: "".into(), ..Default::default() }).unwrap();
        store.clear(Chat::Dm(&peer)).unwrap();
        assert!(store.history(Chat::Dm(&peer), None, 50).is_empty());
    }

    /// Группа и личный чат не смешиваются, даже если идентификатор совпал: вид
    /// чата — часть ключа, а не догадка по длине.
    #[test]
    fn group_and_dm_histories_are_separate() {
        let id32 = key();
        let me = key();
        let store = MessageStore::open(db(), None, &me).unwrap();
        let me_hex = hex::encode(me);
        store.append(Chat::Dm(&id32), &StoredMessage { id: 1, sender: me_hex.clone(), text: "личное".into(), mine: true, time: 1, ..Default::default() }).unwrap();
        store.append(Chat::Group(&id32), &StoredMessage { id: 1, sender: me_hex.clone(), text: "групповое".into(), mine: true, time: 1, ..Default::default() }).unwrap();

        let dm = store.history(Chat::Dm(&id32), None, 50);
        let gr = store.history(Chat::Group(&id32), None, 50);
        assert_eq!(dm.len(), 1);
        assert_eq!(gr.len(), 1);
        assert_eq!(dm[0].text, "личное");
        assert_eq!(gr[0].text, "групповое");
    }

    /// Разные авторы с одинаковым id — разные сообщения (в группе id уникален лишь
    /// в пределах отправителя). Раньше ключ был (чат, id, моё) и такое затирало бы.
    #[test]
    fn same_id_from_different_senders_coexists() {
        let gid = vec![7u8; 16];
        let (me, other) = (key(), key());
        let store = MessageStore::open(db(), None, &me).unwrap();
        store.append(Chat::Group(&gid), &StoredMessage { id: 42, sender: hex::encode(me), text: "моё".into(), mine: true, time: 1, ..Default::default() }).unwrap();
        store.append(Chat::Group(&gid), &StoredMessage { id: 42, sender: hex::encode(other), text: "чужое".into(), mine: false, time: 2, ..Default::default() }).unwrap();
        assert_eq!(store.history(Chat::Group(&gid), None, 50).len(), 2);

        // Удаление адресуется парой «автор + id» и уносит ровно одно.
        let n = store.delete_many(Chat::Group(&gid), &[(42, hex::encode(other))]).unwrap();
        assert_eq!(n, 1);
        let left = store.history(Chat::Group(&gid), None, 50);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].text, "моё");
    }

    /// Пагинация: хвост чата и шаг вверх по курсору. Сообщения одной секунды не
    /// теряются и не дублируются — курсор двойной, (время, строка).
    #[test]
    fn history_pages_from_the_tail_upwards() {
        let peer = key();
        let me = key();
        let store = MessageStore::open(db(), None, &me).unwrap();
        let me_hex = hex::encode(me);
        for i in 1..=10u64 {
            store.append(Chat::Dm(&peer), &StoredMessage {
                id: i, sender: me_hex.clone(), text: format!("m{i}"), mine: true,
                time: 100, // ВСЕ в одну секунду — худший случай для курсора
                ..Default::default()
            }).unwrap();
        }
        let tail = store.history(Chat::Dm(&peer), None, 4);
        assert_eq!(tail.len(), 4);
        assert_eq!(tail.last().unwrap().text, "m10", "хвост — самые новые");

        let cur = (tail[0].time, tail[0].row);
        let prev = store.history(Chat::Dm(&peer), Some(cur), 4);
        assert_eq!(prev.len(), 4);
        assert_eq!(prev.last().unwrap().text, "m6", "страница вверх стыкуется без дыр");
        assert!(prev.iter().all(|m| !tail.iter().any(|t| t.id == m.id)), "и без повторов");
    }

    /// Переезд со старой таблицы: у сообщений появляется явный автор, а ответы
    /// («моё/не моё») превращаются в ссылку на конкретного человека. Это данные
    /// пользователя, поэтому проверяется отдельно от остального.
    #[test]
    fn legacy_messages_table_migrates_with_authors() {
        let (me, peer) = (key(), key());
        let shared = db();
        {
            // Схема ровно та, что была до объединения хранилищ.
            let conn = crate::db::lock(&shared);
            conn.execute_batch(
                "CREATE TABLE messages (
                    peer BLOB NOT NULL, id INTEGER NOT NULL, mine INTEGER NOT NULL,
                    text TEXT NOT NULL, time INTEGER NOT NULL, status TEXT NOT NULL DEFAULT '',
                    kind TEXT NOT NULL DEFAULT 'text', blob_id TEXT NOT NULL DEFAULT '',
                    mime TEXT NOT NULL DEFAULT '', name TEXT NOT NULL DEFAULT '',
                    size INTEGER NOT NULL DEFAULT 0, duration_ms INTEGER NOT NULL DEFAULT 0,
                    width INTEGER NOT NULL DEFAULT 0, height INTEGER NOT NULL DEFAULT 0,
                    thumb_id TEXT NOT NULL DEFAULT '', downloaded INTEGER NOT NULL DEFAULT 0,
                    reply_to INTEGER NOT NULL DEFAULT 0, reply_mine INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (peer, id, mine))",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO messages (peer, id, mine, text, time, status) VALUES (?1, 1, 1, 'моё', 10, 'sent')",
                rusqlite::params![peer.as_slice()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO messages (peer, id, mine, text, time, reply_to, reply_mine)
                 VALUES (?1, 2, 0, 'ответ на моё', 11, 1, 1)",
                rusqlite::params![peer.as_slice()],
            )
            .unwrap();
        }

        let store = MessageStore::open(shared.clone(), None, &me).unwrap();
        let h = store.history(Chat::Dm(&peer), None, 50);
        assert_eq!(h.len(), 2, "вся переписка переехала");
        assert_eq!(h[0].sender, hex::encode(me), "своё сообщение — от меня");
        assert_eq!(h[1].sender, hex::encode(peer), "чужое — от собеседника");
        assert_eq!(h[1].reply_to, 1);
        assert_eq!(h[1].reply_sender, hex::encode(me), "«ответ на моё» стал ссылкой на меня");

        // Старой таблицы больше нет — второй прогон не задваивает историю.
        let store2 = MessageStore::open(shared, None, &me).unwrap();
        assert_eq!(store2.history(Chat::Dm(&peer), None, 50).len(), 2);
    }

    /// Поиск находит сообщение в нужном чате, умеет искать по началу слова и не
    /// путает чаты между собой. Пустой запрос — пустой результат (иначе «поиск»
    /// вываливал бы всю переписку).
    #[test]
    fn search_finds_messages_by_prefix_and_respects_the_chat() {
        let (me, peer, other) = (key(), key(), key());
        let store = MessageStore::open(db(), None, &me).unwrap();
        let me_hex = hex::encode(me);
        let put = |chat: Chat, id: u64, text: &str, time: i64| {
            store
                .append(chat, &StoredMessage {
                    id,
                    sender: me_hex.clone(),
                    text: text.into(),
                    mine: true,
                    time,
                    ..Default::default()
                })
                .unwrap();
        };
        put(Chat::Dm(&peer), 1, "встречаемся у входа в семь", 10);
        put(Chat::Dm(&peer), 2, "не забудь ключи", 20);
        put(Chat::Dm(&other), 3, "встречаемся завтра", 30);

        let all = store.search("встреч", None, 50);
        assert_eq!(all.len(), 2, "нашлись оба чата по началу слова");
        assert_eq!(all[0].2.text, "встречаемся завтра", "свежие первыми");

        let only_peer = store.search("встреч", Some(Chat::Dm(&peer)), 50);
        assert_eq!(only_peer.len(), 1);
        assert_eq!(only_peer[0].0, peer.to_vec(), "результат помнит, где нашёлся");

        assert!(store.search("   ", None, 50).is_empty(), "пустой запрос ничего не ищет");

        // Удалённое сообщение из поиска пропадает — индекс не должен переживать
        // строку истории.
        store.delete_many(Chat::Dm(&other), &[(3, me_hex.clone())]).unwrap();
        assert_eq!(store.search("встреч", None, 50).len(), 1);
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
        let first = |text: &str, t| Some(FirstMessage { id: 7, text: text.into(), time: t });
        assert!(g.upsert_request(&k, "привет", first("привет", 100), 100).unwrap()); // новый
        assert!(!g.upsert_request(&k, "ещё раз", first("ещё раз", 200), 200).unwrap()); // повтор
        assert!(g.has_request(&k));
        let r = g.list_requests();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].preview, "привет"); // первый превью сохранён (анти-DoS)
        assert_eq!(r[0].first.as_ref().map(|m| m.text.as_str()), Some("привет"), "и первое сообщение");
        assert_eq!(r[0].time, 200); // время подвинуто
    }

    #[test]
    fn gate_request_persists_and_take_removes() {
        let k = key();
        let shared = db();
        {
            let mut g = GateStore::open(shared.clone()).unwrap();
            g.upsert_request(&k, "[image]", None, 1).unwrap();
        }
        let mut g2 = GateStore::open(shared).unwrap();
        assert!(g2.has_request(&k));
        let taken = g2.take_request(&k).unwrap();
        assert_eq!(taken.preview, "[image]");
        assert_eq!(taken.first, None, "медиа не откладывается текстом");
    }

    #[test]
    fn gate_first_message_survives_reopen() {
        let k = key();
        let shared = db();
        let msg = FirstMessage { id: 1_700_000_000_123, text: "здравствуйте, это Лена".into(), time: 42 };
        {
            let mut g = GateStore::open(shared.clone()).unwrap();
            g.upsert_request(&k, "здравствуйте…", Some(msg.clone()), 42).unwrap();
        }
        let mut g2 = GateStore::open(shared).unwrap();
        assert_eq!(g2.take_request(&k).unwrap().first, Some(msg), "переживает перезапуск до решения");
        assert!(!g2.has_request(&k));
        assert!(g2.take_request(&k).is_none()); // повторный take — пусто
    }

    #[test]
    fn gate_block_drops_request_and_persists() {
        let k = key();
        let shared = db();
        {
            let mut g = GateStore::open(shared.clone()).unwrap();
            g.upsert_request(&k, "спам", None, 1).unwrap();
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
