//! JNI-кодек диагностики пропускной способности меша (bench). Тонкий слой над
//! методами [`peerage_engine::Engine`].

use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;

use peerage_engine::parse_key;

use crate::{engine, jni_safe, jstr, ret};

/// Включить приём бенч-датаграмм.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_benchRecvStart<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } { e.bench_recv_start(); }
    });
}

/// Статистика приёмника: JSON {pkts, bytes, secs, mbps}.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_benchRecvStats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        ret(&env, &e.bench_recv_stats_json())
    })
}

/// Бластить total_kb КБ на пира. Возвращает JSON {sent_pkts, sent_bytes, secs, mbps}.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_benchBlast<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    total_kb: jint,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        let Some(peer) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "{}") };
        ret(&env, &e.bench_blast(&peer, total_kb))
    })
}
