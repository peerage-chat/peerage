//! Поведение медиа-плоскости на сегодня, на симулированной сети (ADR-014, шаг 2).
//!
//! Эти тесты не описывают, как должно быть, — они фиксируют, как есть, чтобы перенос
//! в `Session` (шаг 3) ничего не поменял незаметно. Когда шаги 5–7 намеренно меняют
//! поведение (перепосылка, запрос ключевого кадра), соответствующий тест меняется
//! вместе с ними и становится критерием приёмки.

use std::time::{Duration, Instant};

use litertc::sim::{BurstLoss, Link, LinkConfig};
use litertc::{
    fragment_video, parse_media, parse_video_frag, write_media_header, JitterBuffer, JitterOut,
    MediaHeader, VideoInfo, VideoReassembler, MAX_VIDEO_CHUNK, MEDIA_HEADER_LEN, TRACK_AUDIO,
    TRACK_VIDEO,
};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn packet(track: u8, seq: u16, ts: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = vec![0u8; MEDIA_HEADER_LEN + payload.len()];
    write_media_header(
        &mut p,
        &MediaHeader {
            track,
            stream_id: 1,
            seq,
            tw_seq: seq,
            ts,
        },
    );
    p[MEDIA_HEADER_LEN..].copy_from_slice(payload);
    p
}

/// Видео 30 fps, кадр в 3 фрагмента, 5 % потерь, разброс задержки. Сегодня потеря
/// одного фрагмента = потеря кадра: перепосылки нет, кадр с дырой не доставляется
/// никогда, а целые кадры доставляются по возрастанию номера.
#[test]
fn video_lost_fragment_loses_the_frame() {
    let t0 = Instant::now();
    let cfg = LinkConfig {
        delay: ms(40),
        jitter: ms(15),
        loss: 0.05,
        ..Default::default()
    };
    let mut link = Link::new(cfg, 11);
    let frames = 300u32;
    let mut seq = 0u16;
    for id in 0..frames {
        let frame = vec![id as u8; MAX_VIDEO_CHUNK * 2 + 100];
        for frag in fragment_video(
            &frame,
            id,
            VideoInfo {
                keyframe: id % 30 == 0,
                ..Default::default()
            },
        ) {
            link.send(t0 + ms(33 * id as u64), packet(TRACK_VIDEO, seq, 0, &frag));
            seq = seq.wrapping_add(1);
        }
    }

    // Какие фрагменты дошли, и что собрал приёмник.
    let mut arrived = vec![[false; 3]; frames as usize];
    let mut delivered = Vec::new();
    let mut asm = VideoReassembler::new();
    let mut now = t0;
    while let Some(at) = link.next_delivery() {
        now = now.max(at);
        while let Some(p) = link.poll(now) {
            let m = parse_media(&p).expect("медиа-пакет");
            let f = parse_video_frag(m.payload).expect("видео-фрагмент");
            arrived[f.frame_id as usize][f.frag_idx as usize] = true;
            if let Some(c) = asm.push(f) {
                delivered.push(c.frame_id);
            }
        }
    }

    let with_hole = arrived.iter().filter(|a| a.contains(&false)).count();
    assert!(
        with_hole > 20,
        "при 5 % потерь дырявых кадров должно быть много: {with_hole}"
    );
    for &id in &delivered {
        assert!(
            !arrived[id as usize].contains(&false),
            "кадр {id} с дырой не может собраться"
        );
    }
    assert!(
        delivered.windows(2).all(|w| w[0] < w[1]),
        "кадры идут по возрастанию"
    );
    // Целый кадр мог не дойти, только если его обогнал более новый (опоздал).
    let whole = frames as usize - with_hole;
    assert!(
        delivered.len() + 10 >= whole,
        "доставлено {} из {whole} целых",
        delivered.len()
    );
}

/// Звук: кадр 20 мс, глубина 4, разброс 30 мс, без потерь — проигрывается всё, без
/// единой ложной потери.
#[test]
fn audio_jitter_without_loss_plays_everything() {
    let (played, lost) = run_audio(
        LinkConfig {
            delay: ms(40),
            jitter: ms(30),
            ..Default::default()
        },
        4,
    );
    assert_eq!(lost, 0, "ложные потери при одном лишь разбросе задержки");
    assert!(played >= 495, "проиграно {played} из 500");
}

/// Звук с потерями пачками: каждая потеря объявляется (`Lost` → PLC), а не
/// превращается в тишину или сдвиг; проиграно + потеряно = отправлено.
#[test]
fn audio_burst_loss_is_reported_frame_by_frame() {
    let burst = BurstLoss {
        enter: 0.02,
        leave: 0.3,
        loss_in_burst: 0.8,
    };
    let cfg = LinkConfig {
        delay: ms(40),
        jitter: ms(10),
        burst: Some(burst),
        ..Default::default()
    };
    let (played, lost) = run_audio(cfg, 4);
    assert!(lost > 10, "потери должны объявляться: {lost}");
    assert!(
        played + lost >= 495,
        "проиграно {played} + потеряно {lost} из 500"
    );
}

/// 500 кадров звука по 20 мс через канал; приёмник забирает кадр каждые 20 мс.
/// Возвращает (проиграно, объявлено потерь).
fn run_audio(cfg: LinkConfig, depth: usize) -> (u32, u32) {
    let t0 = Instant::now();
    let mut link = Link::new(cfg, 23);
    for i in 0..500u16 {
        link.send(
            t0 + ms(20 * i as u64),
            packet(TRACK_AUDIO, i, 960 * i as u32, &[i as u8]),
        );
    }
    let mut jb = JitterBuffer::new(depth);
    let (mut played, mut lost) = (0, 0);
    // Тик воспроизведения каждые 20 мс, с запасом по времени на хвост.
    for tick in 0..700u64 {
        let now = t0 + ms(20 * tick);
        while let Some(p) = link.poll(now) {
            let m = parse_media(&p).expect("медиа-пакет");
            jb.push(m.header.seq, m.payload.to_vec());
        }
        match jb.pop() {
            Some(JitterOut::Frame(_)) => played += 1,
            Some(JitterOut::Lost) => lost += 1,
            None => {}
        }
    }
    (played, lost)
}
