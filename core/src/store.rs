//! Персистентность: профиль, контакты и история сообщений.
//!
//! * [`Profile`] — идентичность, хранится в profile.json (секрет, 0600 на Unix).
//! * [`ContactStore`] — TOFU-контакты в SQLite, in-memory кэш для быстрых чтений.
//! * [`MessageStore`] — история сообщений в SQLite, без кэша (прямые запросы).
//!
//! Оба стора принимают [`crate::db::Db`] — общее `Arc<Mutex<Connection>>`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::{self, Db};
use crate::identity::Identity;
use crate::notify::{ChatNotify, NotifyMode};

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
    #[error("сообщение с таким автором и id уже есть")]
    Duplicate,
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
        let data = crate::vault::open(&profile_purpose(), &std::fs::read(path)?)?;
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
        crate::fsutil::write_atomic_private(path, &crate::vault::seal(&profile_purpose(), &json))?;
        Ok(())
    }
}

/// AAD файла профиля (см. [`crate::vault::purpose`]).
fn profile_purpose() -> Vec<u8> {
    crate::vault::purpose("profile", &[], 0)
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
    /// Когда сверен (unix-секунды); 0 — не сверен или сверен до того, как дату стали
    /// хранить. Меняется только вместе с `verified` — через [`ContactStore::set_verified`].
    #[serde(default)]
    pub verified_at: i64,
    /// Как сверен; `None` — не сверен (или до того, как способ стали хранить).
    #[serde(default)]
    pub verified_how: Option<VerifyHow>,
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
    /// Мы послали запрос в контакты и ждём принятия (7i): наши сообщения ему лежат в
    /// очереди, кроме самого запроса. Снимается его `ContactAccept` или встречным запросом.
    #[serde(default)]
    pub awaiting: bool,
}

/// Способ сверки ключа — показывается в «Сверен: 1 ноября, по коду».
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerifyHow {
    /// Отсканирован код собеседника, личность в нём совпала с записанной.
    Code,
    /// Набор знаков на пару совпал на двух экранах — отметили вручную.
    Set,
}

impl VerifyHow {
    fn as_str(self) -> &'static str {
        match self {
            VerifyHow::Code => "code",
            VerifyHow::Set => "set",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "code" => Some(VerifyHow::Code),
            "set" => Some(VerifyHow::Set),
            _ => None,
        }
    }
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
    pub fn open(db: Db) -> Result<Self, StoreError> {
        let mut store = ContactStore { db, cache: HashMap::new() };
        store.load_cache()?;
        Ok(store)
    }

    fn load_cache(&mut self) -> Result<(), StoreError> {
        let conn = db::lock(&self.db);
        let mut stmt = conn.prepare(
            "SELECT key, name, fingerprint, added, verified, profile_name, avatar_id, helpful, \
             transport_key, transport_epoch, verified_at, verified_how, awaiting FROM contacts",
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
                row.get::<_, i64>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, i64>(12)?,
            ))
        })?;
        for row in rows {
            let (kb, name, fp, added, ver, profile_name, avatar_id, helpful, tk, tepoch, vat, vhow, awaiting) = row?;
            if let Ok(key) = <[u8; 32]>::try_from(kb.as_slice()) {
                self.cache.insert(key, Contact {
                    key: hex::encode(key),
                    name,
                    fingerprint: fp,
                    added,
                    verified: ver != 0,
                    verified_at: vat,
                    verified_how: VerifyHow::parse(&vhow),
                    profile_name,
                    avatar_id,
                    helpful: helpful.max(0) as u32,
                    transport_key: tk,
                    transport_epoch: tepoch.max(0) as u32,
                    awaiting: awaiting != 0,
                });
            }
        }
        Ok(())
    }

    /// Добавить принятый контакт (принял его запрос, тесты, восстановление).
    pub fn add(&mut self, key: &[u8; 32], name: &str, added: i64) -> Result<AddOutcome, StoreError> {
        self.insert(key, name, added, false)
    }

    /// Добавить по коду с запросом в контакты (7i): до его принятия — `awaiting`.
    pub fn add_awaiting(&mut self, key: &[u8; 32], name: &str, added: i64) -> Result<AddOutcome, StoreError> {
        self.insert(key, name, added, true)
    }

    fn insert(&mut self, key: &[u8; 32], name: &str, added: i64, awaiting: bool) -> Result<AddOutcome, StoreError> {
        if self.cache.contains_key(key) {
            return Ok(AddOutcome::AlreadyKnown);
        }
        let contact = Contact {
            key: hex::encode(key),
            name: name.to_string(),
            fingerprint: crate::identity::fingerprint_words(key).join(" "),
            added,
            verified: false,
            verified_at: 0,
            verified_how: None,
            profile_name: String::new(),
            avatar_id: String::new(),
            helpful: 0,
            // Новый контакт живёт в эпохе 0: адрес = ключ личности, пока он сам не
            // объявит смену (ADR-007).
            transport_key: String::new(),
            transport_epoch: 0,
            awaiting,
        };
        db::lock(&self.db).execute(
            "INSERT OR IGNORE INTO contacts (key, name, fingerprint, added, verified, awaiting)
             VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            rusqlite::params![key.as_slice(), &contact.name, &contact.fingerprint, added, awaiting as i64],
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
                 (key, name, fingerprint, added, verified, profile_name, avatar_id, helpful, awaiting)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                key.as_slice(), &c.name, &c.fingerprint, c.added,
                &c.profile_name, &c.avatar_id, c.helpful as i64, c.awaiting as i64,
            ],
        )?;
        self.cache.insert(key, Contact {
            verified: false,
            verified_at: 0,
            verified_how: None,
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

    // Все изменения ниже — сначала в базу, потом в кэш: не записалось — кэш не
    // трогаем, иначе память и диск разошлись бы до перезапуска (R-043).

    /// Отметить сверенным (`Some` — как и когда) или снять отметку (`None`). Флаг,
    /// дата и способ меняются только вместе: «сверен без даты» заново не появится.
    pub fn set_verified(&mut self, key: &[u8; 32], how: Option<(VerifyHow, i64)>) -> Result<bool, StoreError> {
        let Some(c) = self.cache.get_mut(key) else { return Ok(false) };
        let (at, how_s) = how.map_or((0, ""), |(h, at)| (at, h.as_str()));
        db::lock(&self.db).execute(
            "UPDATE contacts SET verified=?1, verified_at=?2, verified_how=?3 WHERE key=?4",
            rusqlite::params![how.is_some() as i64, at, how_s, key.as_slice()],
        )?;
        c.verified = how.is_some();
        c.verified_at = at;
        c.verified_how = how.map(|(h, _)| h);
        Ok(true)
    }

    /// Пир принял наш запрос (или встречно попросился сам): снять `awaiting`. `true` —
    /// если и правда ждали, то есть очередь пора выпускать.
    pub fn mark_accepted(&mut self, key: &[u8; 32]) -> Result<bool, StoreError> {
        let Some(c) = self.cache.get_mut(key).filter(|c| c.awaiting) else { return Ok(false) };
        db::lock(&self.db).execute("UPDATE contacts SET awaiting=0 WHERE key=?1", rusqlite::params![key.as_slice()])?;
        c.awaiting = false;
        Ok(true)
    }

    pub fn rename(&mut self, key: &[u8; 32], name: &str) -> Result<bool, StoreError> {
        let Some(c) = self.cache.get_mut(key) else { return Ok(false) };
        db::lock(&self.db).execute(
            "UPDATE contacts SET name=?1 WHERE key=?2",
            rusqlite::params![name, key.as_slice()],
        )?;
        c.name = name.to_string();
        Ok(true)
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
    pub fn apply_transport_rotate(&mut self, key: &[u8; 32], record: &[u8], sig: &[u8; 64]) -> Result<bool, StoreError> {
        let Some(c) = self.cache.get_mut(key) else { return Ok(false) };
        let Some(rec) = crate::record::AddressRecord::verify(key, record, sig) else { return Ok(false) };
        // Адрес Yggdrasil — 32 байта ключа; другой сети этот профиль не знает (ADR-015).
        if rec.kind != crate::record::NetKind::Yggdrasil || rec.addr.len() != 32 {
            return Ok(false);
        }
        let epoch = rec.epoch;
        if epoch <= c.transport_epoch {
            return Ok(false);
        }
        let tk = hex::encode(&rec.addr);
        db::lock(&self.db).execute(
            "UPDATE contacts SET transport_key=?1, transport_epoch=?2 WHERE key=?3",
            rusqlite::params![&tk, epoch as i64, key.as_slice()],
        )?;
        c.transport_key = tk;
        c.transport_epoch = epoch;
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
        let Some(c) = self.cache.get_mut(key) else { return Ok(false) };
        db::lock(&self.db).execute(
            "UPDATE contacts SET profile_name=?1, avatar_id=?2 WHERE key=?3",
            rusqlite::params![profile_name, avatar_id, key.as_slice()],
        )?;
        c.profile_name = profile_name.to_string();
        c.avatar_id = avatar_id.to_string();
        Ok(true)
    }

    /// Засчитать пиру «полезное дело» (M3, репутация): он потратил на нас свой трафик,
    /// полностью отдав блоб. Растит его бюджет ресурсов (см. `budget::reputation_boost`).
    /// Для не-контакта ничего не делает — заслуги копятся только после гейта (анти-Sybil).
    pub fn bump_helpful(&mut self, key: &[u8; 32]) -> Result<u32, StoreError> {
        let Some(c) = self.cache.get_mut(key) else { return Ok(0) };
        let n = c.helpful.saturating_add(1);
        db::lock(&self.db).execute(
            "UPDATE contacts SET helpful=?1 WHERE key=?2",
            rusqlite::params![n as i64, key.as_slice()],
        )?;
        c.helpful = n;
        Ok(n)
    }

    pub fn remove(&mut self, key: &[u8; 32]) -> Result<bool, StoreError> {
        if !self.cache.contains_key(key) {
            return Ok(false);
        }
        db::lock(&self.db).execute(
            "DELETE FROM contacts WHERE key=?1",
            rusqlite::params![key.as_slice()],
        )?;
        self.cache.remove(key);
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

/// Группа на диске — всё, кроме MLS-состояния.
#[derive(Debug, Clone)]
pub struct GroupRecord {
    pub gid: Vec<u8>,
    pub name: String,
    /// Мы — админ (создатель).
    pub admin: bool,
    /// Ключ админа; `None` — неизвестен.
    pub admin_key: Option<[u8; 32]>,
    pub rekeyed_at: i64,
    /// Фото группы: hex блоба (пусто — нет).
    pub photo: String,
    /// Режим уведомлений участникам по умолчанию (ставит админ).
    pub notify_default: NotifyMode,
}

/// Входящее приглашение в группу.
#[derive(Debug, Clone)]
pub struct GroupInviteRecord {
    pub gid: Vec<u8>,
    pub name: String,
    pub inviter: [u8; 32],
    pub accepted: bool,
}

/// SQLite-метаданные групп: сами группы, кого мы позвали, кто позвал нас.
/// Слой групп держит их в памяти и пишет сюда каждое изменение.
pub struct GroupStore {
    db: Db,
}

impl GroupStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn groups(&self) -> Vec<GroupRecord> {
        let conn = db::lock(&self.db);
        let Ok(mut stmt) = conn.prepare("SELECT gid, name, admin, admin_key, rekeyed_at, photo, notify_default FROM group_meta") else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok(GroupRecord {
                gid: r.get(0)?,
                name: r.get(1)?,
                admin: r.get::<_, i64>(2)? != 0,
                admin_key: <[u8; 32]>::try_from(r.get::<_, Vec<u8>>(3)?.as_slice()).ok(),
                rekeyed_at: r.get(4)?,
                photo: r.get(5)?,
                notify_default: NotifyMode::from_code(r.get(6)?).unwrap_or_default(),
            })
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    /// Записать группу. Время ротации не трогает — у него свой [`Self::set_rekeyed`].
    pub fn save_group(&self, gid: &[u8], name: &str, admin: bool, admin_key: Option<&[u8; 32]>) -> Result<(), StoreError> {
        let key: &[u8] = admin_key.map(|k| k.as_slice()).unwrap_or(&[]);
        db::lock(&self.db).execute(
            "INSERT INTO group_meta (gid, name, admin, admin_key) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(gid) DO UPDATE SET name=excluded.name, admin=excluded.admin, admin_key=excluded.admin_key",
            rusqlite::params![gid, name, admin as i64, key],
        )?;
        Ok(())
    }

    /// Фото группы (hex блоба; пусто — убрать). Отдельно от [`Self::save_group`]: меняется
    /// редко и только по сообщению админа.
    pub fn set_photo(&self, gid: &[u8], photo: &str) -> Result<(), StoreError> {
        db::lock(&self.db).execute("UPDATE group_meta SET photo=?2 WHERE gid=?1", rusqlite::params![gid, photo])?;
        Ok(())
    }

    /// Режим уведомлений по умолчанию — по сообщению админа или его собственный выбор.
    pub fn set_notify_default(&self, gid: &[u8], mode: NotifyMode) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "UPDATE group_meta SET notify_default=?2 WHERE gid=?1",
            rusqlite::params![gid, mode.code()],
        )?;
        Ok(())
    }

    pub fn set_rekeyed(&self, gid: &[u8], at: i64) -> Result<(), StoreError> {
        db::lock(&self.db)
            .execute("UPDATE group_meta SET rekeyed_at=?2 WHERE gid=?1", rusqlite::params![gid, at])?;
        Ok(())
    }

    /// Забыть группу вместе с приглашениями в неё, которые мы разослали.
    /// Забыть группу у себя: метаданные, разосланные приглашения и звонки её комнаты в
    /// журнале — строка «Группа (N)» без группы вела бы в никуда (app-ui.md, «Звонки»).
    pub fn delete_group(&self, gid: &[u8]) -> Result<(), StoreError> {
        let conn = db::lock(&self.db);
        conn.execute("DELETE FROM group_meta WHERE gid=?1", rusqlite::params![gid])?;
        conn.execute("DELETE FROM group_invites_sent WHERE gid=?1", rusqlite::params![gid])?;
        conn.execute("DELETE FROM calls WHERE gid=?1", rusqlite::params![gid])?;
        Ok(())
    }

    pub fn sent_invites(&self) -> Vec<([u8; 32], Vec<u8>)> {
        let conn = db::lock(&self.db);
        let Ok(mut stmt) = conn.prepare("SELECT peer, gid FROM group_invites_sent") else { return Vec::new() };
        let rows = stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?)));
        rows.map(|it| {
            it.filter_map(|r| r.ok())
                .filter_map(|(p, g)| Some((<[u8; 32]>::try_from(p.as_slice()).ok()?, g)))
                .collect()
        })
        .unwrap_or_default()
    }

    pub fn add_sent_invite(&self, peer: &[u8; 32], gid: &[u8]) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT OR IGNORE INTO group_invites_sent (peer, gid) VALUES (?1, ?2)",
            rusqlite::params![peer.as_slice(), gid],
        )?;
        Ok(())
    }

    pub fn remove_sent_invite(&self, peer: &[u8; 32], gid: &[u8]) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "DELETE FROM group_invites_sent WHERE peer=?1 AND gid=?2",
            rusqlite::params![peer.as_slice(), gid],
        )?;
        Ok(())
    }

    pub fn invites(&self) -> Vec<GroupInviteRecord> {
        let conn = db::lock(&self.db);
        let Ok(mut stmt) = conn.prepare("SELECT gid, name, inviter, accepted FROM group_invites") else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?, r.get::<_, i64>(3)?))
        });
        rows.map(|it| {
            it.filter_map(|r| r.ok())
                .filter_map(|(gid, name, inviter, accepted)| {
                    Some(GroupInviteRecord {
                        gid,
                        name,
                        inviter: <[u8; 32]>::try_from(inviter.as_slice()).ok()?,
                        accepted: accepted != 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
    }

    pub fn put_invite(&self, inv: &GroupInviteRecord) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT OR REPLACE INTO group_invites (gid, name, inviter, accepted) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![inv.gid, inv.name, inv.inviter.as_slice(), inv.accepted as i64],
        )?;
        Ok(())
    }

    pub fn remove_invite(&self, gid: &[u8]) -> Result<(), StoreError> {
        db::lock(&self.db).execute("DELETE FROM group_invites WHERE gid=?1", rusqlite::params![gid])?;
        Ok(())
    }
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

/// Входящий «запрос в контакты» (шаг 7i): незнакомец добавил нас по коду и прислал
/// приветствие. Не чат, а карточка на одобрение — замена тихого TOFU-автодобавления.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactRequest {
    pub key: String,
    pub fingerprint: String,
    /// Самоназвание из запроса — «указано отправителем», ничем не подтверждено.
    pub name: String,
    /// Приветствие целиком (до `MAX_GREETING` байт, может быть пустым); при одобрении
    /// ложится в чат первым сообщением.
    pub greeting: String,
    /// id запроса у отправителя: под ним приветствие встанет в чат, его же вернёт
    /// `ContactAccept`.
    pub request_id: u64,
    /// Когда пришёл — и время приветствия в чате.
    pub time: i64,
}

/// Строка блок-листа для экрана «Заблокированные».
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockedPeer {
    /// hex ключа личности.
    pub key: String,
    /// Как пир был показан в момент блокировки; пусто — незнакомец или контакт без имени.
    pub name: String,
    /// Был контактом в момент блокировки.
    pub contact: bool,
    /// Когда заблокирован (unix-секунды).
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
            let mut stmt = conn
                .prepare("SELECT key, fingerprint, name, greeting, request_id, time FROM contact_requests")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?;
            for row in rows {
                let (kb, fingerprint, name, greeting, request_id, time) = row?;
                if let Ok(key) = <[u8; 32]>::try_from(kb.as_slice()) {
                    self.requests.insert(key, ContactRequest {
                        key: hex::encode(key), fingerprint, name, greeting, request_id: request_id as u64, time,
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

    /// Записать входящий запрос. Повторный от того же пира (переотправил, добавил
    /// заново) НЕ заменяет первый — анти-DoS: на незнакомца приходится одна карточка
    /// ограниченного размера. Возвращает `true`, если запрос новый (показать в UI).
    pub fn upsert_request(
        &mut self,
        key: &[u8; 32],
        name: &str,
        greeting: &str,
        request_id: u64,
        time: i64,
    ) -> Result<bool, StoreError> {
        if self.requests.contains_key(key) {
            return Ok(false);
        }
        let fingerprint = crate::identity::fingerprint_words(key).join(" ");
        db::lock(&self.db).execute(
            "INSERT OR IGNORE INTO contact_requests (key, fingerprint, name, greeting, request_id, time)              VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![key.as_slice(), &fingerprint, name, greeting, request_id as i64, time],
        )?;
        self.requests.insert(*key, ContactRequest {
            key: hex::encode(key),
            fingerprint,
            name: name.to_string(),
            greeting: greeting.to_string(),
            request_id,
            time,
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
    /// режется на ingress ещё до расшифровки (см. `peerage_core::budget::Tier`).
    pub fn list_blocked(&self) -> Vec<String> {
        self.blocked.iter().map(hex::encode).collect()
    }

    /// Заблокированные для экрана «Заблокированные» — новые сверху.
    pub fn blocked_rows(&self) -> Result<Vec<BlockedPeer>, StoreError> {
        let conn = db::lock(&self.db);
        let mut stmt = conn.prepare("SELECT key, name, contact, blocked FROM blocklist ORDER BY blocked DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (kb, name, contact, time) = row?;
            if kb.len() == 32 {
                out.push(BlockedPeer { key: hex::encode(kb), name, contact: contact != 0, time });
            }
        }
        Ok(out)
    }

    /// Заблокировать пира: будущие сообщения тихо дропаются. Снимает висящий запрос.
    /// `who` — как пир был показан, если он был контактом (блок удаляет контакт, а
    /// строку «Заблокированных» надо чем-то подписать); `None` — незнакомец.
    pub fn block(&mut self, key: &[u8; 32], time: i64, who: Option<&str>) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "INSERT OR REPLACE INTO blocklist (key, blocked, name, contact) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![key.as_slice(), time, who.unwrap_or(""), i64::from(who.is_some())],
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
    /// Упоминания поверх участков `text` (только группы). Ссылка на человека, а не
    /// строка: имя рисует клиент по своим контактам (см. [`crate::mention`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<crate::mention::Mention>,
    /// Курсор пагинации: номер строки в ЛОКАЛЬНОЙ базе. Только на чтение — при
    /// записи игнорируется, на другом устройстве он свой.
    #[serde(default)]
    pub row: i64,
    /// Реакции под сообщением. Только на чтение: живут в своей таблице, сюда их
    /// кладёт [`MessageStore::history`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<Reaction>,
}

/// Одна реакция под сообщением и кто её поставил (hex, по порядку постановки).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub who: Vec<String>,
}

/// Строка таблицы реакций как есть — для копии данных. Пустой `emoji` — снятая.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionRow {
    pub chat: Vec<u8>,
    pub scope: i64,
    pub msg_id: u64,
    pub msg_sender: String,
    pub who: String,
    pub emoji: String,
    pub at: i64,
}

/// Сгруппировать строки `(id, автор, кто, эмодзи)` (уже по времени) по сообщениям;
/// эмодзи — в порядке первого появления.
fn group_reactions(
    rows: impl Iterator<Item = (u64, String, String, String)>,
) -> std::collections::HashMap<(u64, String), Vec<Reaction>> {
    let mut out: std::collections::HashMap<(u64, String), Vec<Reaction>> = Default::default();
    for (id, sender, who, emoji) in rows {
        let list = out.entry((id, sender)).or_default();
        match list.iter_mut().find(|r| r.emoji == emoji) {
            Some(r) => r.who.push(who),
            None => list.push(Reaction { emoji, who: vec![who] }),
        }
    }
    out
}

/// Запрос [`MessageStore::search_page`].
#[derive(Default)]
pub struct SearchQuery<'a> {
    /// Слова через пробел; находка содержит каждое.
    pub text: &'a str,
    /// Только этот чат; `None` — все.
    pub chat: Option<Chat<'a>>,
    /// Только сообщения этого отправителя (hex ключа).
    pub sender: Option<&'a str>,
    /// Сколько свежих находок пропустить (листание за пределы страницы).
    pub offset: usize,
    pub limit: usize,
}

/// Страница находок: всего совпадений и сами находки `(чат, scope, сообщение)`,
/// свежие первыми.
#[derive(Default)]
pub struct SearchPage {
    pub total: usize,
    pub hits: Vec<(Vec<u8>, i64, StoredMessage)>,
}

/// Вкладка экрана «Медиа и файлы» (app-ui.md): какие сообщения чата в неё попадают.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaTab {
    /// Фото и видео.
    Media,
    Files,
    Voice,
    /// Сообщения со ссылкой `http(s)://` в тексте или подписи.
    Links,
}

impl MediaTab {
    /// Порядок вкладок на экране — и полей в [`MessageStore::media_counts`].
    pub const ALL: [MediaTab; 4] = [MediaTab::Media, MediaTab::Files, MediaTab::Voice, MediaTab::Links];

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "media" => Some(MediaTab::Media),
            "files" => Some(MediaTab::Files),
            "voice" => Some(MediaTab::Voice),
            "links" => Some(MediaTab::Links),
            _ => None,
        }
    }

    /// Условие на строку `m`: одно на счётчик и на страницу, чтобы «N» в шапке
    /// совпадало с тем, что листается.
    fn filter(self) -> &'static str {
        match self {
            MediaTab::Media => "m.kind IN ('image', 'video')",
            MediaTab::Files => "m.kind = 'file'",
            MediaTab::Voice => "m.kind = 'voice'",
            MediaTab::Links => {
                "m.kind NOT IN ('call', 'video_call', 'deleted')
                 AND (instr(lower(m.text), 'http://') > 0 OR instr(lower(m.text), 'https://') > 0)"
            }
        }
    }
}

/// Страница вкладки «Медиа и файлы»: всего в вкладке и сами сообщения, свежие первыми.
#[derive(Default)]
pub struct MediaPage {
    pub total: usize,
    pub items: Vec<StoredMessage>,
}

/// Слово запроса без окончания: у слов от пяти букв отбрасывается последняя гласная,
/// «й» или «ь», и дальше оно ищется началом слова — «сарай» находит «сарая», «книгу» —
/// «книга». Не морфология, а дешёвый шаг к ней: короткие слова не трогаем, чтобы «дом»
/// не стал «до».
fn search_stem(word: &str) -> &str {
    const ENDINGS: &str = "аеёиоуыэюяйьaeiouy";
    let mut chars = word.char_indices().rev();
    match (chars.next(), word.chars().count()) {
        (Some((at, last)), n) if n >= 5 && ENDINGS.contains(last.to_lowercase().next().unwrap_or(last)) => {
            &word[..at]
        }
        _ => word,
    }
}

/// Счётчик и страница по готовому `FROM … WHERE …` — один путь для FTS и LIKE.
fn search_run(
    conn: &rusqlite::Connection,
    from_where: &str,
    params: &[&dyn rusqlite::ToSql],
    q: &SearchQuery,
) -> rusqlite::Result<SearchPage> {
    let total: i64 = conn.query_row(&format!("SELECT count(*) {from_where}"), params, |r| r.get(0))?;
    let sql = format!(
        "SELECT m.chat, m.scope, {MSG_COLS} {from_where}
         ORDER BY m.time DESC, m.rowid DESC LIMIT {} OFFSET {}",
        q.limit, q.offset
    );
    let hits = conn
        .prepare(&sql)?
        .query_map(params, |row| Ok((row.get(0)?, row.get(1)?, read_msg_offset(row, 2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(SearchPage { total: total as usize, hits })
}

/// В какую сторону от курсора листать историю.
#[derive(Clone, Copy)]
enum Side {
    /// Строго выше курсора; `None` — от хвоста.
    Older,
    /// С курсора включительно и ниже.
    Newer,
}

/// Строки чата по одну сторону курсора `(time, row)`, наружу — старые сверху.
/// Курсор двойной, потому что время не уникально: сообщения одной секунды при
/// курсоре только по времени либо потерялись бы, либо пришли дважды.
fn page_rows(
    conn: &rusqlite::Connection,
    chat: Chat,
    side: Side,
    cursor: Option<(i64, i64)>,
    limit: usize,
) -> Vec<StoredMessage> {
    let (cmp, order) = match side {
        Side::Older => ("<", "DESC"),
        Side::Newer => (">=", "ASC"),
    };
    let sql = format!(
        "SELECT {MSG_COLS} FROM chat_messages m
         WHERE m.chat=?1 AND m.scope=?2 AND (?3 = 0 OR (m.time, m.rowid) {cmp} (?4, ?5))
         ORDER BY m.time {order}, m.rowid {order} LIMIT ?6"
    );
    let (has_cursor, t, r) = match cursor {
        Some((t, r)) => (1i64, t, r),
        None => (0i64, 0, 0),
    };
    let mut rows: Vec<StoredMessage> = conn
        .prepare(&sql)
        .and_then(|mut stmt| {
            stmt.query_map(
                rusqlite::params![chat.id(), chat.scope(), has_cursor, t, r, limit as i64],
                read_msg,
            )?
            .collect()
        })
        .unwrap_or_default();
    if let Side::Older = side {
        rows.reverse();
    }
    rows
}

/// Реакции страницы — одним запросом по чату, а не по запросу на сообщение.
fn with_reactions(conn: &rusqlite::Connection, chat: Chat, mut rows: Vec<StoredMessage>) -> Vec<StoredMessage> {
    let mut by_msg = reactions_query(conn, chat, None);
    for m in &mut rows {
        if let Some(r) = by_msg.remove(&(m.id, m.sender.clone())) {
            m.reactions = r;
        }
    }
    rows
}

/// Колонки истории в порядке, который ждёт [`read_msg`]. Один список на все
/// запросы: разъехавшийся порядок здесь — это молча перепутанные поля.
const MSG_COLS: &str = "m.rowid, m.id, m.sender, m.mine, m.text, m.time, m.status, m.kind,
     m.blob_id, m.mime, m.name, m.size, m.duration_ms, m.width, m.height, m.thumb_id,
     m.downloaded, m.reply_to, m.reply_sender, m.system, m.fwd_from, m.fwd_anon,
     m.edited_at, m.mentions";

/// Сколько колонок в [`MSG_COLS`] — с этого места начинаются дописанные к запросу.
/// Держать числом рядом со списком: запрос, который добавит колонку и забудет про
/// сдвиг, прочитает соседнее поле молча.
const MSG_COL_COUNT: usize = 24;

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
    /// Свой режим уведомлений; `None` — как задано по умолчанию.
    pub notify: Option<ChatNotify>,
}

/// Последняя чужая реакция на моё сообщение в чате — для строки списка чатов.
#[derive(Debug, Clone)]
pub struct LastReaction {
    pub chat: Vec<u8>,
    pub scope: i64,
    /// hex поставившего.
    pub who: String,
    pub emoji: String,
    /// Время реакции, мс.
    pub at: i64,
    /// Текст и вид сообщения, на которое поставили.
    pub quote: String,
    pub kind: String,
}

/// Свой режим из колонок `chat_flags`: −1 (и неизвестное) — своего нет.
fn chat_notify(code: i64, until: i64) -> Option<ChatNotify> {
    NotifyMode::from_code(code).map(|mode| ChatNotify { mode, until })
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
    pub(crate) db: Db,
}

impl MessageStore {
    pub fn open(db: Db) -> MessageStore {
        MessageStore { db }
    }

    /// Записать сообщение. Возвращает номер строки — курсор пагинации, который
    /// клиенту нужен и у сообщения, пришедшего событием.
    ///
    /// Существующую строку (тот же автор и id) НЕ перезаписывает — [`StoreError::Duplicate`].
    /// Иначе участник группы мог бы прислать «новое» сообщение с id своего старого и
    /// тихо подменить его текст и вложение в обход правки (R-041); менять
    /// записанное можно только явными путями — `edit_text`, статусы, пометки.
    pub fn append(&self, chat: Chat, msg: &StoredMessage) -> Result<i64, StoreError> {
        insert_message(&db::lock(&self.db), chat, msg)
    }

    /// Записать только что отправленное или принятое: время — не меньше последнего в чате.
    /// Время ставится по часам телефона, а лента упорядочена по нему; скачок часов назад
    /// (NTP, ручная правка) иначе поставил бы новое выше старого (R-119). Возвращает номер
    /// строки и записанное время — его и показывать.
    pub fn append_live(&self, chat: Chat, msg: &StoredMessage) -> Result<(i64, i64), StoreError> {
        let conn = db::lock(&self.db);
        let time = msg.time.max(last_time(&conn, chat));
        let row = insert_message(&conn, chat, &StoredMessage { time, ..msg.clone() })?;
        Ok((row, time))
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
    /// «Прочитано» — последнее слово: запоздавший Ack (повтор после потерянного
    /// подтверждения) не откатывает его к «доставлено».
    pub fn set_status(&self, chat: Chat, id: u64, status: &str) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "UPDATE chat_messages SET status=?1 WHERE chat=?2 AND scope=?3 AND id=?4 AND mine=1 AND status<>'read'",
            rusqlite::params![status, chat.id(), chat.scope(), id as i64],
        )?;
        Ok(n > 0)
    }

    /// Собеседник прочитал наши сообщения до id `upto` (2.10): «доставлено» →
    /// «прочитано». Только доставленные — ждущее в очереди он ещё не видел, хотя id
    /// у него может быть меньше. Звонки и системные строки не читаются. Возвращает
    /// id, что сменили статус, по возрастанию.
    pub fn mark_read(&self, chat: Chat, upto: u64) -> Result<Vec<u64>, StoreError> {
        const WHERE: &str = "chat=?1 AND scope=?2 AND mine=1 AND id<=?3 AND status='delivered'              AND system=0 AND kind NOT IN ('call', 'video_call')";
        let upto = upto.min(i64::MAX as u64) as i64;
        let conn = db::lock(&self.db);
        let ids = conn
            .prepare(&format!("SELECT id FROM chat_messages WHERE {WHERE} ORDER BY id"))?
            .query_map(rusqlite::params![chat.id(), chat.scope(), upto], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        conn.execute(
            &format!("UPDATE chat_messages SET status='read' WHERE {WHERE}"),
            rusqlite::params![chat.id(), chat.scope(), upto],
        )?;
        Ok(ids.into_iter().map(|i| i as u64).collect())
    }

    /// Самое позднее (по id) входящее сообщение человека — до него и читаем.
    /// Звонки и системные строки не в счёт: их id выдаём мы сами, не собеседник.
    pub fn last_incoming_id(&self, chat: Chat) -> Option<u64> {
        db::lock(&self.db)
            .query_row(
                "SELECT max(id) FROM chat_messages WHERE chat=?1 AND scope=?2 AND mine=0                  AND system=0 AND kind NOT IN ('call', 'video_call')",
                rusqlite::params![chat.id(), chat.scope()],
                |r| r.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten()
            .map(|i| i as u64)
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
        self.delete_many(chat, &[(id, sender.to_string())]).map(|_| ())
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
            let params = rusqlite::params![chat.id(), chat.scope(), sender, *id as i64];
            n += tx.execute("DELETE FROM chat_messages WHERE chat=?1 AND scope=?2 AND sender=?3 AND id=?4", params)?;
            tx.execute("DELETE FROM reactions WHERE chat=?1 AND scope=?2 AND msg_sender=?3 AND msg_id=?4", params)?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Отозвать «у всех»: строка остаётся на месте надгробием («Сообщение удалено»,
    /// `kind = "deleted"`), содержимое стирается — текст, вложение, цитата, пересылка.
    /// Время и автор остаются: по ним строка стоит в ленте. Строка не удаляется ещё и
    /// затем, чтобы досылка того же id не вернула отозванное ([`Self::append`] не
    /// перезаписывает). Закреп снимается. Возвращает, сколько строк стало надгробиями.
    pub fn retract_many(&self, chat: Chat, refs: &[(u64, String)]) -> Result<usize, StoreError> {
        let mut conn = db::lock(&self.db);
        let tx = conn.transaction()?;
        let mut n = 0;
        for (id, sender) in refs {
            let params = rusqlite::params![chat.id(), chat.scope(), sender, *id as i64];
            n += tx.execute(
                "UPDATE chat_messages SET kind='deleted', text='', blob_id='', mime='', name='',
                     size=0, duration_ms=0, width=0, height=0, thumb_id='', downloaded=0,
                     reply_to=0, reply_sender='', fwd_from='', fwd_anon=0, edited_at=0, mentions=''
                 WHERE chat=?1 AND scope=?2 AND sender=?3 AND id=?4 AND kind != 'deleted'",
                params,
            )?;
            tx.execute(
                "DELETE FROM pinned WHERE chat=?1 AND scope=?2 AND sender=?3 AND msg_id=?4",
                params,
            )?;
            tx.execute(
                "DELETE FROM reactions WHERE chat=?1 AND scope=?2 AND msg_sender=?3 AND msg_id=?4",
                params,
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
    /// Меняется ТОЛЬКО текст (и упоминания поверх него — их участки привязаны к
    /// старому тексту и без замены указывали бы не туда): тип, вложение и время
    /// остаются прежними — правка это исправление слов, а не подмена содержимого.
    /// Отозванное не правится: правка, догнавшая отзыв, не должна вернуть текст.
    pub fn edit_text(
        &self,
        chat: Chat,
        sender: &str,
        id: u64,
        text: &str,
        mentions: &[crate::mention::Mention],
        at: i64,
    ) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "UPDATE chat_messages SET text=?1, edited_at=?2, mentions=?7
             WHERE chat=?3 AND scope=?4 AND sender=?5 AND id=?6 AND kind != 'deleted'",
            rusqlite::params![
                text, at, chat.id(), chat.scope(), sender, id as i64, mentions_json(mentions),
            ],
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

    /// Поиск по переписке, свежие первыми (общий поиск: первая страница без фильтров).
    pub fn search(
        &self,
        query: &str,
        chat: Option<Chat>,
        limit: usize,
    ) -> Vec<(Vec<u8>, i64, StoredMessage)> {
        self.search_page(&SearchQuery { text: query, chat, limit, ..Default::default() }).hits
    }

    /// Страница находок и их общее число (app-ui.md → «Поиск в чате»). Каждое слово
    /// запроса ищется отдельно, началом слова — «прив» находит «привет», — в тексте или
    /// имени файла; находка содержит все слова (окончание длинного слова отброшено —
    /// [`search_stem`]). Звонки и системные строки — не текст переписки, их нет.
    pub fn search_page(&self, q: &SearchQuery) -> SearchPage {
        let words: Vec<&str> = q.text.split_whitespace().map(search_stem).collect();
        if words.is_empty() || q.limit == 0 {
            return SearchPage::default();
        }
        let conn = db::lock(&self.db);
        let (chat_id, scope) = match q.chat {
            Some(c) => (c.id().to_vec(), c.scope()),
            None => (Vec::new(), -1),
        };
        let sender = q.sender.unwrap_or("");
        // ?1 — запрос FTS (в LIKE не нужен), ?2-?4 — чат и отправитель, дальше — слова LIKE.
        let common = "(?2 < 0 OR (m.chat = ?3 AND m.scope = ?2)) AND (?4 = '' OR m.sender = ?4)
             AND m.system = 0 AND m.kind NOT IN ('call', 'video_call')";
        // FTS5: слово в кавычках (кавычки внутри удвоены — ввод не ломает синтаксис) и `*`
        // — началом слова; пробел между ними — «и».
        let fts: Vec<String> = words.iter().map(|w| format!("\"{}\"*", w.replace('"', "\"\""))).collect();
        let fts_from = format!(
            "FROM chat_search f JOIN chat_messages m ON m.rowid = f.rowid
             WHERE chat_search MATCH ?1 AND {common}"
        );
        let base: [&dyn rusqlite::ToSql; 4] = [&fts.join(" "), &scope, &chat_id, &sender];
        if let Ok(page) = search_run(&conn, &fts_from, &base, q) {
            return page;
        }
        // Фолбэк без FTS: подстрока каждого слова. `%` и `_` экранируем, иначе «100%»
        // превратилось бы в «что угодно».
        let likes: Vec<String> = words
            .iter()
            .map(|w| format!("%{}%", w.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")))
            .collect();
        let each: Vec<String> = (0..likes.len())
            .map(|n| format!("(m.text LIKE ?{0} ESCAPE '\\' OR m.name LIKE ?{0} ESCAPE '\\')", n + 5))
            .collect();
        let like_from = format!("FROM chat_messages m WHERE {common} AND {}", each.join(" AND "));
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&"", &scope, &chat_id, &sender];
        params.extend(likes.iter().map(|l| l as &dyn rusqlite::ToSql));
        search_run(&conn, &like_from, &params, q).unwrap_or_default()
    }

    /// Дни, в которые в чате есть сообщения, в промежутке `[from, to)` (секунды):
    /// `ГГГГ-ММ-ДД` по местному времени — `offset` секунд от UTC (календарь «Перейти к
    /// дате»; один сдвиг на месяц — переход на летнее время внутри месяца не учтён).
    pub fn message_days(&self, chat: Chat, from: i64, to: i64, offset: i64) -> Vec<String> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT DISTINCT date(m.time + ?5, 'unixepoch') FROM chat_messages m
             WHERE m.chat = ?1 AND m.scope = ?2 AND m.time >= ?3 AND m.time < ?4 ORDER BY 1",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(rusqlite::params![chat.id(), chat.scope(), from, to, offset], |r| r.get(0))
            .and_then(|it| it.collect())
            .unwrap_or_default()
    }

    /// Страница вкладки «Медиа и файлы» (3.2): сообщения чата одного вида, свежие
    /// первыми, листание `offset`/`limit`. Системные строки не входят.
    pub fn media_page(&self, chat: Chat, tab: MediaTab, offset: usize, limit: usize) -> MediaPage {
        let conn = db::lock(&self.db);
        let from_where = format!(
            "FROM chat_messages m WHERE m.chat = ?1 AND m.scope = ?2 AND m.system = 0 AND {}",
            tab.filter()
        );
        let params = rusqlite::params![chat.id(), chat.scope()];
        let run = || -> rusqlite::Result<MediaPage> {
            let total: i64 = conn.query_row(&format!("SELECT count(*) {from_where}"), params, |r| r.get(0))?;
            let sql = format!(
                "SELECT {MSG_COLS} {from_where} ORDER BY m.time DESC, m.rowid DESC LIMIT {limit} OFFSET {offset}"
            );
            let items = conn.prepare(&sql)?.query_map(params, read_msg)?.collect::<rusqlite::Result<_>>()?;
            Ok(MediaPage { total: total as usize, items })
        };
        run().unwrap_or_default()
    }

    /// Сколько сообщений в каждой вкладке «Медиа и файлы» — в порядке [`MediaTab::ALL`]
    /// (шапка экрана, строка профиля, разделы пустого поиска).
    pub fn media_counts(&self, chat: Chat) -> [usize; 4] {
        let sums: Vec<String> =
            MediaTab::ALL.iter().map(|t| format!("coalesce(sum({}), 0)", t.filter())).collect();
        let sql = format!(
            "SELECT {} FROM chat_messages m WHERE m.chat = ?1 AND m.scope = ?2 AND m.system = 0",
            sums.join(", ")
        );
        db::lock(&self.db)
            .query_row(&sql, rusqlite::params![chat.id(), chat.scope()], |r| {
                Ok([0, 1, 2, 3].map(|i| r.get::<_, i64>(i).unwrap_or(0) as usize))
            })
            .unwrap_or_default()
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
                    COALESCE(f.pinned_at, 0), COALESCE(f.notify, -1), COALESCE(f.notify_until, 0)
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
                notify: chat_notify(r.get(n + 3)?, r.get(n + 4)?),
                last: read_msg(r)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// По чату — последняя чужая реакция на моё сообщение ([`LastReaction`]): строка
    /// «Боб: 👍 к «…»» в списке чатов (beta.md 7f). Одним запросом, как [`summaries`].
    pub fn last_reactions(&self, me: &str) -> Vec<LastReaction> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT r.chat, r.scope, r.who, r.emoji, r.at, m.text, m.kind
             FROM (SELECT DISTINCT chat, scope FROM reactions
                   WHERE msg_sender = ?1 AND who != ?1 AND emoji != '') c
             JOIN reactions r ON r.rowid =
                 (SELECT r2.rowid FROM reactions r2
                  WHERE r2.chat = c.chat AND r2.scope = c.scope AND r2.msg_sender = ?1 AND r2.who != ?1
                        AND r2.emoji != ''
                  ORDER BY r2.at DESC, r2.rowid DESC LIMIT 1)
             JOIN chat_messages m ON m.chat = r.chat AND m.scope = r.scope
                 AND m.id = r.msg_id AND m.sender = r.msg_sender",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(rusqlite::params![me], |r| {
            Ok(LastReaction {
                chat: r.get(0)?,
                scope: r.get(1)?,
                who: r.get(2)?,
                emoji: r.get(3)?,
                at: r.get(4)?,
                quote: r.get(5)?,
                kind: r.get(6)?,
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

    /// Свой режим уведомлений чата (`None` — вернуть к умолчанию).
    pub fn set_chat_notify(&self, chat: Chat, notify: Option<ChatNotify>) -> Result<(), StoreError> {
        let (mode, until) = notify.map_or((-1, 0), |n| (n.mode.code(), n.until));
        db::lock(&self.db).execute(
            "INSERT INTO chat_flags (chat, scope, notify, notify_until) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat, scope) DO UPDATE SET notify = excluded.notify,
                 notify_until = excluded.notify_until",
            rusqlite::params![chat.id(), chat.scope(), mode, until],
        )?;
        Ok(())
    }

    /// Пометки всех чатов: `(идентификатор, вид, когда закреплён, свой режим)`.
    /// Отдельно от [`summaries`], потому что пометка живёт и у чата без единого
    /// сообщения — закрепить пустой чат можно, и он обязан остаться наверху.
    pub fn chat_flags(&self) -> Vec<(Vec<u8>, i64, i64, Option<ChatNotify>)> {
        let conn = db::lock(&self.db);
        let mut stmt = match conn.prepare(
            "SELECT chat, scope, pinned_at, notify, notify_until FROM chat_flags
             WHERE pinned_at != 0 OR notify != -1",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, chat_notify(r.get(3)?, r.get(4)?)))
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
        let rows = page_rows(&conn, chat, Side::Older, before, limit);
        with_reactions(&conn, chat, rows)
    }

    /// Страница НИЖЕ показанной: курсор — `(time, row)` самого нижнего сообщения.
    /// Нужна, когда лента открыта не с хвоста (переход к находке или к дате).
    pub fn history_after(&self, chat: Chat, after: (i64, i64), limit: usize) -> Vec<StoredMessage> {
        let conn = db::lock(&self.db);
        // rowid целый: «строго после (t, r)» — то же, что «с (t, r + 1)».
        let rows = page_rows(&conn, chat, Side::Newer, Some((after.0, after.1 + 1)), limit);
        with_reactions(&conn, chat, rows)
    }

    /// Окно истории вокруг места `at = (time, row)`: половина страницы до него и
    /// остальное — с него и ниже. `row = 0` — «первое сообщение не раньше `time`»
    /// (переход к дате), `(0, 0)` — начало переписки.
    pub fn history_around(&self, chat: Chat, at: (i64, i64), limit: usize) -> Vec<StoredMessage> {
        let conn = db::lock(&self.db);
        // У начала переписки «до» короче половины — остаток добирается снизу.
        let mut rows = page_rows(&conn, chat, Side::Older, Some(at), limit / 2);
        let rest = limit - rows.len();
        rows.extend(page_rows(&conn, chat, Side::Newer, Some(at), rest));
        with_reactions(&conn, chat, rows)
    }

    /// Поставить, заменить или снять (`emoji` пустой) реакцию `who` на сообщение.
    /// Из двух изменений одного человека побеждает более позднее по `at` — опоздавший
    /// пакет старое не возвращает. true — набор реакций сообщения изменился.
    pub fn react(
        &self,
        chat: Chat,
        msg_id: u64,
        msg_sender: &str,
        who: &str,
        emoji: &str,
        at: i64,
    ) -> Result<bool, StoreError> {
        let n = db::lock(&self.db).execute(
            "INSERT INTO reactions (chat, scope, msg_id, msg_sender, who, emoji, at, seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
             ON CONFLICT(chat, scope, msg_id, msg_sender, who)
             DO UPDATE SET emoji = excluded.emoji, at = excluded.at, seen = 0
             WHERE excluded.at > reactions.at AND excluded.emoji != reactions.emoji",
            rusqlite::params![chat.id(), chat.scope(), msg_id as i64, msg_sender, who, emoji, at],
        )?;
        Ok(n > 0)
    }

    /// Сообщения `me` с непросмотренными чужими реакциями — `(id, автор, время, строка)` в
    /// порядке ленты (старые первыми): «❤» на капсуле ведёт к ним по очереди, а время и
    /// строка открывают окно истории, если сообщения нет в загруженной ленте.
    pub fn unseen_reactions(&self, chat: Chat, me: &str) -> Vec<(u64, String, i64, i64)> {
        let conn = db::lock(&self.db);
        let Ok(mut stmt) = conn.prepare(
            "SELECT r.msg_id, r.msg_sender, MIN(m.time), MIN(m.rowid) FROM reactions r
             JOIN chat_messages m ON m.chat = r.chat AND m.scope = r.scope
                  AND m.id = r.msg_id AND m.sender = r.msg_sender
             WHERE r.chat = ?1 AND r.scope = ?2 AND r.seen = 0 AND r.emoji != ''
                   AND r.msg_sender = ?3 AND r.who != ?3
             GROUP BY r.msg_id, r.msg_sender
             ORDER BY MIN(m.time), MIN(m.rowid)",
        ) else {
            return Vec::new();
        };
        stmt.query_map(rusqlite::params![chat.id(), chat.scope(), me], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .and_then(|rows| rows.collect())
        .unwrap_or_default()
    }

    /// Реакции сообщения просмотрены (оно попало на экран или к нему перешли).
    pub fn mark_reactions_seen(&self, chat: Chat, msg_id: u64, msg_sender: &str) -> Result<(), StoreError> {
        db::lock(&self.db).execute(
            "UPDATE reactions SET seen = 1 WHERE chat=?1 AND scope=?2 AND msg_id=?3 AND msg_sender=?4 AND seen = 0",
            rusqlite::params![chat.id(), chat.scope(), msg_id as i64, msg_sender],
        )?;
        Ok(())
    }

    /// Всё просмотрено — после восстановления из копии: старые реакции не «новые».
    pub fn mark_all_reactions_seen(&self) -> Result<(), StoreError> {
        db::lock(&self.db).execute("UPDATE reactions SET seen = 1 WHERE seen = 0", [])?;
        Ok(())
    }

    /// Реакции одного сообщения — новый набор для события после изменения.
    pub fn reactions(&self, chat: Chat, msg_id: u64, msg_sender: &str) -> Vec<Reaction> {
        let conn = db::lock(&self.db);
        reactions_query(&conn, chat, Some((msg_id, msg_sender)))
            .remove(&(msg_id, msg_sender.to_string()))
            .unwrap_or_default()
    }

    pub fn clear(&self, chat: Chat) -> Result<(), StoreError> {
        let conn = db::lock(&self.db);
        let params = rusqlite::params![chat.id(), chat.scope()];
        conn.execute("DELETE FROM chat_messages WHERE chat=?1 AND scope=?2", params)?;
        conn.execute("DELETE FROM reactions WHERE chat=?1 AND scope=?2", params)?;
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

    /// Все строки реакций для копии данных — вместе с надгробиями (снятыми): без них
    /// восстановление на устройство, где реакция ещё стоит, вернуло бы снятое.
    pub fn export_reactions(&self) -> Vec<ReactionRow> {
        let conn = db::lock(&self.db);
        let Ok(mut stmt) = conn.prepare(
            "SELECT chat, scope, msg_id, msg_sender, who, emoji, at FROM reactions ORDER BY rowid",
        ) else {
            return Vec::new();
        };
        stmt.query_map([], |r| {
            Ok(ReactionRow {
                chat: r.get(0)?,
                scope: r.get(1)?,
                msg_id: r.get::<_, i64>(2)? as u64,
                msg_sender: r.get(3)?,
                who: r.get(4)?,
                emoji: r.get(5)?,
                at: r.get(6)?,
            })
        })
        .and_then(|rows| rows.collect())
        .unwrap_or_default()
    }

    /// Строка звонка в ленте (журнал движка, 2.8б): вставить или обновить исход и
    /// длительность — звонок меняется по ходу, а строка в чате одна. Возвращает её rowid.
    pub fn upsert_call(&self, chat: Chat, m: &StoredMessage) -> Result<i64, StoreError> {
        let conn = db::lock(&self.db);
        // Новая строка звонка — тоже живая (R-119); обновление время не трогает.
        let time = m.time.max(last_time(&conn, chat));
        conn.execute(
            "INSERT INTO chat_messages (chat, scope, id, sender, mine, text, time, kind, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(chat, scope, sender, id)
             DO UPDATE SET text = excluded.text, duration_ms = excluded.duration_ms",
            rusqlite::params![
                chat.id(), chat.scope(), m.id as i64, &m.sender, m.mine as i64, &m.text, time,
                &m.kind, m.duration_ms,
            ],
        )?;
        Ok(conn.query_row(
            "SELECT rowid FROM chat_messages WHERE chat=?1 AND scope=?2 AND sender=?3 AND id=?4",
            rusqlite::params![chat.id(), chat.scope(), &m.sender, m.id as i64],
            |r| r.get(0),
        )?)
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
                  system, fwd_from, fwd_anon, edited_at, mentions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            rusqlite::params![
                chat.id(), chat.scope(), m.id as i64, &m.sender, m.mine as i64,
                &m.text, m.time, &m.status,
                &m.kind, &m.blob_id, &m.mime, &m.name,
                m.size as i64, m.duration_ms as i64, m.width as i64, m.height as i64,
                &m.thumb_id, m.downloaded as i64,
                m.reply_to as i64, &m.reply_sender, m.system as i64, &m.fwd_from,
                m.fwd_anon as i64, m.edited_at, mentions_json(&m.mentions),
            ],
        )?;
        Ok(n > 0)
    }
}

/// Время последней строки чата (0 — пусто): ниже него живое не пишется (R-119).
fn last_time(conn: &rusqlite::Connection, chat: Chat) -> i64 {
    conn.query_row(
        "SELECT COALESCE(MAX(time), 0) FROM chat_messages WHERE chat = ?1 AND scope = ?2",
        rusqlite::params![chat.id(), chat.scope()],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// Вставка строки ленты; существующую (тот же автор и id) не трогает — см. [`MessageStore::append`].
fn insert_message(conn: &rusqlite::Connection, chat: Chat, msg: &StoredMessage) -> Result<i64, StoreError> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO chat_messages
             (chat, scope, id, sender, mine, text, time, status, kind, blob_id, mime, name,
              size, duration_ms, width, height, thumb_id, downloaded, reply_to, reply_sender,
              system, fwd_from, fwd_anon, edited_at, mentions)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
        rusqlite::params![
            chat.id(), chat.scope(), msg.id as i64, &msg.sender, msg.mine as i64,
            &msg.text, msg.time, &msg.status,
            &msg.kind, &msg.blob_id, &msg.mime, &msg.name,
            msg.size as i64, msg.duration_ms as i64, msg.width as i64, msg.height as i64,
            &msg.thumb_id, msg.downloaded as i64,
            msg.reply_to as i64, &msg.reply_sender, msg.system as i64, &msg.fwd_from,
            msg.fwd_anon as i64, msg.edited_at, mentions_json(&msg.mentions),
        ],
    )?;
    if n == 0 {
        return Err(StoreError::Duplicate);
    }
    Ok(conn.last_insert_rowid())
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
        mentions: serde_json::from_str(&row.get::<_, String>(off + 23)?).unwrap_or_default(),
        reactions: Vec::new(),
    })
}

/// Поставленные реакции чата (или одного сообщения), сгруппированные по сообщениям.
fn reactions_query(
    conn: &rusqlite::Connection,
    chat: Chat,
    one: Option<(u64, &str)>,
) -> std::collections::HashMap<(u64, String), Vec<Reaction>> {
    let (only, id, sender) = match one {
        Some((id, s)) => (1i64, id as i64, s),
        None => (0, 0, ""),
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT msg_id, msg_sender, who, emoji FROM reactions
         WHERE chat=?1 AND scope=?2 AND emoji != '' AND (?3 = 0 OR (msg_id = ?4 AND msg_sender = ?5))
         ORDER BY at, rowid",
    ) else {
        return Default::default();
    };
    let rows = stmt.query_map(rusqlite::params![chat.id(), chat.scope(), only, id, sender], |r| {
        Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?, r.get(3)?))
    });
    match rows {
        Ok(it) => group_reactions(it.filter_map(|r| r.ok())),
        Err(_) => Default::default(),
    }
}

/// Упоминания в колонку: пустой список — пустая строка, а не `[]`, чтобы у обычных
/// сообщений колонка не занимала места.
fn mentions_json(m: &[crate::mention::Mention]) -> String {
    if m.is_empty() {
        String::new()
    } else {
        serde_json::to_string(m).unwrap_or_default()
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
    /// Видеозвонок (по приглашению: звали с видео).
    pub video: bool,
}

const CALL_COLS: &str = "id, peer, gid, incoming, outcome, started, duration, video";

fn read_call(r: &rusqlite::Row) -> rusqlite::Result<CallRecord> {
    let peer: Vec<u8> = r.get(1)?;
    Ok(CallRecord {
        id: r.get(0)?,
        peer: <[u8; 32]>::try_from(peer.as_slice()).unwrap_or([0u8; 32]),
        gid: r.get(2)?,
        incoming: r.get::<_, i64>(3)? != 0,
        outcome: r.get(4)?,
        started: r.get(5)?,
        duration: r.get(6)?,
        video: r.get::<_, i64>(7)? != 0,
    })
}

/// Строка журнала: звонок или серия звонков одной комнаты подряд.
#[derive(Debug, Clone)]
pub struct CallRun {
    /// Последний звонок серии (его время и исход показывает строка).
    pub last: CallRecord,
    /// Сколько звонков в серии; 1 — одиночный.
    pub count: u32,
    /// Суммарная длительность серии, секунды.
    pub duration: i64,
}

/// Свернуть подряд идущие звонки одной комнаты в строку «Группа (N)» (app-ui.md,
/// «Звонки»). `rows` — свежие первыми. Пропущенный в серию с отвеченными не входит:
/// вкладка «Пропущенные» должна его видеть. Личные звонки не сворачиваются —
/// там каждый пропущенный важен сам по себе.
pub fn collapse_rooms(rows: Vec<CallRecord>) -> Vec<CallRun> {
    let mut out: Vec<CallRun> = Vec::new();
    for c in rows {
        if let Some(run) = out.last_mut() {
            let same = !c.gid.is_empty()
                && run.last.gid == c.gid
                && (run.last.outcome == "missed") == (c.outcome == "missed");
            if same {
                run.count += 1;
                run.duration += c.duration;
                continue;
            }
        }
        out.push(CallRun { duration: c.duration, last: c, count: 1 });
    }
    out
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
    pub fn start(&self, peer: &[u8; 32], gid: &[u8], incoming: bool, video: bool, outcome: &str, now: i64) -> i64 {
        let conn = db::lock(&self.db);
        let ok = conn.execute(
            "INSERT INTO calls (peer, gid, incoming, outcome, started, video) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![peer.as_slice(), gid, incoming as i64, outcome, now, video as i64],
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
        let Ok(mut stmt) = conn.prepare(&format!("SELECT {CALL_COLS} FROM calls ORDER BY started DESC LIMIT ?1"))
        else {
            return Vec::new();
        };
        let rows = stmt.query_map([limit as i64], read_call);
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    /// Одна запись по id — после её изменения, чтобы обновить строку в ленте.
    pub fn get(&self, id: i64) -> Option<CallRecord> {
        let conn = db::lock(&self.db);
        conn.query_row(&format!("SELECT {CALL_COLS} FROM calls WHERE id = ?1"), [id], read_call)
            .ok()
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
            "INSERT INTO calls (peer, gid, incoming, outcome, started, duration, video)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                c.peer.as_slice(), &c.gid, c.incoming as i64, &c.outcome, c.started, c.duration,
                c.video as i64,
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
        let dir = std::env::temp_dir().join(format!("test-prof-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profile.json");
        let p1 = Profile::load_or_create(&path).unwrap();
        let key1 = p1.identity.public_key_bytes();
        let p2 = Profile::load_or_create(&path).unwrap();
        assert_eq!(p2.identity.public_key_bytes(), key1);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-119: часы скакнули назад — живое сообщение не встаёт выше прежних: его время не
    /// меньше последнего в чате. Копия ([`MessageStore::import_message`]) свои времена хранит.
    #[test]
    fn live_append_never_goes_back_in_time() {
        let store = MessageStore::open(db());
        let c = key();
        let chat = Chat::Dm(&c);
        store.append_live(chat, &StoredMessage { id: 1, text: "раньше".into(), time: 1000, ..Default::default() }).unwrap();
        let (_, t) = store
            .append_live(chat, &StoredMessage { id: 2, text: "позже".into(), time: 970, ..Default::default() })
            .unwrap();
        assert_eq!(t, 1000, "время не меньше последнего");
        let (_, t) = store
            .append_live(chat, &StoredMessage { id: 3, text: "ещё".into(), time: 1005, ..Default::default() })
            .unwrap();
        assert_eq!(t, 1005, "часы впереди — своё время");
        // Строка звонка — так же.
        store
            .upsert_call(chat, &StoredMessage { id: 4, text: "звонок".into(), time: 990, kind: "call".into(), ..Default::default() })
            .unwrap();
        let texts: Vec<String> = store.history(chat, None, 10).into_iter().map(|m| m.text).collect();
        assert_eq!(texts, ["раньше", "позже", "ещё", "звонок"]);
        // Другой чат своим временем не тянет.
        let d = key();
        let (_, t) = store.append_live(Chat::Dm(&d), &StoredMessage { id: 1, time: 5, ..Default::default() }).unwrap();
        assert_eq!(t, 5);
    }

    #[test]
    fn chat_notify_is_stored_and_reset() {
        use crate::notify::{ChatNotify, NotifyMode};
        let store = MessageStore::open(db());
        let (c, empty) = (key(), key());
        store.append(Chat::Dm(&c), &StoredMessage { id: 1, text: "x".into(), ..Default::default() }).unwrap();
        let hour = ChatNotify { mode: NotifyMode::Nothing, until: 3600 };
        store.set_chat_notify(Chat::Dm(&c), Some(hour)).unwrap();
        store.set_chat_notify(Chat::Group(&empty), Some(ChatNotify { mode: NotifyMode::Mentions, until: 0 })).unwrap();
        assert_eq!(store.summaries()[0].notify, Some(hour));
        // Пустой чат с режимом — в пометках (строка в списке без переписки).
        let flags = store.chat_flags();
        assert!(flags.iter().any(|f| f.0 == empty.to_vec() && f.1 == 1 && f.3.map(|n| n.mode) == Some(NotifyMode::Mentions)));
        store.set_chat_notify(Chat::Dm(&c), None).unwrap();
        assert_eq!(store.summaries()[0].notify, None, "вернули к умолчанию");
        assert!(!store.chat_flags().iter().any(|f| f.0 == c.to_vec()));
    }

    /// Строка реакции в списке чатов (beta.md 7f): по чату — последняя чужая реакция на моё
    /// сообщение, с текстом и видом этого сообщения; своя и реакция на чужое — не в счёт.
    #[test]
    fn last_reactions_are_others_on_mine() {
        let store = MessageStore::open(db());
        let (me, bob, ann) = ("aa".repeat(32), "bb".repeat(32), "cc".repeat(32));
        let c = key();
        let chat = Chat::Dm(&c);
        let mine = StoredMessage { id: 1, sender: me.clone(), mine: true, text: "палатку берём?".into(), kind: "text".into(), time: 10, ..Default::default() };
        let theirs = StoredMessage { id: 2, sender: bob.clone(), text: "берём".into(), time: 11, ..Default::default() };
        store.append(chat, &mine).unwrap();
        store.append(chat, &theirs).unwrap();
        assert!(store.last_reactions(&me).is_empty(), "реакций нет");

        store.react(chat, 1, &me, &me, "🔥", 20_000).unwrap(); // своя на своё
        store.react(chat, 2, &bob, &me, "👍", 21_000).unwrap(); // моя на чужое
        assert!(store.last_reactions(&me).is_empty(), "свои и на чужое — не в счёт");

        store.react(chat, 1, &me, &bob, "❤️", 22_000).unwrap();
        store.react(chat, 1, &me, &ann, "😂", 23_000).unwrap();
        let got = store.last_reactions(&me);
        assert_eq!(got.len(), 1);
        let r = &got[0];
        assert_eq!((r.chat.as_slice(), r.scope), (c.as_slice(), 0));
        assert_eq!((r.who.as_str(), r.emoji.as_str(), r.at), (ann.as_str(), "😂", 23_000));
        assert_eq!((r.quote.as_str(), r.kind.as_str()), ("палатку берём?", "text"));

        store.react(chat, 1, &me, &ann, "", 24_000).unwrap(); // сняла — остаётся Боб
        assert_eq!(store.last_reactions(&me)[0].who, bob);
    }

    #[test]
    fn summaries_pick_last_message_per_chat() {
        let store = MessageStore::open(db());
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
        let mut store = ContactStore::open(db()).unwrap();
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
            let mut s = ContactStore::open(shared.clone()).unwrap();
            s.add(&k, "Reload", 0).unwrap();
        }
        let s2 = ContactStore::open(shared).unwrap();
        assert!(s2.contains(&k));
        assert_eq!(s2.get(&k).unwrap().name, "Reload");
    }

    #[test]
    fn contacts_rename_and_remove() {
        let k = key();
        let mut store = ContactStore::open(db()).unwrap();
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
        let mut store = ContactStore::open(shared.clone()).unwrap();
        store.add(&k, "Eve", 0).unwrap();
        assert!(!store.get(&k).unwrap().verified);
        assert!(store.set_verified(&k, Some((VerifyHow::Code, 1_700))).unwrap());
        let c = store.get(&k).unwrap();
        assert_eq!((c.verified, c.verified_at, c.verified_how), (true, 1_700, Some(VerifyHow::Code)));

        // Когда и как — переживают перезапуск.
        let mut s2 = ContactStore::open(shared).unwrap();
        let c = s2.get(&k).unwrap();
        assert_eq!((c.verified, c.verified_at, c.verified_how), (true, 1_700, Some(VerifyHow::Code)));

        // Снять отметку — снять и «когда/как»: сверен без даты не бывает.
        assert!(s2.set_verified(&k, None).unwrap());
        let c = s2.get(&k).unwrap();
        assert_eq!((c.verified, c.verified_at, c.verified_how), (false, 0, None));
    }

    /// Добавленный по коду ждёт принятия, пока пир не ответит; снятие — один раз и
    /// переживает перезапуск.
    #[test]
    fn awaiting_contact_until_accepted() {
        let (k, shared) = (key(), db());
        let mut store = ContactStore::open(shared.clone()).unwrap();
        store.add_awaiting(&k, "Лена", 1).unwrap();
        assert!(store.get(&k).unwrap().awaiting);
        assert!(store.mark_accepted(&k).unwrap(), "ждали — снимаем");
        assert!(!store.mark_accepted(&k).unwrap(), "второй раз — уже нечего");
        assert!(!ContactStore::open(shared).unwrap().get(&k).unwrap().awaiting);

        let other = key();
        store.add(&other, "", 1).unwrap();
        assert!(!store.get(&other).unwrap().awaiting, "принятый контакт не ждёт");
    }

    /// Из копии отметка не переносится — и «когда/как» тоже (решение 2026-08-01).
    #[test]
    fn imported_contact_comes_unverified() {
        let k = key();
        let mut store = ContactStore::open(db()).unwrap();
        let c = Contact {
            key: hex::encode(k),
            name: "Ева".into(),
            fingerprint: String::new(),
            added: 1,
            verified: true,
            verified_at: 5,
            verified_how: Some(VerifyHow::Set),
            profile_name: String::new(),
            avatar_id: String::new(),
            helpful: 0,
            transport_key: String::new(),
            transport_epoch: 0,
            awaiting: false,
        };
        assert!(store.import_contact(&c).unwrap());
        let got = store.get(&k).unwrap();
        assert_eq!((got.verified, got.verified_at, got.verified_how), (false, 0, None));
    }

    #[test]
    fn contacts_profile_persists() {
        let k = key();
        let shared = db();
        let mut store = ContactStore::open(shared.clone()).unwrap();
        store.add(&k, "MyNote", 0).unwrap();
        assert!(store.set_profile(&k, "TheirName", "ab12").unwrap());
        let c = store.get(&k).unwrap();
        assert_eq!(c.name, "MyNote"); // локальное имя не тронуто
        assert_eq!(c.profile_name, "TheirName");
        assert_eq!(c.avatar_id, "ab12");
        // Персист через переоткрытие.
        let s2 = ContactStore::open(shared).unwrap();
        assert_eq!(s2.get(&k).unwrap().profile_name, "TheirName");
        assert_eq!(s2.get(&k).unwrap().avatar_id, "ab12");
    }

    #[test]
    fn transport_rotate_applies_persists_and_resists_replay() {
        use crate::Identity;
        let alice = Identity::from_seed(&[31u8; 32]).unwrap();
        let k = alice.public_key_bytes();
        let shared = db();
        let mut store = ContactStore::open(shared.clone()).unwrap();
        store.add(&k, "Алиса", 0).unwrap();

        // Эпоха 0: адрес совпадает с личностью — старые записи так и живут.
        assert_eq!(store.transport_of(&k), Some(k));
        assert_eq!(store.get(&k).unwrap().transport_epoch, 0);

        // Подписанное объявление принимается, адрес меняется.
        let tk1 = alice.transport_public(1);
        let r1 = alice.address_record(1);
        assert!(store.apply_transport_rotate(&k, &r1.bytes, &r1.sig).unwrap());
        assert_eq!(store.transport_of(&k), Some(tk1));

        // Повтор той же (и более старой) эпохи отвергается: иначе перехваченное
        // объявление вернуло бы жертву на брошенный адрес.
        assert!(!store.apply_transport_rotate(&k, &r1.bytes, &r1.sig).unwrap());
        let r0 = alice.address_record(0);
        assert!(!store.apply_transport_rotate(&k, &r0.bytes, &r0.sig).unwrap());
        assert_eq!(store.transport_of(&k), Some(tk1), "адрес не откатился");

        // Чужая подпись не проходит, даже с новой эпохой.
        let mallory = Identity::from_seed(&[32u8; 32]).unwrap();
        let fake = mallory.address_record(2);
        assert!(!store.apply_transport_rotate(&k, &fake.bytes, &fake.sig).unwrap());
        assert_eq!(store.transport_of(&k), Some(tk1), "захват адреса не прошёл");

        // Персист через переоткрытие: адрес и эпоха переживают перезапуск.
        let s2 = ContactStore::open(shared).unwrap();
        assert_eq!(s2.transport_of(&k), Some(tk1));
        assert_eq!(s2.get(&k).unwrap().transport_epoch, 1);
    }

    #[test]
    fn transport_rotate_ignores_unknown_contact() {
        // Для незнакомца объявление ничем не подкреплено: его личность мы не знаем,
        // а значит и «смена адреса» — просто чужой ключ.
        let stranger = crate::Identity::from_seed(&[33u8; 32]).unwrap();
        let k = stranger.public_key_bytes();
        let r = stranger.address_record(1);
        let mut store = ContactStore::open(db()).unwrap();
        assert!(!store.apply_transport_rotate(&k, &r.bytes, &r.sig).unwrap());
        assert_eq!(store.transport_of(&k), None);
    }

    /// Сообщение в личном чате: автор проставлен явно, статус меняется у своего.
    #[test]
    fn messages_append_status_history() {
        let peer = key();
        let me = key();
        let store = MessageStore::open(db());
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

    /// Отметка «прочитано до id N» (2.10): только свои, только уже доставленные —
    /// ждущее в очереди собеседник ещё не видел, даже если его id меньше. Запоздалый
    /// Ack не откатывает «прочитано» обратно к «доставлено».
    #[test]
    fn read_mark_covers_delivered_own_messages_up_to_it() {
        let peer = key();
        let store = MessageStore::open(db());
        let (me, him) = (hex::encode(key()), hex::encode(peer));
        let chat = Chat::Dm(&peer);
        let put = |id: u64, mine: bool, status: &str, kind: &str| {
            let sender = if mine { me.clone() } else { him.clone() };
            store.append(chat, &StoredMessage { id, sender, text: "x".into(), mine, time: id as i64, status: status.into(), kind: kind.into(), ..Default::default() }).unwrap();
        };
        put(1, true, "delivered", "text");
        put(2, true, "queued", "text");
        put(3, true, "delivered", "text");
        put(4, false, "", "text");
        put(5, true, "delivered", "text");
        put(6, true, "delivered", "call");
        put(7, false, "", "call");

        assert_eq!(store.mark_read(chat, 3).unwrap(), vec![1, 3]);
        assert_eq!(store.mark_read(chat, 3).unwrap(), Vec::<u64>::new(), "повтор ничего не меняет");
        assert!(!store.set_status(chat, 1, "delivered").unwrap());
        let st: Vec<_> = store.history(chat, None, 50).into_iter().map(|m| (m.id, m.status)).collect();
        assert_eq!(&st[..5], &[(1, "read".into()), (2, "queued".into()), (3, "read".into()), (4, "".into()), (5, "delivered".into())]);
        assert_eq!(store.mark_read(chat, 99).unwrap(), vec![5], "звонок не «читается»");

        // Последнее входящее — то, до чего читаем мы сами; строка звонка не в счёт.
        assert_eq!(store.last_incoming_id(chat), Some(4));
        assert_eq!(store.last_incoming_id(Chat::Dm(&key())), None);
    }

    #[test]
    fn messages_clear() {
        let peer = key();
        let me = key();
        let store = MessageStore::open(db());
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
        let store = MessageStore::open(db());
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
        let store = MessageStore::open(db());
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
        let store = MessageStore::open(db());
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

    /// Поиск находит сообщение в нужном чате, умеет искать по началу слова и не
    /// путает чаты между собой. Пустой запрос — пустой результат (иначе «поиск»
    /// вываливал бы всю переписку).
    #[test]
    fn search_finds_messages_by_prefix_and_respects_the_chat() {
        let (me, peer, other) = (key(), key(), key());
        let store = MessageStore::open(db());
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

    /// Поиск в чате (2.9): слова по отдельности, имя файла, отправитель, счётчик и
    /// листание; звонки и системные строки не находятся.
    #[test]
    fn chat_search_takes_words_apart_and_counts() {
        let (me, peer) = (key(), key());
        let store = MessageStore::open(db());
        let (me_hex, peer_hex) = (hex::encode(me), hex::encode(peer));
        let chat = Chat::Dm(&peer);
        let put = |id: u64, sender: &str, text: &str, time: i64, extra: StoredMessage| {
            store
                .append(chat, &StoredMessage {
                    id,
                    sender: sender.into(),
                    text: text.into(),
                    mine: sender == me_hex,
                    time,
                    ..extra
                })
                .unwrap();
        };
        put(1, &peer_hex, "ключ оставлю у соседки", 10, StoredMessage::default());
        put(2, &me_hex, "А второй ключ от сарая у кого?", 20, StoredMessage::default());
        put(3, &peer_hex, "", 30, StoredMessage {
            kind: "file".into(),
            name: "Ключи от сарая.pdf".into(),
            ..Default::default()
        });
        put(4, &peer_hex, "answered", 40, StoredMessage { kind: "call".into(), ..Default::default() });
        put(5, &peer_hex, "ключ", 50, StoredMessage { system: true, ..Default::default() });

        let q = |text, sender, offset, limit| {
            store.search_page(&SearchQuery { text, chat: Some(chat), sender, offset, limit })
        };
        let both = q("сарай ключ", None, 0, 10);
        assert_eq!(both.total, 2, "оба слова, в любом порядке; имя файла тоже ищется");
        assert_eq!(both.hits[0].2.id, 3, "свежие первыми");
        assert_eq!(both.hits[1].2.id, 2);

        let all = q("ключ", None, 0, 10);
        assert_eq!(all.total, 3, "звонок и системная строка — не переписка");

        let page = q("ключ", None, 1, 1);
        assert_eq!((page.total, page.hits.len()), (3, 1), "счётчик — по всем, страница — по limit");
        assert_eq!(page.hits[0].2.id, 2, "offset пропускает свежие");

        let mine = q("ключ", Some(me_hex.as_str()), 0, 10);
        assert_eq!((mine.total, mine.hits[0].2.id), (1, 2), "только мои");
        assert_eq!(q("ключ", Some(peer_hex.as_str()), 0, 10).total, 2);
        assert_eq!(q("  ", None, 0, 10).total, 0);
        assert_eq!(q("ключи", None, 0, 10).total, 3, "окончание длинного слова не мешает");
        assert_eq!(search_stem("дом"), "дом", "короткое слово не режется");
    }

    /// Окна истории для перехода к находке: «вокруг» места и «после» нижнего.
    #[test]
    fn history_windows_around_and_after_a_message() {
        let (me, peer) = (key(), key());
        let store = MessageStore::open(db());
        let me_hex = hex::encode(me);
        let chat = Chat::Dm(&peer);
        for id in 1..=10u64 {
            store
                .append(chat, &StoredMessage {
                    id,
                    sender: me_hex.clone(),
                    text: format!("m{id}"),
                    mine: true,
                    time: 100 + (id as i64 / 2), // по два сообщения на секунду
                    ..Default::default()
                })
                .unwrap();
        }
        let ids = |v: &[StoredMessage]| v.iter().map(|m| m.id).collect::<Vec<_>>();
        let all = store.history(chat, None, 50);
        let at = all.iter().find(|m| m.id == 6).unwrap();
        let around = store.history_around(chat, (at.time, at.row), 4);
        assert_eq!(ids(&around), vec![4, 5, 6, 7], "половина до, остальное — с места");

        let last = around.last().unwrap();
        let after = store.history_after(chat, (last.time, last.row), 2);
        assert_eq!(ids(&after), vec![8, 9], "строго ниже, сообщения одной секунды не теряются");

        let day = store.history_around(chat, (103, 0), 2);
        assert_eq!(ids(&day), vec![5, 6], "row = 0 — первое не раньше времени");
        assert_eq!(ids(&store.history_around(chat, (0, 0), 2)), vec![1, 2], "начало переписки");
    }

    /// Календарь «Перейти к дате»: дни с сообщениями по местному времени.
    #[test]
    fn message_days_follow_the_local_offset() {
        let (me, peer) = (key(), key());
        let store = MessageStore::open(db());
        let chat = Chat::Dm(&peer);
        let day = 86_400;
        // 1 октября 2026, 22:30 UTC — в UTC+3 это уже 2 октября.
        let t = 1_790_893_800;
        for (id, time) in [(1u64, t), (2, t + 2 * day)] {
            store
                .append(chat, &StoredMessage {
                    id,
                    sender: hex::encode(me),
                    text: "x".into(),
                    mine: true,
                    time,
                    ..Default::default()
                })
                .unwrap();
        }
        assert_eq!(store.message_days(chat, t - day, t + 3 * day, 0), vec!["2026-10-01", "2026-10-03"]);
        assert_eq!(
            store.message_days(chat, t - day, t + 3 * day, 3 * 3600),
            vec!["2026-10-02", "2026-10-04"]
        );
        assert!(store.message_days(chat, t + day, t + 2 * day, 0).is_empty(), "промежуток полуоткрытый");
    }

    /// «Медиа и файлы»: вкладки разбирают сообщения чата по видам, свежие первыми;
    /// удалённое, системное и чужие чаты не входят; счётчики совпадают со страницами.
    #[test]
    fn media_tabs_sort_chat_messages_by_kind() {
        let (me, peer, other) = (key(), key(), key());
        let store = MessageStore::open(db());
        let chat = Chat::Dm(&peer);
        let put = |chat: Chat, id: u64, kind: &str, text: &str, system: bool| {
            store
                .append(chat, &StoredMessage {
                    id,
                    sender: hex::encode(me),
                    text: text.into(),
                    mine: true,
                    time: id as i64 * 10,
                    kind: kind.into(),
                    system,
                    ..Default::default()
                })
                .unwrap();
        };
        put(chat, 1, "image", "", false);
        put(chat, 2, "video", "смотри https://example.org/a", false);
        put(chat, 3, "file", "", false);
        put(chat, 4, "voice", "", false);
        put(chat, 5, "text", "без ссылок, просто http", false);
        put(chat, 6, "text", "Тут HTTPS://Wiki.org/x и ещё", false);
        put(chat, 7, "image", "", false);
        put(chat, 8, "text", "вступил по ссылке https://x.org", true);
        put(Chat::Dm(&other), 9, "image", "", false);
        store.retract_many(chat, &[(7, hex::encode(me))]).unwrap();

        let ids = |tab| store.media_page(chat, tab, 0, 10).items.iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(ids(MediaTab::Media), vec![2, 1]);
        assert_eq!(ids(MediaTab::Files), vec![3]);
        assert_eq!(ids(MediaTab::Voice), vec![4]);
        assert_eq!(ids(MediaTab::Links), vec![6, 2], "ссылка в подписи тоже ссылка; регистр не важен");
        assert_eq!(store.media_counts(chat), [2, 1, 1, 2]);

        let page = store.media_page(chat, MediaTab::Media, 1, 1);
        assert_eq!((page.total, page.items.iter().map(|m| m.id).collect::<Vec<_>>()), (2, vec![1]));
        assert_eq!(MediaTab::parse("links"), Some(MediaTab::Links));
        assert_eq!(MediaTab::parse("photo"), None);
    }

    /// Старая база: строки, лёгшие до нового индекса, находятся после миграции.
    #[test]
    fn search_index_is_rebuilt_from_existing_history() {
        let (me, peer) = (key(), key());
        let d = db();
        let store = MessageStore::open(d.clone());
        store
            .append(Chat::Dm(&peer), &StoredMessage {
                id: 1,
                sender: hex::encode(me),
                text: "старое сообщение".into(),
                mine: true,
                time: 1,
                ..Default::default()
            })
            .unwrap();
        {
            // База до 2.9: индекс был другой, нового нет.
            let conn = db::lock(&d);
            conn.execute_batch(
                "DROP TRIGGER chat_search_ai; DROP TRIGGER chat_search_ad; DROP TRIGGER chat_search_au;
                 DROP TABLE chat_search;",
            )
            .unwrap();
            db::migrate(&conn).unwrap();
        }
        assert_eq!(store.search("старое", None, 10).len(), 1);
    }

    /// Репутация копится только у контактов и переживает переоткрытие.
    #[test]
    fn helpful_counter_persists_and_ignores_unknown() {
        let k = key();
        let shared = db();
        let mut s = ContactStore::open(shared.clone()).unwrap();
        s.add(&k, "Helper", 0).unwrap();
        assert_eq!(s.get(&k).unwrap().helpful, 0);
        assert_eq!(s.bump_helpful(&k).unwrap(), 1);
        assert_eq!(s.bump_helpful(&k).unwrap(), 2);
        // Не-контакт заслуг не копит (анти-Sybil: бонус только после гейта).
        assert_eq!(s.bump_helpful(&key()).unwrap(), 0);

        let s2 = ContactStore::open(shared).unwrap();
        assert_eq!(s2.get(&k).unwrap().helpful, 2, "репутация пережила переоткрытие");
    }

    /// Повторный запрос первый не заменяет (анти-DoS), а запрос переживает перезапуск
    /// до решения целиком — с именем, приветствием и id.
    #[test]
    fn gate_request_keeps_first_and_survives_reopen() {
        let k = key();
        let shared = db();
        {
            let mut g = GateStore::open(shared.clone()).unwrap();
            assert!(g.upsert_request(&k, "Лена", "здравствуйте", 7, 100).unwrap(), "новый");
            assert!(!g.upsert_request(&k, "Спам", "ещё раз", 8, 200).unwrap(), "повтор не заменяет");
        }
        let mut g2 = GateStore::open(shared).unwrap();
        let r = g2.take_request(&k).unwrap();
        assert_eq!(
            (r.name.as_str(), r.greeting.as_str(), r.request_id, r.time),
            ("Лена", "здравствуйте", 7, 100)
        );
        assert!(!g2.has_request(&k));
        assert!(g2.take_request(&k).is_none(), "повторный take — пусто");
    }

    #[test]
    fn gate_block_drops_request_and_persists() {
        let k = key();
        let shared = db();
        {
            let mut g = GateStore::open(shared.clone()).unwrap();
            g.upsert_request(&k, "", "спам", 1, 1).unwrap();
            g.block(&k, 2, None).unwrap();
            assert!(g.is_blocked(&k));
            assert!(!g.has_request(&k)); // блок снял запрос
        }
        let mut g2 = GateStore::open(shared).unwrap();
        assert!(g2.is_blocked(&k)); // блок пережил переоткрытие
        assert!(g2.unblock(&k).unwrap());
        assert!(!g2.is_blocked(&k));
        assert!(!g2.unblock(&k).unwrap()); // повторный unblock — уже нет
    }

    /// «Заблокированные» (5.2): имя контакта переживает блок и переоткрытие,
    /// незнакомец — без имени; новые сверху; разблокированный пропадает из списка.
    #[test]
    fn blocked_rows_keep_contact_name() {
        let (gleb, stranger) = (key(), key());
        let shared = db();
        {
            let mut g = GateStore::open(shared.clone()).unwrap();
            g.block(&gleb, 10, Some("Глеб")).unwrap();
            g.block(&stranger, 20, None).unwrap();
        }
        let mut g = GateStore::open(shared).unwrap();
        let rows = g.blocked_rows().unwrap();
        assert_eq!(
            rows.iter().map(|r| (r.name.as_str(), r.contact, r.time)).collect::<Vec<_>>(),
            vec![("", false, 20), ("Глеб", true, 10)]
        );
        assert_eq!(rows[1].key, hex::encode(gleb));
        g.unblock(&stranger).unwrap();
        assert_eq!(g.blocked_rows().unwrap().len(), 1);
    }

    /// Запись группы обновляется, не теряя времени ротации ключа (оно пишется
    /// отдельно); удаление группы забирает и разосланные в неё приглашения.
    #[test]
    fn group_store_keeps_rekey_time_and_forgets_with_group() {
        let (admin, peer) = (key(), key());
        let g = GroupStore::new(db());
        g.save_group(b"g1", "старое имя", false, Some(&admin)).unwrap();
        g.set_rekeyed(b"g1", 77).unwrap();
        g.set_photo(b"g1", "ab12").unwrap();
        g.save_group(b"g1", "новое имя", true, Some(&admin)).unwrap();
        let rec = &g.groups()[0];
        assert_eq!((rec.name.as_str(), rec.admin, rec.admin_key, rec.rekeyed_at), ("новое имя", true, Some(admin), 77));
        assert_eq!(rec.photo, "ab12", "фото пережило перезапись записи группы");

        g.add_sent_invite(&peer, b"g1").unwrap();
        g.add_sent_invite(&peer, b"g2").unwrap();
        g.delete_group(b"g1").unwrap();
        assert!(g.groups().is_empty());
        assert_eq!(g.sent_invites(), vec![(peer, b"g2".to_vec())], "приглашение в другую группу осталось");

        g.put_invite(&GroupInviteRecord { gid: b"g3".to_vec(), name: "зовут".into(), inviter: peer, accepted: false })
            .unwrap();
        g.put_invite(&GroupInviteRecord { gid: b"g3".to_vec(), name: "зовут".into(), inviter: peer, accepted: true })
            .unwrap();
        let inv = g.invites();
        assert_eq!(inv.len(), 1);
        assert!(inv[0].accepted, "согласие перезаписывает карточку");
        g.remove_invite(b"g3").unwrap();
        assert!(g.invites().is_empty());
    }

    /// R-041: повтор (автор, id) не подменяет записанное — ни текст, ни номер строки.
    #[test]
    fn append_does_not_overwrite_existing_message() {
        let peer = key();
        let store = MessageStore::open(db());
        let author = hex::encode(peer);
        let first = StoredMessage { id: 7, sender: author.clone(), text: "оригинал".into(), time: 10, ..Default::default() };
        let row = store.append(Chat::Dm(&peer), &first).unwrap();
        let forged = StoredMessage { id: 7, sender: author.clone(), text: "подмена".into(), time: 99, ..Default::default() };
        assert!(matches!(store.append(Chat::Dm(&peer), &forged), Err(StoreError::Duplicate)));
        let h = store.history(Chat::Dm(&peer), None, 10);
        assert_eq!(h.len(), 1);
        assert_eq!((h[0].text.as_str(), h[0].time, h[0].row), ("оригинал", 10, row));
        // Тот же id от ДРУГОГО автора — другое сообщение.
        let other = StoredMessage { id: 7, sender: hex::encode(key()), text: "чужое".into(), ..Default::default() };
        assert!(store.append(Chat::Dm(&peer), &other).is_ok());
    }

    /// Реакции: одна на человека, позднее (по `at`) побеждает, снятие — тоже изменение
    /// и опоздавшая постановка его не отменяет; история отдаёт их сгруппированными.
    #[test]
    fn reactions_last_word_wins_and_ride_in_history() {
        let peer = key();
        let store = MessageStore::open(db());
        let (author, me, him) = (hex::encode(peer), "me".to_string(), "him".to_string());
        let chat = Chat::Dm(&peer);
        store.append(chat, &StoredMessage { id: 7, sender: author.clone(), text: "м".into(), time: 1, ..Default::default() }).unwrap();

        assert!(store.react(chat, 7, &author, &me, "👍", 10).unwrap());
        assert!(store.react(chat, 7, &author, &him, "👍", 11).unwrap());
        assert!(!store.react(chat, 7, &author, &me, "😂", 9).unwrap(), "опоздавшее старше — не меняет");
        let r = store.reactions(chat, 7, &author);
        assert_eq!(r, vec![Reaction { emoji: "👍".into(), who: vec![me.clone(), him.clone()] }]);

        assert!(store.react(chat, 7, &author, &me, "❤️", 12).unwrap(), "другая заменяет");
        assert!(store.react(chat, 7, &author, &him, "", 13).unwrap(), "снятие");
        assert!(!store.react(chat, 7, &author, &him, "👍", 11).unwrap(), "снятое не воскресает");
        let h = store.history(chat, None, 10);
        assert_eq!(h[0].reactions, vec![Reaction { emoji: "❤️".into(), who: vec![me.clone()] }]);

        // Отзыв стирает и реакции: строка-надгробие голая.
        store.retract_many(chat, &[(7, author.clone())]).unwrap();
        assert!(store.reactions(chat, 7, &author).is_empty());
        assert!(store.history(chat, None, 10)[0].reactions.is_empty());
    }

    /// Непросмотренные реакции (капсула «❤», 5.7): только чужие на МОИ сообщения, по
    /// порядку сообщений в ленте; смена реакции — снова новая, снятая и своя — нет;
    /// просмотр гасит все реакции сообщения, копия данных приходит просмотренной.
    #[test]
    fn unseen_reactions_are_others_on_mine_in_feed_order() {
        let peer = key();
        let store = MessageStore::open(db());
        let (me, him) = ("aa".to_string(), hex::encode(peer));
        let chat = Chat::Dm(&peer);
        for (id, sender, time) in [(1, &me, 10), (2, &him, 20), (3, &me, 30)] {
            store.append(chat, &StoredMessage { id, sender: sender.clone(), text: "м".into(), time, ..Default::default() }).unwrap();
        }
        let unseen = |store: &MessageStore| -> Vec<(u64, String)> {
            store.unseen_reactions(chat, &me).into_iter().map(|(id, s, _, _)| (id, s)).collect()
        };
        store.react(chat, 3, &me, &him, "👍", 100).unwrap();
        store.react(chat, 1, &me, &him, "❤️", 101).unwrap();
        store.react(chat, 2, &him, &me, "👍", 102).unwrap(); // моя на его — не в счёт
        store.react(chat, 3, &me, &me, "😂", 103).unwrap(); // своя на своё — не в счёт
        assert_eq!(unseen(&store), vec![(1, me.clone()), (3, me.clone())]);
        assert_eq!(store.unseen_reactions(chat, &me)[0].2, 10, "время — сообщения, для окна истории");

        store.mark_reactions_seen(chat, 1, &me).unwrap();
        assert_eq!(unseen(&store), vec![(3, me.clone())]);
        store.react(chat, 1, &me, &him, "🔥", 104).unwrap();
        assert_eq!(unseen(&store), vec![(1, me.clone()), (3, me.clone())], "сменил — снова новая");
        store.react(chat, 3, &me, &him, "", 105).unwrap();
        assert_eq!(unseen(&store), vec![(1, me.clone())], "снятая не в счёт");

        store.mark_all_reactions_seen().unwrap();
        assert!(unseen(&store).is_empty());
    }

    /// Отозванное «у всех» оставляет строку на месте: содержимое стёрто, правка и
    /// повторная доставка его не воскрешают, закреп снят, поиск его не находит.
    #[test]
    fn retract_leaves_a_tombstone() {
        let peer = key();
        let store = MessageStore::open(db());
        let author = hex::encode(peer);
        let chat = Chat::Dm(&peer);
        let photo = StoredMessage {
            id: 7, sender: author.clone(), text: "подпись".into(), time: 10, kind: "image".into(),
            blob_id: "ab".into(), thumb_id: "cd".into(), reply_to: 3, reply_sender: author.clone(),
            ..Default::default()
        };
        store.append(chat, &photo).unwrap();
        store.pin(chat, 7, &author, 11, &author).unwrap();

        assert_eq!(store.retract_many(chat, &[(7, author.clone()), (99, author.clone())]).unwrap(), 1);
        let h = store.history(chat, None, 10);
        assert_eq!(h.len(), 1, "строка осталась");
        let t = &h[0];
        assert_eq!((t.kind.as_str(), t.text.as_str(), t.blob_id.as_str(), t.thumb_id.as_str()), ("deleted", "", "", ""));
        assert_eq!((t.time, t.reply_to), (10, 0), "место в ленте то же, цитаты нет");
        assert!(store.pinned(chat).is_empty(), "закреп снят");
        assert!(store.search("подпись", None, 10).is_empty());

        assert!(!store.edit_text(chat, &author, 7, "воскрес", &[], 12).unwrap(), "правка не возвращает текст");
        let again = StoredMessage { id: 7, sender: author.clone(), text: "подпись".into(), ..Default::default() };
        assert!(matches!(store.append(chat, &again), Err(StoreError::Duplicate)), "досылка не воскрешает");
        assert_eq!(store.history(chat, None, 10)[0].text, "");
    }

    /// R-043: не записалось в базу — кэш контактов не меняется (иначе память и
    /// диск разошлись бы до перезапуска).
    #[test]
    fn failed_contact_write_leaves_cache_alone() {
        let k = key();
        let shared = db();
        let mut store = ContactStore::open(shared.clone()).unwrap();
        store.add(&k, "Аня", 1).unwrap();
        crate::db::lock(&shared).execute_batch("PRAGMA query_only = ON;").unwrap();
        assert!(store.rename(&k, "Боря").is_err());
        assert!(store.set_verified(&k, Some((VerifyHow::Set, 1))).is_err());
        assert!(store.set_profile(&k, "Борис", "ab").is_err());
        assert!(store.bump_helpful(&k).is_err());
        assert!(store.remove(&k).is_err());
        let c = store.get(&k).expect("контакт на месте");
        assert_eq!((c.name.as_str(), c.verified, c.profile_name.as_str(), c.helpful), ("Аня", false, "", 0));
    }

    fn call(gid: &[u8], outcome: &str, started: i64, duration: i64) -> CallRecord {
        CallRecord {
            id: started,
            peer: [1; 32],
            gid: gid.to_vec(),
            incoming: true,
            outcome: outcome.into(),
            started,
            duration,
            video: false,
        }
    }

    #[test]
    fn collapse_rooms_merges_only_consecutive_same_room() {
        let rows = vec![
            call(b"g", "answered", 50, 10),
            call(b"g", "answered", 40, 20),
            call(b"", "answered", 30, 5),
            call(b"", "answered", 25, 5),
            call(b"g", "answered", 20, 1),
            call(b"h", "answered", 10, 1),
        ];
        let runs = collapse_rooms(rows);
        let shape: Vec<(i64, u32, i64)> = runs.iter().map(|r| (r.last.started, r.count, r.duration)).collect();
        // Серия «g» сверху свёрнута; личные звонки не сворачиваются; «g» после
        // перерыва — новая строка.
        assert_eq!(shape, vec![(50, 2, 30), (30, 1, 5), (25, 1, 5), (20, 1, 1), (10, 1, 1)]);
    }

    #[test]
    fn collapse_rooms_keeps_missed_apart() {
        let runs = collapse_rooms(vec![call(b"g", "answered", 50, 10), call(b"g", "missed", 40, 0), call(b"g", "missed", 30, 0)]);
        let shape: Vec<(String, u32)> = runs.iter().map(|r| (r.last.outcome.clone(), r.count)).collect();
        assert_eq!(shape, vec![("answered".into(), 1), ("missed".into(), 2)]);
    }

    #[test]
    fn deleting_group_drops_its_room_calls() {
        let shared = db();
        let calls = CallStore::new(shared.clone());
        let groups = GroupStore::new(shared);
        calls.start(&[1; 32], b"g", true, false, "answered", 10);
        calls.start(&[1; 32], b"h", true, false, "answered", 20);
        calls.start(&[1; 32], b"", true, false, "missed", 30);
        groups.delete_group(b"g").unwrap();
        let left: Vec<Vec<u8>> = calls.list(10).into_iter().map(|c| c.gid).collect();
        assert_eq!(left, vec![Vec::new(), b"h".to_vec()]);
    }

}
