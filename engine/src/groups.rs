//! Менеджер групп (MLS) для Android. Живёт в ОДНОМ потоке (event_pump), т.к.
//! [`GroupChat`]/openmls — `!Send`. JNI-команды приходят сюда через канал,
//! групповые события мессенджера — оттуда же. Зеркалит десктопную логику:
//! админ-секвенсор, маршрутизация по group_id, персистентность в data_dir/groups.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::mention::Mention;
use peerage_core::store::{Chat, GroupInviteRecord, GroupStore, MessageStore, StoredMessage};
use peerage_core::Identity;
use peerage_group::peek_group_id;
use peerage_groupchat::{GroupChat, MediaBody};
use peerage_messenger::Messenger;

use crate::LockRecover;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Идентификатор группового сообщения: время в МИЛЛИсекундах. Уникальность нужна
/// только в пределах отправителя (ссылка — пара «автор + id»), а секунд не хватало
/// бы: два сообщения подряд получали бы один id и ответ указывал бы на оба.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Строка списка групп для клиента.
#[derive(serde::Serialize)]
struct GroupRow {
    gid: String,
    name: String,
    members: usize,
    admin: bool,
    /// Ключ админа группы (hex; пусто — неизвестен): метка «админ» у участника.
    admin_key: String,
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
    /// Кого и в какую группу мы позвали (пара — один человек может быть позван в
    /// несколько групп сразу). Ждём от него KeyPackage с этим gid.
    invites: HashSet<([u8; 32], Vec<u8>)>,
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
    /// Метаданные групп и приглашений (SQLite). Всё, кроме MLS-состояния.
    meta: GroupStore,
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
        meta: GroupStore,
    ) -> Self {
        let dir = data_dir.join("groups");
        let me_hex = hex::encode(identity.public_key_bytes());
        let mut m = Self {
            groups: HashMap::new(),
            pending: None,
            pending_join_name: None,
            pending_invites: HashMap::new(),
            invites: HashSet::new(),
            resyncing: HashSet::new(),
            resync_last: HashMap::new(),
            pending_kp: None,
            pending_from: None,
            last_join_retry: None,
            identity,
            messenger,
            dir,
            messages,
            meta,
            me_hex,
            admin_keys: HashMap::new(),
            fresh: Vec::new(),
        };
        m.load_all();
        m.maybe_rekey_admin_groups();
        m
    }

    /// Катящийся PCS (ADR-004 п.7): при старте админ ротирует ключ тех своих групп,
    /// где прошло >суток с прошлой ротации. Fire-and-forget; отставшие вылечатся
    /// resync'ом. Дёшево (раз в сутки на группу) и не завязано на таймер-поток.
    fn maybe_rekey_admin_groups(&mut self) {
        let now = now();
        let stale: Vec<Vec<u8>> = self
            .meta
            .groups()
            .into_iter()
            .filter(|g| now - g.rekeyed_at >= REKEY_INTERVAL_SECS)
            .map(|g| g.gid)
            .filter(|gid| self.groups.get(gid).is_some_and(|e| e.admin))
            .collect();
        for gid in stale {
            if self.rekey(&gid) {
                let _ = self.meta.set_rekeyed(&gid, now);
            }
        }
    }

    fn group_path(&self, id: &[u8]) -> PathBuf {
        self.dir.join(format!("{}.group", hex::encode(id)))
    }

    /// Ключ админа группы (создателя). `None` — группа старая, админ неизвестен.
    pub fn admin_key(&self, gid: &[u8]) -> Option<[u8; 32]> {
        self.admin_keys.get(gid).copied()
    }

    /// Запомнить админа группы. На диск попадёт с ближайшим `save_group`.
    fn remember_admin(&mut self, gid: &[u8], key: [u8; 32]) {
        self.admin_keys.insert(gid.to_vec(), key);
    }

    /// Сохранить группу: MLS-состояние — в свой файл, остальное — в базу.
    fn save_group(&self, id: &[u8]) {
        let Some(e) = self.groups.get(id) else { return };
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = e.chat.save(&self.group_path(id));
        let _ = self
            .meta
            .save_group(id, &e.name, e.admin, self.admin_keys.get(id));
    }

    /// Записать своё/входящее сообщение группы в общее хранилище и отложить его
    /// для UI (см. [`Self::take_fresh`]).
    fn put(&mut self, gid: &[u8], msg: &StoredMessage) {
        if let Ok(row) = self.messages.lock_safe().append(Chat::Group(gid), msg) {
            self.fresh
                .push((gid.to_vec(), StoredMessage { row, ..msg.clone() }));
        }
    }

    /// Системная строка группы (журнал движка, 2.8а): вид, о ком, параметр. Слова
    /// строки собирает клиент — здесь только факт. id — время в мс: события одного
    /// человека не должны затирать друг друга.
    fn put_event(&mut self, gid: &[u8], kind: &str, who: [u8; 32], param: String) {
        let me = self.identity.public_key_bytes();
        self.put(
            gid,
            &StoredMessage {
                id: now_ms(),
                mine: who == me,
                time: now(),
                sender: hex::encode(who),
                kind: kind.into(),
                text: param,
                system: true,
                ..Default::default()
            },
        );
    }

    /// Строки, записанные с прошлого вызова, — для событий `message`.
    pub fn take_fresh(&mut self) -> Vec<(Vec<u8>, StoredMessage)> {
        std::mem::take(&mut self.fresh)
    }

    /// Запомнить имя группы рядом с её перепиской. Нужно копии данных: MLS-состояние
    /// в неё не входит, и после восстановления группа живёт архивом — история есть,
    /// писать нельзя. Без имени она выглядела бы чатом с шестнадцатеричным id.
    fn remember_title(&self, gid: &[u8], name: &str) {
        let _ = self
            .messages
            .lock_safe()
            .set_chat_title(Chat::Group(gid), name);
    }

    fn load_all(&mut self) {
        for g in self.meta.groups() {
            if let Some(k) = g.admin_key {
                self.admin_keys.insert(g.gid.clone(), k);
            }
            let path = self.group_path(&g.gid);
            let Ok(chat) = GroupChat::load(&path, &self.identity, self.messenger.clone()) else {
                continue;
            };
            // Файл обязан быть состоянием ИМЕННО этой группы: подложенное под её имя
            // состояние другой группы расшифровалось бы (id в AAD не входит) и
            // подменило бы собой эту (R-038).
            if chat.group_id().as_deref() != Some(g.gid.as_slice()) {
                log::warn!(
                    "group {}: файл состояния от другой группы — пропущен",
                    hex::encode(&g.gid)
                );
                continue;
            }
            self.remember_title(&g.gid, &g.name);
            self.groups.insert(
                g.gid,
                Entry {
                    chat,
                    name: g.name,
                    admin: g.admin,
                },
            );
        }
        self.invites = self.meta.sent_invites().into_iter().collect();
        for inv in self.meta.invites() {
            self.pending_invites.insert(
                inv.gid,
                PendingInvite {
                    name: inv.name,
                    inviter: inv.inviter,
                    accepted: inv.accepted,
                },
            );
        }
    }

    // ── Команды ─────────────────────────────────────────────────────────────

    /// Создать группу (мы — администратор). Возвращает group_id.
    pub fn create(&mut self, name: &str) -> Option<Vec<u8>> {
        let chat = GroupChat::create(&self.identity, self.messenger.clone()).ok()?;
        let id = chat.group_id()?;
        // Создатель = админ/секвенсор (ADR-004).
        self.groups.insert(
            id.clone(),
            Entry {
                chat,
                name: name.to_string(),
                admin: true,
            },
        );
        self.remember_title(&id, name);
        let me = self.identity.public_key_bytes();
        self.remember_admin(&id, me);
        self.save_group(&id);
        self.put_event(&id, "created", me, String::new());
        Some(id)
    }

    /// Пригласить контакт в группу карточкой-сообщением (решение владельца 2026-06-28):
    /// шлём `GroupInvite` с именем группы; получатель сам решает вступать. Запоминаем
    /// пару (peer, gid): его KeyPackage придёт с этим gid.
    pub fn invite(&mut self, gid: &[u8], peer: [u8; 32]) {
        if let Some(e) = self.groups.get(gid) {
            let _ = self.messenger.send_group_invite(&peer, gid, &e.name);
            self.invites.insert((peer, gid.to_vec()));
            let _ = self.meta.add_sent_invite(&peer, gid);
        }
    }

    // ── Входящие приглашения (карточка на одобрение) ──────────────────────

    /// Записать входящее приглашение как есть в памяти.
    fn save_invite(&self, gid: &[u8]) {
        if let Some(inv) = self.pending_invites.get(gid) {
            let _ = self.meta.put_invite(&GroupInviteRecord {
                gid: gid.to_vec(),
                name: inv.name.clone(),
                inviter: inv.inviter,
                accepted: inv.accepted,
            });
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
        self.pending_invites.insert(
            gid.clone(),
            PendingInvite {
                name,
                inviter: peer,
                accepted,
            },
        );
        self.save_invite(&gid);
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
        let Some(inv) = self.pending_invites.get_mut(gid) else {
            return false;
        };
        // Помечаем ПРИНЯТЫМ и оставляем в списке: карточка исчезнет только когда
        // придёт Welcome. Пригласивший может быть офлайн — тогда мы повторим
        // отправку KeyPackage, как только он появится (см. `on_peer_online`).
        inv.accepted = true;
        let (inviter, name) = (inv.inviter, inv.name.clone());
        self.save_invite(gid);
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
                let _ = self.messenger.send_group_key_reply(&inviter, &gid, &kp);
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
        if self
            .last_join_retry
            .is_some_and(|t| t.elapsed() < JOIN_RETRY_EVERY)
        {
            return;
        }
        let waiting = self
            .pending_invites
            .iter()
            .find(|(_, inv)| inv.accepted)
            .map(|(gid, inv)| (gid.clone(), inv.inviter, inv.name.clone()));
        let Some((gid, inviter, name)) = waiting else {
            return;
        };
        self.last_join_retry = Some(Instant::now());
        match self.pending_kp.clone() {
            // Ожидание уже идёт — шлём тот же KeyPackage ещё раз.
            Some(kp) if self.pending.is_some() => {
                log::info!(
                    "group {}: повторяю KeyPackage пригласившему",
                    hex::encode(&gid)
                );
                let _ = self.messenger.send_group_key_reply(&inviter, &gid, &kp);
            }
            // Состояние потеряно (перезапуск приложения) — начинаем вступление заново.
            _ => {
                log::info!(
                    "group {}: возобновляю вступление после перезапуска",
                    hex::encode(&gid)
                );
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
        let waiting = self
            .pending_invites
            .values()
            .any(|inv| inv.accepted && inv.inviter == peer);
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
            let _ = self.meta.remove_invite(gid);
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
        self.send_text_full(gid, text, reply_to, reply_sender, "", false, Vec::new())
    }

    /// Общий путь: обычный текст, ответ и пересылка (`fwd_from` — hex автора
    /// исходного сообщения, пусто — своё) с упоминаниями.
    #[allow(clippy::too_many_arguments)]
    pub fn send_text_full(
        &mut self,
        gid: &[u8],
        text: &str,
        reply_to: u64,
        reply_sender: &str,
        fwd_from: &str,
        fwd_anon: bool,
        mentions: Vec<Mention>,
    ) -> bool {
        let Some(e) = self.groups.get_mut(gid) else {
            return false;
        };
        // id посеян временем в миллисекундах: он должен быть уникален только в
        // пределах отправителя (ссылка — пара «автор + id»).
        let id = now_ms();
        // У себя храним ровно то, что ушло в сеть — чистка та же, что в слое групп.
        let mentions = peerage_core::mention::clean(text, mentions);
        let sent = e.chat.send_text_full(
            text,
            id,
            reply_to,
            reply_sender,
            fwd_from,
            fwd_anon,
            &mentions,
        );
        if sent.is_err() {
            return false;
        }
        self.put(
            gid,
            &StoredMessage {
                id,
                reply_to,
                reply_sender: reply_sender.to_string(),
                // kind заполняем явно: пустая строка — «неизвестно», и UI рисовал такое
                // сообщение дважды (как текст и как подпись к вложению).
                kind: "text".into(),
                fwd_from: fwd_from.to_string(),
                fwd_anon,
                mentions,
                text: text.to_string(),
                mine: true,
                time: now(),
                sender: self.me_hex.clone(),
                ..Default::default()
            },
        );
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
            let Some(e) = self.groups.get_mut(gid) else {
                return 0;
            };
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
            let _ = self
                .messages
                .lock_safe()
                .retract_many(Chat::Group(gid), &gone);
            // Отправка двигает MLS-ратчет — состояние обязано лечь на диск.
            self.save_group(gid);
        }
        sent
    }

    /// Исправить СВОЁ сообщение в группе. Возвращает `(автор, текст)` при успехе.
    /// Чужое править нельзя: подменять чужие слова — это не редактирование.
    pub fn edit(
        &mut self,
        gid: &[u8],
        id: u64,
        text: &str,
        mentions: &[Mention],
    ) -> Option<(String, String, Vec<Mention>)> {
        let me = self.me_hex.clone();
        if !self
            .messages
            .lock_safe()
            .contains(Chat::Group(gid), &me, id)
        {
            return None;
        }
        let e = self.groups.get_mut(gid)?;
        let mentions = peerage_core::mention::clean(text, mentions.to_vec());
        if e.chat.send_edit(id, text, &mentions).is_err() {
            return None;
        }
        let _ =
            self.messages
                .lock_safe()
                .edit_text(Chat::Group(gid), &me, id, text, &mentions, now());
        // Рассылка двигает MLS-ратчет — состояние обязано лечь на диск.
        self.save_group(gid);
        Some((me, text.to_string(), mentions))
    }

    /// Закрепить/открепить сообщение в группе. Право — только у админа (ADR-004):
    /// иначе любой участник вешал бы своё сообщение в шапку всем остальным.
    /// Возвращает true, если закреп применён и разослан.
    pub fn pin(&mut self, gid: &[u8], id: u64, sender: &str, on: bool) -> bool {
        let Some(e) = self.groups.get_mut(gid) else {
            return false;
        };
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

    /// Поставить, заменить или снять (пустой `emoji`) свою реакцию. Реагировать может
    /// любой участник. Двигает MLS-ратчет → save_group. true — набор изменился.
    pub fn react(&mut self, gid: &[u8], id: u64, sender: &str, emoji: &str, at: i64) -> bool {
        let Some(e) = self.groups.get_mut(gid) else {
            return false;
        };
        if e.chat.send_react(id, sender, emoji, at).is_err() {
            return false;
        }
        // Рассылка двигает MLS-ратчет — состояние обязано лечь на диск.
        self.save_group(gid);
        let me = self.me_hex.clone();
        self.messages
            .lock_safe()
            .react(Chat::Group(gid), id, sender, &me, emoji, at)
            .unwrap_or(false)
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
        self.messages
            .lock_safe()
            .delete_many(Chat::Group(gid), refs)
            .unwrap_or(0)
    }

    /// Отозвать участника из группы (ADR-004). Только админ. Двигает MLS-состояние
    /// → save_group. Возвращает true при успехе. Commit ушёл всем (вкл. удаляемого).
    pub fn remove_member(&mut self, gid: &[u8], peer: [u8; 32]) -> bool {
        let Some(e) = self.groups.get_mut(gid) else {
            return false;
        };
        if !e.admin {
            log::warn!(
                "remove_member: не админ группы {} — отказано",
                hex::encode(gid)
            );
            return false;
        }
        if e.chat.remove_member(peer).is_err() {
            return false;
        }
        self.put_event(gid, "left", peer, String::new());
        self.save_group(gid); // Commit сдвинул эпоху — персистим (инвариант MLS).
        true
    }

    /// Отправить медиа-вложение в группу (M2): MLS-сообщение со ссылкой на блоб.
    /// Байты блоба пушит вызывающий (engine) каждому участнику отдельно. Двигает
    /// sending-ratchet → save_group (как send_text). Возвращает участников для push.
    pub fn send_media(&mut self, gid: &[u8], media: &MediaBody) -> bool {
        let Some(e) = self.groups.get_mut(gid) else {
            return false;
        };
        let id = now_ms();
        let media = &MediaBody {
            id,
            ..media.clone()
        };
        let (fwd_from, fwd_anon) = (media.fwd_from.clone(), media.fwd_anon);
        if e.chat.send_media(media).is_err() {
            return false;
        }
        self.put(
            gid,
            &StoredMessage {
                id,
                mine: true,
                time: now(),
                sender: self.me_hex.clone(),
                // Подпись живёт в `text`: у медиа это поле свободно, как и в DM.
                text: media.caption.clone(),
                kind: media.kind.clone(),
                blob_id: media.hash.clone(),
                mime: media.mime.clone(),
                name: media.name.clone(),
                size: media.size,
                duration_ms: media.duration_ms,
                width: media.width,
                height: media.height,
                thumb_id: media.thumb.clone(),
                fwd_from,
                fwd_anon,
                reply_to: media.reply_to,
                reply_sender: media.reply_sender.clone(),
                downloaded: true, // своё вложение уже у нас
                ..Default::default()
            },
        );
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
        let Some(e) = self.groups.get_mut(gid) else {
            return false;
        };
        if e.chat.rekey_self().is_err() {
            return false;
        }
        self.save_group(gid);
        true
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
                admin_key: self.admin_keys.get(id).map(hex::encode).unwrap_or_default(),
            })
            .collect::<Vec<GroupRow>>();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Хвост истории группы (последние [`crate::HISTORY_PAGE`]), старые сверху.
    /// Клиент читает историю через движок; это — для тестов группового слоя.
    pub fn history_json(&self, gid: &[u8]) -> String {
        if !self.groups.contains_key(gid) {
            return "[]".into();
        }
        let page = self
            .messages
            .lock_safe()
            .history(Chat::Group(gid), None, crate::HISTORY_PAGE);
        serde_json::to_string(&page).unwrap_or_else(|_| "[]".into())
    }

    /// Ключи участников (hex) — для группового звонка.
    pub fn members_json(&self, gid: &[u8]) -> String {
        let Some(e) = self.groups.get(gid) else {
            return "[]".into();
        };
        let arr: Vec<String> = e.chat.members().iter().map(hex::encode).collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Кого позвали в группу и кто ещё не вступил — метка «приглашён · ждёт ответа»
    /// (UI 3.4). JSON-массив hex-ключей; участники сюда не попадают, даже если их
    /// перевыпускают (resync держит их в `invites`, пока не придёт KeyPackage).
    pub fn invited_json(&self, gid: &[u8]) -> String {
        let members = self.members(gid);
        let mut arr: Vec<String> = self
            .invites
            .iter()
            .filter(|(peer, g)| g.as_slice() == gid && !members.contains(peer))
            .map(|(peer, _)| hex::encode(peer))
            .collect();
        arr.sort();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Ключи участников группы (для маячка присутствия комнаты).
    pub fn members(&self, gid: &[u8]) -> Vec<[u8; 32]> {
        self.groups
            .get(gid)
            .map(|e| e.chat.members())
            .unwrap_or_default()
    }

    // ── События мессенджера ───────────────────────────────────────────────

    /// Админ нашей группы просит свежий KeyPackage — это resync (ADR-004): он
    /// перевыпустит нас в группу. Отвечаем только админу и только для его групп:
    /// KeyPackage + принятый Welcome — это вступление, и проситься в него вправе
    /// лишь тот, кто и так ведёт нашу группу. Новые группы идут через карточку.
    pub fn on_key_request(&mut self, peer: [u8; 32], gid: &[u8]) {
        if self.pending.is_some() {
            return;
        }
        let ours = self.admin_keys.get(gid) == Some(&peer) && self.groups.contains_key(gid);
        if !ours {
            log::warn!(
                "запрос KeyPackage от {} — не админ этой нашей группы, игнорирую",
                hex::encode(peer)
            );
            return;
        }
        if let Ok((pending, kp)) = GroupChat::pending(&self.identity, self.messenger.clone()) {
            let _ = self.messenger.send_group_key_reply(&peer, gid, &kp);
            self.pending = Some(pending);
            self.pending_from = Some((peer, vec![gid.to_vec()]));
        }
    }

    /// Контакт прислал KeyPackage — добавляем его в группу, куда приглашали.
    /// RESYNC (ADR-004): если это re-add отставшего, сперва убираем его устаревший
    /// leaf (Remove), затем добавляем свежим (Add) — участник входит с текущей эпохи.
    pub fn on_key_reply(&mut self, peer: [u8; 32], gid: &[u8], key_package: &[u8]) -> bool {
        // Только туда, куда звали: KeyPackage с чужим gid — не приглашение.
        if !self.invites.remove(&(peer, gid.to_vec())) {
            return false;
        }
        let id = gid.to_vec();
        let _ = self.meta.remove_sent_invite(&peer, gid);
        let resync = self.resyncing.remove(&peer);
        // Перевыпуск отставшего — один коммит Remove+Add: остальным это смена эпохи,
        // а не «вышел и вошёл».
        let added = self
            .groups
            .get_mut(&id)
            .map(|e| {
                if resync {
                    e.chat.readd_member(peer, key_package).is_ok()
                } else {
                    e.chat.add_member(peer, key_package).is_ok()
                }
            })
            .unwrap_or(false);
        if added {
            self.save_group(&id);
            if !resync {
                let me = hex::encode(self.identity.public_key_bytes());
                self.put_event(&id, "joined", peer, me);
            }
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
        use peerage_groupchat::Inbound;
        // Известная группа? (Commit или прикладное сообщение)
        let known = peek_group_id(data).filter(|g| self.groups.contains_key(g));
        if let Some(gid) = known {
            let inbound = self.groups.get_mut(&gid).map(|e| e.chat.handle(data));
            let outcome = match inbound {
                Some(Ok(Inbound::Message {
                    sender,
                    text,
                    id,
                    reply_to,
                    reply_sender,
                    fwd_from,
                    fwd_anon,
                    mentions,
                })) => {
                    let sender_hex = hex::encode(sender);
                    self.put(
                        &gid,
                        &StoredMessage {
                            id,
                            reply_to,
                            reply_sender,
                            fwd_from,
                            fwd_anon,
                            mentions,
                            kind: "text".into(),
                            text: text.clone(),
                            mine: false,
                            time: now(),
                            sender: sender_hex.clone(),
                            ..Default::default()
                        },
                    );
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        ..Default::default()
                    }
                }
                Some(Ok(Inbound::Edit {
                    sender,
                    target,
                    text,
                    mentions,
                })) => {
                    let who = hex::encode(sender);
                    let changed = self
                        .messages
                        .lock_safe()
                        .edit_text(Chat::Group(&gid), &who, target, &text, &mentions, now())
                        .unwrap_or(false);
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        edited: changed.then(|| (who, target, text, mentions)),
                        ..Default::default()
                    }
                }
                Some(Ok(Inbound::Pin {
                    actor,
                    target,
                    sender,
                    on,
                })) => {
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
                Some(Ok(Inbound::React {
                    actor,
                    target,
                    sender,
                    emoji,
                    at,
                })) => {
                    // Кто поставил — из MLS (actor), а не из тела: чужую реакцию за
                    // другого участника не подделать.
                    let changed = self
                        .messages
                        .lock_safe()
                        .react(
                            Chat::Group(&gid),
                            target,
                            &sender,
                            &hex::encode(actor),
                            &emoji,
                            at,
                        )
                        .unwrap_or(false);
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        reacted: changed.then_some((target, sender)),
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
                            .retract_many(Chat::Group(&gid), &[(target, who)])
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
                    self.put(
                        &gid,
                        &StoredMessage {
                            id: media.id,
                            mine: false,
                            time: now(),
                            sender: sender_hex.clone(),
                            text: media.caption.clone(),
                            kind: media.kind.clone(),
                            blob_id: media.hash.clone(),
                            mime: media.mime.clone(),
                            name: media.name.clone(),
                            size: media.size,
                            duration_ms: media.duration_ms,
                            width: media.width,
                            height: media.height,
                            thumb_id: media.thumb.clone(),
                            fwd_from: media.fwd_from.clone(),
                            fwd_anon: media.fwd_anon,
                            reply_to: media.reply_to,
                            reply_sender: media.reply_sender.clone(),
                            // Скачано ли — решит верхний слой (он знает хранилище блобов).
                            downloaded: false,
                            ..Default::default()
                        },
                    );
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        media: Some(media),
                        ..Default::default()
                    }
                }
                // Служебное: имя группы — обновляем название (не сообщение). Только от
                // админа: иначе любой участник переименует группу всем.
                Some(Ok(Inbound::Name { actor, name })) => {
                    let changed =
                        name_change_ok(self.admin_key(&gid), actor) && self.set_name(&gid, &name);
                    if changed {
                        self.put_event(&gid, "renamed", actor, name);
                    }
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        renamed: changed,
                        ..Default::default()
                    }
                }
                Some(Ok(Inbound::Admin {
                    actor,
                    key,
                    leaving,
                })) => {
                    let me = self.identity.public_key_bytes();
                    let mut members = self.members(&gid);
                    members.push(me); // ростер MLS — без себя
                    let ok = admin_change_ok(self.admin_key(&gid), actor, key, &members);
                    if ok {
                        self.set_admin(&gid, key);
                        // Старый админ ушёл: отозвать его теперь может только новый.
                        if leaving && key == me {
                            self.remove_member(&gid, actor);
                        }
                    }
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        renamed: ok,
                        ..Default::default()
                    }
                }
                Some(Ok(Inbound::Membership {
                    actor,
                    added,
                    removed,
                })) => {
                    // Добавил тот, кто сделал коммит; убранный — о нём и строка.
                    for who in added {
                        self.put_event(&gid, "joined", who, hex::encode(actor));
                    }
                    for who in removed {
                        self.put_event(&gid, "left", who, String::new());
                    }
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
                    return GroupOutcome {
                        gid: Some(gid),
                        removed: true,
                        ..Default::default()
                    };
                }
                // ADR-004: отстали (пропущен Commit / удалён секрет эпохи) — просим
                // админа заново синхронизировать нас (Remove+Add).
                Some(Ok(Inbound::NeedsResync)) => {
                    log::warn!(
                        "group {}: рассинхрон эпох — запрашиваю resync",
                        hex::encode(&gid)
                    );
                    self.request_resync(&gid);
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        ..Default::default()
                    }
                }
                Some(Err(e)) => {
                    log::warn!("group {} MLS отверг сообщение: {:?}", hex::encode(&gid), e);
                    GroupOutcome {
                        gid: Some(gid.clone()),
                        ..Default::default()
                    }
                }
                _ => GroupOutcome {
                    gid: Some(gid.clone()),
                    ..Default::default()
                },
            };
            self.save_group(&gid);
            return outcome;
        }
        // Иначе — возможно Welcome для ожидающего вступления. Только от того, кому
        // мы отдали KeyPackage: чужой Welcome даже не трогает pending-состояние.
        let Some(allowed) = self
            .pending_from
            .as_ref()
            .filter(|(from, _)| *from == peer)
            .map(|(_, g)| g.clone())
        else {
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
                log::warn!(
                    "Welcome от {} в чужую группу {} — отвергнут",
                    hex::encode(peer),
                    hex::encode(&id)
                );
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
                    return GroupOutcome {
                        gid: Some(id),
                        ..Default::default()
                    };
                }
                // Иначе — первое вступление. Мы НЕ админ (админ — пригласивший).
                // Имя — из принятого инвайта (иначе дефолт; админ всё равно догонит send_name).
                let name = self
                    .pending_join_name
                    .take()
                    .unwrap_or_else(|| "Группа".into());
                // Пригласивший и есть админ: приглашать и добавлять в MLS вправе
                // только он (ADR-004).
                if let Some(inv) = self.pending_invites.get(&id) {
                    let inviter = inv.inviter;
                    self.remember_admin(&id, inviter);
                }
                self.pending_invites.remove(&id);
                let _ = self.meta.remove_invite(&id);
                self.remember_title(&id, &name);
                self.groups.insert(
                    id.clone(),
                    Entry {
                        chat,
                        name,
                        admin: false,
                    },
                );
                self.save_group(&id);
                // Welcome шлёт тот, кто добавил: он и пригласил.
                let me = self.identity.public_key_bytes();
                self.put_event(&id, "joined", me, hex::encode(peer));
                return GroupOutcome {
                    gid: Some(id),
                    joined: true,
                    ..Default::default()
                };
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
        let Some(e) = self.groups.get(gid) else {
            return;
        };
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
        let _ = self.messenger.send_group_key_request(&peer, gid);
        self.invites.insert((peer, gid.to_vec()));
        let _ = self.meta.add_sent_invite(&peer, gid);
        self.resyncing.insert(peer); // on_key_reply сделает Remove+Add, не просто Add
    }

    /// Сменить имя группы у себя. true — имя действительно другое.
    fn set_name(&mut self, gid: &[u8], name: &str) -> bool {
        let Some(e) = self.groups.get_mut(gid) else {
            return false;
        };
        if e.name == name {
            return false;
        }
        e.name = name.to_string();
        self.remember_title(gid, name);
        self.save_group(gid);
        true
    }

    /// Записать нового админа у себя: ключ, своя роль, строка в ленте.
    fn set_admin(&mut self, gid: &[u8], key: [u8; 32]) {
        let me = self.identity.public_key_bytes();
        self.remember_admin(gid, key);
        if let Some(e) = self.groups.get_mut(gid) {
            e.admin = key == me;
        }
        self.save_group(gid);
        self.put_event(gid, "admin", key, String::new());
    }

    /// Переименовать группу (шаг 7j): только админ; имя уходит всем участникам.
    pub fn rename(&mut self, gid: &[u8], name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || !self.groups.get(gid).is_some_and(|e| e.admin) {
            return false;
        }
        if !self.set_name(gid, name) {
            return false;
        }
        if let Some(e) = self.groups.get_mut(gid) {
            let _ = e.chat.send_name(name);
        }
        let me = self.identity.public_key_bytes();
        self.put_event(gid, "renamed", me, name.to_string());
        true
    }

    /// Передать права админа участнику `key` (шаг 7j). `leaving` — и выйти: группу
    /// забываем сразу, а из ростера нас отзовёт уже новый админ по тому же сообщению.
    pub fn transfer_admin(&mut self, gid: &[u8], key: [u8; 32], leaving: bool) -> bool {
        let me = self.identity.public_key_bytes();
        let ok = self
            .groups
            .get(gid)
            .is_some_and(|e| e.admin && key != me && e.chat.members().contains(&key));
        if !ok {
            return false;
        }
        if let Some(e) = self.groups.get_mut(gid) {
            if e.chat.send_admin(&key, leaving).is_err() {
                return false;
            }
        }
        if leaving {
            self.delete_group(gid);
        } else {
            self.set_admin(gid, key);
        }
        true
    }

    /// Выйти из группы (ADR-004): просим админа отозвать нас (Remove+смена эпохи —
    /// настоящий криптографический выход) и локально забываем группу. Если админ
    /// офлайн — остаётся мягкий локальный выход (нас отзовут при его доступности).
    pub fn leave(&mut self, gid: &[u8]) -> bool {
        let Some(e) = self.groups.get(gid) else {
            return false;
        };
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

    /// Удалить группу локально (нас из неё отозвали, ADR-004): файл MLS, запись
    /// в базе, переписка.
    fn delete_group(&mut self, id: &[u8]) {
        self.groups.remove(id);
        self.admin_keys.remove(id);
        self.invites.retain(|(_, g)| g.as_slice() != id);
        let _ = self.meta.delete_group(id);
        let _ = std::fs::remove_file(self.group_path(id));
        {
            let store = self.messages.lock_safe();
            let _ = store.clear(Chat::Group(id));
            let _ = store.forget_chat_title(Chat::Group(id));
            let _ = store.clear_pins(Chat::Group(id));
        }
    }
}

/// Принять имя группы от `actor`: только от админа. Админ неизвестен (группа старше
/// записи админа) — проверить не по чему, принимаем как раньше.
fn name_change_ok(admin: Option<[u8; 32]>, actor: [u8; 32]) -> bool {
    admin.is_none_or(|a| a == actor)
}

/// Принять передачу прав: шлёт текущий админ, новый — другой и действующий участник.
/// Админ неизвестен — передавать нечего: права не возникают из сообщения.
fn admin_change_ok(
    admin: Option<[u8; 32]>,
    actor: [u8; 32],
    key: [u8; 32],
    members: &[[u8; 32]],
) -> bool {
    admin == Some(actor) && key != actor && members.contains(&key)
}

/// Результат обработки группового сообщения для уведомления UI.
#[derive(Default)]
pub struct GroupOutcome {
    pub gid: Option<Vec<u8>>,
    /// Сообщение исправлено автором: `(автор, id, новый текст, упоминания)`.
    pub edited: Option<(String, u64, String, Vec<Mention>)>,
    /// Список закреплённых изменился — UI перечитать шапку чата.
    pub pinned: bool,
    pub joined: bool,
    /// Нас отозвали из группы (ADR-004) — UI должен убрать группу и уведомить.
    pub removed: bool,
    /// Имя группы или её админ изменились (пришло от админа) — UI обновить список.
    pub renamed: bool,
    /// Медиа-сообщение (M2): ссылка на блоб + метаданные карточки. Пусто → не медиа.
    pub media: Option<MediaBody>,
    /// Участник отозвал своё сообщение — строка исчезла из истории, UI перечитать.
    pub retracted: bool,
    /// Реакции под сообщением `(id, автор)` изменились — UI поправить его на месте.
    pub reacted: Option<(u64, String)>,
}

#[cfg(test)]
mod rights_tests {
    use super::{admin_change_ok, name_change_ok};

    const A: [u8; 32] = [1; 32];
    const B: [u8; 32] = [2; 32];
    const C: [u8; 32] = [3; 32];

    #[test]
    fn name_only_from_admin() {
        assert!(name_change_ok(Some(A), A));
        assert!(
            !name_change_ok(Some(A), B),
            "участник не переименует группу"
        );
        assert!(name_change_ok(None, B), "админ неизвестен — как раньше");
    }

    #[test]
    fn admin_transfer_only_from_admin_to_member() {
        let members = [A, B];
        assert!(admin_change_ok(Some(A), A, B, &members));
        assert!(
            !admin_change_ok(Some(A), B, B, &members),
            "участник не назначит себя"
        );
        assert!(
            !admin_change_ok(Some(A), A, C, &members),
            "только действующему участнику"
        );
        assert!(
            !admin_change_ok(Some(A), A, A, &members),
            "себе — не передача"
        );
        assert!(
            !admin_change_ok(None, A, B, &members),
            "без известного админа прав нет"
        );
    }
}
