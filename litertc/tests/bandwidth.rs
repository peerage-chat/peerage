//! Оценка полосы и ровный темп на симулированной сети (ADR-014, шаг 7): цель
//! следует за узким местом, очередь в сети не разрастается. Критерии приёмки из
//! ADR-014 §7.

use std::time::{Duration, Instant};

use litertc::sim::{Link, LinkConfig};
use litertc::{Config, Session, VideoInfo};

const A: u8 = 1;
const B: u8 = 2;
const FRAME_MS: u64 = 33;
const AUDIO_BPS: u32 = 64_000;
/// Кадры забираем сразу, как собраны: здесь важна очередь в сети, а не время показа
/// (его проверяет tests/playout.rs).
const ASAP: Duration = Duration::from_secs(3600);

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
    s.set_bitrate_bounds(150_000, 6_000_000);
    s
}

/// Одна секунда прогона: цель отправителя и задержка кадров у приёмника.
#[derive(Debug, Default, Clone, Copy)]
struct Second {
    target: u32,
    /// Худшая задержка кадра (захват → отдан декодеру) за секунду, мс.
    worst_latency: u64,
    frames: u32,
}

/// Канал с узким местом `bps` и очередью 256 КБ.
fn limited(bps: u64) -> LinkConfig {
    LinkConfig {
        delay: ms(50),
        bandwidth_bps: Some(bps),
        queue_bytes: 256 * 1024,
        ..Default::default()
    }
}

/// A шлёт звук (CBR 64 кбит/с) и видео 30 fps, кадр — по текущей цели (как кодер с
/// заданным битрейтом); B принимает. `link(s)` — канал A→B на секунде `s`.
fn run(secs: u64, link: impl Fn(u64) -> LinkConfig) -> Vec<Second> {
    let t0 = Instant::now();
    let (mut a, mut b) = (session(0xA, B), session(0xB, A));
    let mut to_b = Link::new(link(0), 3);
    let mut to_a = Link::new(
        LinkConfig {
            delay: ms(50),
            ..Default::default()
        },
        4,
    );
    let mut out = vec![Second::default(); secs as usize];
    let mut sent_at = Vec::new();
    for step in 0..secs * 1000 {
        let now = t0 + ms(step);
        let sec = (step / 1000) as usize;
        if step % 1000 == 0 {
            to_b.set_config(link(step / 1000));
        }
        if step % 20 == 0 {
            a.send_audio(now, now, &[0; 160]);
        }
        if step % FRAME_MS == 0 {
            let id = sent_at.len() as u32;
            let keyframe = id == 0 || a.take_keyframe_request();
            let video_bps = a.target_bitrate().saturating_sub(AUDIO_BPS) as u64;
            let mut size = (video_bps * FRAME_MS / 8000).max(200) as usize;
            if keyframe {
                size *= 3;
            }
            let mut frame = vec![0u8; size];
            frame[..4].copy_from_slice(&id.to_be_bytes());
            let info = VideoInfo {
                keyframe,
                ..Default::default()
            };
            a.send_video(now, now, &frame, info);
            sent_at.push(step);
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
        while let Some(f) = b.pull_video(now + ASAP, &A) {
            let id = u32::from_be_bytes([f.data[0], f.data[1], f.data[2], f.data[3]]);
            let s = &mut out[sec];
            s.worst_latency = s.worst_latency.max(step - sent_at[id as usize]);
            s.frames += 1;
        }
        while b.pull_audio(now, &A).is_some() {}
        out[sec].target = a.target_bitrate();
    }
    out
}

/// Полоса 4 Мбит/с, через 10 с — 1 Мбит/с: цель уходит под новую полосу за
/// секунды, после этого очередь в сети не копится (задержка кадров снова мала) и
/// видео идёт.
#[test]
fn target_follows_a_bandwidth_drop() {
    let r = run(30, |s| limited(if s < 10 { 4_000_000 } else { 1_000_000 }));
    for (i, s) in r.iter().enumerate() {
        eprintln!(
            "{i:2}: target {:>7} worst {:>4} ms frames {}",
            s.target, s.worst_latency, s.frames
        );
    }
    // До просадки: сбавили с потолка 6 Мбит/с под 4.
    assert!(
        r[7..10]
            .iter()
            .all(|s| (2_000_000..4_200_000).contains(&s.target)),
        "до просадки"
    );
    // Через 5 с после просадки цель под 1 Мбит/с.
    assert!(
        r[15].target < 1_000_000,
        "цель после просадки {}",
        r[15].target
    );
    // Последние 10 с: кадры идут, и задержка — около базовых 50 мс, а не секунды
    // переполненной очереди.
    for (i, s) in r[20..].iter().enumerate() {
        assert!(s.frames >= 25, "секунда {}: кадров {}", 20 + i, s.frames);
        assert!(
            s.worst_latency < 400,
            "секунда {}: задержка {} мс",
            20 + i,
            s.worst_latency
        );
    }
}

/// Полоса вернулась — цель поднимается следом (не остаётся на дне навсегда).
#[test]
fn target_recovers_when_bandwidth_returns() {
    let r = run(45, |s| limited(if s < 15 { 1_000_000 } else { 4_000_000 }));
    for (i, s) in r.iter().enumerate() {
        eprintln!(
            "{i:2}: target {:>7} worst {:>4} ms frames {}",
            s.target, s.worst_latency, s.frames
        );
    }
    assert!(r[14].target < 1_000_000);
    assert!(
        r[44].target > 2_000_000,
        "цель через 30 с после возврата {}",
        r[44].target
    );
}

/// Широкий канал с неперегрузочными бедами: случайные потери и разброс задержки с
/// перестановками (радио, меш поверх чужих линков). Оценка не должна лечь на дно.
#[test]
fn random_loss_and_jitter_do_not_starve_video() {
    let r = run(30, |_| LinkConfig {
        delay: ms(50),
        jitter: ms(30),
        loss: 0.05,
        ..limited(4_000_000)
    });
    for (i, s) in r.iter().enumerate() {
        eprintln!(
            "{i:2}: target {:>7} worst {:>4} ms frames {}",
            s.target, s.worst_latency, s.frames
        );
    }
    assert!(
        r[20..].iter().all(|s| s.target > 1_500_000),
        "цель при случайных потерях: {:?}",
        r[20..].iter().map(|s| s.target).collect::<Vec<_>>()
    );
}
