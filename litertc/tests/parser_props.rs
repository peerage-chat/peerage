//! Property-тесты разбора медиа: ни один парсер не паникует НИ НА КАКОМ входе —
//! пакеты приходят от участника звонка, который может быть враждебен. Глубже то же
//! гоняет `fuzz/fuzz_targets/media.rs` (cargo-fuzz).

use std::time::{Duration, Instant};

use litertc::{
    parse_keyframe_request, parse_media, parse_nack, parse_report, parse_video_frag, write_nack,
    write_report, Config, Session, VideoInfo, VideoReassembler,
};
use proptest::prelude::*;

proptest! {
    /// Разбор отчёта получателя не паникует, а разобранное переживает запись и разбор.
    #[test]
    fn parse_report_never_panics_and_roundtrips(data in proptest::collection::vec(any::<u8>(), 0..128)) {
        if let Some(r) = parse_report(&data) {
            prop_assert_eq!(parse_report(&write_report(&r)), Some(r));
        }
    }

    /// NACK и запрос ключевого кадра: не паникуют, разобранный NACK переживает запись.
    #[test]
    fn parse_feedback_never_panics_and_roundtrips(data in proptest::collection::vec(any::<u8>(), 0..600)) {
        let _ = parse_keyframe_request(&data);
        if let Some(n) = parse_nack(&data) {
            let again = write_nack(&n).and_then(|b| parse_nack(&b));
            prop_assert_eq!(again.map(|a| a.seqs), Some(n.seqs));
        }
    }

    /// Сессия не паникует на произвольных датаграммах участника, перемешанных с нашей
    /// отправкой и таймерами (первый байт — подсказка типа, чтобы чаще попадать в
    /// разбор NACK, запросов и повторов).
    #[test]
    fn session_never_panics(
        dgs in proptest::collection::vec((0u8..6, proptest::collection::vec(any::<u8>(), 0..96)), 0..64)
    ) {
        let t0 = Instant::now();
        let mut s = Session::new(Config { audio_clock_hz: 48_000, audio_frame_samples: 960, jitter_depth: 2, stream_id: 7 });
        s.set_peers(&[1u8]);
        for (i, (kind, mut d)) in dgs.into_iter().enumerate() {
            let now = t0 + Duration::from_millis(i as u64 * 7);
            if let Some(b) = d.first_mut() {
                *b = [0x20, 0x21, 0x22, 0x23, 0x24, *b][kind as usize];
            }
            if i % 5 == 0 {
                s.send_video(now, &d, VideoInfo { keyframe: i % 10 == 0, ..Default::default() });
            }
            s.handle_datagram(now, 1, &d);
            s.handle_timeout(now);
            while s.poll_transmit().is_some() {}
            while s.pull_video(&1).is_some() {}
            let _ = s.pull_audio(&1);
            s.request_keyframe(now, &1);
        }
    }

    /// Разбор медиа-заголовка не паникует ни на каком входе.
    #[test]
    fn parse_media_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = parse_media(&data);
    }

    /// Разбор видео-фрагмента не паникует.
    #[test]
    fn parse_video_frag_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = parse_video_frag(&data);
    }

    /// Сборщик кадров не паникует на произвольной последовательности «фрагментов»
    /// (включая злонамеренные frame_id/frag_count/frag_idx).
    #[test]
    fn video_reassembler_never_panics(
        frags in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..64), 0..64)
    ) {
        let t0 = Instant::now();
        let mut r = VideoReassembler::new();
        for (i, f) in frags.iter().enumerate() {
            if let Some(frag) = parse_video_frag(f) {
                r.push(frag);
            }
            let now = t0 + Duration::from_millis(i as u64 * 50);
            while r.pop(now, Duration::from_millis(100)).is_some() {}
        }
    }
}
