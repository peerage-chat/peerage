//! Код обмена v2 (8.4): приходит текстом или QR от кого угодно. Разбор обязан не
//! паниковать на любой строке, а разобранный код — кодироваться обратно в разбираемое.
#![no_main]

use libfuzzer_sys::fuzz_target;
use peerage_core::code::ContactCode;
use peerage_core::record::NetKind;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    // С префиксом — сразу к разбору полей: иначе почти всё отсекается на «PEERAGE:».
    for s in [s.to_string(), format!("PEERAGE:{s}")] {
        if let Ok(c) = ContactCode::parse(&s, NetKind::Yggdrasil) {
            assert_eq!(ContactCode::parse(&c.encode(), NetKind::Yggdrasil), Ok(c));
        }
    }
});
