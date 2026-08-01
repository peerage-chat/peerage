//! Контейнер резервной копии ДАННЫХ: переписка, контакты, вложения.
//!
//! **Не путать с копией КЛЮЧА.** Ключ (сид-фраза) возвращает личность — то, кем вас
//! видит сеть. Эта копия возвращает переписку. Разделение намеренное: у них разный
//! срок жизни и разная цена утечки, поэтому и носители разные — 24 слова на бумаге
//! против файла в облаке.
//!
//! **Почему файл шифруется своим паролем, а не ключом хранилища.** Копия по смыслу
//! покидает устройство: её кладут в облако, на карту, на чужой компьютер. Ключ
//! хранилища оттуда недоступен (он в Keystore и не извлекается), да и незачем: файл
//! должен открываться на НОВОМ устройстве, где этого ключа нет. Поэтому — пароль и
//! Argon2id (см. [`crate::kdf`]).
//!
//! **Формат** (потоковый, кадрами — чтобы не держать гигабайты вложений в памяти):
//!
//! ```text
//! "P2PBAK" | version u8 | kdf_profile u8 | salt[16]
//! кадр*:   len u32 LE | nonce[24] | ciphertext(XChaCha20-Poly1305)
//! ```
//!
//! `kdf_profile` — номер профиля стоимости Argon2id ([`crate::kdf::Profile`]).
//! Он лежит В ФАЙЛЕ, потому что стоимость обязана расти вслед за железом, а ключ,
//! выведенный другими параметрами, — другой ключ: без записанного профиля первое же
//! повышение сделало бы все ранее снятые копии нечитаемыми, и человек увидел бы
//! «неверный пароль» там, где пароль верный.
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

const MAGIC: &[u8; 6] = b"P2PBAK";
/// Версия формата. Растёт при несовместимых изменениях; читатель отказывается от
/// незнакомых версий, а не «пытается разобрать» — молча испорченная копия хуже
/// честного отказа.
pub const VERSION: u8 = 1;
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
    #[error("копия сделана более новой версией приложения (формат {0})")]
    Version(u8),
    #[error("неверный пароль или файл повреждён")]
    Decrypt,
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

fn cipher(profile: u8, password: &str, salt: &[u8]) -> Result<XChaCha20Poly1305, BackupError> {
    let key = crate::kdf::derive_key_with(profile, password, salt)
        .map_err(|e| BackupError::Kdf(e.to_string()))?;
    Ok(XChaCha20Poly1305::new(Key::from_slice(&key)))
}

/// Пишущая сторона: кадры шифруются по одному, файл наполняется потоково.
pub struct Writer<W: Write> {
    out: W,
    cipher: XChaCha20Poly1305,
    idx: u64,
}

impl<W: Write> Writer<W> {
    /// Начать копию: записать заголовок и вывести ключ из пароля (Argon2id —
    /// операция на ~секунду, поэтому один раз на файл, а не на кадр).
    pub fn create(mut out: W, password: &str) -> Result<Self, BackupError> {
        let salt = crate::kdf::new_salt();
        out.write_all(MAGIC)?;
        out.write_all(&[VERSION, crate::kdf::CURRENT_PROFILE])?;
        out.write_all(&salt)?;
        let cipher = cipher(crate::kdf::CURRENT_PROFILE, password, &salt)?;
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
}

impl<R: Read> Reader<R> {
    /// Открыть копию. Неверный пароль обнаруживается на первом же кадре — по
    /// провалу аутентификации, а не по «странным» данным.
    pub fn open(mut inp: R, password: &str) -> Result<Self, BackupError> {
        let mut head = [0u8; 8];
        inp.read_exact(&mut head).map_err(|_| BackupError::BadMagic)?;
        if &head[..6] != MAGIC {
            return Err(BackupError::BadMagic);
        }
        if head[6] != VERSION {
            return Err(BackupError::Version(head[6]));
        }
        let mut salt = [0u8; crate::kdf::SALT_LEN];
        inp.read_exact(&mut salt).map_err(|_| BackupError::Truncated)?;
        // Профиль берём ИЗ ФАЙЛА, а не текущий: старые копии обязаны открываться
        // после повышения стоимости.
        let cipher = cipher(head[7], password, &salt)?;
        Ok(Reader { inp, cipher, idx: 0, done: false })
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
        let plain = self
            .cipher
            .decrypt(XNonce::from_slice(&nonce), Payload { msg: &ct, aad: &aad })
            .map_err(|_| BackupError::Decrypt)?;
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

    fn write_sample(password: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut w = Writer::create(&mut buf, password).unwrap();
        w.json(&Rec { who: "аня".into(), n: 1 }).unwrap();
        w.blob("aa11", b"file bytes").unwrap();
        w.json(&Rec { who: "боря".into(), n: 2 }).unwrap();
        w.finish().unwrap();
        buf
    }

    #[test]
    fn roundtrip_preserves_records_and_order() {
        let buf = write_sample("пароль");
        let mut r = Reader::open(&buf[..], "пароль").unwrap();
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

    #[test]
    fn wrong_password_fails_on_first_frame() {
        // Важно, что ошибка приходит сразу и явная: иначе пользователь решил бы,
        // что копия пустая, а не что пароль другой.
        let buf = write_sample("пароль");
        let mut r = Reader::open(&buf[..], "не тот").unwrap();
        assert!(matches!(r.next(), Err(BackupError::Decrypt)));
    }

    #[test]
    fn foreign_file_is_rejected() {
        let mut r = Reader::open(&b"just some bytes here"[..], "x");
        assert!(matches!(r, Err(BackupError::BadMagic)));
        // И новая версия формата — тоже отказ, а не попытка разобрать.
        let mut buf = write_sample("пароль");
        buf[6] = VERSION + 1;
        r = Reader::open(&buf[..], "пароль");
        assert!(matches!(r, Err(BackupError::Version(_))));
    }

    /// Профиль KDF обязан ехать в файле: копия, снятая сегодня, должна открываться
    /// и после того, как мы поднимем стоимость перебора для новых файлов.
    #[test]
    fn header_carries_kdf_profile_and_unknown_one_is_a_clear_error() {
        let buf = write_sample("пароль");
        assert_eq!(buf[7], crate::kdf::CURRENT_PROFILE, "профиль записан в заголовок");

        // Файл из будущего: отказ с внятным текстом, а не «неверный пароль».
        let mut future = buf.clone();
        future[7] = 200;
        match Reader::open(&future[..], "пароль") {
            Err(BackupError::Kdf(msg)) => assert!(msg.contains("200"), "текст ошибки: {msg}"),
            other => panic!("ожидался отказ по профилю, получено {:?}", other.err()),
        }
    }

    #[test]
    fn truncation_is_detected() {
        // Тот, кто держит файл, не знает пароля — но мог бы отрезать хвост.
        // Отсутствие кадра-конца обязано это выдать.
        let buf = write_sample("пароль");
        let cut = &buf[..buf.len() - 40];
        let mut r = Reader::open(cut, "пароль").unwrap();
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
        // Кадры аутентичны по отдельности; их порядок держится только на AAD.
        let buf = write_sample("пароль");
        let head = 8 + crate::kdf::SALT_LEN;
        // Длина первого кадра → его границы; меняем местами с первым... проще:
        // подменяем номер кадра, читая второй кадр как первый.
        let len1 = u32::from_le_bytes(buf[head..head + 4].try_into().unwrap()) as usize;
        let rest = &buf[head + 4 + NONCE_LEN + len1..];
        let mut forged = buf[..head].to_vec();
        forged.extend_from_slice(rest);
        let mut r = Reader::open(&forged[..], "пароль").unwrap();
        assert!(matches!(r.next(), Err(BackupError::Decrypt)));
    }
}
