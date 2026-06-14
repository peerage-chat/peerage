//! Десктоп-приложение: Holo-интерфейс на Slint (нативная отрисовка) поверх
//! движка p2p-messenger. Ядро/транспорт/мессенджер не зависят от UI.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use p2p_call::AudioCall;
use p2p_core::settings::{AudioSettings, OpusMode, Settings, VideoSettings};
use p2p_core::store::{ContactStore, MessageStore, Profile, StoredMessage};
use p2p_messenger::{Event, Messenger};
use p2p_ygg::Node;

slint::include_modules!();

const PUBLIC_PEERS: &str = include_str!("../../peers/public-peers.txt");

/// Общее состояние (Send+Sync) — доступно колбэкам UI и потоку событий.
struct Core {
    messenger: Arc<Messenger>,
    contacts: Mutex<ContactStore>,
    messages: Mutex<MessageStore>,
    me_pub: [u8; 32],
    current_chat: Mutex<Option<[u8; 32]>>,
    unread: Mutex<HashMap<[u8; 32], i32>>,
    search: Mutex<String>,
    settings: Mutex<Settings>,
    settings_path: std::path::PathBuf,
}

fn mode_str(m: OpusMode) -> &'static str {
    match m {
        OpusMode::Voip => "voip",
        OpusMode::Audio => "audio",
        OpusMode::LowDelay => "low_delay",
    }
}
fn parse_mode(s: &str) -> OpusMode {
    match s {
        "voip" => OpusMode::Voip,
        "low_delay" => OpusMode::LowDelay,
        _ => OpusMode::Audio,
    }
}
fn one_of<T: PartialEq + Copy>(v: T, allowed: &[T], default: T) -> T {
    if allowed.contains(&v) { v } else { default }
}

/// Заполнить поля экрана настроек из Settings.
fn populate_settings(ui: &AppWindow, s: &Settings) {
    ui.set_cfg_sample_rate(s.audio.sample_rate.to_string().into());
    ui.set_cfg_channels(s.audio.channels.to_string().into());
    ui.set_cfg_bitrate(s.audio.bitrate.to_string().into());
    ui.set_cfg_frame_ms(s.audio.frame_ms.to_string().into());
    ui.set_cfg_jitter(s.audio.jitter_depth.to_string().into());
    ui.set_cfg_mode(mode_str(s.audio.mode).into());
    ui.set_cfg_fec(s.audio.fec);
    ui.set_cfg_vwidth(s.video.width.to_string().into());
    ui.set_cfg_vheight(s.video.height.to_string().into());
    ui.set_cfg_vfps(s.video.fps.to_string().into());
    ui.set_cfg_vbitrate(s.video.bitrate.to_string().into());
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
fn parse_key(s: &str) -> Option<[u8; 32]> {
    hex::decode(s.trim()).ok()?.as_slice().try_into().ok()
}
fn spawn_connect(m: Arc<Messenger>, key: [u8; 32]) {
    std::thread::spawn(move || loop {
        let _ = m.connect(&key, Duration::from_secs(20));
        std::thread::sleep(Duration::from_secs(5));
    });
}
fn status_mark(s: &str) -> &'static str {
    match s {
        "delivered" => "✓✓",
        "failed" => "не доставлено",
        "sent" => "✓",
        _ => "",
    }
}

/// TOFU: добавить неизвестного пира в контакты. Возвращает true, если новый.
fn ensure_contact(core: &Arc<Core>, peer: [u8; 32]) -> bool {
    let mut store = core.contacts.lock().unwrap();
    if store.contains(&peer) {
        false
    } else {
        let _ = store.add(&peer, "Без имени", now());
        drop(store);
        spawn_connect(core.messenger.clone(), peer);
        true
    }
}

fn rebuild_contacts(ui: &AppWindow, core: &Arc<Core>) {
    let term = core.search.lock().unwrap().to_lowercase();
    let unread = core.unread.lock().unwrap();
    let store = core.contacts.lock().unwrap();
    let mut items: Vec<ContactItem> = store
        .list()
        .iter()
        .filter(|c| term.is_empty() || c.name.to_lowercase().contains(&term))
        .map(|c| {
            let key = parse_key(&c.key).unwrap_or([0u8; 32]);
            let short_fp = c.fingerprint.split_whitespace().take(3).collect::<Vec<_>>().join(" ") + "…";
            ContactItem {
                key: c.key.clone().into(),
                name: c.name.clone().into(),
                fingerprint: short_fp.into(),
                online: core.messenger.is_online(&key),
                unread: *unread.get(&key).unwrap_or(&0),
            }
        })
        .collect();
    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    ui.set_contacts(ModelRc::new(VecModel::from(items)));
}

fn rebuild_messages(ui: &AppWindow, core: &Arc<Core>) {
    let Some(key) = *core.current_chat.lock().unwrap() else { return };
    let store = core.messages.lock().unwrap();
    let items: Vec<MsgItem> = store
        .history(&key)
        .iter()
        .map(|m| MsgItem {
            text: m.text.clone().into(),
            mine: m.mine,
            time: fmt_time(m.time).into(),
            status: status_mark(&m.status).into(),
        })
        .collect();
    ui.set_messages(ModelRc::new(VecModel::from(items)));
}

fn fmt_time(t: i64) -> String {
    // Локальное HH:MM без внешних зависимостей.
    let secs = t.rem_euclid(86400);
    let h = (secs / 3600) as u32;
    let m = (secs % 3600 / 60) as u32;
    format!("{h:02}:{m:02}")
}

fn update_net(ui: &AppWindow, core: &Arc<Core>) {
    let peers = core.messenger.net_peers();
    let tree = core.messenger.net_tree();
    let online = peers >= 1 && tree > 2;
    ui.set_net_online(online);
    ui.set_net_status(if online { format!("в сети · пиров {peers}") } else { format!("подключение… ({peers})") }.into());
}

// Состояние звонка живёт на UI-потоке (AudioCall !Send). Все обращения — с UI.
#[derive(Default)]
struct CallCtx {
    call_id: Option<u64>,
    peer: Option<[u8; 32]>,
    incoming: bool,
    active: bool,
    muted: bool,
    started: Option<Instant>,
    call: Option<AudioCall>,
}

thread_local! {
    static CALL: RefCell<CallCtx> = RefCell::new(CallCtx::default());
}

fn gen_call_id() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

fn name_of(core: &Arc<Core>, peer: &[u8; 32]) -> String {
    let hexk = hex::encode(peer);
    core.contacts
        .lock()
        .unwrap()
        .get(peer)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| hexk[..8].to_string())
}

fn refresh_call_ui(ui: &AppWindow, core: &Arc<Core>) {
    CALL.with(|c| {
        let c = c.borrow();
        let peer_name = c.peer.map(|p| name_of(core, &p)).unwrap_or_default();
        ui.set_call_peer(peer_name.into());
        ui.set_call_incoming(c.incoming);
        ui.set_call_active(c.active);
        ui.set_call_muted(c.muted);
        let state = if c.incoming {
            "Входящий звонок"
        } else if c.active {
            "Идёт звонок"
        } else {
            "Звоним…"
        };
        ui.set_call_state(state.into());
    });
}

fn start_audio(core: &Arc<Core>, peer: [u8; 32]) -> Option<AudioCall> {
    let settings = core.settings.lock().unwrap().audio;
    match AudioCall::start(core.messenger.clone(), peer, settings) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("[call] не удалось начать звук: {e}");
            None
        }
    }
}

fn reset_call(ui: &AppWindow, core: &Arc<Core>) {
    CALL.with(|c| *c.borrow_mut() = CallCtx::default());
    let back = if core.current_chat.lock().unwrap().is_some() { 3 } else { 0 };
    ui.set_screen(back);
}

fn main() -> Result<(), slint::PlatformError> {
    // --- движок ---
    let dir = match std::env::var_os("P2P_DATA_DIR") {
        Some(d) => std::path::PathBuf::from(d),
        None => directories_data_dir(),
    };
    std::fs::create_dir_all(&dir).ok();

    let identity = Profile::load_or_create(&dir.join("profile.json")).expect("профиль").identity;
    let me_pub = identity.public_key_bytes();
    let me_words: Vec<SharedString> = identity.fingerprint_words().iter().map(|s| (*s).into()).collect();

    let node = Node::start(&identity.seed()).expect("узел");
    let address = node.address();
    for uri in PUBLIC_PEERS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let _ = node.add_peer(uri);
    }

    let contacts = ContactStore::load(&dir.join("contacts.json")).expect("контакты");
    let messages = MessageStore::load(&dir.join("messages.json")).expect("сообщения");
    let contact_keys: Vec<[u8; 32]> = contacts.list().iter().filter_map(|c| parse_key(&c.key)).collect();

    let (messenger, rx) = Messenger::start(identity, node);
    let messenger = Arc::new(messenger);
    for k in &contact_keys {
        spawn_connect(messenger.clone(), *k);
    }

    let settings_path = dir.join("settings.json");
    let settings = Settings::load_or_create(&settings_path).unwrap_or_default();

    let core = Arc::new(Core {
        messenger,
        contacts: Mutex::new(contacts),
        messages: Mutex::new(messages),
        me_pub,
        current_chat: Mutex::new(None),
        unread: Mutex::new(HashMap::new()),
        search: Mutex::new(String::new()),
        settings: Mutex::new(settings),
        settings_path,
    });

    // --- UI ---
    let ui = AppWindow::new()?;
    ui.set_my_address(address.into());
    ui.set_my_pubkey(hex::encode(me_pub).into());
    ui.set_my_words(ModelRc::new(VecModel::from(me_words)));
    populate_settings(&ui, &core.settings.lock().unwrap());
    rebuild_contacts(&ui, &core);
    update_net(&ui, &core);

    // колбэки
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_search_changed(move |t| {
            *c.search.lock().unwrap() = t.to_string();
            rebuild_contacts(&w.unwrap(), &c);
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_open_chat(move |key_str| {
            let ui = w.unwrap();
            let Some(key) = parse_key(&key_str) else { return };
            *c.current_chat.lock().unwrap() = Some(key);
            c.unread.lock().unwrap().insert(key, 0);
            let (name, online) = {
                let store = c.contacts.lock().unwrap();
                let name = store.get(&key).map(|x| x.name.clone()).unwrap_or_default();
                (name, c.messenger.is_online(&key))
            };
            ui.set_chat_name(name.into());
            ui.set_chat_online(online);
            rebuild_messages(&ui, &c);
            rebuild_contacts(&ui, &c);
            ui.set_screen(3);
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_send_message(move |text| {
            let text = text.trim().to_string();
            if text.is_empty() { return; }
            let Some(key) = *c.current_chat.lock().unwrap() else { return };
            if let Ok(id) = c.messenger.send_text(&key, &text) {
                let _ = c.messages.lock().unwrap().append(&key, StoredMessage {
                    id, text, mine: true, time: now(), status: "sent".into(),
                });
                rebuild_messages(&w.unwrap(), &c);
            }
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_add_contact(move |key_str, name| {
            let ui = w.unwrap();
            let Some(key) = parse_key(&key_str) else {
                ui.set_add_error("неверный ключ (нужно 64 hex-символа)".into());
                return;
            };
            if key == c.me_pub {
                ui.set_add_error("это ваш собственный ключ".into());
                return;
            }
            let name = if name.trim().is_empty() { "Без имени".to_string() } else { name.trim().to_string() };
            if let Err(e) = c.contacts.lock().unwrap().add(&key, &name, now()) {
                ui.set_add_error(format!("{e}").into());
                return;
            }
            spawn_connect(c.messenger.clone(), key);
            ui.set_add_error("".into());
            rebuild_contacts(&ui, &c);
            ui.set_screen(0);
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_open_info(move || {
            let ui = w.unwrap();
            let Some(key) = *c.current_chat.lock().unwrap() else { return };
            let store = c.contacts.lock().unwrap();
            if let Some(ct) = store.get(&key) {
                ui.set_info_name(ct.name.clone().into());
                ui.set_info_key(ct.key.clone().into());
                let words: Vec<SharedString> = ct.fingerprint.split_whitespace().map(|w| w.into()).collect();
                ui.set_info_words(ModelRc::new(VecModel::from(words)));
                ui.set_info_online(c.messenger.is_online(&key));
                ui.set_info_error("".into());
                ui.set_screen(4);
            }
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_rename_contact(move |name| {
            let ui = w.unwrap();
            let Some(key) = *c.current_chat.lock().unwrap() else { return };
            if name.trim().is_empty() {
                ui.set_info_error("имя не может быть пустым".into());
                return;
            }
            let _ = c.contacts.lock().unwrap().rename(&key, name.trim());
            ui.set_chat_name(name.trim().into());
            rebuild_contacts(&ui, &c);
            ui.set_screen(3);
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_delete_contact(move || {
            let ui = w.unwrap();
            let Some(key) = *c.current_chat.lock().unwrap() else { return };
            let _ = c.contacts.lock().unwrap().remove(&key);
            let _ = c.messages.lock().unwrap().clear(&key);
            *c.current_chat.lock().unwrap() = None;
            rebuild_contacts(&ui, &c);
            ui.set_screen(0);
        });
    }
    {
        let w = ui.as_weak();
        ui.on_copy_pubkey(move || {
            let ui = w.unwrap();
            let key = ui.get_my_pubkey().to_string();
            let ok = arboard::Clipboard::new().and_then(|mut c| c.set_text(key)).is_ok();
            ui.set_copy_label(if ok { "Скопировано ✓".into() } else { "Не удалось скопировать".into() });
        });
    }

    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_save_settings(move || {
            let ui = w.unwrap();
            let pi = |s: slint::SharedString, d: i64| s.to_string().trim().parse::<i64>().unwrap_or(d);

            let sample_rate = one_of(
                pi(ui.get_cfg_sample_rate(), 48000) as u32,
                &[8000, 12000, 16000, 24000, 48000],
                48000,
            );
            let channels = one_of(pi(ui.get_cfg_channels(), 1) as u16, &[1, 2], 1);
            let bitrate = (pi(ui.get_cfg_bitrate(), 96000) as i32).clamp(6000, 510000);
            let frame_ms = one_of(pi(ui.get_cfg_frame_ms(), 20) as u32, &[10, 20, 40, 60], 20);
            let jitter_depth = (pi(ui.get_cfg_jitter(), 3) as usize).clamp(1, 20);
            let audio = AudioSettings {
                sample_rate,
                channels,
                bitrate,
                frame_ms,
                mode: parse_mode(&ui.get_cfg_mode()),
                fec: ui.get_cfg_fec(),
                jitter_depth,
            };
            let video = VideoSettings {
                width: (pi(ui.get_cfg_vwidth(), 1920) as u32).clamp(160, 7680),
                height: (pi(ui.get_cfg_vheight(), 1080) as u32).clamp(120, 4320),
                fps: (pi(ui.get_cfg_vfps(), 60) as u32).clamp(1, 120),
                bitrate: (pi(ui.get_cfg_vbitrate(), 8_000_000) as i32).clamp(100_000, 100_000_000),
            };
            let new = Settings { audio, video };

            *c.settings.lock().unwrap() = new;
            match new.save(&c.settings_path) {
                Ok(()) => ui.set_settings_status("Сохранено ✓ (применится к следующему звонку)".into()),
                Err(e) => ui.set_settings_status(format!("Ошибка: {e}").into()),
            }
            populate_settings(&ui, &new); // показать нормализованные значения
        });
    }

    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_start_call(move || {
            let ui = w.unwrap();
            let Some(peer) = *c.current_chat.lock().unwrap() else { return };
            let id = gen_call_id();
            let _ = c.messenger.send_call_invite(&peer, id, true, false);
            CALL.with(|cell| {
                *cell.borrow_mut() = CallCtx { call_id: Some(id), peer: Some(peer), ..Default::default() }
            });
            refresh_call_ui(&ui, &c);
            ui.set_screen(6);
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_accept_call(move || {
            let ui = w.unwrap();
            let info = CALL.with(|cell| { let cell = cell.borrow(); (cell.call_id, cell.peer) });
            if let (Some(id), Some(peer)) = info {
                let _ = c.messenger.send_call_accept(&peer, id);
                let ac = start_audio(&c, peer);
                CALL.with(|cell| {
                    let mut cell = cell.borrow_mut();
                    cell.active = ac.is_some();
                    cell.incoming = false;
                    cell.started = Some(Instant::now());
                    cell.call = ac;
                });
                refresh_call_ui(&ui, &c);
            }
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_hangup_call(move || {
            let ui = w.unwrap();
            let info = CALL.with(|cell| { let cell = cell.borrow(); (cell.call_id, cell.peer) });
            if let (Some(id), Some(peer)) = info {
                let _ = c.messenger.send_call_end(&peer, id);
            }
            reset_call(&ui, &c);
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        ui.on_toggle_mute(move || {
            let ui = w.unwrap();
            CALL.with(|cell| {
                let mut cell = cell.borrow_mut();
                cell.muted = !cell.muted;
                if let Some(call) = &cell.call {
                    call.set_muted(cell.muted);
                }
            });
            refresh_call_ui(&ui, &c);
        });
    }

    // --- поток событий движка → UI ---
    {
        let core_ev = core.clone();
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            for ev in rx.iter() {
                let core = core_ev.clone();
                let _ = weak.upgrade_in_event_loop(move |ui| handle_event(&ui, &core, ev));
            }
        });
    }

    // --- периодическое обновление сети/статусов ---
    let timer = slint::Timer::default();
    {
        let c = core.clone();
        let w = ui.as_weak();
        timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
            if let Some(ui) = w.upgrade() {
                update_net(&ui, &c);
                rebuild_contacts(&ui, &c);
                if ui.get_screen() == 3 {
                    if let Some(key) = *c.current_chat.lock().unwrap() {
                        ui.set_chat_online(c.messenger.is_online(&key));
                    }
                }
                // Длительность активного звонка.
                if let Some(secs) = CALL.with(|cell| {
                    let cell = cell.borrow();
                    if cell.active { cell.started.map(|s| s.elapsed().as_secs()) } else { None }
                }) {
                    ui.set_call_timer(format!("{:02}:{:02}", secs / 60, secs % 60).into());
                }
            }
        });
    }

    ui.run()
}

fn handle_event(ui: &AppWindow, core: &Arc<Core>, ev: Event) {
    match ev {
        Event::Presence { peer, online } => {
            ensure_contact(core, peer);
            rebuild_contacts(ui, core);
            if *core.current_chat.lock().unwrap() == Some(peer) {
                ui.set_chat_online(online);
            }
        }
        Event::Text { peer, id, text } => {
            ensure_contact(core, peer);
            let _ = core.messages.lock().unwrap().append(&peer, StoredMessage {
                id, text, mine: false, time: now(), status: String::new(),
            });
            let is_current = *core.current_chat.lock().unwrap() == Some(peer);
            if is_current {
                rebuild_messages(ui, core);
            } else {
                let mut u = core.unread.lock().unwrap();
                *u.entry(peer).or_insert(0) += 1;
            }
            rebuild_contacts(ui, core);
        }
        Event::Ack { peer, id } => {
            let _ = core.messages.lock().unwrap().set_status(&peer, id, "delivered");
            if *core.current_chat.lock().unwrap() == Some(peer) {
                rebuild_messages(ui, core);
            }
        }
        Event::Undelivered { peer, id } => {
            let _ = core.messages.lock().unwrap().set_status(&peer, id, "failed");
            if *core.current_chat.lock().unwrap() == Some(peer) {
                rebuild_messages(ui, core);
            }
        }
        Event::CallInvite { peer, call_id, .. } => {
            let busy = CALL.with(|c| c.borrow().call_id.is_some());
            if busy {
                let _ = core.messenger.send_call_end(&peer, call_id); // занято — отклоняем
            } else {
                CALL.with(|c| {
                    *c.borrow_mut() =
                        CallCtx { call_id: Some(call_id), peer: Some(peer), incoming: true, ..Default::default() }
                });
                refresh_call_ui(ui, core);
                ui.set_screen(6);
            }
        }
        Event::CallAccept { peer, call_id } => {
            let ours = CALL.with(|c| {
                let c = c.borrow();
                c.call_id == Some(call_id) && c.peer == Some(peer) && !c.active && !c.incoming
            });
            if ours {
                let ac = start_audio(core, peer);
                CALL.with(|c| {
                    let mut c = c.borrow_mut();
                    c.active = ac.is_some();
                    c.started = Some(Instant::now());
                    c.call = ac;
                });
                refresh_call_ui(ui, core);
            }
        }
        Event::CallEnd { call_id, .. } => {
            let ours = CALL.with(|c| c.borrow().call_id == Some(call_id));
            if ours {
                reset_call(ui, core);
            }
        }
    }
}

/// Каталог данных по умолчанию: $XDG_DATA_HOME/p2p-messenger или ~/.local/share/...
fn directories_data_dir() -> std::path::PathBuf {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        return std::path::PathBuf::from(x).join("p2p-messenger");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(h).join(".local/share/p2p-messenger");
    }
    std::path::PathBuf::from("./p2p-data")
}
