//! JNI-обёртки групп (MLS). Мутации (create/invite/sendText) уходят групповому
//! актору (поток event_pump) fire-and-forget; read-данные (list/history/members)
//! отдаются мгновенно из кэша [`crate::Session`]. Вынесено из `lib.rs`, логика
//! не менялась.

use std::sync::Arc;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use crate::{jni_safe, jstr, parse_key, ret, GroupCmd, Job, LockRecover, Session};

/// Снапшот присутствия группы (агрегат считает движок, не клиент — ADR-003).
/// `total` — всего участников (вкл. себя), `online` — сколько сейчас в сети (вкл. себя).
#[derive(serde::Serialize)]
struct GroupPresence {
    total: usize,
    online: usize,
}

// ── Группы (MLS) ───────────────────────────────────────────────────────────

// Команды уходят групповому актору (event_pump) fire-and-forget — результат
// приходит в UI через событие `groups_changed` + обновлённый кэш. Read-запросы
// (list/history/members) отдают кэш мгновенно, без блокировки event_pump.

/// Создать группу. Fire-and-forget: результат — через событие `groups_changed`.
/// Возвращает true (запрос принят).

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupCreate<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    name: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        if session_ptr == 0 { return JNI_FALSE; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let name = jstr(&mut env, &name);
        let sent = session.jobs.send(Job::Cmd(GroupCmd::Create { name })).is_ok();
        if sent { JNI_TRUE } else { JNI_FALSE }
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
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let (Some(gid), Some(peer)) = (
            hex::decode(jstr(&mut env, &gid_hex)).ok(),
            parse_key(&jstr(&mut env, &peer_hex)),
        ) else { return };
        let _ = session.jobs.send(Job::Cmd(GroupCmd::Invite { gid, peer }));
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
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Some(gid) = hex::decode(jstr(&mut env, &gid_hex)).ok() else { return };
        let text = jstr(&mut env, &text);
        let _ = session.jobs.send(Job::Cmd(GroupCmd::SendText { gid, text }));
    });
}

/// Список групп как JSON-массив [{gid, name, members}]. Читает кэш — мгновенно.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupList<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        if session_ptr == 0 { return ret(&env, "[]"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let json = session.group_cache.lock_safe().list.clone();
        ret(&env, if json.is_empty() { "[]" } else { &json })
    })
}

/// История группового чата как JSON-массив [{text, mine, time}]. Читает кэш.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupHistory<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        if session_ptr == 0 { return ret(&env, "[]"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "[]") };
        let json = session.group_cache.lock_safe().history.get(&gid).cloned().unwrap_or_else(|| "[]".into());
        ret(&env, &json)
    })
}

/// Ключи участников группы как JSON-массив. Читает кэш.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupMembers<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        if session_ptr == 0 { return ret(&env, "[]"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "[]") };
        let json = session.group_cache.lock_safe().members.get(&gid).cloned().unwrap_or_else(|| "[]".into());
        ret(&env, &json)
    })
}

/// Снапшот присутствия группы: JSON {total, online}. Агрегат «N уч · M в сети»
/// считает движок (MLS-ростер ∩ presence), клиент лишь рендерит. Презенс
/// динамичен → считаем на момент чтения (как netStats), а не из кэша.
/// Кэш `members` хранит ДРУГИХ участников (без себя), поэтому +1 = мы сами.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_groupPresence<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    gid_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        if session_ptr == 0 { return ret(&env, "{}"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Ok(gid) = hex::decode(jstr(&mut env, &gid_hex).trim()) else { return ret(&env, "{}") };
        let json = session.group_cache.lock_safe().members.get(&gid).cloned().unwrap_or_else(|| "[]".into());
        let others: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
        let online_others = others
            .iter()
            .filter_map(|h| parse_key(h))
            .filter(|k| session.messenger.is_online(k))
            .count();
        let presence = GroupPresence {
            total: others.len() + 1,        // +1 = мы сами
            online: online_others + 1,      // мы всегда «в сети» в своей группе
        };
        ret(&env, &serde_json::to_string(&presence).unwrap_or_else(|_| "{}".into()))
    })
}
