//! Снятие EXIF перед отправкой: разбор JPEG/PNG из галереи (файл мог прийти
//! откуда угодно). Вход подаётся с обоими магическими заголовками.
#![no_main]

use libfuzzer_sys::fuzz_target;
use p2p_blob::scrub::scrub;

fuzz_target!(|data: &[u8]| {
    let _ = scrub("image/*", data);
    let mut jpeg = vec![0xFF, 0xD8];
    jpeg.extend_from_slice(data);
    let _ = scrub("image/jpeg", &jpeg);
    let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(data);
    let _ = scrub("image/png", &png);
});
