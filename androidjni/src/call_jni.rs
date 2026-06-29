//! JNI-кодек звонков/комнат (Discord-style присутствие + DM-звонки).
//! Тонкий слой: декодирует аргументы и зовёт методы [`p2p_engine::Engine`].

use jni::objects::{JByteArray, JClass, JShortArray, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_FALSE};
use jni::JNIEnv;

use p2p_engine::parse_key;

use crate::{engine, jni_safe, jstr, ret};

/// hex(gid) из JString или ранний возврат дефолта.
macro_rules! gid_or {
    ($env:expr, $s:expr, $ret:expr) => {
        match hex::decode(jstr(&mut $env, &$s).trim()) { Ok(g) => g, Err(_) => return $ret }
    };
}

/// Позвонить пиру: создать DM-комнату, отправить RoomInvite, слать маячок.
/// Возвращает room_id hex.
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
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        let Some(peer) = parse_key(&jstr(&mut env, &peer_hex)) else { return ret(&env, "") };
        let room_id = e.room_invite_dm(peer, audio != JNI_FALSE, video != JNI_FALSE);
        ret(&env, &hex::encode(&room_id))
    })
}

/// Отклонить входящий DM-звонок.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomDeclineDm<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        if let Some(peer) = parse_key(&jstr(&mut env, &peer_hex)) { e.room_decline_dm(peer); }
    });
}

/// Принять DM-звонок: войти в комнату, зарегистрировать dm_peer. Возвращает room_id hex.
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
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        let Some(peer) = parse_key(&jstr(&mut env, &peer_hex)) else { return ret(&env, "") };
        let room_id = e.room_join_dm(peer, audio != JNI_FALSE, video != JNI_FALSE);
        ret(&env, &hex::encode(&room_id))
    })
}

/// Живые участники комнаты по маячкам: JSON [{peer,audio,video}].
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomPresencePeers<'local>(
    mut env: JNIEnv<'local>, _class: JClass<'local>,
    session_ptr: jlong, gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let gid = gid_or!(env, gid_hex, ret(&env, "[]"));
        ret(&env, &e.room_presence_peers_json(&gid))
    })
}

/// Войти в комнату `gid` (начать слать маячок присутствия).
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
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let gid = gid_or!(env, gid_hex, ());
        e.room_join(gid, audio != JNI_FALSE, video != JNI_FALSE);
    });
}

/// Выйти из комнаты `gid`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomLeave<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let gid = gid_or!(env, gid_hex, ());
        e.room_leave(gid);
    });
}

/// Текущие участники комнаты `gid`: JSON [{peer,audio,video}].
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomParticipants<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let gid = gid_or!(env, gid_hex, ret(&env, "[]"));
        ret(&env, &e.room_participants_json(&gid))
    })
}

/// Запустить мультипати аудио комнаты `gid`. JSON {sample_rate,channels,frame_len}.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomAudioStart<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    bitrate: jint,
    music: jboolean,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        let gid = gid_or!(env, gid_hex, ret(&env, "{}"));
        ret(&env, &e.room_audio_start(gid, bitrate, music != JNI_FALSE))
    })
}

/// Hex-ключи говорящих сейчас участников (active-speaker). JSON-массив.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomSpeakingPeers<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.room_speaking_json())
    })
}

/// Сменить Opus-битрейт текущей аудио-сессии на лету.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomSetAudioBitrate<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    bitrate: jint,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } {
            e.room_set_audio_bitrate(bitrate);
        }
    });
}

/// Остановить мультипати аудио комнаты.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomAudioStop<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } { e.room_audio_stop(); }
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
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let n = n.max(0) as usize;
        if n == 0 { return; }
        let mut buf = vec![0i16; n];
        if env.get_short_array_region(&data, 0, &mut buf).is_err() { return; }
        e.room_push_audio(&buf);
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
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        let len = env.get_array_length(&out).unwrap_or(0).max(0) as usize;
        if len == 0 { return 0; }
        let mut buf = vec![0i16; len];
        let n = e.room_pull_audio(&mut buf);
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
    if let Some(e) = unsafe { engine(session_ptr) } { e.room_set_muted(muted != JNI_FALSE); }
}

/// Закодированный H264 access-unit → фрагментация + мультикаст всем участникам.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomPushVideo<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    data: JByteArray<'local>,
    keyframe: jboolean,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let bytes = env.convert_byte_array(&data).unwrap_or_default();
        if bytes.is_empty() { return; }
        e.room_push_video(&bytes, keyframe != JNI_FALSE);
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
        if let Some(e) = unsafe { engine(session_ptr) } { e.room_set_video_cap(cap.max(1) as usize); }
    });
}

/// Следующий входящий H264-пакет от peer. Пустой массив если очередь пуста.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_roomPullVideoPacket<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
) -> JByteArray<'local> {
    let empty = env.new_byte_array(0).unwrap();
    let Some(e) = (unsafe { engine(session_ptr) }) else { return empty };
    let Some(key) = parse_key(&jstr(&mut env, &peer_hex)) else { return empty };
    match e.room_pull_video_packet(&key) {
        Some(f) => {
            let arr = env.new_byte_array(f.len() as i32).unwrap();
            // SAFETY: u8 и i8 одинаковый размер/выравнивание; JNI хранит байты.
            let signed = unsafe { std::slice::from_raw_parts(f.as_ptr() as *const i8, f.len()) };
            let _ = env.set_byte_array_region(&arr, 0, signed);
            arr
        }
        None => empty,
    }
}
