//! Губы со звуком (ADR-014, шаг 8b) на симулированной сети: звук и видео одного
//! захвата слышны и видны в одно время, какой бы трек ни шёл дольше.

use std::time::{Duration, Instant};

use litertc::sim::{Link, LinkConfig};
use litertc::{Config, JitterOut, Session, VideoInfo};

const A: u8 = 1;
const B: u8 = 2;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn session(stream_id: u32, peer: u8) -> Session<u8> {
    let mut s = Session::new(Config {
        audio_clock_hz: 48_000,
        audio_frame_samples: 960,
        jitter_depth: 3,
        stream_id,
    });
    s.set_peers(&[peer]);
    s.set_bitrate_bounds(150_000, 2_000_000);
    s
}

/// Средние за последние 5 с отставания «захват → слышно» и «захват → видно», мс.
/// `output` — задержка вывода звука у B, `render` — его видео-декодера.
fn run(secs: u64, output: Duration, render: Duration) -> (f64, f64) {
    let t0 = Instant::now();
    let (mut a, mut b) = (session(0xA, B), session(0xB, A));
    let link = LinkConfig {
        delay: ms(40),
        jitter: ms(20),
        loss: 0.01,
        ..Default::default()
    };
    let mut to_b = Link::new(link, 7);
    let mut to_a = Link::new(link, 8);
    b.set_video_render_delay(&A, render);
    let (mut heard, mut seen) = (Vec::new(), Vec::new());
    let tail = (secs - 5) * 1000;
    for step in 0..secs * 1000 {
        let now = t0 + ms(step);
        let stamp = (step as u32).to_be_bytes();
        if step % 20 == 0 {
            a.send_audio(now, now, &stamp);
        }
        if step % 33 == 0 {
            let mut frame = vec![0u8; 2000];
            frame[..4].copy_from_slice(&stamp);
            let info = VideoInfo {
                keyframe: step == 0 || a.take_keyframe_request(),
                ..Default::default()
            };
            a.send_video(now, now, &frame, info);
        }
        for s in [&mut a, &mut b] {
            if s.poll_timeout().is_some_and(|t| t <= now) {
                s.handle_timeout(now);
            }
        }
        while let Some(t) = a.poll_transmit() {
            to_b.send(now, t.data);
        }
        while let Some(t) = b.poll_transmit() {
            to_a.send(now, t.data);
        }
        while let Some(p) = to_b.poll(now) {
            b.handle_datagram(now, A, &p);
        }
        while let Some(p) = to_a.poll(now) {
            a.handle_datagram(now, B, &p);
        }
        let captured = |d: &[u8]| u32::from_be_bytes([d[0], d[1], d[2], d[3]]) as f64;
        if step % 20 == 0 {
            let at = now + output;
            if let Some(JitterOut::Frame(p)) = b.pull_audio(at, &A) {
                if step >= tail {
                    heard.push((at - t0).as_secs_f64() * 1000.0 - captured(&p));
                }
            }
        }
        while let Some(f) = b.pull_video(now, &A) {
            if step >= tail {
                let at = f.play_at + render;
                seen.push((at - t0).as_secs_f64() * 1000.0 - captured(&f.data));
            }
        }
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    (mean(&heard), mean(&seen))
}

/// Медленный видео-декодер: звук ждёт видео.
#[test]
fn audio_waits_for_slow_video() {
    let (audio, video) = run(30, ms(40), ms(250));
    eprintln!("звук {audio:.0} мс, видео {video:.0} мс");
    assert!(
        (video - audio).abs() <= 35.0,
        "звук {audio:.0}, видео {video:.0}"
    );
}

/// Долгий вывод звука (Bluetooth): видео ждёт звук.
#[test]
fn video_waits_for_slow_audio() {
    let (audio, video) = run(30, ms(350), ms(20));
    eprintln!("звук {audio:.0} мс, видео {video:.0} мс");
    assert!(
        (video - audio).abs() <= 35.0,
        "звук {audio:.0}, видео {video:.0}"
    );
}
