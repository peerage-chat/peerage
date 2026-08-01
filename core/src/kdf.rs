//! Вывод ключа из пароля пользователя (Argon2id) — для уровня at-rest «пароль» и
//! для резервной копии данных.
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
//! # Профиль КАЖДОГО файла записывается в сам файл
//!
//! Параметры (память/проходы) со временем обязаны расти вслед за железом. Но ключ,
//! выведенный с другими параметрами, — ДРУГОЙ ключ: подними мы 64 МиБ до 128 МиБ,
//! не записав это в файл, и все существующие хранилища и все ранее снятые копии
//! перестали бы открываться, причём с сообщением «неверный пароль» — человек решил
//! бы, что перепутал пароль, и стёр бы данные.
//!
//! Поэтому у каждого файла есть **номер профиля** ([`Profile`]), и он хранится
//! рядом с солью: `vault.meta` — третьей строкой, заголовок копии — байтом. Новый
//! профиль добавляется в таблицу, старые остаются читаемыми навсегда.
//!
//! **Номер, а не сами числа**, потому что числа из файла — это ввод: файл с
//! «памятью 4 ГиБ» положил бы приложение при попытке открытия, ещё до всякой
//! проверки пароля. Из таблицы можно взять только то, что мы сами туда положили.

use argon2::{Algorithm, Argon2, Params, Version};

/// Память, КиБ. 64 МиБ: нижняя граница, за которой перебор на GPU дешевеет.
pub const ARGON_MEM_KIB: u32 = 64 * 1024;
/// Число проходов.
pub const ARGON_TIME: u32 = 3;
/// Параллелизм (одна полоса: на телефоне лишние потоки не ускоряют, а греют).
pub const ARGON_LANES: u32 = 1;
/// Длина соли.
pub const SALT_LEN: usize = 16;

/// Профиль стоимости KDF. Номер пишется в файл; менять СМЫСЛ существующего номера
/// нельзя никогда — это сделает нечитаемыми уже созданные файлы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    pub id: u8,
    /// Память, КиБ.
    pub mem_kib: u32,
    /// Проходы.
    pub time: u32,
    /// Полосы.
    pub lanes: u32,
}

/// Профиль 1 (2026): 64 МиБ / 3 прохода / 1 полоса — ~1 с на среднем телефоне.
const PROFILE_1: Profile =
    Profile { id: 1, mem_kib: ARGON_MEM_KIB, time: ARGON_TIME, lanes: ARGON_LANES };

/// Чем защищаются НОВЫЕ файлы. Поднимать вместе с добавлением нового профиля в
/// [`profile`], а не правкой существующего.
pub const CURRENT_PROFILE: u8 = 1;

/// Параметры по номеру профиля. `None` — номер из будущей версии приложения.
pub fn profile(id: u8) -> Option<Profile> {
    match id {
        1 => Some(PROFILE_1),
        _ => None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KdfError {
    #[error("пустой пароль")]
    EmptyPassword,
    #[error("неверная длина соли: ожидалось {SALT_LEN}, получено {0}")]
    BadSalt(usize),
    #[error("файл защищён несовместимой версией приложения (профиль {0})")]
    UnknownProfile(u8),
    #[error("argon2: {0}")]
    Argon(String),
}

/// Вывести 32-байтный ключ шифрования из пароля и соли по ТЕКУЩЕМУ профилю.
///
/// Для чтения существующего файла берите [`derive_key_with`] и профиль ИЗ ФАЙЛА:
/// иначе после первого же повышения стоимости старые файлы перестанут открываться.
pub fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], KdfError> {
    derive_key_with(CURRENT_PROFILE, password, salt)
}

/// То же с явным профилем (номер прочитан из файла).
///
/// Соль обязана быть случайной и храниться рядом с зашифрованным ключом: без неё
/// одинаковые пароли давали бы одинаковые ключи, и радужная таблица окупалась бы
/// на всех пользователях разом.
pub fn derive_key_with(profile_id: u8, password: &str, salt: &[u8]) -> Result<[u8; 32], KdfError> {
    if password.is_empty() {
        return Err(KdfError::EmptyPassword);
    }
    if salt.len() != SALT_LEN {
        return Err(KdfError::BadSalt(salt.len()));
    }
    let p = profile(profile_id).ok_or(KdfError::UnknownProfile(profile_id))?;
    let params = Params::new(p.mem_kib, p.time, p.lanes, Some(32))
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

    /// Смысл номера профиля — договор с уже существующими на устройствах файлами.
    /// Если этот тест упал, значит кто-то изменил параметры СУЩЕСТВУЮЩЕГО профиля:
    /// все хранилища и копии, созданные раньше, стали нечитаемыми. Нужен НОВЫЙ
    /// номер, а не правка старого.
    #[test]
    fn profile_1_parameters_are_frozen() {
        let p = profile(1).unwrap();
        assert_eq!((p.mem_kib, p.time, p.lanes), (64 * 1024, 3, 1));
    }

    #[test]
    fn key_depends_on_profile_and_unknown_profile_is_a_clear_error() {
        // Ключ, выведенный другим профилем, — другой ключ; поэтому файл обязан
        // нести свой номер, а не полагаться на текущий.
        let salt = [9u8; SALT_LEN];
        assert!(derive_key_with(1, "тайна", &salt).is_ok());
        assert!(matches!(
            derive_key_with(200, "тайна", &salt),
            Err(KdfError::UnknownProfile(200))
        ));
    }
}
