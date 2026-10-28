//! Показ видео по меткам захвата (ADR-014, шаг 8) на симулированной сети: кадры
//! выходят в темпе камеры отправителя, а не рывками доставки и не по fps из
//! заголовка.

use std::time::{Duration, Instant};

use litertc::sim::{Link, LinkConfig};
use litertc::{Config, Session, VideoInfo};

const A: u8 = 1;
const B: u8 = 2;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn session(stream_id: u32, peer: u8) -> Session<u8> {
    let mut s = Session::new(Config {
        audio_clock_hz: 48_000,
        audio_frame_samples: 960,
        jitter_depth: 4,
        stream_id,
    });
    s.set_peers(&[peer]);
    s.set_bitrate_bounds(150_000, 2_000_000);
    s
}

/// Кадр, как его увидел зритель.
struct Shown {
    captured: u64,
    play_at: Instant,
}

/// A шлёт видео: кадр захвачен в моменты `capture` (мс). B забирает кадры, как
/// декодер, — когда пора, и «показывает» в `play_at`.
fn run(capture: &[u64], link: LinkConfig) -> (Instant, Vec<Shown>) {
    let t0 = Instant::now();
    let (mut a, mut b) = (session(0xA, B), session(0xB, A));
    let mut to_b = Link::new(link, 5);
    let mut to_a = Link::new(
        LinkConfig {
            delay: ms(40),
            ..Default::default()
        },
        6,
    );
    let mut next = 0;
    let mut shown = Vec::new();
    let end = capture.last().copied().unwrap_or(0) + 2000;
    for step in 0..end {
        let now = t0 + ms(step);
        while next < capture.len() && capture[next] <= step {
            let mut frame = vec![0u8; 3000];
            frame[..4].copy_from_slice(&(next as u32).to_be_bytes());
            let info = VideoInfo {
                keyframe: next == 0 || a.take_keyframe_request(),
                ..Default::default()
            };
            a.send_video(now, &frame, info);
            next += 1;
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
        while let Some(f) = b.pull_video(now, &A) {
            let id = u32::from_be_bytes([f.data[0], f.data[1], f.data[2], f.data[3]]);
            shown.push(Shown {
                captured: capture[id as usize],
                play_at: f.play_at,
            });
        }
    }
    (t0, shown)
}

/// Камера отдаёт 20–28 кадров в секунду вместо 30 (темнота, эмулятор), сеть
/// дёргается и теряет: промежутки показа повторяют промежутки захвата, пауз нет.
#[test]
fn playback_follows_the_camera_not_the_network() {
    // Промежутки захвата 36–50 мс, псевдослучайно.
    let mut capture = Vec::new();
    let mut t = 0u64;
    for i in 0..1500u64 {
        capture.push(t);
        t += 36 + (i * 7919 % 15);
    }
    let (t0, shown) = run(
        &capture,
        LinkConfig {
            delay: ms(40),
            jitter: ms(30),
            loss: 0.02,
            ..Default::default()
        },
    );
    assert!(
        shown.len() >= capture.len() * 97 / 100,
        "показано {}",
        shown.len()
    );
    // После разгона (5 с): шаг показа = шаг захвата, кроме редких сбоев.
    let settled: Vec<&[Shown]> = shown.windows(2).filter(|w| w[0].captured >= 5000).collect();
    let off = settled
        .iter()
        .filter(|w| {
            let cap = w[1].captured - w[0].captured;
            let play = (w[1].play_at - w[0].play_at).as_millis() as u64;
            play.abs_diff(cap) > 5
        })
        .count();
    assert!(
        off * 100 <= settled.len() * 2,
        "шаг показа разошёлся с захватом у {off} из {}",
        settled.len()
    );
    // Задержка «захват → показ» — около сети плюс запас на её разброс и повторы.
    let mut delays: Vec<u64> = shown
        .iter()
        .filter(|s| s.captured >= 5000)
        .map(|s| (s.play_at - t0).as_millis() as u64 - s.captured)
        .collect();
    delays.sort_unstable();
    let median = delays[delays.len() / 2];
    eprintln!(
        "показано {} из {}, шаг разошёлся у {off} из {}, задержка: медиана {median} мс, худшая {} мс",
        shown.len(),
        capture.len(),
        settled.len(),
        delays.last().unwrap_or(&0)
    );
    assert!((70..300).contains(&median), "задержка показа {median} мс");
}
