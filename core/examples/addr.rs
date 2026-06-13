//! Печатает адрес Yggdrasil для заданного hex-публичного ключа ed25519.
//! Используется для кросс-проверки с yggdrasil-go (см. ygg/cmd/yggaddr).
//!
//! Запуск: `cargo run -p p2p-core --example addr -- <hex-pubkey>`

use p2p_core::identity::addr_for_key;

fn main() {
    let arg = std::env::args().nth(1).expect("укажите hex-публичный ключ (32 байта)");
    let bytes = hex::decode(arg.trim()).expect("неверный hex");
    let key: [u8; 32] = bytes.as_slice().try_into().expect("нужно ровно 32 байта");
    println!("{}", addr_for_key(&key));
}
