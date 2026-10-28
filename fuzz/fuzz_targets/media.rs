//! Медиа-канал: заголовки пакетов и обратной связи, сборщик кадров и целая сессия на
//! потоке датаграмм от одного участника звонка (вход — пакеты с префиксом длины).
#![no_main]

use std::time::{Duration, Instant};

use libfuzzer_sys::fuzz_target;
use litertc::{
    parse_keyframe_request, parse_media, parse_nack, parse_report, parse_video_frag, Config,
    Session, VideoInfo, VideoReassembler,
};

fuzz_target!(|data: &[u8]| {
    let _ = parse_media(data);
    let _ = parse_report(data);
    let _ = parse_nack(data);
    let _ = parse_keyframe_request(data);
    let t0 = Instant::now();
    let mut asm = VideoReassembler::new();
    let mut s = Session::new(Config {
        audio_clock_hz: 48_000,
        audio_frame_samples: 960,
        jitter_depth: 2,
        stream_id: 7,
    });
    s.set_peers(&[1u8]);
    let mut rest = data;
    let mut i = 0u64;
    while rest.len() >= 2 {
        let n = u16::from_be_bytes([rest[0], rest[1]]) as usize;
        rest = &rest[2..];
        let (pkt, tail) = rest.split_at(n.min(rest.len()));
        rest = tail;
        i += 1;
        let now = t0 + Duration::from_millis(i * 7);
        if let Some(f) = parse_video_frag(pkt) {
            asm.push(f);
        }
        while let Some(frame) = asm.pop(now, Duration::from_millis(100)) {
            assert!(frame.data.len() <= 1024 * 8_000, "кадр больше допустимого");
        }
        // Своя отправка — чтобы NACK было что повторять.
        if i % 4 == 0 {
            s.send_video(now, pkt, VideoInfo::default());
        }
        s.handle_datagram(now, 1, pkt);
        s.handle_timeout(now);
        while s.poll_transmit().is_some() {}
        while s.pull_video(&1).is_some() {}
    }
});
