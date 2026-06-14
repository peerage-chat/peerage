//! p2p-media — медиа-цепочка поверх лёгкого транспорта [`p2p_messenger`].
//!
//! Сейчас: аудио-кодек (Opus, полное качество) и джиттер-буфер. Захват/вывод
//! (cpal) и видео (H.264) подключаются поверх этих примитивов.
//!
//! Приоритет — эффективность без потери качества: декодирование и обработка
//! выполняются вне приёмного потока транспорта, без лишних аллокаций.

pub mod codec;
pub mod jitter;

pub use codec::{AudioConfig, AudioDecoder, AudioEncoder, CodecError};
pub use jitter::{JitterBuffer, JitterOut};
