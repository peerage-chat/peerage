//! Property-тесты парсеров недоверенного входа (веха M6): ни один парсер не должен
//! паниковать НИ НА КАКОМ входе — только возвращать None/Err. proptest подсовывает
//! тысячи случайных байтовых последовательностей. Гоняется на хосте (`cargo test`).

use peerage_core::media::{parse_media, parse_video_frag, VideoReassembler};
use peerage_core::protocol::Payload;
use peerage_core::session::Sessions;
use peerage_core::Identity;
use proptest::prelude::*;

proptest! {
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

    /// Пакет сессии и расшифрованный текст не роняют разбор на мусоре.
    #[test]
    fn session_and_payload_never_panic(data in proptest::collection::vec(any::<u8>(), 0..5000)) {
        let me = Identity::from_seed(&[1u8; 32]).unwrap();
        let mut s = Sessions::new(&me);
        let _ = s.receive(&data, std::time::Instant::now(), &mut rand_core::OsRng);
        let _ = Payload::from_padded(&data);
    }
}
