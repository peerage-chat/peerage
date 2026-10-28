//! Property-тесты разбора медиа: ни один парсер не паникует НИ НА КАКОМ входе —
//! пакеты приходят от участника звонка, который может быть враждебен. Глубже то же
//! гоняет `fuzz/fuzz_targets/media.rs` (cargo-fuzz).

use litertc::{parse_media, parse_report, parse_video_frag, write_report, VideoReassembler};
use proptest::prelude::*;

proptest! {
    /// Разбор отчёта получателя не паникует, а разобранное переживает запись и разбор.
    #[test]
    fn parse_report_never_panics_and_roundtrips(data in proptest::collection::vec(any::<u8>(), 0..128)) {
        if let Some(r) = parse_report(&data) {
            prop_assert_eq!(parse_report(&write_report(&r)), Some(r));
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
        let mut r = VideoReassembler::new();
        for f in &frags {
            if let Some(frag) = parse_video_frag(f) {
                let _ = r.push(frag);
            }
        }
    }
}
