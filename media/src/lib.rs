//! peerage-media — медиа-цепочка поверх лёгкого транспорта [`peerage_messenger`].
//!
//! Сейчас: аудио-кодек (Opus, полное качество) и джиттер-буфер. Захват/вывод
//! (cpal) и видео (H.264) подключаются поверх этих примитивов.
//!
//! Приоритет — эффективность без потери качества: декодирование и обработка
//! выполняются вне приёмного потока транспорта, без лишних аллокаций.

pub mod codec;
pub mod jitter;
pub mod video;

pub use codec::{AudioDecoder, AudioEncoder, CodecError};
pub use jitter::{JitterBuffer, JitterOut};
pub use video::{
    push_source, DecodedFrame, FrameFeeder, PushSource, VideoDecoder, VideoEncoder, VideoError,
    VideoSource,
};
// Параметры кодека живут в ядре (настраиваются пользователем, сохраняются).
pub use peerage_core::settings::{AudioSettings, OpusMode, VideoSettings};
