//! JNI-обёртки диагностики пропускной способности меша (bench).
//! Вынесено из `lib.rs`; обращается к `pub(crate)` `Session`/`BenchRecv`/`BENCH_TRACK`.
//! Логика не менялась при выносе.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;

use crate::{jni_safe, jstr, parse_key, ret, BenchRecv, LockRecover, Session, BENCH_TRACK};

// ── Диагностика: тест пропускной способности меша ──────────────────────────

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_benchRecvStart<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if session_ptr == 0 { return; }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let rx = session.messenger.media_channel(8192);
        let st = Arc::new(BenchRecv {
            bytes: AtomicU64::new(0), pkts: AtomicU64::new(0),
            running: AtomicBool::new(true), start: Instant::now(),
        });
        *session.bench.lock_safe() = Some(st.clone());
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
    });
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_benchRecvStats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        if session_ptr == 0 { return ret(&env, "{}"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let g = session.bench.lock_safe();
        let json = match g.as_ref() {
            Some(st) => {
                let bytes = st.bytes.load(Ordering::Relaxed);
                let pkts = st.pkts.load(Ordering::Relaxed);
                let secs = st.start.elapsed().as_secs_f64().max(0.001);
                serde_json::json!({"pkts": pkts, "bytes": bytes, "secs": secs, "mbps": bytes as f64 * 8.0 / 1e6 / secs}).to_string()
            }
            None => "{}".into(),
        };
        ret(&env, &json)
    })
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_benchBlast<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    total_kb: jint,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        if session_ptr == 0 { return ret(&env, "{}"); }
        let session = unsafe { &*(session_ptr as *const Arc<Session>) };
        let Some(peer) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "{}") };
        let payload = vec![0u8; 1200];
        let target = (total_kb.max(0) as u64) * 1024;
        let mut sent_bytes = 0u64;
        let mut sent_pkts = 0u64;
        let mut seq = 0u16;
        let t0 = Instant::now();
        while sent_bytes < target && t0.elapsed() < Duration::from_secs(10) {
            if session.messenger.send_media(&peer, BENCH_TRACK, seq, 0, &payload).is_ok() {
                sent_bytes += payload.len() as u64;
                sent_pkts += 1;
                seq = seq.wrapping_add(1);
            }
        }
        let secs = t0.elapsed().as_secs_f64().max(0.001);
        ret(&env, &serde_json::json!({"sent_pkts": sent_pkts, "sent_bytes": sent_bytes, "secs": secs, "mbps": sent_bytes as f64 * 8.0 / 1e6 / secs}).to_string())
    })
}
