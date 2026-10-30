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

mod avsync;
mod bwe;
pub mod jitter;
mod nack;
mod pacer;
mod playout;
mod probe;
pub mod reassembly;
pub mod session;
pub mod sim;
pub mod stats;
pub mod wire;

pub use jitter::{JitterBuffer, JitterOut};
pub use reassembly::{CompleteFrame, VideoReassembler};
pub use session::{Config, LipSync, PeerStats, Session, Transmit, VideoFrame};
pub use stats::TrackStats;
pub use wire::{
    fragment_video, parse_keyframe_request, parse_media, parse_nack, parse_report,
    parse_transport_feedback, parse_video_frag, write_keyframe_request, write_media_header,
    write_nack, write_report, write_transport_feedback, MediaFrame, MediaHeader, Nack, Report,
    ReportBlock, TransportFeedback, VideoFrag, VideoInfo, KEYFRAME_REQUEST_V2, MAX_MEDIA_PAYLOAD,
    MAX_NACK_ITEMS, MAX_VIDEO_CHUNK, MAX_VIDEO_FRAGS, MEDIA_HEADER_LEN, MEDIA_RTX_V2, MEDIA_V2,
    NACK_V2, REPORT_V2, TRACK_AUDIO, TRACK_SCREEN, TRACK_VIDEO, TRANSPORT_FEEDBACK_V2,
    VIDEO_CLOCK_HZ, VIDEO_HDR, VIDEO_KEYFRAME,
};
