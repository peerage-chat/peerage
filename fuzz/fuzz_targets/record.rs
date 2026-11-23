//! Подписанная запись v2 (адрес в сети): приходит в объявлении о смене адреса и в коде
//! обмена от кого угодно. Разбор обязан не паниковать на любых байтах, а принятая запись —
//! быть ровно той, что подписал ключ личности.
#![no_main]

use libfuzzer_sys::fuzz_target;
use peerage_core::record::AddressRecord;
use peerage_core::Identity;

fuzz_target!(|data: &[u8]| {
    let me = Identity::from_seed(&[7u8; 32]).unwrap();
    let key = me.public_key_bytes();
    // Чужие байты с чужой подписью: не паниковать и не принимать.
    if data.len() >= 64 {
        let (sig, rec) = data.split_at(64);
        let sig: [u8; 64] = sig.try_into().unwrap();
        assert!(AddressRecord::verify(&key, rec, &sig).is_none());
    }
    // Своя запись с адресом из данных: принимается и разбирается обратно в то же.
    let addr = &data[..data.len().min(600)];
    let epoch = data.iter().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(*b as u32));
    let r = AddressRecord { kind: peerage_core::record::NetKind::Yggdrasil, epoch, addr: addr.to_vec() };
    let s = r.sign(&me);
    assert_eq!(AddressRecord::verify(&key, &s.bytes, &s.sig), Some(r));
});
