//! Движок мессенджера — device-independent ядро (ADR-003). Владеет состоянием
//! сессии (контакты, сообщения, группы, комнаты-звонки), исполняет команды и
//! отдаёт события. НЕ зависит от JNI/Android — `androidjni` лишь маршалит вызовы.
//! Десктоп может использовать тот же крейт.
//!
//! Контракт: клиент шлёт команды (методы `Engine`) и читает снапшоты; движок
//! асинхронно копит события (`poll_event`). Домейн-логику (агрегаты, презенс,
//! маршрутизацию) считает движок, не клиент. Поэтому ядро юнит-тестируется без
//! устройства (см. `net.rs`, тесты презенса ниже).

pub mod groups;
pub mod net;
pub mod room_call;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::settings::AudioSettings;
use p2p_core::store::{ContactStore, MessageStore, Profile, StoredMessage};
use p2p_core::Identity;
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

use groups::GroupManager;
use net::{GroupPresence, NetStats, PeerStat};
use room_call::RoomCallEngine;

/// Отказоустойчивый захват мьютекса: если другой поток паниковал держа его,
/// `.unwrap()` тоже паникует → cascade crash. Забираем внутреннее значение,
/// игнорируя poison — данные остаются консистентными.
pub trait LockRecover<T> {
    fn lock_safe(&self) -> std::sync::MutexGuard<'_, T>;
}
impl<T> LockRecover<T> for Mutex<T> {
    fn lock_safe(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Команда группового актора (исполняется в потоке event_pump, т.к. GroupChat !Send).
/// Все команды — fire-and-forget. Read-данные UI читает из кэша [`Engine::group_cache`].
pub(crate) enum GroupCmd {
    Create { name: String },
    Invite { gid: Vec<u8>, peer: [u8; 32] },
    SendText { gid: Vec<u8>, text: String },
}

/// Работа для потока event_pump: событие мессенджера, групповая команда либо
/// рассылка маячка присутствия.
pub(crate) enum Job {
    Ev(Event),
    Cmd(GroupCmd),
    Beacon,
    /// Рассылка «я вышел» членам комнаты `gid` (мгновенное исчезновение).
    Leave(Vec<u8>),
}

/// Период рассылки маячка присутствия в комнатах (пакеты крошечные).
const BEACON_INTERVAL: Duration = Duration::from_secs(2);
/// Не видели маячок дольше — участник считается вышедшим (страховка на вылет/потерю
/// связи). 12с = 6 интервалов: переживаем многосекундные провалы доставки, не
/// выкидывая собеседника. Явный выход (PRESENCE_LEFT) убирает участника мгновенно.
pub const PRESENCE_TTL: Duration = Duration::from_secs(12);
/// Бит в `flags`: «я выхожу из комнаты» — получатель убирает участника сразу.
const PRESENCE_LEFT: u8 = 0x80;

pub const BENCH_TRACK: u8 = 200;

const PUBLIC_PEERS: &str = include_str!("../../peers/public-peers.txt");

/// Присутствие в комнатах-звонках (Discord-style).
#[derive(Default)]
pub(crate) struct RoomState {
    /// Комнаты, в которых МЫ сейчас находимся: gid → наши A/V-флаги.
    pub(crate) joined: HashMap<Vec<u8>, u8>,
    /// Присутствие других: gid → (peer → (время последнего маячка, флаги)).
    pub(crate) remote: HashMap<Vec<u8>, HashMap<[u8; 32], (Instant, u8)>>,
    /// DM-комнаты: room_id → peer (для маячков и LEFT-сигнала без MLS-группы).
    pub(crate) dm_peers: HashMap<Vec<u8>, [u8; 32]>,
}

impl RoomState {
    /// Живые участники `gid` (маячок не старше `ttl` относительно `now`).
    /// Чистая логика презенса — тестируется без устройства (инъекция времени).
    pub(crate) fn live_with_flags(&self, gid: &[u8], now: Instant, ttl: Duration) -> Vec<([u8; 32], u8)> {
        self.remote
            .get(gid)
            .map(|m| {
                m.iter()
                    .filter(|(_, (t, _))| now.saturating_duration_since(*t) < ttl)
                    .map(|(p, (_, f))| (*p, *f))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Выкинуть протухшее присутствие (старше `ttl`) из всех комнат.
    pub(crate) fn prune(&mut self, now: Instant, ttl: Duration) {
        for peers in self.remote.values_mut() {
            peers.retain(|_, (t, _)| now.saturating_duration_since(*t) < ttl);
        }
    }
}

/// Активный мультипати аудио-звонок комнаты (один на процесс).
pub(crate) struct RoomCall {
    pub(crate) gid: Vec<u8>,
    pub(crate) engine: Arc<RoomCallEngine>,
    /// Цели мультикаста — синхронизируется с живыми участниками (см. sync).
    pub(crate) members: Arc<Mutex<Vec<[u8; 32]>>>,
}

pub(crate) struct BenchRecv {
    pub(crate) bytes: AtomicU64,
    pub(crate) pkts: AtomicU64,
    pub(crate) running: AtomicBool,
    pub(crate) start: Instant,
}

/// Кэш групповых read-данных для UI. event_pump поддерживает его актуальным после
/// каждой мутации. Снапшоты `group_*_json` читают кэш мгновенно.
#[derive(Default)]
struct GroupCache {
    list: String,
    history: HashMap<Vec<u8>, String>,
    members: HashMap<Vec<u8>, String>,
}

/// Состояние сессии приложения — корень контракта движок↔клиент.
pub struct Engine {
    messenger: Arc<Messenger>,
    contacts: Mutex<ContactStore>,
    messages: Mutex<MessageStore>,
    me_pub: [u8; 32],
    events: Mutex<VecDeque<String>>,
    group_cache: Mutex<GroupCache>,
    jobs: Sender<Job>,
    room_call: Mutex<Option<RoomCall>>,
    rooms: Mutex<RoomState>,
    bench: Mutex<Option<Arc<BenchRecv>>>,
}

/// Результат инициализации: движок + публичная идентичность для UI.
pub struct StartOk {
    pub engine: Arc<Engine>,
    pub pubkey: [u8; 32],
    pub address: String,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// hex(32 байта) → ключ. Кодек-хелпер (строка↔ключ); экспортируется для androidjni.
pub fn parse_key(s: &str) -> Option<[u8; 32]> {
    hex::decode(s.trim()).ok()?.as_slice().try_into().ok()
}

/// Фоновый коннект к пиру (как на десктопе): держим попытки, пока процесс жив.
pub(crate) fn spawn_connect(m: Arc<Messenger>, key: [u8; 32]) {
    std::thread::spawn(move || loop {
        let _ = m.connect(&key, Duration::from_secs(20));
        std::thread::sleep(Duration::from_secs(5));
    });
}

// ── Публичный API движка (команды/снапшоты) ─────────────────────────────────
// Каждый метод — операция контракта. androidjni декодирует аргументы (hex→ключ)
// и маршалит результат; домейн-логику считает здесь движок.
impl Engine {
    // — Жизненный цикл —
    /// Следующее событие (JSON) или None, если очередь пуста.
    pub fn poll_event(&self) -> Option<String> {
        self.events.lock_safe().pop_front()
    }

    /// Корректно остановить транспорт/мессенджер.
    pub fn shutdown(&self) {
        self.messenger.shutdown();
    }

    pub fn me_pub(&self) -> [u8; 32] {
        self.me_pub
    }

    // — Сообщения 1:1 —
    /// Отправить текст. Возвращает id (≥0) или None при ошибке.
    pub fn send_text(&self, key: &[u8; 32], text: &str) -> Option<i64> {
        match self.messenger.send_text(key, text) {
            Ok(id) => {
                let _ = self.messages.lock_safe().append(key, StoredMessage {
                    id, text: text.to_string(), mine: true, time: now(), status: "sent".into(),
                });
                Some(id as i64)
            }
            Err(_) => None,
        }
    }

    pub fn connect(&self, key: [u8; 32]) {
        spawn_connect(self.messenger.clone(), key);
    }

    // — Контакты (TOFU) —
    /// Добавить контакт. Возвращает "added" | "known" | "error".
    pub fn add_contact(&self, key: &[u8; 32], name: &str) -> &'static str {
        let name = if name.trim().is_empty() {
            format!("{}…", &hex::encode(key)[..8])
        } else {
            name.to_string()
        };
        match self.contacts.lock_safe().add(key, &name, now()) {
            Ok(p2p_core::store::AddOutcome::Added) => {
                spawn_connect(self.messenger.clone(), *key);
                "added"
            }
            Ok(p2p_core::store::AddOutcome::AlreadyKnown) => "known",
            Err(_) => "error",
        }
    }

    pub fn contacts_json(&self) -> String {
        serde_json::to_string(&self.contacts.lock_safe().list()).unwrap_or_else(|_| "[]".into())
    }

    pub fn history_json(&self, key: &[u8; 32]) -> String {
        serde_json::to_string(&self.messages.lock_safe().history(key)).unwrap_or_else(|_| "[]".into())
    }

    pub fn set_verified(&self, key: &[u8; 32], verified: bool) {
        let _ = self.contacts.lock_safe().set_verified(key, verified);
    }

    pub fn rename_contact(&self, key: &[u8; 32], name: &str) {
        let _ = self.contacts.lock_safe().rename(key, name);
    }

    pub fn remove_contact(&self, key: &[u8; 32]) {
        let _ = self.contacts.lock_safe().remove(key);
    }

    pub fn is_online(&self, key: &[u8; 32]) -> bool {
        self.messenger.is_online(key)
    }

    // — Сеть —
    pub fn net_peers(&self) -> i32 {
        self.messenger.net_peers() as i32
    }

    pub fn net_tree(&self) -> i32 {
        self.messenger.net_tree()
    }

    /// Снапшот стата сети: пиры + агрегаты. Агрегаты считает движок (см. net.rs).
    pub fn net_stats(&self) -> NetStats {
        let peers: Vec<PeerStat> =
            serde_json::from_str(&self.messenger.net_peers_json()).unwrap_or_default();
        NetStats::aggregate(peers, self.net_tree())
    }

    /// Принудительно переподключить публичных пиров (обход backoff).
    pub fn reconnect_peers(&self) {
        for uri in public_peer_uris() {
            self.messenger.add_peer(uri);
        }
        log::info!("reconnect_peers: re-added public peers");
    }

    // — Группы (MLS) — мутации fire-and-forget через актора, чтение из кэша —
    /// Создать группу. Возвращает true, если команда принята актором.
    pub fn group_create(&self, name: &str) -> bool {
        self.jobs.send(Job::Cmd(GroupCmd::Create { name: name.to_string() })).is_ok()
    }

    pub fn group_invite(&self, gid: Vec<u8>, peer: [u8; 32]) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::Invite { gid, peer }));
    }

    pub fn group_send_text(&self, gid: Vec<u8>, text: String) {
        let _ = self.jobs.send(Job::Cmd(GroupCmd::SendText { gid, text }));
    }

    pub fn group_list_json(&self) -> String {
        let s = self.group_cache.lock_safe().list.clone();
        if s.is_empty() { "[]".into() } else { s }
    }

    pub fn group_history_json(&self, gid: &[u8]) -> String {
        self.group_cache.lock_safe().history.get(gid).cloned().unwrap_or_else(|| "[]".into())
    }

    pub fn group_members_json(&self, gid: &[u8]) -> String {
        self.group_cache.lock_safe().members.get(gid).cloned().unwrap_or_else(|| "[]".into())
    }

    /// Снапшот присутствия группы «N уч · M в сети» (ростер ∩ online). Презенс
    /// динамичен → считаем на момент чтения. Кэш `members` — ДРУГИЕ (без себя).
    pub fn group_presence(&self, gid: &[u8]) -> GroupPresence {
        let json = self.group_members_json(gid);
        let others: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
        let online_others = others
            .iter()
            .filter_map(|h| parse_key(h))
            .filter(|k| self.messenger.is_online(k))
            .count();
        GroupPresence::compute(others.len(), online_others)
    }

    // — Комнаты-звонки (присутствие, Discord-style) —
    pub(crate) fn room_engine(&self) -> Option<Arc<RoomCallEngine>> {
        self.room_call.lock_safe().as_ref().map(|r| r.engine.clone())
    }

    pub(crate) fn live_participants(&self, gid: &[u8]) -> Vec<[u8; 32]> {
        self.rooms.lock_safe().live_with_flags(gid, Instant::now(), PRESENCE_TTL)
            .into_iter().map(|(p, _)| p).collect()
    }

    fn sync_room_members(&self) {
        let guard = self.room_call.lock_safe();
        if let Some(rc) = guard.as_ref() {
            let mut live = self.live_participants(&rc.gid);
            // SEC-2 (ADR-004): для ГРУППОВОЙ комнаты в состав звонка допускаем только
            // пиров из текущего MLS-ростера — маячок присутствия по gid сам по себе не
            // пропуск. DM-комнаты (room_id в dm_peers) не фильтруем: там ростера нет.
            // Fail-open, если ростер ещё не в кэше (не рвём легитимный звонок): гейт
            // обретает зубы после отзыва (SEC-1), когда ростер реально сужается.
            let is_dm = self.rooms.lock_safe().dm_peers.contains_key(&rc.gid);
            if !is_dm {
                let roster = self.group_roster_set(&rc.gid);
                if !roster.is_empty() {
                    live.retain(|p| roster.contains(p));
                }
            }
            *rc.members.lock_safe() = live;
        }
    }

    /// Множество ключей текущего MLS-ростера группы (без себя) из кэша — для SEC-2.
    fn group_roster_set(&self, gid: &[u8]) -> std::collections::HashSet<[u8; 32]> {
        serde_json::from_str::<Vec<String>>(&self.group_members_json(gid))
            .unwrap_or_default()
            .iter()
            .filter_map(|h| parse_key(h))
            .collect()
    }

    /// DM-звонок: создать комнату, послать RoomInvite, начать слать маячок.
    /// Возвращает room_id.
    pub fn room_invite_dm(&self, peer: [u8; 32], audio: bool, video: bool) -> Vec<u8> {
        let room_id = p2p_core::dm_room_id(&self.me_pub, &peer);
        let _ = self.messenger.send_room_invite(&peer, &room_id, audio, video);
        let flags = av_flags(audio, video);
        {
            let mut rs = self.rooms.lock_safe();
            rs.joined.insert(room_id.clone(), flags);
            rs.dm_peers.insert(room_id.clone(), peer);
        }
        let _ = self.jobs.send(Job::Beacon);
        room_id
    }

    pub fn room_decline_dm(&self, peer: [u8; 32]) {
        let room_id = p2p_core::dm_room_id(&self.me_pub, &peer);
        let _ = self.messenger.send_room_decline(&peer, &room_id);
    }

    /// Принять DM-звонок: войти в комнату с регистрацией dm_peer. Возвращает room_id.
    pub fn room_join_dm(&self, peer: [u8; 32], audio: bool, video: bool) -> Vec<u8> {
        let room_id = p2p_core::dm_room_id(&self.me_pub, &peer);
        let flags = av_flags(audio, video);
        {
            let mut rs = self.rooms.lock_safe();
            rs.joined.insert(room_id.clone(), flags);
            rs.dm_peers.insert(room_id.clone(), peer);
        }
        let _ = self.jobs.send(Job::Beacon);
        room_id
    }

    pub fn room_join(&self, gid: Vec<u8>, audio: bool, video: bool) {
        self.rooms.lock_safe().joined.insert(gid, av_flags(audio, video));
        let _ = self.jobs.send(Job::Beacon);
    }

    pub fn room_leave(&self, gid: Vec<u8>) {
        {
            let mut rs = self.rooms.lock_safe();
            rs.joined.remove(&gid);
            rs.remote.remove(&gid);
        }
        let _ = self.jobs.send(Job::Leave(gid));
    }

    /// Живые участники любой комнаты по маячкам: JSON [{peer,audio,video}].
    pub fn room_presence_peers_json(&self, gid: &[u8]) -> String {
        let live = self.rooms.lock_safe().live_with_flags(gid, Instant::now(), PRESENCE_TTL);
        room_members_json(live)
    }

    /// Текущие участники комнаты (то же, что presence_peers — по маячкам).
    pub fn room_participants_json(&self, gid: &[u8]) -> String {
        self.room_presence_peers_json(gid)
    }

    /// Запустить мультипати аудио комнаты. `bitrate` (бит/с, >0) и `music` (Audio-режим
    /// vs Voip) — из настроек звука; 0 = дефолт. Возвращает JSON {sample_rate,channels,frame_len}.
    pub fn room_audio_start(&self, gid: Vec<u8>, bitrate: i32, music: bool) -> String {
        let mut audio = AudioSettings::default();
        audio.jitter_depth = 5;
        if bitrate > 0 {
            audio.bitrate = bitrate;
            audio.mode = if music {
                p2p_core::settings::OpusMode::Audio
            } else {
                p2p_core::settings::OpusMode::Voip
            };
        }
        let members = Arc::new(Mutex::new(self.live_participants(&gid)));
        let engine = RoomCallEngine::start(self.messenger.clone(), audio, members.clone());
        let old = self.room_call.lock_safe().replace(RoomCall { gid, engine, members });
        if let Some(old) = old {
            std::thread::spawn(move || old.engine.stop());
        }
        call_params(&audio)
    }

    pub fn room_audio_stop(&self) {
        let rc = self.room_call.lock_safe().take();
        if let Some(rc) = rc {
            std::thread::spawn(move || rc.engine.stop());
        }
    }

    pub fn room_push_audio(&self, samples: &[i16]) {
        if let Some(engine) = self.room_engine() {
            engine.push_audio(samples);
        }
    }

    pub fn room_pull_audio(&self, out: &mut [i16]) -> usize {
        self.room_engine().map(|e| e.pull_audio(out)).unwrap_or(0)
    }

    /// Hex-ключи говорящих сейчас участников (active-speaker). "[]" вне звонка.
    pub fn room_speaking_json(&self) -> String {
        self.room_engine().map(|e| e.speaking_json()).unwrap_or_else(|| "[]".into())
    }

    /// Сменить Opus-битрейт текущей аудио-сессии на лету (настройки звука без перезвона).
    pub fn room_set_audio_bitrate(&self, bitrate: i32) {
        if let Some(e) = self.room_engine() {
            e.set_bitrate(bitrate);
        }
    }

    /// Накопленные байты трафика по участникам {hex: rx, "self": tx}. "{}" вне звонка.
    pub fn room_traffic_json(&self) -> String {
        self.room_engine().map(|e| e.traffic_json()).unwrap_or_else(|| "{}".into())
    }

    pub fn room_set_muted(&self, muted: bool) {
        if let Some(engine) = self.room_engine() {
            engine.set_muted(muted);
        }
    }

    pub fn room_push_video(&self, frame: &[u8], keyframe: bool) {
        if let Some(engine) = self.room_engine() {
            engine.push_video(frame, keyframe);
        }
    }

    pub fn room_set_video_cap(&self, cap: usize) {
        if let Some(engine) = self.room_engine() {
            engine.set_video_cap(cap);
        }
    }

    pub fn room_pull_video_packet(&self, peer: &[u8; 32]) -> Option<Vec<u8>> {
        self.room_engine().and_then(|e| e.pull_video_for(peer))
    }

    // — Диагностика: тест пропускной способности меша —
    pub fn bench_recv_start(&self) {
        let rx = self.messenger.media_channel(8192);
        let st = Arc::new(BenchRecv {
            bytes: AtomicU64::new(0), pkts: AtomicU64::new(0),
            running: AtomicBool::new(true), start: Instant::now(),
        });
        *self.bench.lock_safe() = Some(st.clone());
        std::thread::spawn(move || {
            while st.running.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(p) if p.track == BENCH_TRACK => {
                        st.bytes.fetch_add(p.payload.len() as u64, Ordering::Relaxed);
                        st.pkts.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return,
                }
            }
        });
    }

    pub fn bench_recv_stats_json(&self) -> String {
        let g = self.bench.lock_safe();
        match g.as_ref() {
            Some(st) => {
                let bytes = st.bytes.load(Ordering::Relaxed);
                let pkts = st.pkts.load(Ordering::Relaxed);
                let secs = st.start.elapsed().as_secs_f64().max(0.001);
                serde_json::json!({
                    "pkts": pkts, "bytes": bytes, "secs": secs,
                    "mbps": bytes as f64 * 8.0 / 1e6 / secs,
                }).to_string()
            }
            None => "{}".into(),
        }
    }

    pub fn bench_blast(&self, peer: &[u8; 32], total_kb: i32) -> String {
        let payload = vec![0u8; 1200];
        let target = (total_kb.max(0) as u64) * 1024;
        let mut sent_bytes = 0u64;
        let mut sent_pkts = 0u64;
        let mut seq = 0u16;
        let t0 = Instant::now();
        while sent_bytes < target && t0.elapsed() < Duration::from_secs(10) {
            if self.messenger.send_media(peer, BENCH_TRACK, seq, 0, &payload).is_ok() {
                sent_bytes += payload.len() as u64;
                sent_pkts += 1;
                seq = seq.wrapping_add(1);
            }
        }
        let secs = t0.elapsed().as_secs_f64().max(0.001);
        serde_json::json!({
            "sent_pkts": sent_pkts, "sent_bytes": sent_bytes, "secs": secs,
            "mbps": sent_bytes as f64 * 8.0 / 1e6 / secs,
        }).to_string()
    }
}

// ── Хелперы маршалинга/презенса ─────────────────────────────────────────────

fn av_flags(audio: bool, video: bool) -> u8 {
    let mut f = 0u8;
    if audio { f |= 0b01; }
    if video { f |= 0b10; }
    f
}

fn room_members_json(live: Vec<([u8; 32], u8)>) -> String {
    let arr: Vec<serde_json::Value> = live
        .into_iter()
        .map(|(p, f)| serde_json::json!({
            "peer": hex::encode(p), "audio": f & 0b01 != 0, "video": f & 0b10 != 0,
        }))
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
}

fn call_params(audio: &AudioSettings) -> String {
    serde_json::json!({
        "call_id": 0,
        "sample_rate": audio.sample_rate,
        "channels": audio.channels,
        "frame_len": audio.interleaved_len(),
    })
    .to_string()
}

fn public_peer_uris() -> impl Iterator<Item = &'static str> {
    PUBLIC_PEERS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

// ── Внутренняя оркестрация: event_pump + обработчики ────────────────────────

/// TOFU: добавить неизвестного пира в контакты (имя по умолчанию — префикс ключа).
fn ensure_contact(engine: &Engine, peer: [u8; 32]) {
    let mut store = engine.contacts.lock_safe();
    if !store.contains(&peer) {
        let hex = hex::encode(peer);
        let name = format!("{}…", &hex[..8]);
        let _ = store.add(&peer, &name, now());
    }
}

fn push_event(engine: &Engine, json: serde_json::Value) {
    engine.events.lock_safe().push_back(json.to_string());
}

/// Поток event_pump = групповой актор: владеет [`GroupManager`] (GroupChat !Send),
/// обрабатывает события мессенджера и групповые команды — всё в одном потоке.
fn event_pump(
    engine: Arc<Engine>,
    job_rx: Receiver<Job>,
    identity: Identity,
    messenger: Arc<Messenger>,
    data_dir: PathBuf,
) {
    let mut groups = GroupManager::new(identity, messenger.clone(), data_dir);
    refresh_group_cache(&engine, &groups); // прайм кэша
    push_event(&engine, serde_json::json!({"type": "groups_changed"}));
    let mut last_beacon = Instant::now();
    loop {
        match job_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Job::Ev(ev)) => handle_event(&engine, &mut groups, ev),
            Ok(Job::Cmd(cmd)) => handle_cmd(&engine, &mut groups, cmd),
            Ok(Job::Beacon) => {
                last_beacon = Instant::now();
                beacon_tick(&engine, &groups, &messenger);
            }
            Ok(Job::Leave(gid)) => {
                // Стабильная комната: если мы УЖЕ перезашли — отложенный Leave устарел.
                if engine.rooms.lock_safe().joined.contains_key(&gid) {
                    // перезашли — ничего не делаем
                } else {
                    let dm_peer = engine.rooms.lock_safe().dm_peers.remove(&gid);
                    if let Some(peer) = dm_peer {
                        let _ = messenger.send_room_presence(&peer, &gid, PRESENCE_LEFT);
                    } else {
                        for m in groups.members(&gid) {
                            if m == engine.me_pub { continue; }
                            let _ = messenger.send_room_presence(&m, &gid, PRESENCE_LEFT);
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if last_beacon.elapsed() >= BEACON_INTERVAL {
            last_beacon = Instant::now();
            beacon_tick(&engine, &groups, &messenger);
        }
    }
}

/// Разослать маячок присутствия членам всех наших комнат и подчистить протухших.
fn beacon_tick(engine: &Arc<Engine>, groups: &GroupManager, messenger: &Messenger) {
    let joined: Vec<(Vec<u8>, u8)> = {
        let rs = engine.rooms.lock_safe();
        rs.joined.iter().map(|(g, f)| (g.clone(), *f)).collect()
    };
    for (gid, flags) in joined {
        let dm_peer = engine.rooms.lock_safe().dm_peers.get(&gid).copied();
        if let Some(peer) = dm_peer {
            let _ = messenger.send_room_presence(&peer, &gid, flags);
        } else {
            for m in groups.members(&gid) {
                if m == engine.me_pub { continue; }
                let _ = messenger.send_room_presence(&m, &gid, flags);
            }
        }
    }
    engine.rooms.lock_safe().prune(Instant::now(), PRESENCE_TTL);
    engine.sync_room_members();
}

/// Обработать одно событие мессенджера: персист + JSON в очередь + маршрутизация групп.
fn handle_event(engine: &Arc<Engine>, groups: &mut GroupManager, ev: Event) {
    match ev {
        Event::Presence { peer, online } => {
            if online {
                ensure_contact(engine, peer);
            }
            push_event(engine, serde_json::json!({
                "type": "presence", "peer": hex::encode(peer), "online": online,
            }));
        }
        Event::Text { peer, id, text } => {
            ensure_contact(engine, peer);
            let t = now();
            let _ = engine.messages.lock_safe().append(&peer, StoredMessage {
                id, text: text.clone(), mine: false, time: t, status: String::new(),
            });
            push_event(engine, serde_json::json!({
                "type": "text", "peer": hex::encode(peer), "id": id, "text": text, "time": t,
            }));
        }
        Event::Ack { peer, id } => {
            let _ = engine.messages.lock_safe().set_status(&peer, id, "delivered");
            push_event(engine, serde_json::json!({"type": "ack", "peer": hex::encode(peer), "id": id}));
        }
        Event::Undelivered { peer, id } => {
            let _ = engine.messages.lock_safe().set_status(&peer, id, "failed");
            push_event(engine, serde_json::json!({"type": "undelivered", "peer": hex::encode(peer), "id": id}));
        }
        Event::RoomInvite { peer, room_id, audio, video } => push_event(engine, serde_json::json!({
            "type": "room_invite", "peer": hex::encode(peer),
            "room_id": hex::encode(&room_id), "audio": audio, "video": video,
        })),
        Event::RoomDecline { peer, room_id } => push_event(engine, serde_json::json!({
            "type": "room_decline", "peer": hex::encode(peer), "room_id": hex::encode(&room_id),
        })),
        Event::Group { data, .. } => {
            let out = groups.on_group(&data);
            if let Some(gid) = &out.gid {
                if out.text.is_some() || out.system {
                    // Обновляем кэш истории этого gid: group_history_json читает кэш,
                    // а не live-messages. Без этого полученное сообщение пропадает при
                    // переоткрытии чата (отображается только live-событием).
                    engine.group_cache.lock_safe().history.insert(gid.clone(), groups.history_json(gid));
                    push_event(engine, serde_json::json!({
                        "type": "group_msg",
                        "gid": hex::encode(gid),
                        "text": out.text.clone().unwrap_or_default(),
                        "sender": out.sender.clone().unwrap_or_default(),
                        "system": out.system,
                    }));
                }
            }
            if out.joined {
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
            if out.removed {
                // ADR-004: нас отозвали — обновляем кэш и уведомляем UI отдельным
                // событием (чтобы показать «вы удалены» и закрыть экран группы).
                refresh_group_cache(engine, groups);
                if let Some(gid) = &out.gid {
                    push_event(engine, serde_json::json!({
                        "type": "group_removed", "gid": hex::encode(gid),
                    }));
                }
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
        }
        Event::GroupKeyRequest { peer } => groups.on_key_request(peer),
        Event::GroupKeyReply { peer, key_package } => {
            if groups.on_key_reply(peer, &key_package) {
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
        }
        Event::RoomPresence { peer, gid, flags } => {
            let left = flags & PRESENCE_LEFT != 0;
            {
                let mut rs = engine.rooms.lock_safe();
                if left {
                    if let Some(m) = rs.remote.get_mut(&gid) {
                        m.remove(&peer);
                    }
                } else {
                    rs.remote.entry(gid.clone()).or_default().insert(peer, (Instant::now(), flags));
                }
            }
            push_event(engine, serde_json::json!({
                "type": "room_presence", "gid": hex::encode(&gid),
                "peer": hex::encode(peer), "flags": flags, "left": left,
            }));
            engine.sync_room_members();
        }
    }
}

/// Пересчитать кэш групповых read-данных (зовётся после каждой мутации).
fn refresh_group_cache(engine: &Arc<Engine>, groups: &GroupManager) {
    let mut cache = engine.group_cache.lock_safe();
    cache.list = groups.list_json();
    for gid in groups.group_ids() {
        cache.history.insert(gid.clone(), groups.history_json(&gid));
        cache.members.insert(gid.clone(), groups.members_json(&gid));
    }
}

/// Исполнить групповую команду (в потоке event_pump). Fire-and-forget.
fn handle_cmd(engine: &Arc<Engine>, groups: &mut GroupManager, cmd: GroupCmd) {
    match cmd {
        GroupCmd::Create { name } => {
            if groups.create(&name).is_some() {
                refresh_group_cache(engine, groups);
                push_event(engine, serde_json::json!({"type": "groups_changed"}));
            }
        }
        GroupCmd::Invite { gid, peer } => groups.invite(&gid, peer),
        GroupCmd::SendText { gid, text } => {
            if groups.send_text(&gid, &text) {
                refresh_group_cache(engine, groups);
            }
        }
    }
}

/// Удалить зашифрованные пользовательские данные при потере vault-ключа.
fn wipe_user_data(dir: &Path) {
    for name in &["profile.json", "contacts.json", "messages.json", "messenger.db",
                  "messenger.db-shm", "messenger.db-wal"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("groups")) {
        for e in entries.flatten() { let _ = std::fs::remove_file(e.path()); }
    }
}

impl Engine {
    /// Инициализация. `dir` — приватный каталог данных. `vault_key` — 32-байтовый
    /// ключ at-rest (None → at-rest отключено). Поднимает транспорт, хранилища,
    /// поток event_pump. Возвращает движок + публичную идентичность.
    pub fn start(dir: PathBuf, vault_key: Option<[u8; 32]>) -> Result<StartOk, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        if let Some(key) = vault_key {
            p2p_core::vault::init_with_key(key);
        }

        let identity = match Profile::load_or_create(&dir.join("profile.json")) {
            Ok(p) => p.identity,
            Err(p2p_core::store::StoreError::Vault(p2p_core::vault::VaultError::Decrypt)) => {
                // Vault key lost (factory reset / переустановка поверх старых данных).
                log::warn!("vault key mismatch: wiping user data and regenerating identity");
                wipe_user_data(&dir);
                Profile::load_or_create(&dir.join("profile.json"))
                    .map_err(|e| e.to_string())?
                    .identity
            }
            Err(e) => return Err(e.to_string()),
        };
        let me_pub = identity.public_key_bytes();

        let node = Node::start(&identity.seed()).map_err(|e| e.to_string())?;
        let address = node.address();
        for uri in public_peer_uris() {
            let _ = node.add_peer(uri);
        }

        let db = p2p_core::db::open(&dir).map_err(|e| format!("db: {e}"))?;
        let contacts = ContactStore::open(db.clone(), Some(&dir.join("contacts.json")))
            .map_err(|e| format!("contacts: {e}"))?;
        let messages = MessageStore::open(db.clone(), Some(&dir.join("messages.json")))
            .map_err(|e| format!("messages: {e}"))?;
        let contact_keys: Vec<[u8; 32]> =
            contacts.list().iter().filter_map(|c| parse_key(&c.key)).collect();

        let identity_for_groups = identity.clone();
        let (messenger, rx) = Messenger::start(identity, node);
        let messenger = Arc::new(messenger);
        for k in &contact_keys {
            spawn_connect(messenger.clone(), *k);
        }

        // Актор-канал: события мессенджера и групповые команды идут в один поток.
        let (job_tx, job_rx) = std::sync::mpsc::channel::<Job>();
        {
            let job_tx = job_tx.clone();
            std::thread::spawn(move || {
                while let Ok(ev) = rx.recv() {
                    if job_tx.send(Job::Ev(ev)).is_err() {
                        break;
                    }
                }
            });
        }

        let engine = Arc::new(Engine {
            messenger: messenger.clone(),
            contacts: Mutex::new(contacts),
            messages: Mutex::new(messages),
            me_pub,
            events: Mutex::new(VecDeque::new()),
            group_cache: Mutex::new(GroupCache::default()),
            jobs: job_tx,
            room_call: Mutex::new(None),
            rooms: Mutex::new(RoomState::default()),
            bench: Mutex::new(None),
        });

        let pump_engine = engine.clone();
        let groups_dir = dir.clone();
        std::thread::spawn(move || {
            event_pump(pump_engine, job_rx, identity_for_groups, messenger, groups_dir)
        });

        Ok(StartOk { engine, pubkey: me_pub, address })
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;

    fn k(b: u8) -> [u8; 32] { [b; 32] }

    #[test]
    fn live_filters_stale_beacons() {
        let mut rs = RoomState::default();
        let now = Instant::now();
        let ttl = Duration::from_secs(12);
        let gid = vec![1u8, 2, 3];
        let fresh = now;
        let stale = now - Duration::from_secs(20);
        let m = rs.remote.entry(gid.clone()).or_default();
        m.insert(k(1), (fresh, 0b01));
        m.insert(k(2), (stale, 0b11));
        let live = rs.live_with_flags(&gid, now, ttl);
        assert_eq!(live.len(), 1, "протухший маячок должен быть отфильтрован");
        assert_eq!(live[0], (k(1), 0b01));
    }

    #[test]
    fn prune_drops_stale() {
        let mut rs = RoomState::default();
        let now = Instant::now();
        let ttl = Duration::from_secs(12);
        let gid = vec![9u8];
        let m = rs.remote.entry(gid.clone()).or_default();
        m.insert(k(1), (now, 0));
        m.insert(k(2), (now - Duration::from_secs(30), 0));
        rs.prune(now, ttl);
        assert_eq!(rs.remote.get(&gid).unwrap().len(), 1);
    }

    #[test]
    fn empty_room_has_no_participants() {
        let rs = RoomState::default();
        assert!(rs.live_with_flags(&[7u8], Instant::now(), Duration::from_secs(12)).is_empty());
    }
}
