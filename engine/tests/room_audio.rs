//! Звук звонка на петле: голос A доходит до B через движок комнаты (Opus,
//! джиттер, микс) — и не пропадает, если посреди звонка кто-то запустит
//! диагностику, забирающую медиа-канал мессенджера.
//!
//! Прогон из WSL: scripts/test-all.sh (LD_LIBRARY_PATH=ygg/libygg).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::Engine;
use peerage_messenger::loopback::LoopbackNet;

fn id(n: u8) -> Identity {
    Identity::from_seed(&[n; 32]).unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-audio-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, ident: Identity, dir: PathBuf) -> Arc<Engine> {
    let key = ident.public_key_bytes();
    Engine::start_with_transport(dir, ident, net.endpoint(key))
        .unwrap()
        .engine
}

/// Звонок A ↔ B в комнате 1:1 на петле `net`.
struct Call {
    a: Arc<Engine>,
    b: Arc<Engine>,
    a_key: [u8; 32],
    /// Кадр звука A: синус 440 Гц.
    tone: Vec<i16>,
}

fn call(net: &LoopbackNet, seed: u8) -> Call {
    let (a_id, b_id) = (id(seed), id(seed + 1));
    let (a_key, b_key) = (a_id.public_key_bytes(), b_id.public_key_bytes());
    let a = start(net, a_id, tmpdir("a"));
    let b = start(net, b_id, tmpdir("b"));
    a.add_contact(&b_key, "B");
    b.add_contact(&a_key, "A");

    let room = a.room_invite_dm(b_key, true, false);
    b.room_join_dm(a_key, true, false);
    let params: serde_json::Value =
        serde_json::from_str(&a.room_audio_start(room.clone(), 0, false)).unwrap();
    b.room_audio_start(room, 0, false);
    let frame = params["frame_len"].as_u64().unwrap() as usize;
    let rate = params["sample_rate"].as_f64().unwrap();
    let tone = (0..frame)
        .map(|i| ((i as f64 * 440.0 * std::f64::consts::TAU / rate).sin() * 8000.0) as i16)
        .collect();
    Call { a, b, a_key, tone }
}

impl Call {
    /// Кадр звука A в реальном темпе (`mic` — с микрофона, иначе A молчит), B
    /// забирает кадр микса; `until` по B — до 10 с.
    fn talk_until(&self, mic: bool, until: impl Fn(&Engine, &[i16]) -> bool) -> bool {
        let mut out = vec![0i16; self.tone.len()];
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if mic {
                self.a.room_push_audio(&self.tone, Duration::ZERO);
            }
            std::thread::sleep(Duration::from_millis(20));
            let n = self.b.room_pull_audio(&mut out, Duration::ZERO);
            if until(&self.b, &out[..n]) {
                return true;
            }
        }
        false
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        self.a.room_audio_stop();
        self.b.room_audio_stop();
    }
}

fn loud(pcm: &[i16]) -> bool {
    pcm.iter().any(|s| s.unsigned_abs() > 500)
}

/// R-033: замер пропускной способности, запущенный во время звонка, не отбирает
/// у звонка медиа-канал — голос A по-прежнему слышен у B.
#[test]
fn bench_during_a_call_does_not_mute_it() {
    let net = LoopbackNet::new();
    let c = call(&net, 101);
    c.b.bench_recv_start();
    // A говорит (синус 440 Гц кадрами в реальном темпе), B слушает.
    assert!(
        c.talk_until(true, |_, pcm| loud(pcm)),
        "голос A дошёл до B, хотя B запустил замер посреди звонка"
    );
}

/// B видит, что A говорит (рамка говорящего), и трафик: сколько пришло от A; свой
/// исходящий — только медиа (у молчащего B ноль, у A — его звук). A выключил микрофон — рамка у B гаснет.
#[test]
fn listener_sees_who_speaks_and_the_traffic() {
    let net = LoopbackNet::new();
    let c = call(&net, 111);
    let a_hex = hex::encode(c.a_key);
    let speaking = |b: &Engine| b.room_speaking_json().contains(&a_hex);
    assert!(c.talk_until(true, |b, _| speaking(b)), "A говорит");
    let traffic: serde_json::Value = serde_json::from_str(&c.b.room_traffic_json()).unwrap();
    assert!(traffic[&a_hex].as_u64().unwrap() > 0, "{traffic}");
    assert_eq!(
        traffic["self"], 0,
        "B молчит — своих медиа не шлёт: {traffic}"
    );
    let sent: serde_json::Value = serde_json::from_str(&c.a.room_traffic_json()).unwrap();
    assert!(sent["self"].as_u64().unwrap() > 0, "{sent}");
    c.a.room_set_muted(true);
    assert!(c.talk_until(true, |b, _| !speaking(b)), "A замолчал");
    assert_eq!(c.b.room_speaking_json(), "[]");
}

/// Кадр камеры A доходит до B целиком и с тем, что о нём сказал кодер.
#[test]
fn video_frame_reaches_the_other_side() {
    let net = LoopbackNet::new();
    let c = call(&net, 121);
    let key = litertc::VideoInfo {
        keyframe: true,
        rotation: 1,
        fps: 30,
    };
    let frame: Vec<u8> = (0..5000u32).map(|i| i as u8).collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = None;
    while got.is_none() && Instant::now() < deadline {
        c.a.room_push_video(&frame, key, Duration::ZERO);
        got = c.b.room_pull_video(&c.a_key, Duration::from_millis(200));
    }
    let got = got.expect("кадр дошёл");
    assert_eq!((got.data, got.info), (frame, key));
    assert!(c.b.room_traffic_json().contains(&hex::encode(c.a_key)));
}
