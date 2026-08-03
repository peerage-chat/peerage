//! JNI-кодек групп (MLS). Мутации (create/invite/sendText) — fire-and-forget в
//! движок; read-данные (list/history/members/presence) — снапшоты движка.

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use p2p_engine::parse_key;

use crate::{engine, jni_safe, jstr, ret};

/// Создать группу. Fire-and-forget: результат — через событие `groups_changed`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupCreate<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    name: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        if e.group_create(&jstr(&mut env, &name)) { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Пригласить контакт в группу. Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupInvite<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    peer_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let (Some(gid), Some(peer)) = (
            hex::decode(jstr(&mut env, &gid_hex).trim()).ok(),
            parse_key(&jstr(&mut env, &peer_hex)),
        ) else { return };
        e.group_invite(gid, peer);
    });
}

/// Входящие приглашения в группы: JSON [{gid, name, inviter}]. Снапшот движка.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupInvites<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.group_invites_json())
    })
}

/// Принять приглашение в группу (карточка). Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_acceptGroupInvite<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        e.accept_group_invite(gid);
    });
}

/// Отклонить приглашение в группу. Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_declineGroupInvite<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        e.decline_group_invite(gid);
    });
}

/// Отправить текст в группу. Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupSendText<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    text: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        e.group_send_text(gid, jstr(&mut env, &text));
    });
}

/// Текст в группу ОТВЕТОМ на сообщение `replyTo` автора `replySender` (hex).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupSendTextReply<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    text: JString<'local>,
    reply_to: jlong,
    reply_sender: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let text = jstr(&mut env, &text);
        let sender = jstr(&mut env, &reply_sender);
        e.group_send_text_reply(gid, text, reply_to.max(0) as u64, sender);
    });
}

/// Удалить групповые сообщения У СЕБЯ. `refsJson` — `[{"id":…,"sender":"hex"}, …]`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupDeleteMessages<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    refs_json: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let raw: serde_json::Value =
            serde_json::from_str(&jstr(&mut env, &refs_json)).unwrap_or(serde_json::Value::Null);
        let refs: Vec<(u64, String)> = raw
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some((v["id"].as_u64()?, v["sender"].as_str().unwrap_or("").to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        e.group_delete_local(gid, refs);
    });
}

/// Отозвать СВОИ групповые сообщения у участников. `idsJson` — `[123, …]`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupRetractMessages<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    ids_json: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let ids: Vec<u64> = serde_json::from_str(&jstr(&mut env, &ids_json)).unwrap_or_default();
        e.group_retract(gid, ids);
    });
}

/// Собрать [`p2p_engine::MediaInfo`] из аргументов JNI (длительность/кадр/превью/
/// подпись). Метаданные извлекает клиент — движок медиа не декодирует.
fn media_info<'local>(
    env: &mut JNIEnv<'local>,
    duration_ms: jint,
    width: jint,
    height: jint,
    thumb: &JByteArray<'local>,
    caption: &JString<'local>,
) -> p2p_engine::MediaInfo {
    let thumb_bytes = env.convert_byte_array(thumb).unwrap_or_default();
    p2p_engine::MediaInfo {
        fwd_from: String::new(),
        duration_ms: duration_ms.max(0) as u32,
        width: width.max(0) as u32,
        height: height.max(0) as u32,
        thumb: (!thumb_bytes.is_empty()).then_some(thumb_bytes),
        caption: jstr(env, caption),
    }
}

/// Отправить медиа-вложение в группу (M2): scrub + MLS-ссылка всем. Мелкое движок
/// пушит участникам сразу, крупное они запрашивают сами. Fire-and-forget.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupSendMedia<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    content: JByteArray<'local>,
    mime: JString<'local>,
    name: JString<'local>,
    duration_ms: jint,
    width: jint,
    height: jint,
    thumb: JByteArray<'local>,
    caption: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let bytes = env.convert_byte_array(&content).unwrap_or_default();
        if bytes.is_empty() { return; }
        let (mime, name) = (jstr(&mut env, &mime), jstr(&mut env, &name));
        let info = media_info(&mut env, duration_ms, width, height, &thumb, &caption);
        e.group_send_media(gid, bytes, mime, name, info);
    });
}

/// Отправить в группу вложение ФАЙЛОМ (потоково). Fire-and-forget.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupSendMediaFile<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    path: JString<'local>,
    mime: JString<'local>,
    name: JString<'local>,
    duration_ms: jint,
    width: jint,
    height: jint,
    thumb: JByteArray<'local>,
    caption: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let path = std::path::PathBuf::from(jstr(&mut env, &path));
        let (mime, name) = (jstr(&mut env, &mime), jstr(&mut env, &name));
        let info = media_info(&mut env, duration_ms, width, height, &thumb, &caption);
        e.group_send_media_file(gid, path, mime, name, info);
    });
}

/// Отозвать участника группы (ADR-004, только админ). Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupRemoveMember<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    peer_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let (Some(gid), Some(peer)) = (
            hex::decode(jstr(&mut env, &gid_hex).trim()).ok(),
            parse_key(&jstr(&mut env, &peer_hex)),
        ) else { return };
        e.group_remove_member(gid, peer);
    });
}

/// Ротация ключа группы (PCS, ADR-004). Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupRekey<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        e.group_rekey(gid);
    });
}

/// Выйти из группы (ADR-004). Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupLeave<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        e.group_leave(gid);
    });
}

/// Список групп: JSON [{gid, name, members}]. Снапшот движка.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupList<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.group_list_json())
    })
}

/// История группы: JSON [{text, mine, time, sender, system}]. Снапшот движка.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupHistory<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "[]") };
        ret(&env, &e.group_history_json(&gid))
    })
}

/// Страница истории группы ВЫШЕ показанной; курсор — `time`/`row` верхнего сообщения.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupHistoryPage<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    before_time: jlong,
    before_row: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "[]") };
        ret(&env, &e.group_history_page_json(&gid, before_time, before_row))
    })
}

/// Ключи участников группы (hex): JSON-массив. Снапшот движка.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupMembers<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "[]") };
        ret(&env, &e.group_members_json(&gid))
    })
}

/// Снапшот присутствия группы: JSON {total, online}. Агрегат считает движок.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupPresence<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "{}") };
        ret(&env, &serde_json::to_string(&e.group_presence(&gid)).unwrap_or_else(|_| "{}".into()))
    })
}
