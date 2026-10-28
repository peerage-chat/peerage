//! JNI-кодек звонков/комнат (Discord-style присутствие + DM-звонки).
//! Тонкий слой: декодирует аргументы и зовёт методы [`peerage_engine::Engine`].

use jni::objects::{JByteArray, JClass, JShortArray, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use peerage_engine::parse_key;

use crate::{empty_bytes, engine, jni_safe, jstr, ret};

/// hex(gid) из JString или ранний возврат дефолта.
macro_rules! gid_or {
    ($env:expr, $s:expr, $ret:expr) => {
        match hex::decode(jstr(&mut $env, &$s).trim()) { Ok(g) => g, Err(_) => return $ret }
    };
}

/// Позвонить пиру: создать DM-комнату, отправить RoomInvite, слать маячок.
/// Возвращает room_id hex.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomInviteDm<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomDeclineDm<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomJoinDm<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomPresencePeers<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomJoin<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomLeave<'local>(
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

/// Позвать участников группы в её комнату. `peers_csv` — hex-ключи через запятую.
/// Возвращает, скольким ушло приглашение (ключи вне ростера не считаются).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomRing<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    peers_csv: JString<'local>,
    audio: jboolean,
    video: jboolean,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        let gid = gid_or!(env, gid_hex, 0);
        let csv = jstr(&mut env, &peers_csv);
        let peers: Vec<[u8; 32]> =
            csv.split(',').map(str::trim).filter(|s| !s.is_empty()).filter_map(parse_key).collect();
        e.room_ring(&gid, &peers, audio != JNI_FALSE, video != JNI_FALSE) as jint
    })
}

/// Отклонить приглашение в комнату группы (только журнал, звонящему ничего).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomDeclineGroup<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        e.room_decline_group();
    });
}

/// Текущие участники комнаты `gid`: JSON [{peer,audio,video}].
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomParticipants<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomAudioStart<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomSpeakingPeers<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.room_speaking_json())
    })
}

/// Накопленные байты трафика по участникам (JSON {hex: rx, "self": tx}).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomTrafficJson<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        ret(&env, &e.room_traffic_json())
    })
}

/// Сменить Opus-битрейт текущей аудио-сессии на лету.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomSetAudioBitrate<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomAudioStop<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomPushAudio<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    data: JShortArray<'local>,
    n: jint,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        // `n` — слово Kotlin; длина массива — факт. Больше длины читать нельзя:
        // JNI оставил бы висящее исключение, и оно выстрелило бы на следующем вызове.
        let len = env.get_array_length(&data).unwrap_or(0).max(0) as usize;
        let n = (n.max(0) as usize).min(len);
        if n == 0 { return; }
        let mut buf = vec![0i16; n];
        if env.get_short_array_region(&data, 0, &mut buf).is_err() { return; }
        e.room_push_audio(&buf);
    });
}

/// Заполнить массив смешанным PCM комнаты. Возвращает число реальных сэмплов.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomPullAudio<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_roomSetMuted<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    muted: jboolean,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } { e.room_set_muted(muted != JNI_FALSE); }
    });
}

/// Закодированный H264 access-unit → фрагментация + мультикаст всем участникам.
/// `rotation` — угол сенсора / 90 (приёмник доворачивает), `fps` — темп вывода у
/// приёмника; оба едут в протоколе LiteRTC. Возвращает `true`, если участник просит
/// ключевой кадр — кодеру выдать его следующим.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomPushVideo<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    data: JByteArray<'local>,
    keyframe: jboolean,
    rotation: jint,
    fps: jint,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let bytes = env.convert_byte_array(&data).unwrap_or_default();
        if bytes.is_empty() { return JNI_FALSE; }
        let info = litertc::VideoInfo {
            keyframe: keyframe != JNI_FALSE,
            rotation: rotation.rem_euclid(4) as u8,
            fps: fps.clamp(0, 255) as u8,
        };
        if e.room_push_video(&bytes, info) { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Декодер видео участника начат заново (тайл появился, декодер пересоздан) —
/// попросить у него ключевой кадр.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomRequestKeyframe<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Some(key) = parse_key(&jstr(&mut env, &peer_hex)) else { return };
        e.room_request_keyframe(&key);
    });
}

/// Задать кап входящей видео-очереди (кадров) — политику бюджета считает Kotlin.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomSetVideoCap<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    cap: jint,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } { e.room_set_video_cap(cap.max(1) as usize); }
    });
}

/// Следующий входящий H264-кадр от peer. Пустой массив если очередь пуста. Раскладка
/// (только для JNI, не протокол): `[поворот (четверти оборота)][fps][access-unit…]`.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_roomPullVideoPacket<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
) -> JByteArray<'local> {
    jni_safe!(empty_bytes(&env), {
        let empty = empty_bytes(&env);
        let Some(e) = (unsafe { engine(session_ptr) }) else { return empty };
        let Some(key) = parse_key(&jstr(&mut env, &peer_hex)) else { return empty };
        match e.room_pull_video(&key) {
            Some(f) => {
                let mut out = Vec::with_capacity(2 + f.data.len());
                out.push(f.info.rotation);
                out.push(f.info.fps);
                out.extend_from_slice(&f.data);
                env.byte_array_from_slice(&out).unwrap_or(empty)
            }
            None => empty,
        }
    })
}
