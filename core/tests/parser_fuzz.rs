//! Property-тесты парсеров недоверенного входа (веха M6): ни один парсер не должен
//! паниковать НИ НА КАКОМ входе — только возвращать None/Err. proptest подсовывает
//! тысячи случайных байтовых последовательностей. Гоняется на хосте (`cargo test`).
//! Медиа-парсеры — в `litertc/tests/parser_props.rs`.

use peerage_core::protocol::Payload;
use peerage_core::session::Sessions;
use peerage_core::Identity;
use proptest::prelude::*;

proptest! {
    /// Пакет сессии и расшифрованный текст не роняют разбор на мусоре.
    #[test]
    fn session_and_payload_never_panic(data in proptest::collection::vec(any::<u8>(), 0..5000)) {
        let me = Identity::from_seed(&[1u8; 32]).unwrap();
        let mut s = Sessions::new(&me);
        let _ = s.receive(&data, std::time::Instant::now(), &mut rand_core::OsRng);
        let _ = Payload::from_padded(&data);
    }
}
