//! Менеджер групп (MLS) для Android. Живёт в ОДНОМ потоке (event_pump), т.к.
//! [`GroupChat`]/openmls — `!Send`. JNI-команды приходят сюда через канал,
//! групповые события мессенджера — оттуда же. Зеркалит десктопную логику:
//! админ-секвенсор, маршрутизация по group_id, персистентность в data_dir/groups.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_group::peek_group_id;
use p2p_groupchat::GroupChat;
use p2p_messenger::Messenger;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub struct GMsg {
    pub text: String,
    pub mine: bool,
    pub time: i64,
    /// hex ed25519-pubkey отправителя (для имени/цвета/аватарки в UI).
    pub sender: String,
    /// Системное событие (смена состава) — рендерится отдельной строкой.
    pub system: bool,
}

struct Entry {
    chat: GroupChat,
    name: String,
    messages: Vec<GMsg>,
}

pub struct GroupManager {
    groups: HashMap<Vec<u8>, Entry>,
    pending: Option<GroupChat>,
    invites: HashMap<[u8; 32], Vec<u8>>,
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
            invites: HashMap::new(),
            identity,
            messenger,
            dir,
        };
        m.load_all();
        m
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
                    text: v["text"].as_str()?.to_string(),
                    time: v["time"].as_i64().unwrap_or(0),
                    sender: v["sender"].as_str().unwrap_or("").to_string(),
                    system: v["system"].as_bool().unwrap_or(false),
                })
            })
            .collect()
    }

    fn load_all(&mut self) {
        let Ok(bytes) = std::fs::read(self.dir.join("index.json")) else { return };
        let Ok(idx) = serde_json::from_slice::<HashMap<String, String>>(&bytes) else { return };
        for (hexid, name) in idx {
            let Ok(raw) = hex::decode(&hexid) else { continue };
            let path = self.group_path(&raw);
            if let Ok(chat) = GroupChat::load(&path, &self.identity, self.messenger.clone()) {
                for m in chat.members() {
                    crate::spawn_connect(self.messenger.clone(), m);
                }
                let messages = self.load_msgs(&raw);
                self.groups.insert(raw, Entry { chat, name, messages });
            }
        }
    }

    // ── Команды ─────────────────────────────────────────────────────────────

    /// Создать группу (мы — администратор). Возвращает group_id.
    pub fn create(&mut self, name: &str) -> Option<Vec<u8>> {
        let chat = GroupChat::create(&self.identity, self.messenger.clone()).ok()?;
        let id = chat.group_id()?;
        self.groups.insert(id.clone(), Entry { chat, name: name.to_string(), messages: Vec::new() });
        self.save_group(&id);
        Some(id)
    }

    /// Пригласить контакт в группу (просим его KeyPackage).
    pub fn invite(&mut self, gid: &[u8], peer: [u8; 32]) {
        let sent = if let Some(e) = self.groups.get(gid) {
            e.chat.invite(peer);
            true
        } else {
            false
        };
        if sent {
            self.invites.insert(peer, gid.to_vec());
        }
    }

    /// Отправить текст в группу. Возвращает true при успехе.
    pub fn send_text(&mut self, gid: &[u8], text: &str) -> bool {
        let Some(e) = self.groups.get_mut(gid) else { return false };
        if e.chat.send_text(text).is_err() {
            return false;
        }
        let me = hex::encode(self.identity.public_key_bytes());
        e.messages.push(GMsg { text: text.to_string(), mine: true, time: now(), sender: me, system: false });
        self.save_msgs(gid);
        // КРИТИЧНО: отправка двигает MLS sending-ratchet — его надо ПЕРСИСТИТЬ, иначе
        // после рестарта/убийства приложения отправитель грузит старое состояние и
        // шлёт со «старого» поколения → получатель (он-то приём сохраняет) уже
        // удалил тот секрет (forward secrecy) → "secret deleted" → сообщения не доходят.
        self.save_group(gid);
        true
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
    pub fn on_key_reply(&mut self, peer: [u8; 32], key_package: &[u8]) -> bool {
        let Some(id) = self.invites.remove(&peer) else { return false };
        let added = self
            .groups
            .get_mut(&id)
            .map(|e| e.chat.add_member(peer, key_package).is_ok())
            .unwrap_or(false);
        if added {
            crate::spawn_connect(self.messenger.clone(), peer);
            self.save_group(&id);
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
                            sender: sender_hex.clone(), system: false,
                        });
                    }
                    self.save_msgs(&gid);
                    GroupOutcome {
                        gid: Some(gid.clone()), text: Some(text),
                        sender: Some(sender_hex), system: false, joined: false,
                    }
                }
                Some(Ok(Inbound::Membership { actor })) => {
                    let actor_hex = hex::encode(actor);
                    if let Some(e) = self.groups.get_mut(&gid) {
                        e.messages.push(GMsg {
                            text: String::new(), mine: false, time: now(),
                            sender: actor_hex.clone(), system: true,
                        });
                    }
                    self.save_msgs(&gid);
                    GroupOutcome {
                        gid: Some(gid.clone()), text: None,
                        sender: Some(actor_hex), system: true, joined: false,
                    }
                }
                Some(Err(e)) => {
                    log::warn!("group {} MLS отверг сообщение: {:?}", hex::encode(&gid), e);
                    GroupOutcome { gid: Some(gid.clone()), text: None, sender: None, system: false, joined: false }
                }
                _ => GroupOutcome { gid: Some(gid.clone()), text: None, sender: None, system: false, joined: false },
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
                self.groups.insert(id.clone(), Entry { chat, name: "Группа".into(), messages: Vec::new() });
                self.save_group(&id);
                return GroupOutcome { gid: Some(id), text: None, sender: None, system: false, joined: true };
            }
        }
        GroupOutcome { gid: None, text: None, sender: None, system: false, joined: false }
    }
}

/// Результат обработки группового сообщения для уведомления UI.
pub struct GroupOutcome {
    pub gid: Option<Vec<u8>>,
    pub text: Option<String>,
    /// hex отправителя (для Message) или актора (для системного события).
    pub sender: Option<String>,
    /// Системное событие (смена состава).
    pub system: bool,
    pub joined: bool,
}
