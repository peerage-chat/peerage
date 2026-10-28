//! LiteRTC — медиа-плоскость звонков Peerage: формат пакетов, сборка видео-кадров,
//! джиттер-буфер звука и симулятор сети для тестов. Решение и план — ADR-014.
//!
//! Правила крейта:
//!
//! - **Sans-IO.** Ни потоков, ни сокетов, ни часов: вызывающий передаёт время и
//!   данные, забирает результат. Поэтому всё тестируется на виртуальном времени
//!   ([`sim`]), а «положить трубку» никогда не ждёт за очередью внутри LiteRTC
//!   (инвариант 12) — своих очередей с потоками здесь нет.
//! - **LiteRTC не аутентифицирует.** Отправитель пакета уже доказан снаружи (в
//!   Peerage — сессией 1:1, SEC-4); сюда приходит расшифрованное. Но участник звонка
//!   может быть враждебен, поэтому любой разбор — без паник, любая память — с пределом.
//! - **Без зависимостей от Peerage.** Ни `peerage-*`, ни Yggdrasil, ни MLS, ни Android.

#![forbid(unsafe_code)]

pub mod jitter;
pub mod reassembly;
pub mod session;
pub mod sim;
pub mod wire;

pub use jitter::{JitterBuffer, JitterOut};
pub use reassembly::{CompleteFrame, VideoReassembler};
pub use session::{Config, PeerStats, Session, Transmit, VideoFrame};
pub use wire::{
    fragment_video, parse_media, parse_video_frag, write_media_header, MediaFrame, MediaHeader,
    VideoFrag, VideoInfo, MAX_MEDIA_PAYLOAD, MAX_VIDEO_CHUNK, MAX_VIDEO_FRAGS, MEDIA_HEADER_LEN,
    MEDIA_V2, TRACK_AUDIO, TRACK_SCREEN, TRACK_VIDEO, VIDEO_CLOCK_HZ, VIDEO_HDR, VIDEO_KEYFRAME,
};
