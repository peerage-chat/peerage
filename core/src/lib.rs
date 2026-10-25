//! p2p-core — общее ядро P2P мессенджера.
//!
//! Содержит платформо-независимую логику: идентичность, криптографию,
//! сетевой протокол. Используется и Desktop (Tauri), и Android (через JNI).

pub mod kdf;
pub mod backup;
pub mod budget;
pub mod db;
pub mod fsutil;
pub mod identity;
pub mod media;
pub mod noise;
pub mod outbox;
pub mod pad;
pub mod protocol;
pub mod ratelimit;
pub mod session;
pub mod settings;
pub mod store;
pub mod vault;

pub use identity::{Identity, IdentityError};

/// Детерминированный room_id для DM-звонков.
/// = первые 16 байт SHA-256(min(keyA,keyB) ++ max(keyA,keyB)).
/// Обе стороны вычисляют одно и то же значение независимо.
pub fn dm_room_id(a: &[u8; 32], b: &[u8; 32]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    if a <= b { h.update(a); h.update(b); } else { h.update(b); h.update(a); }
    h.finalize()[..16].to_vec()
}
