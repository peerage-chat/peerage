//! Отчёты получателя на симулированной сети (ADR-014, шаг 5): потери, джиттер и RTT,
//! которые видят обе стороны, совпадают с тем, что делает канал.

use std::time::{Duration, Instant};

use litertc::sim::{Link, LinkConfig};
use litertc::{Config, PeerStats, Session, TRACK_AUDIO};

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
    s
}

/// A говорит `secs` секунд (кадр 20 мс; `muted(i)` — кадр не отправлен), B молчит.
/// Таймеры обеих сторон срабатывают как в движке. Возвращает статистику A о B и B об A.
fn call(
    ab: LinkConfig,
    ba: LinkConfig,
    secs: u64,
    muted: impl Fn(u64) -> bool,
) -> (PeerStats, PeerStats) {
    let t0 = Instant::now();
    let (mut a, mut b) = (session(0xA, B), session(0xB, A));
    let (mut to_b, mut to_a) = (Link::new(ab, 1), Link::new(ba, 2));
    for step in 0..secs * 1000 {
        let now = t0 + ms(step);
        if step % 20 == 0 {
            if muted(step / 20) {
                a.skip_audio();
            } else {
                a.send_audio(now, &[0; 80]);
            }
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
        // Звук B проигрывает, иначе буфер копится — как потребитель в движке.
        if step % 20 == 0 {
            b.pull_audio(&A);
        }
    }
    (a.stats(&B).unwrap(), b.stats(&A).unwrap())
}

/// 10 % потерь от A к B: B видит их у себя, A — из отчёта B; RTT — сумма задержек.
#[test]
fn loss_and_rtt_are_seen_by_both_sides() {
    let ab = LinkConfig {
        delay: ms(40),
        loss: 0.10,
        ..Default::default()
    };
    let ba = LinkConfig {
        delay: ms(60),
        ..Default::default()
    };
    let (a_about_b, b_about_a) = call(ab, ba, 20, |_| false);

    let inbound = b_about_a.inbound[TRACK_AUDIO as usize].expect("B принимает звук A");
    let lost_share = inbound.lost as f32 / inbound.highest_seq as f32;
    assert!(
        (0.07..0.13).contains(&lost_share),
        "B: потеряно {lost_share:.3}"
    );

    let outbound = a_about_b.outbound[TRACK_AUDIO as usize].expect("A знает, как B его слышит");
    assert!(
        outbound.lost.abs_diff(inbound.lost) < 60,
        "отчёт отстаёт не больше чем на секунду"
    );
    assert!(
        outbound.fraction_lost < 0.3,
        "доля за интервал {}",
        outbound.fraction_lost
    );

    let rtt = a_about_b.rtt.expect("RTT у A");
    assert!(
        rtt >= ms(95) && rtt <= ms(110),
        "RTT {rtt:?}, задержки 40 + 60 мс"
    );
    assert!(
        b_about_a.rtt.is_some(),
        "RTT и у B: отчёты ходят в обе стороны"
    );
}

/// Выключенный микрофон — не потери: номер пакета в тишине не тратится.
#[test]
fn mute_is_not_loss() {
    let clean = LinkConfig {
        delay: ms(30),
        ..Default::default()
    };
    // Каждые 2 с микрофон выключен на 1 с.
    let (a_about_b, b_about_a) = call(clean, clean, 10, |frame| (frame / 50) % 2 == 1);
    let inbound = b_about_a.inbound[TRACK_AUDIO as usize].unwrap();
    assert_eq!(
        inbound.lost, 0,
        "B насчитал потери на выключенном микрофоне"
    );
    assert_eq!(a_about_b.outbound[TRACK_AUDIO as usize].unwrap().lost, 0);
}

/// Джиттер канала виден в статистике в миллисекундах.
#[test]
fn jitter_is_reported() {
    let ab = LinkConfig {
        delay: ms(30),
        jitter: ms(40),
        ..Default::default()
    };
    let ba = LinkConfig {
        delay: ms(30),
        ..Default::default()
    };
    let (_, b_about_a) = call(ab, ba, 10, |_| false);
    let j = b_about_a.inbound[TRACK_AUDIO as usize].unwrap().jitter_ms;
    // Равномерный разброс 0…40 мс: средняя |разница| соседних ≈ 13 мс.
    assert!((8.0..20.0).contains(&j), "джиттер {j} мс");
}
