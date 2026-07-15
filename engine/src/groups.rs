//! Менеджер групп (MLS) для Android. Живёт в ОДНОМ потоке (event_pump), т.к.
//! [`GroupChat`]/openmls — `!Send`. JNI-команды приходят сюда через канал,
//! групповые события мессенджера — оттуда же. Зеркалит десктопную логику:
//! админ-секвенсор, маршрутизация по group_id, персистентность в data_dir/groups.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_group::peek_group_id;
use p2p_groupchat::GroupChat;
use p2p_messenger::Messenger;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Default)]
pub struct GMsg {
    pub text: String,
    pub mine: bool,
    pub time: i64,
    /// hex ed25519-pubkey отправителя (для имени/цвета/аватарки в UI).
    pub sender: String,
    /// Системное событие (смена состава) — рендерится отдельной строкой.
    pub system: bool,
    /// Тип (M2): "text" (по умолчанию) | "image" | "voice" | "file".
    pub kind: String,
    /// hex-хэш блоба вложения (M2); пусто для текста.
    pub blob_id: String,
    pub mime: String,
    pub name: String,
}

struct Entry {
    chat: GroupChat,
    name: String,
    messages: Vec<GMsg>,
    /// Мы — админ/секвенсор этой группы (создатель). Только админ вправе отзывать
    /// участников и коммитить ротацию (ADR-004: избегаем одновременных Commit).
    admin: bool,
}

/// Не чаще одного resync-обслуживания на пира+группу за это окно (анти-амплификация).
const RESYNC_COOLDOWN: Duration = Duration::from_secs(10);
/// Не чаще раза в сутки админ ротирует ключ группы (катящийся PCS без лишней churn).
const REKEY_INTERVAL_SECS: i64 = 24 * 3600;

/// Входящее приглашение в группу, ожидающее решения пользователя (карточка).
#[derive(Clone)]
struct PendingInvite {
    name: String,
    inviter: [u8; 32],
}

pub struct GroupManager {
    groups: HashMap<Vec<u8>, Entry>,
    pending: Option<GroupChat>,
    /// Имя группы для текущего вступления (из принятого инвайта) — чтобы на Welcome
    /// поставить настоящее название сразу, а не дефолт.
    pending_join_name: Option<String>,
    /// Входящие приглашения (gid → кто зовёт + имя), ждут accept/decline. Персистятся.
    pending_invites: HashMap<Vec<u8>, PendingInvite>,
    invites: HashMap<[u8; 32], Vec<u8>>,
    /// Пиры, для которых текущий KeyPackage-обмен — это RE-ADD (resync, ADR-004),
    /// а не первое приглашение: на `on_key_reply` делаем Remove+Add, не просто Add.
    resyncing: HashSet<[u8; 32]>,
    /// Троттлинг resync-обслуживания на админе: (peer,gid) → время последнего.
    resync_last: HashMap<([u8; 32], Vec<u8>), Instant>,
    identity: Identity,
    messenger: Arc<Messenger>,
    dir: PathBuf,
}

impl GroupManager {
    pub fn new(identity: Identity, messenger: Arc<Messenger>, data_dir: PathBuf) -> Self {
        let dir = data_dir.join("groups");
        let mut m = Self {
            groups: HashMap::new(),
            pending: None,
            pending_join_name: None,
            pending_invites: HashMap::new(),
            invites: HashMap::new(),
            resyncing: HashSet::new(),
            resync_last: HashMap::new(),
            identity,
            messenger,
            dir,
        };
        m.load_all();
        m.load_pending_invites();
        m.maybe_rekey_admin_groups();
        m
    }

    /// Времена последней ротации ключа (hex gid → unix) из `rekey.json`.
    fn rekey_times(&self) -> HashMap<String, i64> {
        std::fs::read(self.dir.join("rekey.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Катящийся PCS (ADR-004 п.7): при старте админ ротирует ключ тех своих групп,
    /// где прошло >суток с прошлой ротации. Fire-and-forget; отставшие вылечатся
    /// resync'ом. Дёшево (раз в сутки на группу) и не завязано на таймер-поток.
    fn maybe_rekey_admin_groups(&mut self) {
        let now = now();
        let mut times = self.rekey_times();
        let stale: Vec<Vec<u8>> = self
            .groups
            .iter()
            .filter(|(id, e)| {
                e.admin && now - times.get(&hex::encode(id)).copied().unwrap_or(0) >= REKEY_INTERVAL_SECS
            })
            .map(|(id, _)| id.clone())
            .collect();
        if stale.is_empty() {
            return;
        }
        for gid in stale {
            if self.rekey(&gid) {
                times.insert(hex::encode(&gid), now);
            }
        }
        if let Ok(j) = serde_json::to_vec(&times) {
            let _ = std::fs::write(self.dir.join("rekey.json"), j);
        }
    }

    fn group_path(&self, id: &[u8]) -> PathBuf {
        self.dir.join(format!("{}.group", hex::encode(id)))
    }
    fn msgs_path(&self, id: &[u8]) -> PathBuf {
        self.dir.join(format!("{}.msgs.json", hex::encode(id)))
    }

    fn save_index(&self) {
        let idx: HashMap<String, String> =
            self.groups.iter().map(|(id, e)| (hex::encode(id), e.name.clone())).collect();
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(j) = serde_json::to_vec_pretty(&idx) {
            let _ = std::fs::write(self.dir.join("index.json"), j);
        }
        self.save_admins();
    }

    /// Флаг «мы админ» хранится отдельным файлом (admins.json — массив hex gid),
    /// чтобы не ломать формат index.json существующих установок (ADR-004).
    fn save_admins(&self) {
        let admins: Vec<String> = self
            .groups
            .iter()
            .filter(|(_, e)| e.admin)
            .map(|(id, _)| hex::encode(id))
            .collect();
        if let Ok(j) = serde_json::to_vec(&admins) {
            let _ = std::fs::write(self.dir.join("admins.json"), j);
        }
    }

    /// Множество gid, где мы админ (из admins.json).
    fn load_admins(&self) -> std::collections::HashSet<Vec<u8>> {
        std::fs::read(self.dir.join("admins.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
            .unwrap_or_default()
            .iter()
            .filter_map(|h| hex::decode(h).ok())
            .collect()
    }

    fn save_group(&self, id: &[u8]) {
        if let Some(e) = self.groups.get(id) {
            let _ = e.chat.save(&self.group_path(id));
        }
        self.save_index();
    }

    fn save_msgs(&self, id: &[u8]) {
        let Some(e) = self.groups.get(id) else { return };
        let arr: Vec<serde_json::Value> = e
            .messages
            .iter()
            .map(|m| serde_json::json!({
                "mine": m.mine, "text": m.text, "time": m.time,
                "sender": m.sender, "system": m.system,
                "kind": m.kind, "blob_id": m.blob_id, "mime": m.mime, "name": m.name,
            }))
            .collect();
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(j) = serde_json::to_vec(&arr) {
            let _ = std::fs::write(self.msgs_path(id), p2p_core::vault::seal(&j));
        }
    }

    fn load_msgs(&self, id: &[u8]) -> Vec<GMsg> {
        let Ok(raw) = std::fs::read(self.msgs_path(id)) else { return Vec::new() };
        let Ok(bytes) = p2p_core::vault::open(&raw) else { return Vec::new() };
        let Ok(arr) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|v| {
                Some(GMsg {
                    mine: v["mine"].as_bool()?,
                    text: v["text"].as_str().unwrap_or("").to_string(),
                    time: v["time"].as_i64().unwrap_or(0),
                    sender: v["sender"].as_str().unwrap_or("").to_string(),
                    system: v["system"].as_bool().unwrap_or(false),
                    kind: v["kind"].as_str().unwrap_or("").to_string(),
                    blob_id: v["blob_id"].as_str().unwrap_or("").to_string(),
                    mime: v["mime"].as_str().unwrap_or("").to_string(),
                    name: v["name"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect()
    }

    fn load_all(&mut self) {
        let Ok(bytes) = std::fs::read(self.dir.join("index.json")) else { return };
        let Ok(idx) = serde_json::from_slice::<HashMap<String, String>>(&bytes) else { return };
        let admins = self.load_admins();
        for (hexid, name) in idx {
            let Ok(raw) = hex::decode(&hexid) else { continue };
            let path = self.group_path(&raw);
            if let Ok(chat) = GroupChat::load(&path, &self.identity, self.messenger.clone()) {
                for m in chat.members() {
                    crate::spawn_connect(self.messenger.clone(), m);
                }
                let messages = self.load_msgs(&raw);
                let admin = admins.contains(&raw);
                self.groups.insert(raw, Entry { chat, name, messages, admin });
            }
        }
    }

    // ── Команды ─────────────────────────────────────────────────────────────

    /// Создать группу (мы — администратор). Возвращает group_id.
    pub fn create(&mut self, name: &str) -> Option<Vec<u8>> {
        let chat = GroupChat::create(&self.identity, self.messenger.clone()).ok()?;
        let id = chat.group_id()?;
        // Создатель = админ/секвенсор (ADR-004).
        self.groups.insert(id.clone(), Entry { chat, name: name.to_string(), messages: Vec::new(), admin: true });
        self.save_group(&id);
        Some(id)
    }

    /// Пригласить контакт в группу карточкой-сообщением (решение владельца 2026-06-28):
    /// шлём `GroupInvite` с именем группы; получатель сам решает вступать. Запоминаем
    /// peer→gid, чтобы на его будущий KeyPackage (accept) добавить в нужную группу.
    pub fn invite(&mut self, gid: &[u8], peer: [u8; 32]) {
        if let Some(e) = self.groups.get(gid) {
            let _ = self.messenger.send_group_invite(&peer, gid, &e.name);
            self.invites.insert(peer, gid.to_vec());
        }
    }

    // ── Входящие приглашения (карточка на одобрение) ──────────────────────

    fn pending_invites_path(&self) -> PathBuf {
        self.dir.join("pending_invites.json")
    }

    fn save_pending_invites(&self) {
        let map: serde_json::Value = serde_json::json!(self
            .pending_invites
            .iter()
            .map(|(gid, inv)| (
                hex::encode(gid),
                serde_json::json!({ "name": inv.name, "inviter": hex::encode(inv.inviter) }),
            ))
            .collect::<serde_json::Map<_, _>>());
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(j) = serde_json::to_vec(&map) {
            let _ = std::fs::write(self.pending_invites_path(), j);
        }
    }

    fn load_pending_invites(&mut self) {
        let Ok(bytes) = std::fs::read(self.pending_invites_path()) else { return };
        let Ok(map) = serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes) else {
            return;
        };
        for (gid_hex, v) in map {
            let (Ok(gid), Some(inviter)) = (
                hex::decode(&gid_hex),
                v["inviter"].as_str().and_then(crate::parse_key),
            ) else {
                continue;
            };
            self.pending_invites.insert(gid, PendingInvite {
                name: v["name"].as_str().unwrap_or("").to_string(),
                inviter,
            });
        }
    }

    /// Пришло приглашение в группу. Если уже состоим — игнор (это не первый вход).
    /// Возвращает true, если запись добавлена (UI показать карточку).
    pub fn on_invite(&mut self, peer: [u8; 32], gid: Vec<u8>, name: String) -> bool {
        if self.groups.contains_key(&gid) {
            return false;
        }
        self.pending_invites.insert(gid, PendingInvite { name, inviter: peer });
        self.save_pending_invites();
        true
    }

    /// Входящие приглашения как JSON [{gid, name, inviter}].
    pub fn invites_json(&self) -> String {
        let arr: Vec<serde_json::Value> = self
            .pending_invites
            .iter()
            .map(|(gid, inv)| serde_json::json!({
                "gid": hex::encode(gid),
                "name": inv.name,
                "inviter": hex::encode(inv.inviter),
            }))
            .collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Принять приглашение: создаём pending-участника и шлём KeyPackage пригласившему
    /// (дальше как обычный join — админ добавит нас, придёт Welcome). Имя группы из
    /// инвайта запоминаем, чтобы показать сразу. Возвращает true при успехе.
    pub fn accept_invite(&mut self, gid: &[u8]) -> bool {
        let Some(inv) = self.pending_invites.remove(gid) else { return false };
        self.save_pending_invites();
        if self.pending.is_some() {
            // Уже идёт другое вступление — вернём инвайт в очередь, попробуем позже.
            self.pending_invites.insert(gid.to_vec(), inv);
            self.save_pending_invites();
            return false;
        }
        match GroupChat::pending(&self.identity, self.messenger.clone()) {
            Ok((pending, kp)) => {
                let _ = self.messenger.send_group_key_reply(&inv.inviter, &kp);
                self.pending = Some(pending);
                self.pending_join_name = Some(inv.name);
                true
            }
            Err(_) => false,
        }
    }

    /// Отклонить приглашение (тихо убрать из очереди).
    pub fn decline_invite(&mut self, gid: &[u8]) -> bool {
        let removed = self.pending_invites.remove(gid).is_some();
        if removed {
            self.save_pending_invites();
        }
        removed
    }

    /// Отправить текст в группу. Возвращает true при успехе.
    pub fn send_text(&mut self, gid: &[u8], text: &str) -> bool {
        let Some(e) = self.groups.get_mut(gid) else { return false };
        if e.chat.send_text(text).is_err() {
            return false;
        }
        let me = hex::encode(self.identity.public_key_bytes());
        e.messages.push(GMsg {
            text: text.to_string(), mine: true, time: now(), sender: me, ..Default::default()
        });
        self.save_msgs(gid);
        // КРИТИЧНО: отправка двигает MLS sending-ratchet — его надо ПЕРСИСТИТЬ, иначе
        // после рестарта/убийства приложения отправитель грузит старое состояние и
        // шлёт со «старого» поколения → получатель (он-то приём сохраняет) уже
        // удалил тот секрет (forward secrecy) → "secret deleted" → сообщения не доходят.
        self.save_group(gid);
        true
    }

    /// Отозвать участника из группы (ADR-004). Только админ. Двигает MLS-состояние
    /// → save_group. Возвращает true при успехе. Commit ушёл всем (вкл. удаляемого).
    pub fn remove_member(&mut self, gid: &[u8], peer: [u8; 32]) -> bool {
        let Some(e) = self.groups.get_mut(gid) else { return false };
        if !e.admin {
            log::warn!("remove_member: не админ группы {} — отказано", hex::encode(gid));
            return false;
        }
        if e.chat.remove_member(peer).is_err() {
            return false;
        }
        // Системная строка «участник удалён» (актор — удалённый).
        e.messages.push(GMsg {
            mine: true, time: now(), sender: hex::encode(peer), system: true, ..Default::default()
        });
        self.save_msgs(gid);
        self.save_group(gid); // Commit сдвинул эпоху — персистим (инвариант MLS).
        true
    }

    /// Отправить медиа-вложение в группу (M2): MLS-сообщение со ссылкой на блоб.
    /// Байты блоба пушит вызывающий (engine) каждому участнику отдельно. Двигает
    /// sending-ratchet → save_group (как send_text). Возвращает участников для push.
    pub fn send_media(&mut self, gid: &[u8], kind: &str, blob_id: &str, mime: &str, name: &str) -> bool {
        let Some(e) = self.groups.get_mut(gid) else { return false };
        if e.chat.send_media(kind, blob_id, mime, name).is_err() {
            return false;
        }
        let me = hex::encode(self.identity.public_key_bytes());
        e.messages.push(GMsg {
            mine: true, time: now(), sender: me,
            kind: kind.into(), blob_id: blob_id.into(), mime: mime.into(), name: name.into(),
            ..Default::default()
        });
        self.save_msgs(gid);
        self.save_group(gid);
        true
    }

    /// Ротация собственного ключа группы (PCS, ADR-004). Двигает эпоху → save_group.
    pub fn rekey(&mut self, gid: &[u8]) -> bool {
        let Some(e) = self.groups.get_mut(gid) else { return false };
        if e.chat.rekey_self().is_err() {
            return false;
        }
        self.save_group(gid);
        true
    }

    /// Мы ли админ группы (для гейта UI «удалить участника»).
    pub fn is_admin(&self, gid: &[u8]) -> bool {
        self.groups.get(gid).map(|e| e.admin).unwrap_or(false)
    }

    /// Все id групп (для обновления кэша истории/участников разом).
    pub fn group_ids(&self) -> Vec<Vec<u8>> {
        self.groups.keys().cloned().collect()
    }

    pub fn list_json(&self) -> String {
        let arr: Vec<serde_json::Value> = self
            .groups
            .iter()
            .map(|(id, e)| serde_json::json!({
                "gid": hex::encode(id),
                "name": e.name,
                "members": e.chat.member_count(),
                "admin": e.admin,
            }))
            .collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    pub fn history_json(&self, gid: &[u8]) -> String {
        let Some(e) = self.groups.get(gid) else { return "[]".into() };
        let arr: Vec<serde_json::Value> = e
            .messages
            .iter()
            .map(|m| serde_json::json!({
                "text": m.text, "mine": m.mine, "time": m.time,
                "sender": m.sender, "system": m.system,
                "kind": m.kind, "blob_id": m.blob_id, "mime": m.mime, "name": m.name,
            }))
            .collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Ключи участников (hex) — для группового звонка.
    pub fn members_json(&self, gid: &[u8]) -> String {
        let Some(e) = self.groups.get(gid) else { return "[]".into() };
        let arr: Vec<String> = e.chat.members().iter().map(hex::encode).collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Ключи участников группы (для маячка присутствия комнаты).
    pub fn members(&self, gid: &[u8]) -> Vec<[u8; 32]> {
        self.groups.get(gid).map(|e| e.chat.members()).unwrap_or_default()
    }

    // ── События мессенджера ───────────────────────────────────────────────

    /// Контакт просит наш KeyPackage (нас приглашают) — отвечаем и ждём Welcome.
    pub fn on_key_request(&mut self, peer: [u8; 32]) {
        if self.pending.is_some() {
            return;
        }
        if let Ok((pending, kp)) = GroupChat::pending(&self.identity, self.messenger.clone()) {
            let _ = self.messenger.send_group_key_reply(&peer, &kp);
            self.pending = Some(pending);
        }
    }

    /// Контакт прислал KeyPackage — добавляем его в группу, куда приглашали.
    /// RESYNC (ADR-004): если это re-add отставшего, сперва убираем его устаревший
    /// leaf (Remove), затем добавляем свежим (Add) — участник входит с текущей эпохи.
    pub fn on_key_reply(&mut self, peer: [u8; 32], key_package: &[u8]) -> bool {
        let Some(id) = self.invites.remove(&peer) else { return false };
        let resync = self.resyncing.remove(&peer);
        let added = self
            .groups
            .get_mut(&id)
            .map(|e| {
                if resync {
                    let _ = e.chat.remove_member(peer); // снять устаревший leaf
                }
                e.chat.add_member(peer, key_package).is_ok()
            })
            .unwrap_or(false);
        if added {
            crate::spawn_connect(self.messenger.clone(), peer);
            self.save_group(&id);
            // Догоняем нового участника настоящим именем группы (иначе у него дефолт).
            // Welcome уже ушёл в add_member (раньше в очереди) → имя применится после
            // вступления. Идемпотентно и для уже состоящих участников.
            if let Some(e) = self.groups.get_mut(&id) {
                let name = e.name.clone();
                let _ = e.chat.send_name(&name);
            }
        }
        added
    }

    /// Входящее групповое сообщение. Возвращает Some((gid_hex, text)) для
    /// прикладного сообщения (чтобы UI показал), либо None (Commit/Welcome).
    pub fn on_group(&mut self, data: &[u8]) -> GroupOutcome {
        use p2p_groupchat::Inbound;
        // Известная группа? (Commit или прикладное сообщение)
        let known = peek_group_id(data).filter(|g| self.groups.contains_key(g));
        if let Some(gid) = known {
            let inbound = self.groups.get_mut(&gid).map(|e| e.chat.handle(data));
            let outcome = match inbound {
                Some(Ok(Inbound::Message { sender, text })) => {
                    let sender_hex = hex::encode(sender);
                    if let Some(e) = self.groups.get_mut(&gid) {
                        e.messages.push(GMsg {
                            text: text.clone(), mine: false, time: now(),
                            sender: sender_hex.clone(), ..Default::default()
                        });
                    }
                    self.save_msgs(&gid);
                    GroupOutcome {
                        gid: Some(gid.clone()), text: Some(text),
                        sender: Some(sender_hex), ..Default::default()
                    }
                }
                Some(Ok(Inbound::Media { sender, kind, hash, mime, name })) => {
                    let sender_hex = hex::encode(sender);
                    if let Some(e) = self.groups.get_mut(&gid) {
                        e.messages.push(GMsg {
                            mine: false, time: now(), sender: sender_hex.clone(),
                            kind: kind.clone(), blob_id: hash.clone(), mime: mime.clone(), name: name.clone(),
                            ..Default::default()
                        });
                    }
                    self.save_msgs(&gid);
                    GroupOutcome {
                        gid: Some(gid.clone()), sender: Some(sender_hex),
                        media: Some(GroupMediaRef { kind, blob_id: hash, mime, name }),
                        ..Default::default()
                    }
                }
                // Служебное: админ прислал имя группы — обновляем название (не сообщение).
                Some(Ok(Inbound::Name { name })) => {
                    let changed = if let Some(e) = self.groups.get_mut(&gid) {
                        let diff = e.name != name;
                        e.name = name;
                        diff
                    } else {
                        false
                    };
                    if changed {
                        self.save_group(&gid); // персист имени в index.json
                    }
                    GroupOutcome { gid: Some(gid.clone()), renamed: changed, ..Default::default() }
                }
                Some(Ok(Inbound::Membership { actor })) => {
                    let actor_hex = hex::encode(actor);
                    if let Some(e) = self.groups.get_mut(&gid) {
                        e.messages.push(GMsg {
                            mine: false, time: now(), sender: actor_hex.clone(), system: true,
                            ..Default::default()
                        });
                    }
                    self.save_msgs(&gid);
                    GroupOutcome {
                        gid: Some(gid.clone()), sender: Some(actor_hex), system: true,
                        ..Default::default()
                    }
                }
                // ADR-004: нас отозвали — чистим локальное состояние группы и
                // сообщаем UI. Ранний возврат: save_group ниже уже не нужен.
                Some(Ok(Inbound::SelfRemoved)) => {
                    log::info!("group {}: нас удалили из группы", hex::encode(&gid));
                    self.delete_group(&gid);
                    return GroupOutcome { gid: Some(gid), removed: true, ..Default::default() };
                }
                // ADR-004: отстали (пропущен Commit / удалён секрет эпохи) — просим
                // админа заново синхронизировать нас (Remove+Add).
                Some(Ok(Inbound::NeedsResync)) => {
                    log::warn!("group {}: рассинхрон эпох — запрашиваю resync", hex::encode(&gid));
                    self.request_resync(&gid);
                    GroupOutcome { gid: Some(gid.clone()), ..Default::default() }
                }
                Some(Err(e)) => {
                    log::warn!("group {} MLS отверг сообщение: {:?}", hex::encode(&gid), e);
                    GroupOutcome { gid: Some(gid.clone()), ..Default::default() }
                }
                _ => GroupOutcome { gid: Some(gid.clone()), ..Default::default() },
            };
            self.save_group(&gid);
            return outcome;
        }
        // Иначе — возможно Welcome для ожидающего вступления.
        let joined_gid = self.pending.as_mut().and_then(|p| {
            let _ = p.handle(data);
            p.group_id()
        });
        if let Some(id) = joined_gid {
            if let Some(chat) = self.pending.take() {
                for m in chat.members() {
                    crate::spawn_connect(self.messenger.clone(), m);
                }
                // RESYNC (ADR-004): Welcome для УЖЕ известной группы = нас заново
                // добавили после рассинхрона. Меняем только MLS-состояние (chat),
                // сохраняя имя и историю сообщений; событие тихое (не «joined»).
                if let Some(existing) = self.groups.get_mut(&id) {
                    existing.chat = chat;
                    self.save_group(&id);
                    log::info!("group {}: resync завершён (перевступили)", hex::encode(&id));
                    return GroupOutcome { gid: Some(id), ..Default::default() };
                }
                // Иначе — первое вступление. Мы НЕ админ (админ — пригласивший).
                // Имя — из принятого инвайта (иначе дефолт; админ всё равно догонит send_name).
                let name = self.pending_join_name.take().unwrap_or_else(|| "Группа".into());
                self.pending_invites.remove(&id);
                self.save_pending_invites();
                self.groups.insert(id.clone(), Entry { chat, name, messages: Vec::new(), admin: false });
                self.save_group(&id);
                return GroupOutcome { gid: Some(id), joined: true, ..Default::default() };
            }
        }
        GroupOutcome::default()
    }

    /// Отставший просит админа заново синхронизировать нас (ADR-004): рассылаем
    /// `GroupResyncRequest` текущему (последнему известному) ростеру — среагирует
    /// только админ. Fire-and-forget; троттлится на приёмнике.
    fn request_resync(&self, gid: &[u8]) {
        if let Some(e) = self.groups.get(gid) {
            for peer in e.chat.members() {
                let _ = self.messenger.send_group_resync_request(&peer, gid);
            }
        }
    }

    /// Админ получил запрос resync (ADR-004). Обслуживаем, только если мы админ,
    /// проситель в текущем ростере (отозванного не «лечим» назад) и не слишком
    /// часто. Просим свежий KeyPackage и помечаем обмен как re-add.
    pub fn on_resync_request(&mut self, peer: [u8; 32], gid: &[u8]) {
        let Some(e) = self.groups.get(gid) else { return };
        if !e.admin || !e.chat.members().contains(&peer) {
            return;
        }
        let now = Instant::now();
        let key = (peer, gid.to_vec());
        if let Some(t) = self.resync_last.get(&key) {
            if now.duration_since(*t) < RESYNC_COOLDOWN {
                return;
            }
        }
        self.resync_last.insert(key, now);
        let _ = self.messenger.send_group_key_request(&peer);
        self.invites.insert(peer, gid.to_vec());
        self.resyncing.insert(peer); // on_key_reply сделает Remove+Add, не просто Add
    }

    /// Выйти из группы (ADR-004): просим админа отозвать нас (Remove+смена эпохи —
    /// настоящий криптографический выход) и локально забываем группу. Если админ
    /// офлайн — остаётся мягкий локальный выход (нас отзовут при его доступности).
    pub fn leave(&mut self, gid: &[u8]) -> bool {
        let Some(e) = self.groups.get(gid) else { return false };
        for peer in e.chat.members() {
            let _ = self.messenger.send_group_leave_request(&peer, gid);
        }
        self.delete_group(gid);
        true
    }

    /// Админ получил запрос на выход участника (ADR-004): отзываем его, если мы
    /// админ и он в текущем ростере. Переиспользует `remove_member`. Возвращает
    /// true, если отзыв произошёл (UI/кэш надо обновить).
    pub fn on_leave_request(&mut self, peer: [u8; 32], gid: &[u8]) -> bool {
        let is_admin_member = self
            .groups
            .get(gid)
            .map(|e| e.admin && e.chat.members().contains(&peer))
            .unwrap_or(false);
        is_admin_member && self.remove_member(gid, peer)
    }

    /// Удалить группу локально (нас из неё отозвали, ADR-004): файлы + кэш + индекс.
    fn delete_group(&mut self, id: &[u8]) {
        self.groups.remove(id);
        let _ = std::fs::remove_file(self.group_path(id));
        let _ = std::fs::remove_file(self.msgs_path(id));
        self.save_index();
    }
}

/// Результат обработки группового сообщения для уведомления UI.
#[derive(Default)]
pub struct GroupOutcome {
    pub gid: Option<Vec<u8>>,
    pub text: Option<String>,
    /// hex отправителя (для Message) или актора (для системного события).
    pub sender: Option<String>,
    /// Системное событие (смена состава).
    pub system: bool,
    pub joined: bool,
    /// Нас отозвали из группы (ADR-004) — UI должен убрать группу и уведомить.
    pub removed: bool,
    /// Имя группы изменилось (пришло от админа) — UI обновить название в списке.
    pub renamed: bool,
    /// Медиа-сообщение (M2): тип + ссылка на блоб. Пусто → не медиа.
    pub media: Option<GroupMediaRef>,
}

/// Ссылка на медиа-вложение группового сообщения (M2).
#[derive(Clone)]
pub struct GroupMediaRef {
    pub kind: String,
    pub blob_id: String,
    pub mime: String,
    pub name: String,
}
