//! JNI-обёртки звонков/комнат (Discord-style присутствие + DM-звонки).
//!
//! Вынесено из `lib.rs` (которая разрослась): эти функции обращаются к
//! `pub(crate)`-полям и методам [`crate::Session`] напрямую. Логика не менялась
//! при выносе — чистая нарезка по доменам.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use jni::objects::{JByteArray, JClass, JShortArray, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_FALSE};
use jni::JNIEnv;

use p2p_core::settings::AudioSettings;

use crate::room_call::RoomCallEngine;
use crate::{
    jni_safe, jstr, parse_key, ret, Job, LockRecover, RoomCall, Session, PRESENCE_TTL,
};

/// JSON-параметры для настройки аудио на стороне Kotlin (AudioRecord/Track).
fn call_params(call_id: u64, audio: &AudioSettings) -> String {
    serde_json::json!({
        "call_id": call_id,
        "sample_rate": audio.sample_rate,
        "channels": audio.channels,
        "frame_len": audio.interleaved_len(),
    })
    .to_string()
}

// ── Комнаты-звонки (присутствие + DM-звонки, Discord-style) ─────────────────

/// Позвонить пиру: создать DM-комнату (room_id = sha256(sorted keys)[..16]),
/// отправить RoomInvite и начать слать маячок. Возвращает room_id hex.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomInviteDm<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
    audio: jboolean,
    video: jboolean,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        if session_ptr == 0 { return ret(&env, ""); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Some(peer) = parse_key(&jstr(&mut env, &peer_hex)) else { return ret(&env, "") };
        let room_id = p2p_core::dm_room_id(&session.me_pub, &peer);
        let audio_b = audio != JNI_FALSE;
        let video_b = video != JNI_FALSE;
        let _ = session.messenger.send_room_invite(&peer, &room_id, audio_b, video_b);
        let mut flags = 0u8;
        if audio_b { flags |= 0b01; }
        if video_b { flags |= 0b10; }
        {
            let mut rs = session.rooms.lock_safe();
            rs.joined.insert(room_id.clone(), flags);
            rs.dm_peers.insert(room_id.clone(), peer);
        }
        let _ = session.jobs.send(Job::Beacon);
        ret(&env, &hex::encode(&room_id))
    })
}

/// Отклонить входящий DM-звонок. Шлём RoomDecline пиру, не вступаем в комнату.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomDeclineDm<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Some(peer) = parse_key(&jstr(&mut env, &peer_hex)) else { return };
        let room_id = p2p_core::dm_room_id(&session.me_pub, &peer);
        let _ = session.messenger.send_room_decline(&peer, &room_id);
    });
}

/// Принять DM-звонок: вычислить room_id, зарегистрировать dm_peer (маячки пойдут
/// к пиру, а не MLS-участникам), войти в комнату. Возвращает room_id hex.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomJoinDm<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
    audio: jboolean,
    video: jboolean,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        if session_ptr == 0 { return ret(&env, ""); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Some(peer) = parse_key(&jstr(&mut env, &peer_hex)) else { return ret(&env, "") };
        let room_id = p2p_core::dm_room_id(&session.me_pub, &peer);
        let mut flags = 0u8;
        if audio != JNI_FALSE { flags |= 0b01; }
        if video != JNI_FALSE { flags |= 0b10; }
        {
            let mut rs = session.rooms.lock_safe();
            rs.joined.insert(room_id.clone(), flags);
            rs.dm_peers.insert(room_id.clone(), peer);
        }
        let _ = session.jobs.send(Job::Beacon);
        ret(&env, &hex::encode(&room_id))
    })
}

/// Живые участники комнаты `gid` по последним маячкам (не старше PRESENCE_TTL).
/// Возвращает JSON: `[{"peer":"hex","audio":bool,"video":bool},...]`.
/// Работает для ЛЮБОЙ комнаты, не только для той, в которой мы сами находимся.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomPresencePeers<'local>(
    mut env: JNIEnv<'local>, _class: JClass<'local>,
    session_ptr: jlong, gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        if session_ptr == 0 { return ret(&env, "[]"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let gid_str = jstr(&mut env, &gid_hex);
        let Ok(gid) = hex::decode(gid_str.trim()) else { return ret(&env, "[]"); };
        let arr: Vec<serde_json::Value> = session.live_participants_with_flags(&gid)
            .into_iter()
            .map(|(p, f)| serde_json::json!({
                "peer": hex::encode(p),
                "audio": f & 0x01 != 0,
                "video": f & 0x02 != 0,
            }))
            .collect();
        ret(&env, &serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into()))
    })
}

/// Войти в комнату `gid` (начать слать маячок присутствия). `audio`/`video` —
/// наши текущие A/V-флаги (для H1.0 медиа ещё нет, флаги информативны).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomJoin<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    audio: jboolean,
    video: jboolean,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let mut flags = 0u8;
        if audio != JNI_FALSE { flags |= 0b01; }
        if video != JNI_FALSE { flags |= 0b10; }
        session.rooms.lock_safe().joined.insert(gid, flags);
        let _ = session.jobs.send(Job::Beacon);
    });
}

/// Выйти из комнаты `gid` (перестать слать маячок, забыть её присутствие).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomLeave<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        if let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) {
            { let mut rs = session.rooms.lock_safe(); rs.joined.remove(&gid); rs.remote.remove(&gid); }
            let _ = session.jobs.send(Job::Leave(gid));
        }
    });
}

/// Текущие участники комнаты `gid` (чей маячок свежий): JSON `[{peer,audio,video}]`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomParticipants<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        if session_ptr == 0 { return ret(&env, "[]"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "[]") };
        let now = Instant::now();
        let rs = session.rooms.lock_safe();
        let arr: Vec<serde_json::Value> = rs.remote.get(&gid).map(|peers| {
            peers.iter()
                .filter(|(_, (t, _))| now.saturating_duration_since(*t) < PRESENCE_TTL)
                .map(|(p, (_, f))| serde_json::json!({
                    "peer": hex::encode(p), "audio": f & 0b01 != 0, "video": f & 0b10 != 0,
                }))
                .collect()
        }).unwrap_or_default();
        ret(&env, &serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into()))
    })
}

/// Запустить мультипати аудио комнаты `gid`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomAudioStart<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        if session_ptr == 0 { return ret(&env, "{}"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "{}") };
        let mut audio = AudioSettings::default();
        audio.jitter_depth = 5;
        let members = Arc::new(Mutex::new(session.live_participants(&gid)));
        let engine = RoomCallEngine::start(session.messenger.clone(), audio, members.clone());
        let old = session.room_call.lock_safe().replace(RoomCall { gid, engine, members });
        if let Some(old) = old { std::thread::spawn(move || old.engine.stop()); }
        ret(&env, &call_params(0, &audio))
    })
}

/// Остановить мультипати аудио комнаты.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomAudioStop<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let rc = session.room_call.lock_safe().take();
        if let Some(rc) = rc { std::thread::spawn(move || rc.engine.stop()); }
    });
}

/// Захваченный PCM с микрофона → мультикаст-движку комнаты.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomPushAudio<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    data: JShortArray<'local>,
    n: jint,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let n = n.max(0) as usize;
        if n == 0 { return; }
        let mut buf = vec![0i16; n];
        if env.get_short_array_region(&data, 0, &mut buf).is_err() { return; }
        if let Some(engine) = session.room_engine() {
            engine.push_audio(&buf);
        }
    });
}

/// Заполнить массив смешанным PCM комнаты. Возвращает число реальных сэмплов.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomPullAudio<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    out: JShortArray<'local>,
) -> jint {
    jni_safe!(0, {
        if session_ptr == 0 { return 0; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let len = env.get_array_length(&out).unwrap_or(0).max(0) as usize;
        if len == 0 { return 0; }
        let mut buf = vec![0i16; len];
        let n = session.room_engine().map(|e| e.pull_audio(&mut buf)).unwrap_or(0);
        let _ = env.set_short_array_region(&out, 0, &buf);
        n as jint
    })
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomSetMuted<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    muted: jboolean,
) {
    if session_ptr == 0 { return; }
    let session = unsafe { &*(session_ptr as *const Arc<Session>) };
    if let Some(engine) = session.room_engine() {
        engine.set_muted(muted != JNI_FALSE);
    }
}

/// Закодированный H264 access-unit → фрагментация + мультикаст всем участникам комнаты.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomPushVideo<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    data: JByteArray<'local>,
    keyframe: jboolean,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let bytes = env.convert_byte_array(&data).unwrap_or_default();
        if bytes.is_empty() { return; }
        if let Some(engine) = session.room_engine() {
            engine.push_video(&bytes, keyframe != JNI_FALSE);
        }
    });
}

/// Задать кап входящей видео-очереди (кадров) — политику бюджета считает Kotlin.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomSetVideoCap<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    cap: jint,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        if let Some(engine) = session.room_engine() {
            engine.set_video_cap(cap.max(1) as usize);
        }
    });
}

/// Следующий входящий H264-пакет от peer. Возвращает пустой массив если очередь пуста.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomPullVideoPacket<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
) -> JByteArray<'local> {
    if session_ptr == 0 { return env.new_byte_array(0).unwrap(); }
    let session = unsafe { &*(session_ptr as *const Arc<Session>) };
    let hex: String = match env.get_string(&peer_hex) {
        Ok(s) => s.into(),
        Err(_) => return env.new_byte_array(0).unwrap(),
    };
    let mut key = [0u8; 32];
    if hex::decode_to_slice(&hex, &mut key).is_err() {
        return env.new_byte_array(0).unwrap();
    }
    let frame = session.room_engine().and_then(|e| e.pull_video_for(&key));
    match frame {
        Some(f) => {
            let arr = env.new_byte_array(f.len() as i32).unwrap();
            // SAFETY: u8 и i8 одинаковый размер/выравнивание; JNI хранит байты.
            let signed = unsafe { std::slice::from_raw_parts(f.as_ptr() as *const i8, f.len()) };
            let _ = env.set_byte_array_region(&arr, 0, signed);
            arr
        }
        None => env.new_byte_array(0).unwrap(),
    }
}
