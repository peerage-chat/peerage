//! Двусторонний звонок через узкое место с бездонным буфером (ADR-014, шаг 7):
//! линки Yggdrasil идут поверх TCP, и при резкой просадке полосы очередь копится в
//! буферах ниже нас без потерь. Обратная связь, по которой сбавляют, стоит в той же
//! очереди с другой стороны — без окна перегрузки к её приходу очередь уже на
//! десятки секунд, и звонок (а с ним маячки присутствия комнаты) встаёт.

use std::time::{Duration, Instant};

use litertc::sim::{Link, LinkConfig};
use litertc::{Config, Session, VideoInfo};

const A: u8 = 1;
const B: u8 = 2;
const FRAME_MS: u64 = 33;
const AUDIO_BPS: u32 = 64_000;
/// Кадры забираем сразу, как собраны: важна очередь в сети, а не время показа.
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
    s.set_bitrate_bounds(150_000, 2_000_000);
    s
}

/// Узкое место `bps` с задержкой `delay_ms` в одну сторону и буфером, которого
/// хватает на минуты.
fn bloated(delay_ms: u64, bps: u64) -> LinkConfig {
    LinkConfig {
        delay: ms(delay_ms),
        bandwidth_bps: Some(bps),
        queue_bytes: 64 << 20,
        ..Default::default()
    }
}

/// Одна сторона звонка: звук CBR и видео 30 fps по текущей цели (как кодер с
/// заданным битрейтом).
struct Side {
    s: Session<u8>,
    peer: u8,
    sent_at: Vec<u64>,
}

impl Side {
    fn send(&mut self, now: Instant, step: u64) {
        if step.is_multiple_of(20) {
            self.s.send_audio(now, now, &[0; 160]);
        }
        if step.is_multiple_of(FRAME_MS) {
            let id = self.sent_at.len() as u32;
            let keyframe = id == 0 || self.s.take_keyframe_request();
            let video_bps = self.s.target_bitrate().saturating_sub(AUDIO_BPS) as u64;
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
            self.s.send_video(now, now, &frame, info);
            self.sent_at.push(step);
        }
    }
}

/// Забрать собранное у `rx`: худшая задержка кадров собеседника, отправленных в
/// `sent_at`, мс.
fn received(rx: &mut Side, sent_at: &[u64], now: Instant, step: u64) -> u64 {
    let mut worst = 0;
    while let Some(f) = rx.s.pull_video(now + ASAP, &rx.peer) {
        let id = u32::from_be_bytes([f.data[0], f.data[1], f.data[2], f.data[3]]);
        worst = worst.max(step - sent_at[id as usize]);
    }
    while rx.s.pull_audio(now, &rx.peer).is_some() {}
    worst
}

/// Одна секунда прогона.
#[derive(Debug, Default, Clone, Copy)]
struct Second {
    /// Худшая задержка кадров (захват → собран у собеседника), мс, A→B и B→A.
    ab: u64,
    ba: u64,
    /// Цель кодера A в конце секунды.
    target: u32,
}

/// Звонок по секундам; полоса обоих направлений на секунде `s` — `bps(s)`.
fn run(secs: u64, delay_ms: u64, bps: impl Fn(u64) -> u64) -> Vec<Second> {
    let t0 = Instant::now();
    let mut a = Side {
        s: session(0xA, B),
        peer: B,
        sent_at: Vec::new(),
    };
    let mut b = Side {
        s: session(0xB, A),
        peer: A,
        sent_at: Vec::new(),
    };
    let mut to_b = Link::new(bloated(delay_ms, bps(0)), 3);
    let mut to_a = Link::new(bloated(delay_ms, bps(0)), 4);
    let mut out = vec![Second::default(); secs as usize];
    for step in 0..secs * 1000 {
        let now = t0 + ms(step);
        let sec = (step / 1000) as usize;
        if step % 1000 == 0 {
            to_b.set_config(bloated(delay_ms, bps(step / 1000)));
            to_a.set_config(bloated(delay_ms, bps(step / 1000)));
        }
        a.send(now, step);
        b.send(now, step);
        for side in [&mut a, &mut b] {
            if side.s.poll_timeout().is_some_and(|t| t <= now) {
                side.s.handle_timeout(now);
            }
        }
        while let Some(t) = a.s.poll_transmit() {
            to_b.send(now, t.data);
        }
        while let Some(t) = b.s.poll_transmit() {
            to_a.send(now, t.data);
        }
        while let Some(p) = to_b.poll(now) {
            b.s.handle_datagram(now, A, &p);
        }
        while let Some(p) = to_a.poll(now) {
            a.s.handle_datagram(now, B, &p);
        }
        let ab = received(&mut b, &a.sent_at, now, step);
        let ba = received(&mut a, &b.sent_at, now, step);
        let o = &mut out[sec];
        (o.ab, o.ba, o.target) = (o.ab.max(ab), o.ba.max(ba), a.s.target_bitrate());
    }
    for (i, s) in out.iter().enumerate() {
        eprintln!(
            "{i:2}: A→B {:>6} ms  B→A {:>6} ms  target {:>7}",
            s.ab, s.ba, s.target
        );
    }
    out
}

/// Полоса в обе стороны 4 Мбит/с, на 10-й секунде — 400 кбит/с. Очередь не должна
/// уходить за секунды (маячки присутствия комнаты живут 12 с), а через 10 с после
/// просадки задержка снова около базовой.
#[test]
fn sudden_drop_with_deep_buffers_does_not_stall_the_call() {
    let r = run(30, 50, |s| if s < 10 { 4_000_000 } else { 400_000 });
    let peak = r[10..].iter().map(|s| s.ab.max(s.ba)).max().unwrap();
    assert!(peak < 3000, "очередь после просадки: {peak} мс");
    for (i, s) in r[20..].iter().enumerate() {
        assert!(
            s.ab < 600 && s.ba < 600,
            "секунда {}: {}/{} мс",
            20 + i,
            s.ab,
            s.ba
        );
    }
}

/// Окно и предел роста от дошедшего не запирают друг друга: на чистом широком
/// канале с долгим RTT (600 мс) оценка после узкого места разгоняется обратно до
/// потолка, а задержка всё это время — распространение, без очереди: отправку
/// держит не окно. (Окно меньше того, что канал держит в полёте, держало бы
/// отправку, дошедшее отставало бы от оценки, и предел роста не пускал бы её выше.)
#[test]
fn window_does_not_cap_growth_on_a_long_clean_path() {
    let r = run(70, 300, |s| {
        if (10..20).contains(&s) {
            300_000
        } else {
            20_000_000
        }
    });
    assert!(r[9].target > 1_800_000, "до узкого места: {}", r[9].target);
    assert!(r[19].target < 600_000, "узкое место: {}", r[19].target);
    for (i, s) in r[25..].iter().enumerate() {
        assert!(s.ab < 400, "секунда {}: {} мс", 25 + i, s.ab);
    }
    // Рост по 8 % в секунду (около прошлой ёмкости — по пакету за RTT) — с 0,3 до 2
    // Мбит/с на RTT 600 мс почти 40 с.
    let top = r[60..].iter().map(|s| s.target).min().unwrap();
    assert!(top > 1_800_000, "через 40 с на широком канале: {top}");
}

/// Звонок начинается сразу в узком месте с долгим RTT. С потолка оценка утопила бы
/// его: очередь стояла бы с первой секунды, первый замер RTT был бы уже раздут ею
/// (2,5 с), а детектор задержки ловит рост очереди, не её саму, — и не сработал бы ни
/// разу. Старт снизу, и пробы показывают узкое место, а не поднимают выше него.
#[test]
fn call_starting_in_a_bottleneck_does_not_drown() {
    let r = run(40, 300, |_| 300_000);
    let peak = r.iter().map(|s| s.ab.max(s.ba)).max().unwrap();
    assert!(peak < 5000, "очередь на старте: {peak} мс");
    for (i, s) in r[30..].iter().enumerate() {
        assert!(
            s.ab < 1500 && s.ba < 1500,
            "секунда {}: {}/{} мс",
            30 + i,
            s.ab,
            s.ba
        );
    }
}
