//! JNI-кодек групп (MLS). Мутации (create/invite/sendText) — fire-and-forget в
//! движок; read-данные (list/history/members/presence) — снапшоты движка.

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use peerage_engine::parse_key;

use crate::{engine, jni_safe, jstr, ret};

/// Создать группу и позвать `invite_json` (JSON-массив hex-ключей; кривые ключи
/// пропускаются). Fire-and-forget: результат — через событие `groups_changed`.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupCreate<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    name: JString<'local>,
    invite_json: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let keys: Vec<String> = serde_json::from_str(&jstr(&mut env, &invite_json)).unwrap_or_default();
        let invite = keys.iter().filter_map(|k| parse_key(k)).collect();
        if e.group_create_inviting(&jstr(&mut env, &name), invite) { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Пригласить контакт в группу. Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupInvite<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_groupInvites<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_acceptGroupInvite<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_declineGroupInvite<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_groupSendText<'local>(
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

/// Текст в группу ОТВЕТОМ на сообщение `replyTo` автора `replySender` (hex;
/// `replyTo = 0` — не ответ). `mentionsJson` — `[{"at","len","key"}]`, байты UTF-8.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupSendTextReply<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    text: JString<'local>,
    reply_to: jlong,
    reply_sender: JString<'local>,
    mentions_json: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let text = jstr(&mut env, &text);
        let sender = jstr(&mut env, &reply_sender);
        let mentions = crate::parse_mentions(&jstr(&mut env, &mentions_json));
        e.group_send_text_reply(gid, text, reply_to.max(0) as u64, sender, mentions);
    });
}

/// Удалить групповые сообщения У СЕБЯ. `refsJson` — `[{"id":…,"sender":"hex"}, …]`.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupDeleteMessages<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_groupRetractMessages<'local>(
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

/// Собрать [`peerage_engine::MediaInfo`] из аргументов JNI (длительность/кадр/превью/
/// подпись). Метаданные извлекает клиент — движок медиа не декодирует.
fn media_info<'local>(
    env: &mut JNIEnv<'local>,
    duration_ms: jint,
    width: jint,
    height: jint,
    thumb: &JByteArray<'local>,
    caption: &JString<'local>,
    reply_to: jlong,
    reply_sender: &JString<'local>,
) -> peerage_engine::MediaInfo {
    let thumb_bytes = env.convert_byte_array(thumb).unwrap_or_default();
    peerage_engine::MediaInfo {
        fwd_from: String::new(),
        fwd_anon: false,
        duration_ms: duration_ms.max(0) as u32,
        width: width.max(0) as u32,
        height: height.max(0) as u32,
        thumb: (!thumb_bytes.is_empty()).then_some(thumb_bytes),
        caption: jstr(env, caption),
        reply_to: reply_to.max(0) as u64,
        reply_sender: jstr(env, reply_sender),
    }
}

/// Отправить медиа-вложение в группу (M2): scrub + MLS-ссылка всем. Мелкое движок
/// пушит участникам сразу, крупное они запрашивают сами. Fire-and-forget.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_chat_peerage_Native_groupSendMedia<'local>(
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
    reply_to: jlong,
    reply_sender_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let bytes = env.convert_byte_array(&content).unwrap_or_default();
        if bytes.is_empty() { return; }
        let (mime, name) = (jstr(&mut env, &mime), jstr(&mut env, &name));
        let info =
            media_info(&mut env, duration_ms, width, height, &thumb, &caption, reply_to, &reply_sender_hex);
        e.group_send_media(gid, bytes, mime, name, info);
    });
}

/// Отправить в группу вложение ФАЙЛОМ (потоково). Fire-and-forget.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_chat_peerage_Native_groupSendMediaFile<'local>(
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
    reply_to: jlong,
    reply_sender_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let path = std::path::PathBuf::from(jstr(&mut env, &path));
        let (mime, name) = (jstr(&mut env, &mime), jstr(&mut env, &name));
        let info =
            media_info(&mut env, duration_ms, width, height, &thumb, &caption, reply_to, &reply_sender_hex);
        e.group_send_media_file(gid, path, mime, name, info);
    });
}

/// Отозвать участника группы (ADR-004, только админ). Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupRemoveMember<'local>(
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

/// Передать права админа участнику (шаг 7j); `leaving` — и выйти. Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupTransferAdmin<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    peer_hex: JString<'local>,
    leaving: jboolean,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let (Some(gid), Some(peer)) = (
            hex::decode(jstr(&mut env, &gid_hex).trim()).ok(),
            parse_key(&jstr(&mut env, &peer_hex)),
        ) else { return };
        e.group_transfer_admin(gid, peer, leaving != JNI_FALSE);
    });
}

/// Фото группы (только админ): байты картинки, пусто — убрать. Возвращает hex блоба.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupSetPhoto<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    content: JByteArray<'local>,
    mime: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "") };
        let bytes = env.convert_byte_array(&content).unwrap_or_default();
        let mime = jstr(&mut env, &mime);
        ret(&env, &e.group_set_photo(gid, bytes, &mime))
    })
}

/// Переименовать группу (только админ). Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupRename<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    name: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        e.group_rename(gid, &jstr(&mut env, &name));
    });
}

/// Режим уведомлений участникам по умолчанию (`all` / `mentions`; только админ).
/// Fire-and-forget: итог — `groups_changed`.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupSetNotifyDefault<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
    mode: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return };
        let Some(mode) = peerage_core::notify::NotifyMode::parse(&jstr(&mut env, &mode)) else { return };
        e.group_set_notify_default(gid, mode);
    });
}

/// Ротация ключа группы (PCS, ADR-004). Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupRekey<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_groupLeave<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_groupList<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_groupHistory<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_groupHistoryPage<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_groupMembers<'local>(
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

/// Позванные в группу и ещё не вступившие (hex): JSON-массив. Снапшот движка.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_groupInvited<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "[]") };
        ret(&env, &e.group_invited_json(&gid))
    })
}
