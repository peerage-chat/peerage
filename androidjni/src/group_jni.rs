//! JNI-кодек групп (MLS). Мутации (create/invite/sendText) — fire-and-forget в
//! движок; read-данные (list/history/members/presence) — снапшоты движка.

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong, jstring, JNI_FALSE, JNI_TRUE};
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
