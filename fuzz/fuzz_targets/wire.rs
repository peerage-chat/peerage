//! Провод: открытый текст чат-канала после расшифровки сессией. Контакт может
//! прислать любой открытый текст под валидным шифром, поэтому разбор паддинга и
//! payload фаззится напрямую. Сами пакеты сессии — цель `session`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use peerage_core::protocol::Payload;

fuzz_target!(|data: &[u8]| {
    let _ = Payload::from_padded(data);
    if let Ok(p) = Payload::from_bytes(data) {
        // Принятое обязано и пере-кодироваться, и прочитаться обратно: payload
        // ложится в офлайн-очередь и уходит пересылкой.
        let again = p.to_bytes().expect("разобранный payload кодируется");
        Payload::from_bytes(&again).expect("пере-кодированный payload читается");
    }
});
