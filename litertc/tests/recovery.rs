//! Восстановление видео на симулированной сети (ADR-014, шаг 6): повторы по NACK,
//! запрос ключевого кадра и пропуск до него. Критерии приёмки из ADR-014 §7.

use std::time::{Duration, Instant};

use litertc::sim::{Link, LinkConfig};
use litertc::{
    parse_media, write_keyframe_request, write_nack, Config, Nack, Session, VideoInfo,
    MAX_VIDEO_CHUNK, MEDIA_RTX_V2, TRACK_VIDEO,
};

const A: u8 = 1;
const B: u8 = 2;
const FRAME_MS: u64 = 33;
/// Кадры забираем сразу, как собраны: здесь важно, что дошло, а не когда показать.
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
    // Кадры здесь — фиксированного размера (~4 Мбит/с), а проверяются повторы, не
    // оценка полосы: держим её у потолка.
    s.set_bitrate_bounds(10_000_000, 10_000_000);
    s
}

/// Кадр в три фрагмента; первые 4 байта — его номер, чтобы приёмник мог проверить.
fn frame(id: u32) -> Vec<u8> {
    let mut f = vec![id as u8; MAX_VIDEO_CHUNK * 2 + 100];
    f[..4].copy_from_slice(&id.to_be_bytes());
    f
}

struct Run {
    /// Номера кадров, отданных декодеру B, по порядку.
    delivered: Vec<u32>,
    /// Номера ключевых кадров, отправленных A.
    keyframes: Vec<u32>,
    a: Session<u8>,
    b: Session<u8>,
}

/// A шлёт видео 30 fps `secs` секунд; ключевой кадр — первый и по запросу. B видит
/// поток с кадра `join_at` (раньше пакеты к нему не доходят); на `restart_at` мс
/// декодер B начинается заново. Таймеры — как в движке.
fn run(ab: LinkConfig, ba: LinkConfig, secs: u64, join_at: u32, restart_at: Option<u64>) -> Run {
    let t0 = Instant::now();
    let (mut a, mut b) = (session(0xA, B), session(0xB, A));
    let (mut to_b, mut to_a) = (Link::new(ab, 7), Link::new(ba, 8));
    let mut r = Run {
        delivered: Vec::new(),
        keyframes: Vec::new(),
        a: session(0, 0),
        b: session(0, 0),
    };
    for step in 0..secs * 1000 {
        let now = t0 + ms(step);
        // До захода пакеты к B не доходят (с ровным темпом хвост кадра уходит позже
        // самого кадра — поэтому по времени, а не по кадру).
        let joined = step >= join_at as u64 * FRAME_MS;
        if step % FRAME_MS == 0 {
            let id = (step / FRAME_MS) as u32;
            let keyframe = id == 0 || a.take_keyframe_request();
            if keyframe {
                r.keyframes.push(id);
            }
            let info = VideoInfo {
                keyframe,
                ..Default::default()
            };
            a.send_video(now, now, &frame(id), info);
            while let Some(t) = a.poll_transmit() {
                if joined {
                    to_b.send(now, t.data);
                }
            }
        }
        if restart_at == Some(step) {
            b.request_keyframe(now, &A);
            r.delivered.push(u32::MAX); // отметка перезапуска
        }
        for s in [&mut a, &mut b] {
            if s.poll_timeout().is_some_and(|t| t <= now) {
                s.handle_timeout(now);
            }
        }
        while let Some(t) = a.poll_transmit() {
            if joined {
                to_b.send(now, t.data);
            }
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
            r.delivered.push(u32::from_be_bytes([
                f.data[0], f.data[1], f.data[2], f.data[3],
            ]));
        }
    }
    r.a = a;
    r.b = b;
    r
}

/// Каждый отданный кадр декодируем: ключевой или сразу за отданным предыдущим.
fn assert_decodable(r: &Run) {
    for (i, &id) in r
        .delivered
        .iter()
        .enumerate()
        .filter(|(_, &id)| id != u32::MAX)
    {
        let follows = i > 0 && r.delivered[i - 1].wrapping_add(1) == id;
        assert!(
            r.keyframes.contains(&id) || follows,
            "кадр {id} отдан без опоры"
        );
    }
}

/// 5 % потерь, RTT 200 мс: потерянные фрагменты повторяются, и видео идёт почти
/// без пропусков, хотя периодических ключевых кадров нет вовсе.
#[test]
fn lost_fragments_are_retransmitted() {
    let lossy = LinkConfig {
        delay: ms(100),
        jitter: ms(10),
        loss: 0.05,
        ..Default::default()
    };
    let r = run(lossy, lossy, 20, 0, None);
    assert_decodable(&r);
    let sent = (20_000 / FRAME_MS) as usize;
    // Кадр теряется, только если фрагмент не дошёл обе попытки NACK (~1 % потерь):
    // тогда ключевой кадр, около секунды без картинки.
    assert!(
        r.delivered.len() * 100 >= sent * 93,
        "отдано {} кадров из {sent}",
        r.delivered.len()
    );
    // Без повторов 14 % кадров имели бы дыру, и каждый тянул бы за собой всё до
    // следующего ключевого.
    let rtx = r.a.stats(&B).unwrap().retransmitted;
    assert!(rtx > 50, "повторов {rtx}");
    assert!(
        r.keyframes.len() <= 3,
        "ключевые кадры {:?}: повторы должны справляться сами",
        r.keyframes
    );
}

/// Зашёл посреди потока: первые P-кадры не на что опереть, B просит ключевой, и
/// картинка появляется через RTT, а не через интервал ключевых кадров.
#[test]
fn late_joiner_gets_a_keyframe_on_request() {
    let clean = LinkConfig {
        delay: ms(50),
        ..Default::default()
    };
    let r = run(clean, clean, 3, 30, None);
    assert_decodable(&r);
    let first = *r.delivered.first().expect("видео появилось");
    assert!(r.keyframes.contains(&first));
    assert!(
        first <= 30 + 200 / FRAME_MS as u32,
        "первый кадр {first}: ключевой по запросу должен прийти за ~RTT"
    );
    assert_eq!(r.b.stats(&A).unwrap().keyframe_requests, 1);
    assert!(r.delivered.len() >= 50);
}

/// Декодер начат заново (тайл снова на экране): ему сразу нужен ключевой кадр, и он
/// приходит за RTT, а не по расписанию.
#[test]
fn restarted_decoder_gets_a_keyframe() {
    let clean = LinkConfig {
        delay: ms(50),
        ..Default::default()
    };
    let r = run(clean, clean, 3, 0, Some(1500));
    assert_decodable(&r);
    let mark = r.delivered.iter().position(|&id| id == u32::MAX).unwrap();
    let after = r.delivered[mark + 1];
    assert!(
        r.keyframes.contains(&after),
        "после перезапуска первым идёт ключевой"
    );
    assert!(
        after <= 1500 / FRAME_MS as u32 + 200 / FRAME_MS as u32,
        "кадр {after}"
    );
    assert_eq!(r.keyframes.len(), 2);
}

/// Ключевой кадр в пути — не повод его просить: на чистом канале запросов нет.
#[test]
fn clean_start_asks_nothing() {
    let clean = LinkConfig {
        delay: ms(50),
        ..Default::default()
    };
    let r = run(clean, clean, 3, 0, None);
    assert_eq!(r.keyframes, [0]);
    assert_eq!(r.b.stats(&A).unwrap().keyframe_requests, 0);
    assert_eq!(r.a.stats(&B).unwrap().retransmitted, 0);
    // Все кадры по порядку, кроме последнего: он ещё в пути.
    assert_eq!(r.delivered, (0..3000 / FRAME_MS as u32).collect::<Vec<_>>());
}

/// Враждебный участник засыпает нас NACK-ами на всё подряд и запросами ключевого
/// кадра: повторы не выходят за долю от отправленного ему, а кодер не начинает
/// выдавать одни ключевые.
#[test]
fn hostile_feedback_stays_within_budget() {
    let t0 = Instant::now();
    let mut a = session(0xA, B);
    let mut first_bytes = 0usize;
    let mut rtx_bytes = 0usize;
    let mut keyframes = 0;
    let mut last_seq = 0u16;
    for step in 0..10_000u64 {
        let now = t0 + ms(step);
        if step % FRAME_MS == 0 {
            let id = (step / FRAME_MS) as u32;
            let keyframe = a.take_keyframe_request();
            keyframes += keyframe as u32;
            let info = VideoInfo {
                keyframe,
                ..Default::default()
            };
            a.send_video(now, now, &frame(id), info);
        }
        // Каждую миллисекунду — NACK на 128 × 17 последних номеров и запрос ключевого.
        let seqs: Vec<u16> = (0..2176u16)
            .rev()
            .map(|i| last_seq.wrapping_sub(i))
            .collect();
        let nack = Nack {
            track: TRACK_VIDEO,
            stream_id: 0xA,
            seqs,
        };
        a.handle_datagram(now, B, &write_nack(&nack).unwrap());
        a.handle_datagram(now, B, &write_keyframe_request(TRACK_VIDEO, 0xA));
        while let Some(t) = a.poll_transmit() {
            let m = parse_media(&t.data).unwrap();
            if t.data[0] == MEDIA_RTX_V2 {
                rtx_bytes += m.payload.len();
            } else {
                first_bytes += m.payload.len();
                last_seq = m.header.seq;
            }
        }
    }
    assert!(
        rtx_bytes as f64 <= first_bytes as f64 * 0.25 + 256.0 * 1024.0,
        "повторов {rtx_bytes} байт при {first_bytes} отправленных"
    );
    assert!(rtx_bytes > 0, "бюджет повторов не нулевой");
    assert!(keyframes <= 12, "ключевых кадров за 10 с: {keyframes}");
}
