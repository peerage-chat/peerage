//! Контейнер резервной копии ДАННЫХ: переписка, контакты, вложения.
//!
//! **Не путать с копией КЛЮЧА.** Ключ (сид-фраза) возвращает личность — то, кем вас
//! видит сеть. Эта копия возвращает переписку. Разделение намеренное: у них разный
//! срок жизни и разная цена утечки, поэтому и носители разные — 24 слова на бумаге
//! против файла в облаке.
//!
//! **Чем закрыт файл (шаг 7k).** Копия по смыслу покидает устройство: её кладут в облако,
//! на карту, на чужой компьютер, а открывать её нужно на НОВОМ устройстве. Ключ хранилища
//! для этого не годится (он в Keystore одного телефона). По умолчанию ключ файла выводится
//! из личности — `Identity::derive_subkey(`[`SUBKEY_CONTEXT`]`)`, то есть из тех же 24 слов:
//! помнить нечего, на новом телефоне «24 слова → файл → готово». По желанию сверху —
//! пароль (Argon2id, [`crate::kdf`]): ключ файла тогда смешивает оба, и без пароля такую
//! копию не откроют и 24 слова. Следствие: копию открывает только та же личность.
//!
//! **Формат** (потоковый, кадрами — чтобы не держать гигабайты вложений в памяти):
//!
//! ```text
//! "PEERBK" | version u8 | flags u8 | kdf_profile u8 | salt[16]
//! кадр*:   len u32 LE | nonce[24] | ciphertext(XChaCha20-Poly1305)
//! ```
//!
//! `flags` бит 0 — «есть пароль»: восстановление спрашивает пароль только тогда.
//! Подмена флага ничего не даёт — ключ файла выводится по нему, и с другим флагом
//! первый кадр не расшифруется.
//!
//! `kdf_profile` — номер профиля стоимости Argon2id ([`crate::kdf::Profile`]).
//! Он лежит В ФАЙЛЕ, потому что стоимость обязана расти вслед за железом, а ключ,
//! выведенный другими параметрами, — другой ключ: без записанного профиля первое же
//! повышение сделало бы все ранее снятые копии нечитаемыми, и человек увидел бы
//! «неверный пароль» там, где пароль верный. Без пароля профиль не используется.
//!
//! Открытый текст кадра — `[kind u8][payload]`: `0` — JSON-запись, `1` — блоб
//! (`[id_len u8][id][bytes]`), `2` — конец.
//!
//! **Номер кадра идёт в AAD, а поток обязан заканчиваться кадром-концом.** Без
//! этого тот, кто держит файл, мог бы переставить или отрезать куски (например,
//! удалить неудобную часть переписки), не зная пароля: каждый кадр аутентичен сам
//! по себе, но их ПОРЯДОК и ПОЛНОТА — нет.

use std::io::{Read, Write};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};

const MAGIC: &[u8; 6] = b"PEERBK";
/// Версия формата. Растёт при несовместимых изменениях; читатель отказывается от
/// незнакомых версий, а не «пытается разобрать» — молча испорченная копия хуже
/// честного отказа.
pub const VERSION: u8 = 2;
/// Метка подключа личности для копий: `Identity::derive_subkey(SUBKEY_CONTEXT)`.
pub const SUBKEY_CONTEXT: &str = "peerage:backup:v1";
/// Домен ключа файла: подключ личности + соль (+ ключ из пароля).
const FILE_KEY_CONTEXT: &str = "peerage:backup-file:v2";
const FLAG_PASSWORD: u8 = 1;
const HEADER_LEN: usize = 9;
const NONCE_LEN: usize = 24;
/// Потолок на кадр (16 МиБ): защита от файла, который заявляет гигабайтный кадр и
/// заставляет нас выделить память до всякой проверки подписи.
const MAX_FRAME: u32 = 16 * 1024 * 1024;

const KIND_JSON: u8 = 0;
const KIND_BLOB: u8 = 1;
const KIND_END: u8 = 2;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("это не файл резервной копии")]
    BadMagic,
    #[error("формат копии {0} не поддерживается")]
    Version(u8),
    #[error("неверный пароль или файл повреждён")]
    Decrypt,
    #[error("копия другой личности или файл повреждён")]
    Foreign,
    #[error("копия закрыта паролем")]
    PasswordRequired,
    #[error("файл обрезан: копия неполная")]
    Truncated,
    #[error("ввод-вывод: {0}")]
    Io(String),
    #[error("{0}")]
    Kdf(String),
    #[error("разбор записи: {0}")]
    Parse(String),
}

impl From<std::io::Error> for BackupError {
    fn from(e: std::io::Error) -> Self {
        BackupError::Io(e.to_string())
    }
}

/// Одна прочитанная запись.
pub enum Frame {
    /// JSON-запись (секция данных).
    Json(Vec<u8>),
    /// Файл вложения: контент-адресный id и байты.
    Blob { id: String, data: Vec<u8> },
}

/// Ключ файла: подключ личности и соль, с паролем — ещё и Argon2id от него.
fn cipher(
    identity_key: &[u8; 32],
    salt: &[u8],
    password: Option<(&str, u8)>,
) -> Result<XChaCha20Poly1305, BackupError> {
    let mut ikm = zeroize::Zeroizing::new(Vec::with_capacity(32 + salt.len() + 32));
    ikm.extend_from_slice(identity_key);
    ikm.extend_from_slice(salt);
    if let Some((pw, profile)) = password {
        let kp = crate::kdf::derive_key_with(profile, pw, salt)
            .map_err(|e| BackupError::Kdf(e.to_string()))?;
        ikm.extend_from_slice(&kp);
    }
    let key = zeroize::Zeroizing::new(blake3::derive_key(FILE_KEY_CONTEXT, &ikm));
    Ok(XChaCha20Poly1305::new(Key::from_slice(&key[..])))
}

/// Заголовок копии: закрыта ли паролем. Читается до всякой расшифровки — чтобы
/// восстановление спрашивало пароль только тогда, когда он есть.
pub fn needs_password<R: Read>(mut inp: R) -> Result<bool, BackupError> {
    Ok(read_header(&mut inp)?.0 & FLAG_PASSWORD != 0)
}

/// Магия, версия, флаги, профиль и соль; незнакомые версия и флаги — отказ.
fn read_header<R: Read>(inp: &mut R) -> Result<(u8, u8, [u8; crate::kdf::SALT_LEN]), BackupError> {
    let mut head = [0u8; HEADER_LEN];
    inp.read_exact(&mut head).map_err(|_| BackupError::BadMagic)?;
    if &head[..6] != MAGIC {
        return Err(BackupError::BadMagic);
    }
    if head[6] != VERSION {
        return Err(BackupError::Version(head[6]));
    }
    if head[7] & !FLAG_PASSWORD != 0 {
        return Err(BackupError::Parse(format!("неизвестные флаги копии {:#04x}", head[7])));
    }
    let mut salt = [0u8; crate::kdf::SALT_LEN];
    inp.read_exact(&mut salt).map_err(|_| BackupError::Truncated)?;
    Ok((head[7], head[8], salt))
}

/// Пишущая сторона: кадры шифруются по одному, файл наполняется потоково.
pub struct Writer<W: Write> {
    out: W,
    cipher: XChaCha20Poly1305,
    idx: u64,
}

impl<W: Write> Writer<W> {
    /// Начать копию: записать заголовок и вывести ключ файла из подключа личности
    /// [`SUBKEY_CONTEXT`] (+ пароля, если задан; Argon2id — операция на ~секунду,
    /// поэтому один раз на файл, а не на кадр). Пустой пароль — то же, что без пароля.
    pub fn create(mut out: W, identity_key: &[u8; 32], password: Option<&str>) -> Result<Self, BackupError> {
        let password = password.filter(|p| !p.is_empty());
        let salt = crate::kdf::new_salt();
        let flags = if password.is_some() { FLAG_PASSWORD } else { 0 };
        out.write_all(MAGIC)?;
        out.write_all(&[VERSION, flags, crate::kdf::CURRENT_PROFILE])?;
        out.write_all(&salt)?;
        let cipher = cipher(identity_key, &salt, password.map(|p| (p, crate::kdf::CURRENT_PROFILE)))?;
        Ok(Writer { out, cipher, idx: 0 })
    }

    fn frame(&mut self, kind: u8, body: &[u8]) -> Result<(), BackupError> {
        let mut plain = Vec::with_capacity(body.len() + 1);
        plain.push(kind);
        plain.extend_from_slice(body);

        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let aad = self.idx.to_le_bytes();
        let ct = self
            .cipher
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: &plain, aad: &aad })
            .map_err(|_| BackupError::Decrypt)?;

        self.out.write_all(&(ct.len() as u32).to_le_bytes())?;
        self.out.write_all(&nonce)?;
        self.out.write_all(&ct)?;
        self.idx += 1;
        Ok(())
    }

    /// Записать секцию данных.
    pub fn json<T: serde::Serialize>(&mut self, rec: &T) -> Result<(), BackupError> {
        let body = serde_json::to_vec(rec).map_err(|e| BackupError::Parse(e.to_string()))?;
        self.frame(KIND_JSON, &body)
    }

    /// Записать файл вложения.
    pub fn blob(&mut self, id: &str, data: &[u8]) -> Result<(), BackupError> {
        let id_bytes = id.as_bytes();
        if id_bytes.len() > u8::MAX as usize {
            return Err(BackupError::Parse("слишком длинный id блоба".into()));
        }
        let mut body = Vec::with_capacity(1 + id_bytes.len() + data.len());
        body.push(id_bytes.len() as u8);
        body.extend_from_slice(id_bytes);
        body.extend_from_slice(data);
        self.frame(KIND_BLOB, &body)
    }

    /// Закрыть копию. **Обязательно:** без кадра-конца читатель считает файл
    /// обрезанным — иначе оборванная на середине запись выглядела бы как целая.
    pub fn finish(mut self) -> Result<(), BackupError> {
        self.frame(KIND_END, &[])?;
        self.out.flush()?;
        Ok(())
    }
}

/// Читающая сторона.
pub struct Reader<R: Read> {
    inp: R,
    cipher: XChaCha20Poly1305,
    idx: u64,
    done: bool,
    /// Закрыта ли паролем: от этого зависит, что значит провал первого кадра.
    password: bool,
}

impl<R: Read> Reader<R> {
    /// Открыть копию ключом личности [`SUBKEY_CONTEXT`] (+ паролем, если файл им
    /// закрыт). Чужая личность и неверный пароль обнаруживаются на первом же кадре —
    /// по провалу аутентификации, а не по «странным» данным. Пароль к копии без
    /// пароля не мешает: он просто не нужен.
    pub fn open(mut inp: R, identity_key: &[u8; 32], password: Option<&str>) -> Result<Self, BackupError> {
        let (flags, profile, salt) = read_header(&mut inp)?;
        let with_password = flags & FLAG_PASSWORD != 0;
        let password = match (with_password, password.filter(|p| !p.is_empty())) {
            (true, None) => return Err(BackupError::PasswordRequired),
            // Профиль берём ИЗ ФАЙЛА, а не текущий: старые копии обязаны открываться
            // после повышения стоимости.
            (true, Some(p)) => Some((p, profile)),
            (false, _) => None,
        };
        let cipher = cipher(identity_key, &salt, password)?;
        Ok(Reader { inp, cipher, idx: 0, done: false, password: with_password })
    }

    /// Следующая запись; `None` — корректный конец файла.
    pub fn next(&mut self) -> Result<Option<Frame>, BackupError> {
        if self.done {
            return Ok(None);
        }
        let mut len = [0u8; 4];
        self.inp.read_exact(&mut len).map_err(|_| BackupError::Truncated)?;
        let len = u32::from_le_bytes(len);
        if len > MAX_FRAME {
            return Err(BackupError::Parse(format!("кадр {len} байт — больше допустимого")));
        }
        let mut nonce = [0u8; NONCE_LEN];
        self.inp.read_exact(&mut nonce).map_err(|_| BackupError::Truncated)?;
        let mut ct = vec![0u8; len as usize];
        self.inp.read_exact(&mut ct).map_err(|_| BackupError::Truncated)?;

        let aad = self.idx.to_le_bytes();
        // Первый кадр без пароля не открылся — ключ личности не тот (или файл испорчен);
        // с паролем — скорее пароль. Дальше по файлу — только порча.
        let first_foreign = self.idx == 0 && !self.password;
        let plain = self
            .cipher
            .decrypt(XNonce::from_slice(&nonce), Payload { msg: &ct, aad: &aad })
            .map_err(|_| if first_foreign { BackupError::Foreign } else { BackupError::Decrypt })?;
        self.idx += 1;

        let (kind, body) = plain.split_first().ok_or(BackupError::Decrypt)?;
        match *kind {
            KIND_JSON => Ok(Some(Frame::Json(body.to_vec()))),
            KIND_BLOB => {
                let (n, rest) = body.split_first().ok_or(BackupError::Decrypt)?;
                let n = *n as usize;
                if rest.len() < n {
                    return Err(BackupError::Decrypt);
                }
                let id = String::from_utf8(rest[..n].to_vec())
                    .map_err(|_| BackupError::Parse("id блоба не UTF-8".into()))?;
                Ok(Some(Frame::Blob { id, data: rest[n..].to_vec() }))
            }
            KIND_END => {
                self.done = true;
                Ok(None)
            }
            other => Err(BackupError::Parse(format!("неизвестный тип записи {other}"))),
        }
    }

    /// Дошли ли до кадра-конца. Проверять после обхода: `next()` вернёт `None` и
    /// на обрыве, и на честном конце — различает их только это.
    pub fn complete(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Rec {
        who: String,
        n: u32,
    }

    const ME: [u8; 32] = [7u8; 32];
    const OTHER: [u8; 32] = [8u8; 32];

    fn write_sample(key: &[u8; 32], password: Option<&str>) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut w = Writer::create(&mut buf, key, password).unwrap();
        w.json(&Rec { who: "аня".into(), n: 1 }).unwrap();
        w.blob("aa11", b"file bytes").unwrap();
        w.json(&Rec { who: "боря".into(), n: 2 }).unwrap();
        w.finish().unwrap();
        buf
    }

    fn read_all(buf: &[u8], key: &[u8; 32], password: Option<&str>) -> Result<(Vec<Rec>, usize), BackupError> {
        let mut r = Reader::open(buf, key, password)?;
        let (mut jsons, mut blobs) = (Vec::new(), 0);
        while let Some(f) = r.next()? {
            match f {
                Frame::Json(b) => jsons.push(serde_json::from_slice::<Rec>(&b).unwrap()),
                Frame::Blob { .. } => blobs += 1,
            }
        }
        assert!(r.complete());
        Ok((jsons, blobs))
    }

    #[test]
    fn roundtrip_preserves_records_and_order() {
        let buf = write_sample(&ME, None);
        let mut r = Reader::open(&buf[..], &ME, None).unwrap();
        let mut jsons = Vec::new();
        let mut blobs = Vec::new();
        while let Some(f) = r.next().unwrap() {
            match f {
                Frame::Json(b) => jsons.push(serde_json::from_slice::<Rec>(&b).unwrap()),
                Frame::Blob { id, data } => blobs.push((id, data)),
            }
        }
        assert!(r.complete());
        assert_eq!(jsons, vec![Rec { who: "аня".into(), n: 1 }, Rec { who: "боря".into(), n: 2 }]);
        assert_eq!(blobs, vec![("aa11".to_string(), b"file bytes".to_vec())]);
    }

    /// Без пароля копию открывает та же личность — и только она.
    #[test]
    fn without_password_only_the_same_identity_opens_it() {
        let buf = write_sample(&ME, None);
        assert!(!needs_password(&buf[..]).unwrap());
        assert_eq!(read_all(&buf, &ME, None).unwrap().1, 1);
        // Лишний пароль к копии без пароля не мешает.
        assert!(read_all(&buf, &ME, Some("зачем-то")).is_ok());
        let mut r = Reader::open(&buf[..], &OTHER, None).unwrap();
        assert!(matches!(r.next(), Err(BackupError::Foreign)));
    }

    /// С паролем нужны оба: и личность, и пароль. Без пароля — внятный отказ до
    /// расшифровки, а не «повреждён».
    #[test]
    fn with_password_both_identity_and_password_are_needed() {
        let buf = write_sample(&ME, Some("пароль"));
        assert!(needs_password(&buf[..]).unwrap());
        assert!(matches!(Reader::open(&buf[..], &ME, None), Err(BackupError::PasswordRequired)));
        assert!(matches!(Reader::open(&buf[..], &ME, Some("")), Err(BackupError::PasswordRequired)));
        let mut r = Reader::open(&buf[..], &ME, Some("не тот")).unwrap();
        assert!(matches!(r.next(), Err(BackupError::Decrypt)));
        let mut r = Reader::open(&buf[..], &OTHER, Some("пароль")).unwrap();
        assert!(matches!(r.next(), Err(BackupError::Decrypt)));
        assert_eq!(read_all(&buf, &ME, Some("пароль")).unwrap().0.len(), 2);
    }

    /// Пустой пароль при создании — копия без пароля, а не «закрыта пустым».
    #[test]
    fn empty_password_means_no_password() {
        let buf = write_sample(&ME, Some(""));
        assert!(!needs_password(&buf[..]).unwrap());
        assert!(read_all(&buf, &ME, None).is_ok());
    }

    /// Снять флаг пароля, чтобы открыть без него, нельзя: ключ выводится по флагу.
    #[test]
    fn clearing_the_password_flag_does_not_bypass_it() {
        let mut buf = write_sample(&ME, Some("пароль"));
        buf[7] = 0;
        let mut r = Reader::open(&buf[..], &ME, None).unwrap();
        assert!(r.next().is_err());
        buf[7] = 0x80;
        assert!(matches!(Reader::open(&buf[..], &ME, None), Err(BackupError::Parse(_))));
    }

    #[test]
    fn foreign_file_is_rejected() {
        let mut r = Reader::open(&b"just some bytes here"[..], &ME, None);
        assert!(matches!(r, Err(BackupError::BadMagic)));
        // И другая версия формата (в том числе v1 до шага 7k) — отказ, а не попытка разобрать.
        let mut buf = write_sample(&ME, None);
        buf[6] = VERSION + 1;
        r = Reader::open(&buf[..], &ME, None);
        assert!(matches!(r, Err(BackupError::Version(_))));
        buf[6] = 1;
        assert!(matches!(Reader::open(&buf[..], &ME, None), Err(BackupError::Version(1))));
    }

    /// Профиль KDF обязан ехать в файле: копия, снятая сегодня, должна открываться
    /// и после того, как мы поднимем стоимость перебора для новых файлов.
    #[test]
    fn header_carries_kdf_profile_and_unknown_one_is_a_clear_error() {
        let buf = write_sample(&ME, Some("пароль"));
        assert_eq!(buf[8], crate::kdf::CURRENT_PROFILE, "профиль записан в заголовок");

        // Файл из будущего: отказ с внятным текстом, а не «неверный пароль».
        let mut future = buf.clone();
        future[8] = 200;
        match Reader::open(&future[..], &ME, Some("пароль")) {
            Err(BackupError::Kdf(msg)) => assert!(msg.contains("200"), "текст ошибки: {msg}"),
            other => panic!("ожидался отказ по профилю, получено {:?}", other.err()),
        }
    }

    #[test]
    fn truncation_is_detected() {
        // Тот, кто держит файл, не знает ключа — но мог бы отрезать хвост.
        // Отсутствие кадра-конца обязано это выдать.
        let buf = write_sample(&ME, None);
        let cut = &buf[..buf.len() - 40];
        let mut r = Reader::open(cut, &ME, None).unwrap();
        let mut err = None;
        loop {
            match r.next() {
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        assert!(!r.complete(), "обрезанный файл не должен считаться полным");
        assert!(matches!(err, Some(BackupError::Truncated) | None));
    }

    #[test]
    fn reordered_frames_are_rejected() {
        // Кадры аутентичны по отдельности; их порядок держится только на AAD:
        // второй кадр, поставленный первым, не открывается.
        let buf = write_sample(&ME, None);
        let head = HEADER_LEN + crate::kdf::SALT_LEN;
        let len1 = u32::from_le_bytes(buf[head..head + 4].try_into().unwrap()) as usize;
        let rest = &buf[head + 4 + NONCE_LEN + len1..];
        let mut forged = buf[..head].to_vec();
        forged.extend_from_slice(rest);
        let mut r = Reader::open(&forged[..], &ME, None).unwrap();
        assert!(r.next().is_err());
    }
}
