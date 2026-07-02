//! Property-тесты блоб-слоя (веха M6): парсер манифеста и приём чанков не паникуют
//! ни на каком входе; валидный блоб всегда собирается байт-в-байт. Хост, `cargo test`.

use p2p_blob::{build, Limits, Manifest, Reassembler};
use proptest::prelude::*;

proptest! {
    /// Разбор манифеста не паникует на любой строке (мусор → Err).
    #[test]
    fn manifest_from_json_never_panics(s in ".*") {
        let _ = Manifest::from_json(&s);
    }

    /// Приём чанка не паникует на произвольном индексе/данных (чужой/битый → Err,
    /// вне диапазона → BadIndex).
    #[test]
    fn reassembler_add_never_panics(
        content in proptest::collection::vec(any::<u8>(), 0..50_000),
        idx in any::<usize>(),
        data in proptest::collection::vec(any::<u8>(), 0..30_000),
    ) {
        let (manifest, _) = build(&content, "x/y");
        if let Ok(mut r) = Reassembler::new(manifest, &Limits::default()) {
            let _ = r.add(idx, &data);
        }
    }

    /// Инвариант: любой валидный блоб нарезается и собирается обратно байт-в-байт,
    /// а id совпадает (контент-адресация).
    #[test]
    fn blob_roundtrip_property(content in proptest::collection::vec(any::<u8>(), 0..80_000)) {
        let (manifest, chunks) = build(&content, "x/y");
        let expect_id = manifest.id;
        let mut r = Reassembler::new(manifest, &Limits::default()).unwrap();
        for (i, c) in chunks.iter().enumerate() {
            r.add(i, c).unwrap();
        }
        let out = r.finish().unwrap();
        prop_assert_eq!(&out, &content);
        prop_assert_eq!(expect_id, p2p_blob::blob_id(&content));
    }
}
