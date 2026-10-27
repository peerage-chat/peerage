//! Медиа-канал: заголовок пакета, видео-фрагмент и сборщик кадров на потоке
//! фрагментов от одного участника звонка (вход — пакеты с префиксом длины).
#![no_main]

use libfuzzer_sys::fuzz_target;
use litertc::{parse_media, parse_video_frag, VideoReassembler};

fuzz_target!(|data: &[u8]| {
    let _ = parse_media(data);
    let mut asm = VideoReassembler::new();
    let mut rest = data;
    while rest.len() >= 2 {
        let n = u16::from_be_bytes([rest[0], rest[1]]) as usize;
        rest = &rest[2..];
        let (pkt, tail) = rest.split_at(n.min(rest.len()));
        rest = tail;
        if let Some(f) = parse_video_frag(pkt) {
            if let Some(frame) = asm.push(f) {
                assert!(frame.data.len() <= 1024 * 8_000, "кадр больше допустимого");
            }
        }
    }
});
