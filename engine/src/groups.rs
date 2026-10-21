//! Менеджер групп (MLS) для Android. Живёт в ОДНОМ потоке (event_pump), т.к.
//! [`GroupChat`]/openmls — `!Send`. JNI-команды приходят сюда через канал,
//! групповые события мессенджера — оттуда же. Зеркалит десктопную логику:
//! админ-секвенсор, маршрутизация по group_id, персистентность в data_dir/groups.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::store::{Chat, MessageStore, StoredMessage};
use p2p_core::Identity;
use p2p_group::peek_group_id;
use p2p_groupchat::{GroupChat, MediaBody};
use p2p_messenger::Messenger;
use serde::{Deserialize, Serialize};

use crate::LockRecover;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Идентификатор группового сообщения: время в МИЛЛИсекундах. Уникальность нужна
/// только в пределах отправителя (ссылка — пара «автор + id»), а секунд не хватало
/// бы: два сообщения подряд получали бы один id и ответ указывал бы на оба.
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Строка истории группы В СТАРОМ формате (JSON-файл рядом с MLS-состоянием).
/// Нужна ровно для одного: перелить накопленное в общее хранилище сообщений и
/// забыть про файл. Новые записи сюда не попадают.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LegacyGMsg {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    reply_to: u64,
    #[serde(default)]
    reply_sender: String,
    text: String,
    mine: bool,
    time: i64,
    sender: String,
    #[serde(default)]
    system: bool,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    blob_id: String,
    #[serde(default)]
    mime: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    duration_ms: u32,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    thumb_id: String,
    #[serde(default)]
    downloaded: bool,
}

/// Строка списка групп для клиента.
#[derive(serde::Serialize)]
struct GroupRow {
    gid: String,
    name: String,
    members: usize,
    admin: bool,
}

/// Приглашение на диске (gid — ключ карты, поэтому в теле его нет).
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredInvite {
    name: String,
    inviter: String,
    /// Приглашение уже принято, ждём Welcome от пригласившего.
    #[serde(default)]
    accepted: bool,
}

/// Карточка приглашения в группу для клиента.
#[derive(serde::Serialize)]
struct GroupInviteDto {
    gid: String,
    name: String,
    inviter: String,
    /// «Вступить» нажато, ждём ответа пригласившего (он мог быть офлайн).
    waiting: bool,
}

struct Entry {
    chat: GroupChat,
    name: String,
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
    /// «Вступить» уже нажато: KeyPackage отправлен, ждём Welcome. Карточка при этом
    /// НЕ исчезает — иначе, если пригласивший в этот момент офлайн, вступление
    /// теряется молча и повторить его нечем.
    accepted: bool,
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
    /// KeyPackage текущего вступления. Повторяем ИМЕННО его: новый сделал бы
    /// Welcome на прежний нерасшифровываемым (pending-состояние у нас одно).
    pending_kp: Option<Vec<u8>>,
    /// От кого ждём Welcome и в какие группы согласны по нему войти. Welcome
    /// от кого-то другого или в другую группу не принимаем: иначе любой, кто
    /// дождался нашего KeyPackage, вписал бы нас в группу, куда мы не шли.
    pending_from: Option<([u8; 32], Vec<Vec<u8>>)>,
    /// Когда последний раз повторяли KeyPackage — троттлинг маячка.
    last_join_retry: Option<Instant>,
    identity: Identity,
    messenger: Arc<Messenger>,
    dir: PathBuf,
    /// Общее хранилище переписки: личные чаты, «Сохранённое» и группы в одной
    /// таблице. Своего списка сообщений у группы больше нет — иначе каждая фича
    /// чата пишется дважды, а копия данных о группах не знает.
    messages: Arc<Mutex<MessageStore>>,
    /// hex своего ключа: автор наших сообщений и признак «моё» при чтении.
    me_hex: String,
    /// Кто админ каждой группы (gid → ключ). Нужен, чтобы проверять права на
    /// действия, которые приходят ИЗВНЕ: закреп в шапку ставит только админ.
    admin_keys: HashMap<Vec<u8>, [u8; 32]>,
    /// Записанные, но ещё не отданные клиенту строки `(gid, сообщение)`. Каждая
    /// строка, которую этот слой кладёт в историю, уходит в UI событием целиком —
    /// своё, входящее и системное одной формой, без перечитывания ленты.
    fresh: Vec<(Vec<u8>, StoredMessage)>,
}

/// Как часто повторять принятое, но не завершённое вступление в группу.
/// Мессенджер сам повторяет неподтверждённое лишь `message_ttl` (5 минут),
/// дальше пакет тихо выбрасывается — а пригласивший может вернуться и через сутки.
const JOIN_RETRY_EVERY: Duration = Duration::from_secs(20);

impl GroupManager {
    pub fn new(
        identity: Identity,
        messenger: Arc<Messenger>,
        data_dir: PathBuf,
        messages: Arc<Mutex<MessageStore>>,
    ) -> Self {
        let dir = data_dir.join("groups");
        let me_hex = hex::encode(identity.public_key_bytes());
        let mut m = Self {
            groups: HashMap::new(),
            pending: None,
            pending_join_name: None,
            pending_invites: HashMap::new(),
            invites: HashMap::new(),
            resyncing: HashSet::new(),
            resync_last: HashMap::new(),
            pending_kp: None,
            pending_from: None,
            last_join_retry: None,
            identity,
            messenger,
            dir,
            messages,
            me_hex,
            admin_keys: HashMap::new(),
            fresh: Vec::new(),
        };
        m.load_admin_keys();
        m.load_all();
        m.load_pending_invites();
        m.load_sent_invites();
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
            let _ = p2p_core::fsutil::write_atomic(&self.dir.join("rekey.json"), j);
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
            let _ = p2p_core::fsutil::write_atomic(&self.dir.join("index.json"), j);
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
            let _ = p2p_core::fsutil::write_atomic(&self.dir.join("admins.json"), j);
        }
    }

    fn admin_keys_path(&self) -> PathBuf {
        self.dir.join("group_admins.json")
    }

    fn load_admin_keys(&mut self) {
        let Ok(bytes) = std::fs::read(self.admin_keys_path()) else { return };
        let Ok(map) = serde_json::from_slice::<HashMap<String, String>>(&bytes) else { return };
        for (gid_hex, key_hex) in map {
            let (Ok(gid), Some(key)) = (hex::decode(&gid_hex), crate::parse_key(&key_hex)) else {
                continue;
            };
            self.admin_keys.insert(gid, key);
        }
    }

    fn save_admin_keys(&self) {
        let map: HashMap<String, String> =
            self.admin_keys.iter().map(|(g, k)| (hex::encode(g), hex::encode(k))).collect();
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(j) = serde_json::to_vec(&map) {
            let _ = p2p_core::fsutil::write_atomic(&self.admin_keys_path(), j);
        }
    }

    /// Ключ админа группы (создателя). `None` — группа старая, админ неизвестен.
    pub fn admin_key(&self, gid: &[u8]) -> Option<[u8; 32]> {
        self.admin_keys.get(gid).copied()
    }

    fn remember_admin(&mut self, gid: &[u8], key: [u8; 32]) {
        self.admin_keys.insert(gid.to_vec(), key);
        self.save_admin_keys();
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

    /// Перелить историю группы из старого JSON-файла в общее хранилище и убрать
    /// файл (переименованием — на случай, если разбор чего-то не понял). Разово:
    /// на второй запуск файла уже нет.
    fn migrate_msgs_file(&self, id: &[u8]) {
        let path = self.msgs_path(id);
        let Ok(raw) = std::fs::read(&path) else { return };
        let Ok(bytes) = p2p_core::vault::open(&raw) else { return };
        // Битый файл — не повод ронять приложение на старте: пропускаем.
        let old: Vec<LegacyGMsg> = serde_json::from_slice(&bytes).unwrap_or_default();
        let store = self.messages.lock_safe();
        for m in &old {
            let _ = store.import_message(Chat::Group(id), &StoredMessage {
                id: m.id,
                sender: m.sender.clone(),
                text: m.text.clone(),
                mine: m.mine,
                time: m.time,
                status: String::new(),
                kind: if m.kind.is_empty() { "text".into() } else { m.kind.clone() },
                blob_id: m.blob_id.clone(),
                mime: m.mime.clone(),
                name: m.name.clone(),
                size: m.size,
                duration_ms: m.duration_ms,
                width: m.width,
                height: m.height,
                thumb_id: m.thumb_id.clone(),
                downloaded: m.downloaded,
                reply_to: m.reply_to,
                reply_sender: m.reply_sender.clone(),
                system: m.system,
                // Пересылок в старом формате не было — переносить нечего.
                fwd_from: String::new(),
                fwd_anon: false,
                edited_at: 0,
                row: 0,
            });
        }
        drop(store);
        log::info!("group {}: история переехала в общее хранилище ({} шт.)", hex::encode(id), old.len());
        let _ = std::fs::rename(&path, path.with_extension("msgs.bak"));
    }

    /// Записать своё/входящее сообщение группы в общее хранилище и отложить его
    /// для UI (см. [`Self::take_fresh`]).
    fn put(&mut self, gid: &[u8], msg: &StoredMessage) {
        if let Ok(row) = self.messages.lock_safe().append(Chat::Group(gid), msg) {
            self.fresh.push((gid.to_vec(), StoredMessage { row, ..msg.clone() }));
        }
    }

    /// Строки, записанные с прошлого вызова, — для событий `message`.
    pub fn take_fresh(&mut self) -> Vec<(Vec<u8>, StoredMessage)> {
        std::mem::take(&mut self.fresh)
    }

    /// Запомнить имя группы рядом с её перепиской. Нужно копии данных: MLS-состояние
    /// в неё не входит, и после восстановления группа живёт архивом — история есть,
    /// писать нельзя. Без имени она выглядела бы чатом с шестнадцатеричным id.
    fn remember_title(&self, gid: &[u8], name: &str) {
        let _ = self.messages.lock_safe().set_chat_title(Chat::Group(gid), name);
    }

    fn load_all(&mut self) {
        let Ok(bytes) = std::fs::read(self.dir.join("index.json")) else { return };
        let Ok(idx) = serde_json::from_slice::<HashMap<String, String>>(&bytes) else { return };
        let admins = self.load_admins();
        for (hexid, name) in idx {
            let Ok(raw) = hex::decode(&hexid) else { continue };
            let path = self.group_path(&raw);
            if let Ok(chat) = GroupChat::load(&path, &self.identity, self.messenger.clone()) {
                self.migrate_msgs_file(&raw);
                let admin = admins.contains(&raw);
                self.remember_title(&raw, &name);
                self.groups.insert(raw, Entry { chat, name, admin });
            }
        }
    }

    // ── Команды ─────────────────────────────────────────────────────────────

    /// Создать группу (мы — администратор). Возвращает group_id.
    pub fn create(&mut self, name: &str) -> Option<Vec<u8>> {
        let chat = GroupChat::create(&self.identity, self.messenger.clone()).ok()?;
        let id = chat.group_id()?;
        // Создатель = админ/секвенсор (ADR-004).
        self.groups.insert(id.clone(), Entry { chat, name: name.to_string(), admin: true });
        self.remember_title(&id, name);
        let me = self.identity.public_key_bytes();
        self.remember_admin(&id, me);
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
            self.save_sent_invites();
        }
    }

    fn sent_invites_path(&self) -> PathBuf {
        self.dir.join("sent_invites.json")
    }

    /// Кого и куда мы позвали. Живёт на диске, потому что приглашённый может нажать
    /// «Вступить» через сутки — а к тому времени мы успеем перезапуститься, и без
    /// этой записи его KeyPackage будет нечем сопоставить с группой (молча в никуда).
    fn save_sent_invites(&self) {
        let map: HashMap<String, String> =
            self.invites.iter().map(|(p, g)| (hex::encode(p), hex::encode(g))).collect();
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(j) = serde_json::to_vec(&map) {
            let _ = p2p_core::fsutil::write_atomic(&self.sent_invites_path(), j);
        }
    }

    fn load_sent_invites(&mut self) {
        let Ok(bytes) = std::fs::read(self.sent_invites_path()) else { return };
        let Ok(map) = serde_json::from_slice::<HashMap<String, String>>(&bytes) else { return };
        for (peer_hex, gid_hex) in map {
            let (Some(peer), Ok(gid)) = (crate::parse_key(&peer_hex), hex::decode(&gid_hex)) else {
                continue;
            };
            self.invites.insert(peer, gid);
        }
    }

    // ── Входящие приглашения (карточка на одобрение) ──────────────────────

    fn pending_invites_path(&self) -> PathBuf {
        self.dir.join("pending_invites.json")
    }

    fn save_pending_invites(&self) {
        // На диск — та же карточка, что уходит клиенту, только без gid (он в ключе).
        let map: HashMap<String, StoredInvite> = self
            .pending_invites
            .iter()
            .map(|(gid, inv)| {
                (
                    hex::encode(gid),
                    StoredInvite {
                        name: inv.name.clone(),
                        inviter: hex::encode(inv.inviter),
                        accepted: inv.accepted,
                    },
                )
            })
            .collect();
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(j) = serde_json::to_vec(&map) {
            let _ = p2p_core::fsutil::write_atomic(&self.pending_invites_path(), j);
        }
    }

    fn load_pending_invites(&mut self) {
        let Ok(bytes) = std::fs::read(self.pending_invites_path()) else { return };
        let Ok(map) = serde_json::from_slice::<HashMap<String, StoredInvite>>(&bytes) else {
            return;
        };
        for (gid_hex, v) in map {
            // Битую запись пропускаем молча: приглашение — не то, ради чего стоит
            // ронять загрузку остальных.
            let (Ok(gid), Some(inviter)) = (hex::decode(&gid_hex), crate::parse_key(&v.inviter)) else {
                continue;
            };
            self.pending_invites
                .insert(gid, PendingInvite { name: v.name, inviter, accepted: v.accepted });
        }
    }

    /// Пришло приглашение в группу. Если уже состоим — игнор (это не первый вход).
    /// Возвращает true, если запись добавлена (UI показать карточку).
    pub fn on_invite(&mut self, peer: [u8; 32], gid: Vec<u8>, name: String) -> bool {
        if self.groups.contains_key(&gid) {
            return false;
        }
        // Пригласивший мог послать карточку повторно (перезапуск, ретрай). Если мы
        // уже согласились — согласие сохраняем, иначе «ждём ответа» откатилось бы в
        // кнопки, а движок перестал бы повторять KeyPackage.
        let accepted = self.pending_invites.get(&gid).is_some_and(|i| i.accepted);
        self.pending_invites.insert(gid, PendingInvite { name, inviter: peer, accepted });
        self.save_pending_invites();
        true
    }

    /// Входящие приглашения как JSON [{gid, name, inviter}].
    pub fn invites_json(&self) -> String {
        let arr: Vec<GroupInviteDto> = self
            .pending_invites
            .iter()
            .map(|(gid, inv)| GroupInviteDto {
                gid: hex::encode(gid),
                name: inv.name.clone(),
                inviter: hex::encode(inv.inviter),
                waiting: inv.accepted,
            })
            .collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Принять приглашение: создаём pending-участника и шлём KeyPackage пригласившему
    /// (дальше как обычный join — админ добавит нас, придёт Welcome). Имя группы из
    /// инвайта запоминаем, чтобы показать сразу. Возвращает true при успехе.
    pub fn accept_invite(&mut self, gid: &[u8]) -> bool {
        let Some(inv) = self.pending_invites.get_mut(gid) else { return false };
        // Помечаем ПРИНЯТЫМ и оставляем в списке: карточка исчезнет только когда
        // придёт Welcome. Пригласивший может быть офлайн — тогда мы повторим
        // отправку KeyPackage, как только он появится (см. `on_peer_online`).
        inv.accepted = true;
        let (inviter, name) = (inv.inviter, inv.name.clone());
        self.save_pending_invites();
        if self.pending.is_some() {
            // Уже идёт другое вступление — этим займёмся, когда освободимся.
            return false;
        }
        self.send_key_package_to(inviter, gid.to_vec(), name)
    }

    /// Отправить свой KeyPackage пригласившему и начать ожидание Welcome.
    fn send_key_package_to(&mut self, inviter: [u8; 32], gid: Vec<u8>, name: String) -> bool {
        match GroupChat::pending(&self.identity, self.messenger.clone()) {
            Ok((pending, kp)) => {
                let _ = self.messenger.send_group_key_reply(&inviter, &kp);
                self.pending = Some(pending);
                self.pending_kp = Some(kp);
                self.pending_from = Some((inviter, vec![gid]));
                self.pending_join_name = Some(name);
                self.last_join_retry = Some(Instant::now());
                true
            }
            Err(_) => false,
        }
    }

    /// Повторить принятое, но не завершённое вступление — не чаще `JOIN_RETRY_EVERY`.
    /// Зовётся с маячка, поэтому НЕ зависит от presence: пригласивший может не быть
    /// нашим контактом, и тогда событий о его появлении нам никто не пришлёт.
    pub fn retry_pending_joins(&mut self) {
        if self.last_join_retry.is_some_and(|t| t.elapsed() < JOIN_RETRY_EVERY) {
            return;
        }
        let waiting = self
            .pending_invites
            .iter()
            .find(|(_, inv)| inv.accepted)
            .map(|(gid, inv)| (gid.clone(), inv.inviter, inv.name.clone()));
        let Some((gid, inviter, name)) = waiting else { return };
        self.last_join_retry = Some(Instant::now());
        match self.pending_kp.clone() {
            // Ожидание уже идёт — шлём тот же KeyPackage ещё раз.
            Some(kp) if self.pending.is_some() => {
                log::info!("group {}: повторяю KeyPackage пригласившему", hex::encode(&gid));
                let _ = self.messenger.send_group_key_reply(&inviter, &kp);
            }
            // Состояние потеряно (перезапуск приложения) — начинаем вступление заново.
            _ => {
                log::info!("group {}: возобновляю вступление после перезапуска", hex::encode(&gid));
                self.send_key_package_to(inviter, gid, name);
            }
        }
    }

    /// Пир появился в сети: если мы приняли его приглашение, но Welcome так и не
    /// пришёл (он был офлайн, когда мы нажимали «Вступить»), — повторяем KeyPackage.
    ///
    /// Без этого «Вступить» работало только при онлайн-пригласившем, а иначе
    /// вступление терялось навсегда: карточка снята, повторить нечем.
    pub fn on_peer_online(&mut self, peer: [u8; 32]) {
        let waiting =
            self.pending_invites.values().any(|inv| inv.accepted && inv.inviter == peer);
        if !waiting {
            return;
        }
        // Ждать до 20 с незачем — адресат заведомо в сети.
        self.last_join_retry = None;
        self.retry_pending_joins();
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
        self.send_text_reply(gid, text, 0, "")
    }

    /// Отправить текст ответом на сообщение `reply_to` автора `reply_sender` (hex).
    /// `reply_to = 0` — обычное сообщение.
    pub fn send_text_reply(
        &mut self,
        gid: &[u8],
        text: &str,
        reply_to: u64,
        reply_sender: &str,
    ) -> bool {
        self.send_text_full(gid, text, reply_to, reply_sender, "", false)
    }

    /// Общий путь: обычный текст, ответ и пересылка (`fwd_from` — hex автора
    /// исходного сообщения, пусто — своё).
    pub fn send_text_full(
        &mut self,
        gid: &[u8],
        text: &str,
        reply_to: u64,
        reply_sender: &str,
        fwd_from: &str,
        fwd_anon: bool,
    ) -> bool {
        let Some(e) = self.groups.get_mut(gid) else { return false };
        // id посеян временем в миллисекундах: он должен быть уникален только в
        // пределах отправителя (ссылка — пара «автор + id»).
        let id = now_ms();
        if e.chat.send_text_full(text, id, reply_to, reply_sender, fwd_from, fwd_anon).is_err() {
            return false;
        }
        self.put(gid, &StoredMessage {
            id,
            reply_to,
            reply_sender: reply_sender.to_string(),
            // kind заполняем явно: пустая строка — «неизвестно», и UI рисовал такое
            // сообщение дважды (как текст и как подпись к вложению).
            kind: "text".into(),
            fwd_from: fwd_from.to_string(),
            fwd_anon,
            text: text.to_string(), mine: true, time: now(),
            sender: self.me_hex.clone(), ..Default::default()
        });
        // КРИТИЧНО: отправка двигает MLS sending-ratchet — его надо ПЕРСИСТИТЬ, иначе
        // после рестарта/убийства приложения отправитель грузит старое состояние и
        // шлёт со «старого» поколения → получатель (он-то приём сохраняет) уже
        // удалил тот секрет (forward secrecy) → "secret deleted" → сообщения не доходят.
        self.save_group(gid);
        true
    }

    /// Отозвать СВОИ сообщения у участников («удалить у всех»). Возвращает, сколько
    /// просьб отправлено. Чужие молча пропускаются: отзывать чужое — не «удалить», а
    /// «переписать чужую историю». Сообщения без id (старый клиент) отозвать нельзя.
    pub fn retract(&mut self, gid: &[u8], ids: &[u64]) -> usize {
        let me = self.me_hex.clone();
        let mut sent = 0;
        let mut gone: Vec<(u64, String)> = Vec::new();
        {
            let store = self.messages.lock_safe();
            let Some(e) = self.groups.get_mut(gid) else { return 0 };
            for id in ids {
                if *id == 0 {
                    continue;
                }
                let mine = store.contains(Chat::Group(gid), &me, *id);
                if mine && e.chat.send_retract(*id).is_ok() {
                    sent += 1;
                    gone.push((*id, me.clone()));
                }
            }
        }
        if sent > 0 {
            let _ = self.messages.lock_safe().delete_many(Chat::Group(gid), &gone);
            // Отправка двигает MLS-ратчет — состояние обязано лечь на диск.
            self.save_group(gid);
        }
        sent
    }

    /// Исправить СВОЁ сообщение в группе. Возвращает `(автор, текст)` при успехе.
    /// Чужое править нельзя: подменять чужие слова — это не редактирование.
    pub fn edit(&mut self, gid: &[u8], id: u64, text: &str) -> Option<(String, String)> {
        let me = self.me_hex.clone();
        if !self.messages.lock_safe().contains(Chat::Group(gid), &me, id) {
            return None;
        }
        let e = self.groups.get_mut(gid)?;
        if e.chat.send_edit(id, text).is_err() {
            return None;
        }
        let _ = self.messages.lock_safe().edit_text(Chat::Group(gid), &me, id, text, now());
        // Рассылка двигает MLS-ратчет — состояние обязано лечь на диск.
        self.save_group(gid);
        Some((me, text.to_string()))
    }

    /// Закрепить/открепить сообщение в группе. Право — только у админа (ADR-004):
    /// иначе любой участник вешал бы своё сообщение в шапку всем остальным.
    /// Возвращает true, если закреп применён и разослан.
    pub fn pin(&mut self, gid: &[u8], id: u64, sender: &str, on: bool) -> bool {
        let Some(e) = self.groups.get_mut(gid) else { return false };
        if !e.admin {
            log::warn!("pin: не админ группы {} — отказано", hex::encode(gid));
            return false;
        }
        if e.chat.send_pin(id, sender, on).is_err() {
            return false;
        }
        self.apply_pin(gid, id, sender, on, &self.me_hex.clone());
        // Рассылка двигает MLS-ратчет — состояние обязано лечь на диск.
        self.save_group(gid);
        true
    }

    /// Положить закреп в общее хранилище (свой и входящий путь — один).
    fn apply_pin(&self, gid: &[u8], id: u64, sender: &str, on: bool, by: &str) {
        let store = self.messages.lock_safe();
        let _ = if on {
            store.pin(Chat::Group(gid), id, sender, now(), by)
        } else {
            store.unpin(Chat::Group(gid), id, sender).map(|_| ())
        };
    }

    /// Удалить сообщения ТОЛЬКО У СЕБЯ: пары `(id, автор-hex)`.
    pub fn delete_local(&mut self, gid: &[u8], refs: &[(u64, String)]) -> usize {
        if !self.groups.contains_key(gid) {
            return 0;
        }
        self.messages.lock_safe().delete_many(Chat::Group(gid), refs).unwrap_or(0)
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
        // Системная строка «участник удалён» (актор — удалённый). id — время в мс:
        // системные строки одного участника не должны затирать друг друга.
        self.put(gid, &StoredMessage {
            id: now_ms(), mine: true, time: now(),
            sender: hex::encode(peer), system: true, ..Default::default()
        });
        self.save_group(gid); // Commit сдвинул эпоху — персистим (инвариант MLS).
        true
    }

    /// Отправить медиа-вложение в группу (M2): MLS-сообщение со ссылкой на блоб.
    /// Байты блоба пушит вызывающий (engine) каждому участнику отдельно. Двигает
    /// sending-ratchet → save_group (как send_text). Возвращает участников для push.
    pub fn send_media(&mut self, gid: &[u8], media: &MediaBody) -> bool {
        let Some(e) = self.groups.get_mut(gid) else { return false };
        let id = now_ms();
        let media = &MediaBody { id, ..media.clone() };
        let (fwd_from, fwd_anon) = (media.fwd_from.clone(), media.fwd_anon);
        if e.chat.send_media(media).is_err() {
            return false;
        }
        self.put(gid, &StoredMessage {
            id,
            mine: true, time: now(), sender: self.me_hex.clone(),
            // Подпись живёт в `text`: у медиа это поле свободно, как и в DM.
            text: media.caption.clone(),
            kind: media.kind.clone(), blob_id: media.hash.clone(),
            mime: media.mime.clone(), name: media.name.clone(),
            size: media.size, duration_ms: media.duration_ms,
            width: media.width, height: media.height,
            thumb_id: media.thumb.clone(),
            fwd_from,
            fwd_anon,
            reply_to: media.reply_to,
            reply_sender: media.reply_sender.clone(),
            downloaded: true, // своё вложение уже у нас
            ..Default::default()
        });
        self.save_group(gid);
        true
    }

    /// Отметить вложение скачанным во ВСЕХ группах: блоб контент-адресный, один и
    /// тот же файл в разных чатах скачан разом (как `mark_downloaded` для DM).
    /// Возвращает gid'ы, где что-то изменилось — верхний слой обновит кэш истории.
    pub fn mark_downloaded(&mut self, blob_id_hex: &str) -> Vec<Vec<u8>> {
        // Саму пометку ставит хранилище — сразу во всех чатах, где есть этот блоб
        // (он контент-адресный). Здесь остаётся только сказать верхнему слою, какие
        // группы надо перерисовать.
        let store = self.messages.lock_safe();
        let _ = store.mark_downloaded(blob_id_hex);
        store
            .chats_with_blob(1, blob_id_hex)
            .into_iter()
            .filter(|gid| self.groups.contains_key(gid))
            .collect()
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
        let arr = self
            .groups
            .iter()
            .map(|(id, e)| GroupRow {
                gid: hex::encode(id),
                name: e.name.clone(),
                members: e.chat.member_count(),
                admin: e.admin,
            })
            .collect::<Vec<GroupRow>>();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Хвост истории группы (последние [`crate::HISTORY_PAGE`]), старые сверху.
    /// Схема строки — та же `StoredMessage`, что и в личном чате.
    pub fn history_json(&self, gid: &[u8]) -> String {
        self.history_page_json(gid, None)
    }

    /// Страница истории группы с курсором `(время, строка)` — как в личном чате.
    pub fn history_page_json(&self, gid: &[u8], before: Option<(i64, i64)>) -> String {
        if !self.groups.contains_key(gid) {
            return "[]".into();
        }
        let page = self.messages.lock_safe().history(Chat::Group(gid), before, crate::HISTORY_PAGE);
        serde_json::to_string(&page).unwrap_or_else(|_| "[]".into())
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

    /// Админ нашей группы просит свежий KeyPackage — это resync (ADR-004): он
    /// перевыпустит нас в группу. Отвечаем только админу и только для его групп:
    /// KeyPackage + принятый Welcome — это вступление, и проситься в него вправе
    /// лишь тот, кто и так ведёт нашу группу. Новые группы идут через карточку.
    pub fn on_key_request(&mut self, peer: [u8; 32]) {
        if self.pending.is_some() {
            return;
        }
        let gids: Vec<Vec<u8>> = self
            .admin_keys
            .iter()
            .filter(|(gid, admin)| **admin == peer && self.groups.contains_key(*gid))
            .map(|(gid, _)| gid.clone())
            .collect();
        if gids.is_empty() {
            log::warn!("запрос KeyPackage от {} — не админ наших групп, игнорирую", hex::encode(peer));
            return;
        }
        if let Ok((pending, kp)) = GroupChat::pending(&self.identity, self.messenger.clone()) {
            let _ = self.messenger.send_group_key_reply(&peer, &kp);
            self.pending = Some(pending);
            self.pending_from = Some((peer, gids));
        }
    }

    /// Контакт прислал KeyPackage — добавляем его в группу, куда приглашали.
    /// RESYNC (ADR-004): если это re-add отставшего, сперва убираем его устаревший
    /// leaf (Remove), затем добавляем свежим (Add) — участник входит с текущей эпохи.
    pub fn on_key_reply(&mut self, peer: [u8; 32], key_package: &[u8]) -> bool {
        let Some(id) = self.invites.remove(&peer) else { return false };
        self.save_sent_invites();
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
    pub fn on_group(&mut self, peer: [u8; 32], data: &[u8]) -> GroupOutcome {
        use p2p_groupchat::Inbound;
        // Известная группа? (Commit или прикладное сообщение)
        let known = peek_group_id(data).filter(|g| self.groups.contains_key(g));
        if let Some(gid) = known {
            let inbound = self.groups.get_mut(&gid).map(|e| e.chat.handle(data));
            let outcome = match inbound {
                Some(Ok(Inbound::Message {
                    sender, text, id, reply_to, reply_sender, fwd_from, fwd_anon,
                })) => {
                    let sender_hex = hex::encode(sender);
                    self.put(&gid, &StoredMessage {
                        id,
                        reply_to,
                        reply_sender,
                        fwd_from,
                        fwd_anon,
                        kind: "text".into(),
                        text: text.clone(), mine: false, time: now(),
                        sender: sender_hex.clone(), ..Default::default()
                    });
                    GroupOutcome {
                        gid: Some(gid.clone()), ..Default::default()
                    }
                }
                Some(Ok(Inbound::Edit { sender, target, text })) => {
                    let who = hex::encode(sender);
                    let changed = self
                        .messages
                        .lock_safe()
                        .edit_text(Chat::Group(&gid), &who, target, &text, now())
                        .unwrap_or(false);
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        edited: changed.then(|| (who, target, text)),
                        ..Default::default()
                    }
                }
                Some(Ok(Inbound::Pin { actor, target, sender, on })) => {
                    // Право проверяем ЗДЕСЬ: отправитель мог быть пропатчен, а в
                    // группе админ один и он известен из ростера.
                    let by_admin = self.admin_key(&gid) == Some(actor);
                    if by_admin {
                        self.apply_pin(&gid, target, &sender, on, &hex::encode(actor));
                    }
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        pinned: by_admin,
                        ..Default::default()
                    }
                }
                Some(Ok(Inbound::Retract { sender, target })) => {
                    // Автор берётся ИЗ MLS, а не из тела: иначе участник мог бы
                    // стереть чужое сообщение, подставив чужой ключ.
                    let who = hex::encode(sender);
                    let removed = if target == 0 {
                        // Сообщение без id (старый клиент) отозвать не за что.
                        0
                    } else {
                        self.messages
                            .lock_safe()
                            .delete_many(Chat::Group(&gid), &[(target, who)])
                            .unwrap_or(0)
                    };
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        retracted: removed > 0,
                        ..Default::default()
                    }
                }
                Some(Ok(Inbound::Media { sender, media })) => {
                    let sender_hex = hex::encode(sender);
                    self.put(&gid, &StoredMessage {
                        id: media.id,
                        mine: false, time: now(), sender: sender_hex.clone(),
                            text: media.caption.clone(),
                            kind: media.kind.clone(), blob_id: media.hash.clone(),
                            mime: media.mime.clone(), name: media.name.clone(),
                            size: media.size, duration_ms: media.duration_ms,
                            width: media.width, height: media.height,
                            thumb_id: media.thumb.clone(),
                        fwd_from: media.fwd_from.clone(),
                        fwd_anon: media.fwd_anon,
                        reply_to: media.reply_to,
                        reply_sender: media.reply_sender.clone(),
                        // Скачано ли — решит верхний слой (он знает хранилище блобов).
                        downloaded: false,
                        ..Default::default()
                    });
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        media: Some(media),
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
                        let name = self.groups.get(&gid).map(|e| e.name.clone()).unwrap_or_default();
                        self.remember_title(&gid, &name);
                        self.save_group(&gid); // персист имени в index.json
                    }
                    GroupOutcome { gid: Some(gid.clone()), renamed: changed, ..Default::default() }
                }
                Some(Ok(Inbound::Membership { actor })) => {
                    let actor_hex = hex::encode(actor);
                    self.put(&gid, &StoredMessage {
                        id: now_ms(), mine: false, time: now(),
                        sender: actor_hex.clone(), system: true,
                        ..Default::default()
                    });
                    GroupOutcome {
                        gid: Some(gid.clone()),
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
        // Иначе — возможно Welcome для ожидающего вступления. Только от того, кому
        // мы отдали KeyPackage: чужой Welcome даже не трогает pending-состояние.
        let Some(allowed) = self.pending_from.as_ref().filter(|(from, _)| *from == peer).map(|(_, g)| g.clone()) else {
            return GroupOutcome::default();
        };
        let joined_gid = self.pending.as_mut().and_then(|p| {
            let _ = p.handle(data);
            p.group_id()
        });
        if let Some(id) = joined_gid {
            if !allowed.contains(&id) {
                // Нужный отправитель, но не та группа. Pending уже израсходован этим
                // Welcome — сбрасываем вступление, маячок начнёт его заново.
                log::warn!("Welcome от {} в чужую группу {} — отвергнут", hex::encode(peer), hex::encode(&id));
                self.pending = None;
                self.pending_kp = None;
                self.pending_from = None;
                return GroupOutcome::default();
            }
            if let Some(chat) = self.pending.take() {
                self.pending_kp = None;
                self.pending_from = None;
                self.last_join_retry = None;
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
                // Пригласивший и есть админ: приглашать и добавлять в MLS вправе
                // только он (ADR-004).
                if let Some(inv) = self.pending_invites.get(&id) {
                    let inviter = inv.inviter;
                    self.remember_admin(&id, inviter);
                }
                self.pending_invites.remove(&id);
                self.save_pending_invites();
                self.remember_title(&id, &name);
                self.groups.insert(id.clone(), Entry { chat, name, admin: false });
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
        self.save_sent_invites();
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
        self.admin_keys.remove(id);
        self.save_admin_keys();
        let _ = std::fs::remove_file(self.group_path(id));
        let _ = std::fs::remove_file(self.msgs_path(id));
        {
            let store = self.messages.lock_safe();
            let _ = store.clear(Chat::Group(id));
            let _ = store.forget_chat_title(Chat::Group(id));
            let _ = store.clear_pins(Chat::Group(id));
        }
        self.save_index();
    }
}

/// Результат обработки группового сообщения для уведомления UI.
#[derive(Default)]
pub struct GroupOutcome {
    pub gid: Option<Vec<u8>>,
    /// Сообщение исправлено автором: `(автор, id, новый текст)`.
    pub edited: Option<(String, u64, String)>,
    /// Список закреплённых изменился — UI перечитать шапку чата.
    pub pinned: bool,
    pub joined: bool,
    /// Нас отозвали из группы (ADR-004) — UI должен убрать группу и уведомить.
    pub removed: bool,
    /// Имя группы изменилось (пришло от админа) — UI обновить название в списке.
    pub renamed: bool,
    /// Медиа-сообщение (M2): ссылка на блоб + метаданные карточки. Пусто → не медиа.
    pub media: Option<MediaBody>,
    /// Участник отозвал своё сообщение — строка исчезла из истории, UI перечитать.
    pub retracted: bool,
}
