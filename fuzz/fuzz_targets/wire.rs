//! Провод: всё, что приходит в канал чата до и после расшифровки. Контакт может
//! прислать любой открытый текст под валидным шифром, поэтому разбор паддинга и
//! payload фаззится напрямую, а конверт целиком — на мусоре.
#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use p2p_core::protocol::{decode, Payload};
use p2p_core::Identity;

fn me() -> &'static Identity {
    static ME: OnceLock<Identity> = OnceLock::new();
    ME.get_or_init(|| Identity::from_seed(&[1; 32]).unwrap())
}

fuzz_target!(|data: &[u8]| {
    let _ = decode(me(), data);
    let _ = p2p_core::pad::unpad(data);
    if let Ok(p) = Payload::from_bytes(data) {
        // Принятое обязано и пере-кодироваться, и прочитаться обратно: payload
        // ложится в офлайн-очередь и уходит пересылкой.
        let again = p.to_bytes().expect("разобранный payload кодируется");
        Payload::from_bytes(&again).expect("пере-кодированный payload читается");
    }
});
