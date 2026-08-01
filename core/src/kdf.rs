//! Вывод ключа из пароля пользователя (Argon2id) — для уровня at-rest «пароль».
//!
//! **Зачем отдельно от [`crate::vault`].** Vault шифрует данные ключом, который
//! хранит ОС; здесь мы получаем ключ из того, что пользователь ЗНАЕТ. Разница
//! принципиальная: ключ из Keystore отдаётся по запросу приложения, то есть тому,
//! кто держит разблокированный телефон. Знание паролем не отдаётся — данные
//! остаются шифрованными, пока его не ввели.
//!
//! **Почему Argon2id, а не PBKDF2/scrypt.** Пароли короткие, и единственная защита
//! от перебора — стоимость одной попытки. Argon2id требует памяти, что бьёт по
//! GPU/ASIC-перебору сильнее, чем итерации PBKDF2; это же рекомендация OWASP и
//! победитель PHC.
//!
//! Параметры подобраны под мобильное железо: 64 МиБ памяти, 3 прохода. На среднем
//! телефоне это ~0.5–1 с — терпимо при разблокировке (не на каждое сообщение), но
//! дорого при переборе. Числа зафиксированы в файле-заголовке хранилища, чтобы их
//! можно было поднять позже, не ломая существующие установки.

use argon2::{Algorithm, Argon2, Params, Version};

/// Память, КиБ. 64 МиБ: нижняя граница, за которой перебор на GPU дешевеет.
pub const ARGON_MEM_KIB: u32 = 64 * 1024;
/// Число проходов.
pub const ARGON_TIME: u32 = 3;
/// Параллелизм (одна полоса: на телефоне лишние потоки не ускоряют, а греют).
pub const ARGON_LANES: u32 = 1;
/// Длина соли.
pub const SALT_LEN: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum KdfError {
    #[error("пустой пароль")]
    EmptyPassword,
    #[error("неверная длина соли: ожидалось {SALT_LEN}, получено {0}")]
    BadSalt(usize),
    #[error("argon2: {0}")]
    Argon(String),
}

/// Вывести 32-байтный ключ шифрования из пароля и соли (Argon2id).
///
/// Соль обязана быть случайной и храниться рядом с зашифрованным ключом: без неё
/// одинаковые пароли давали бы одинаковые ключи, и радужная таблица окупалась бы
/// на всех пользователях разом.
pub fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], KdfError> {
    if password.is_empty() {
        return Err(KdfError::EmptyPassword);
    }
    if salt.len() != SALT_LEN {
        return Err(KdfError::BadSalt(salt.len()));
    }
    let params = Params::new(ARGON_MEM_KIB, ARGON_TIME, ARGON_LANES, Some(32))
        .map_err(|e| KdfError::Argon(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| KdfError::Argon(e.to_string()))?;
    Ok(out)
}

/// Свежая случайная соль.
pub fn new_salt() -> [u8; SALT_LEN] {
    use rand_core::RngCore;
    let mut s = [0u8; SALT_LEN];
    rand_core::OsRng.fill_bytes(&mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_password_and_salt_give_same_key() {
        let salt = [7u8; SALT_LEN];
        assert_eq!(derive_key("тайна", &salt).unwrap(), derive_key("тайна", &salt).unwrap());
    }

    #[test]
    fn different_salt_gives_different_key() {
        // Иначе одна радужная таблица окупалась бы на всех пользователях сразу.
        let a = derive_key("тайна", &[1u8; SALT_LEN]).unwrap();
        let b = derive_key("тайна", &[2u8; SALT_LEN]).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn different_password_gives_different_key() {
        let salt = [3u8; SALT_LEN];
        assert_ne!(derive_key("тайна", &salt).unwrap(), derive_key("тайна1", &salt).unwrap());
    }

    #[test]
    fn empty_password_and_bad_salt_are_errors() {
        // Пустой пароль — не «ключ из пустой строки», а отказ: иначе пользователь
        // включил бы защиту, которой нет.
        assert!(matches!(derive_key("", &[0u8; SALT_LEN]), Err(KdfError::EmptyPassword)));
        assert!(matches!(derive_key("x", &[0u8; 4]), Err(KdfError::BadSalt(4))));
    }

    #[test]
    fn salt_is_random() {
        assert_ne!(new_salt(), new_salt());
    }
}
