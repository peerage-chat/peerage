//! Демонстрация модуля идентичности: генерирует новую идентичность
//! и печатает адрес Yggdrasil, ключи и BIP39-fingerprint.
//!
//! Запуск: `cargo run -p peerage-core --example whoami`

use peerage_core::Identity;

fn main() {
    let id = Identity::generate();

    println!("== Новая идентичность узла ==\n");
    println!("ed25519 публичный ключ : {}", hex::encode(id.public_key_bytes()));
    println!("Yggdrasil адрес         : {}", id.yggdrasil_address());
    println!("\nFingerprint (8 слов BIP39):");
    println!("  {}", id.fingerprint());
    println!("\nseed (СЕКРЕТ, для сохранения): {}", hex::encode(id.seed()));
}
