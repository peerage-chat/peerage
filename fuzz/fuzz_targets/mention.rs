//! Упоминания группового сообщения: участок текста + ключ. Участник группы может
//! прислать любые числа под валидным MLS, а клиент режет по ним строку для
//! подсветки — поэтому то, что пропускает `mention::clean`, обязано резаться без
//! паники, не перекрываться и не выходить за текст.
#![no_main]

use libfuzzer_sys::fuzz_target;
use peerage_core::mention::{clean, Mention, MAX_MENTIONS};

fuzz_target!(|data: &[u8]| {
    // Первые 2 байта — длина текста, дальше текст, остаток — тройки (at, len, ключ).
    if data.len() < 2 {
        return;
    }
    let n = (u16::from_le_bytes([data[0], data[1]]) as usize).min(data.len() - 2);
    let text = String::from_utf8_lossy(&data[2..2 + n]).into_owned();
    let rest = &data[2 + n..];
    let mentions: Vec<Mention> = rest
        .chunks(9)
        .filter(|c| c.len() == 9)
        .map(|c| Mention {
            at: u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
            len: u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            // Ключ: то валидный hex, то мусор — чтобы проверка ключа тоже работала.
            key: if c[8] & 1 == 0 { "ab".repeat(32) } else { format!("{:x}", c[8]) },
        })
        .collect();

    let out = clean(&text, mentions);
    assert!(out.len() <= MAX_MENTIONS);
    let mut end_prev = 0usize;
    for m in &out {
        let (at, end) = (m.at as usize, (m.at + m.len) as usize);
        assert!(m.len > 0 && at >= end_prev && end <= text.len());
        let _ = &text[at..end]; // паника здесь = кривой участок прошёл чистку
        assert_eq!(m.key.len(), 64);
        end_prev = end;
    }
    // Чистка идемпотентна: повторная (на приёме после отправки) ничего не меняет.
    assert_eq!(clean(&text, out.clone()), out);
});
