//! p2p-core — общее ядро P2P мессенджера.
//!
//! Содержит платформо-независимую логику: идентичность, криптографию,
//! сетевой протокол. Используется и Desktop (Tauri), и Android (через JNI).

pub mod identity;

pub use identity::{Identity, IdentityError};
