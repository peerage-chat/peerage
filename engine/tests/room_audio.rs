//! Звук звонка на петле: голос A доходит до B через движок комнаты (Opus,
//! джиттер, микс) — и не пропадает, если посреди звонка кто-то запустит
//! диагностику, забирающую медиа-канал мессенджера.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::Identity;
use p2p_engine::Engine;
use p2p_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-audio-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key)).unwrap().engine
}

/// R-033: замер пропускной способности, запущенный во время звонка, не отбирает
/// у звонка медиа-канал — голос A по-прежнему слышен у B.
#[test]
fn bench_during_a_call_does_not_mute_it() {
    let net = LoopbackNet::new();
    let (a_id, b_id) = (id(101), id(102));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(&net, a_id, tmpdir("a"));
    let b = start(&net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    let room = a.room_invite_dm(b_key, true, false);
    b.room_join_dm(a_key, true, false);
    let params: serde_json::Value = serde_json::from_str(&a.room_audio_start(room.clone(), 0, false)).unwrap();
    b.room_audio_start(room, 0, false);
    let frame = params["frame_len"].as_u64().unwrap() as usize;
    let rate = params["sample_rate"].as_f64().unwrap();

    b.bench_recv_start();

    // A говорит (синус 440 Гц кадрами в реальном темпе), B слушает.
    let tone: Vec<i16> = (0..frame).map(|i| ((i as f64 * 440.0 * 6.283 / rate).sin() * 8000.0) as i16).collect();
    let mut heard = false;
    let mut out = vec![0i16; frame];
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !heard {
        a.room_push_audio(&tone);
        std::thread::sleep(Duration::from_millis(20));
        let n = b.room_pull_audio(&mut out);
        heard = out[..n].iter().any(|s| s.unsigned_abs() > 500);
    }
    a.room_audio_stop();
    b.room_audio_stop();
    assert!(heard, "голос A дошёл до B, хотя B запустил замер посреди звонка");
}
