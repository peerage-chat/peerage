//! Машина состояний Noise для сессии 1:1 v2 (ADR-012).
//!
//! Реализован ровно тот кусок спецификации Noise (rev. 34), что нам нужен: DH
//! 25519, шифр ChaChaPoly, хэш BLAKE2s, шаблоны `XX` и `XXhfs` — XX с гибридными
//! токенами `e1`/`ekem1` (KEM-расширение Noise «HFS»), где KEM — ML-KEM-768.
//! Общий код для обоих шаблонов сделан нарочно: классический XX сверяется со
//! `snow` байт-в-байт (тест ниже), и гибридный вариант идёт по той же проверенной
//! дорожке, отличаясь только двумя токенами.
//!
//! Что здесь НЕТ: psk, fallback, одноразовые данные в первом сообщении, свои
//! шаблоны. Подписи статических ключей, `mac1` и сессии поверх — в
//! [`crate::session`].

use blake2::{Blake2s256, Digest};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::ChaCha20Poly1305;
use hmac::{Mac, SimpleHmac};
use libcrux_ml_kem::mlkem768::{self, MlKem768Ciphertext, MlKem768PrivateKey, MlKem768PublicKey};
use rand_core::{CryptoRng, RngCore};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

pub const HASHLEN: usize = 32;
const DHLEN: usize = 32;
const TAGLEN: usize = 16;
/// Размер ключа инкапсуляции ML-KEM-768.
pub const KEM_PK_LEN: usize = 1184;
/// Размер шифртекста ML-KEM-768.
pub const KEM_CT_LEN: usize = 1088;

#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum NoiseError {
    #[error("сообщение рукопожатия короче положенного")]
    Truncated,
    #[error("расшифровка или проверка подлинности не удалась")]
    Decrypt,
    #[error("ход рукопожатия не наш или оно уже закончено")]
    OutOfTurn,
    #[error("ключ собеседника непригоден (нулевой DH или битый ключ KEM)")]
    BadKey,
}

/// Токены шаблонов Noise (без psk).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    E,
    S,
    EE,
    ES,
    SE,
    SS,
    /// Эфемерный ключ KEM (HFS).
    E1,
    /// Инкапсуляция к `e1` собеседника; секрет KEM уходит в MixKey (HFS).
    Ekem1,
}

/// Шаблон рукопожатия: имя протокола и токены сообщений по очереди.
pub struct Pattern {
    pub name: &'static str,
    pub messages: &'static [&'static [Token]],
}

use Token::*;

/// Классический XX — только как эталон для сверки со `snow`.
pub const XX: Pattern = Pattern {
    name: "Noise_XX_25519_ChaChaPoly_BLAKE2s",
    messages: &[&[E], &[E, EE, S, ES], &[S, SE]],
};

/// Рабочий шаблон сессии 1:1 v2 (ADR-012).
pub const XX_HFS: Pattern = Pattern {
    name: "Noise_XXhfs_25519+MLKEM768_ChaChaPoly_BLAKE2s",
    messages: &[&[E, E1], &[E, EE, Ekem1, S, ES], &[S, SE]],
};

fn hash(parts: &[&[u8]]) -> [u8; HASHLEN] {
    let mut h = Blake2s256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; HASHLEN] {
    // HMAC-BLAKE2s, как требует Noise (не keyed-режим самого BLAKE2).
    let mut m = <SimpleHmac<Blake2s256> as Mac>::new_from_slice(key).expect("HMAC принимает ключ любой длины");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

/// HKDF из спецификации Noise с двумя выходами.
fn hkdf2(ck: &[u8; HASHLEN], ikm: &[u8]) -> (Zeroizing<[u8; HASHLEN]>, Zeroizing<[u8; HASHLEN]>) {
    let temp = Zeroizing::new(hmac(ck, &[ikm]));
    let o1 = Zeroizing::new(hmac(&*temp, &[&[1]]));
    let o2 = Zeroizing::new(hmac(&*temp, &[&*o1, &[2]]));
    (o1, o2)
}

/// ChaCha20-Poly1305 с nonce Noise: 4 нулевых байта и счётчик little-endian.
fn nonce(n: u64) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[4..].copy_from_slice(&n.to_le_bytes());
    out
}

/// Зашифровать на ключе `k` с явным номером `n` (транспорт сессии и рукопожатие).
pub fn aead_seal(k: &[u8; 32], n: u64, ad: &[u8], pt: &[u8]) -> Vec<u8> {
    ChaCha20Poly1305::new(k.into())
        .encrypt(&nonce(n).into(), Payload { msg: pt, aad: ad })
        .expect("ChaCha20-Poly1305 не отказывает в шифровании")
}

/// Расшифровать на ключе `k` с явным номером `n`.
pub fn aead_open(k: &[u8; 32], n: u64, ad: &[u8], ct: &[u8]) -> Result<Vec<u8>, NoiseError> {
    ChaCha20Poly1305::new(k.into())
        .decrypt(&nonce(n).into(), Payload { msg: ct, aad: ad })
        .map_err(|_| NoiseError::Decrypt)
}

#[derive(Clone)]
struct CipherState {
    k: Option<Zeroizing<[u8; 32]>>,
    n: u64,
}

impl CipherState {
    fn encrypt(&mut self, ad: &[u8], pt: &[u8]) -> Vec<u8> {
        match &self.k {
            None => pt.to_vec(),
            Some(k) => {
                let ct = aead_seal(k, self.n, ad, pt);
                self.n += 1;
                ct
            }
        }
    }

    fn decrypt(&mut self, ad: &[u8], ct: &[u8]) -> Result<Vec<u8>, NoiseError> {
        match &self.k {
            None => Ok(ct.to_vec()),
            Some(k) => {
                let pt = aead_open(k, self.n, ad, ct)?;
                self.n += 1;
                Ok(pt)
            }
        }
    }
}

#[derive(Clone)]
struct SymmetricState {
    ck: Zeroizing<[u8; HASHLEN]>,
    h: [u8; HASHLEN],
    cs: CipherState,
}

impl SymmetricState {
    fn new(protocol_name: &str) -> Self {
        let name = protocol_name.as_bytes();
        let h = if name.len() <= HASHLEN {
            let mut h = [0u8; HASHLEN];
            h[..name.len()].copy_from_slice(name);
            h
        } else {
            hash(&[name])
        };
        SymmetricState { ck: Zeroizing::new(h), h, cs: CipherState { k: None, n: 0 } }
    }

    fn mix_hash(&mut self, data: &[u8]) {
        self.h = hash(&[&self.h, data]);
    }

    fn mix_key(&mut self, ikm: &[u8]) {
        let (ck, k) = hkdf2(&self.ck, ikm);
        self.ck = ck;
        self.cs = CipherState { k: Some(k), n: 0 };
    }

    fn has_key(&self) -> bool {
        self.cs.k.is_some()
    }

    fn encrypt_and_hash(&mut self, pt: &[u8]) -> Vec<u8> {
        let ct = self.cs.encrypt(&self.h, pt);
        self.mix_hash(&ct);
        ct
    }

    fn decrypt_and_hash(&mut self, ct: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let pt = self.cs.decrypt(&self.h, ct)?;
        self.mix_hash(ct);
        Ok(pt)
    }
}

/// Ключи завершённого рукопожатия.
pub struct Split {
    /// Ключ для того, что МЫ шлём.
    pub send: Zeroizing<[u8; 32]>,
    /// Ключ для того, что шлют НАМ.
    pub recv: Zeroizing<[u8; 32]>,
    /// Хэш рукопожатия — привязка к этой сессии (экспорт ключей, напр. медиа).
    pub hash: [u8; HASHLEN],
    /// Статический ключ собеседника (X25519), подтверждённый рукопожатием.
    pub remote_static: [u8; 32],
}

/// Одна сторона рукопожатия по шаблону `pattern`. `Clone` — чтобы читать чужое
/// сообщение на копии: неудачное чтение портит состояние.
#[derive(Clone)]
pub struct HandshakeState {
    pattern: &'static Pattern,
    initiator: bool,
    sym: SymmetricState,
    s: StaticSecret,
    e: Option<StaticSecret>,
    re: Option<PublicKey>,
    rs: Option<PublicKey>,
    /// Секрет своего эфемерного ключа KEM (`e1`) — до прихода `ekem1`.
    kem: Option<MlKem768PrivateKey>,
    re1: Option<MlKem768PublicKey>,
    /// Номер следующего сообщения шаблона.
    next: usize,
}

impl HandshakeState {
    /// `s` — свой статический ключ X25519; `prologue` обязан совпасть у сторон.
    pub fn new(pattern: &'static Pattern, initiator: bool, prologue: &[u8], s: StaticSecret) -> Self {
        let mut sym = SymmetricState::new(pattern.name);
        sym.mix_hash(prologue);
        HandshakeState { pattern, initiator, sym, s, e: None, re: None, rs: None, kem: None, re1: None, next: 0 }
    }

    fn my_turn(&self) -> bool {
        // Чётные сообщения пишет инициатор.
        (self.next % 2 == 0) == self.initiator
    }

    /// Статический ключ собеседника, как только он пришёл (до конца рукопожатия).
    pub fn remote_static(&self) -> Option<[u8; 32]> {
        self.rs.map(|k| k.to_bytes())
    }

    pub fn is_finished(&self) -> bool {
        self.next == self.pattern.messages.len()
    }

    fn dh(&self, mine: &StaticSecret, theirs: &PublicKey) -> Result<Zeroizing<[u8; DHLEN]>, NoiseError> {
        let shared = mine.diffie_hellman(theirs);
        // Нулевой (не-вкладной) результат — собеседник подсунул точку малого порядка.
        if !shared.was_contributory() {
            return Err(NoiseError::BadKey);
        }
        Ok(Zeroizing::new(shared.to_bytes()))
    }

    fn mix_dh(&mut self, t: Token) -> Result<(), NoiseError> {
        // Ключа ещё нет — значит, токен пришёл не в свой ход.
        let missing = NoiseError::OutOfTurn;
        let (mine, theirs) = match (t, self.initiator) {
            (EE, _) => (self.e.as_ref().ok_or(missing)?, self.re.as_ref().ok_or(missing)?),
            (ES, true) | (SE, false) => (self.e.as_ref().ok_or(missing)?, self.rs.as_ref().ok_or(missing)?),
            (ES, false) | (SE, true) => (&self.s, self.re.as_ref().ok_or(missing)?),
            (SS, _) => (&self.s, self.rs.as_ref().ok_or(missing)?),
            _ => unreachable!("mix_dh зовётся только для DH-токенов"),
        };
        let shared = self.dh(mine, theirs)?;
        self.sym.mix_key(&*shared);
        Ok(())
    }

    /// Написать очередное сообщение рукопожатия с полезной нагрузкой `payload`.
    pub fn write_message<R: RngCore + CryptoRng>(&mut self, payload: &[u8], rng: &mut R) -> Result<Vec<u8>, NoiseError> {
        if self.is_finished() || !self.my_turn() {
            return Err(NoiseError::OutOfTurn);
        }
        let mut out = Vec::new();
        for &t in self.pattern.messages[self.next] {
            match t {
                E => {
                    let mut sk = Zeroizing::new([0u8; 32]);
                    rng.fill_bytes(&mut *sk);
                    let e = StaticSecret::from(*sk);
                    let pubk = PublicKey::from(&e);
                    out.extend_from_slice(pubk.as_bytes());
                    self.sym.mix_hash(pubk.as_bytes());
                    self.e = Some(e);
                }
                S => {
                    let ct = self.sym.encrypt_and_hash(PublicKey::from(&self.s).as_bytes());
                    out.extend_from_slice(&ct);
                }
                E1 => {
                    let mut seed = Zeroizing::new([0u8; 64]);
                    rng.fill_bytes(&mut *seed);
                    let kp = mlkem768::generate_key_pair(*seed);
                    let ct = self.sym.encrypt_and_hash(kp.public_key().as_slice());
                    out.extend_from_slice(&ct);
                    self.kem = Some(kp.private_key().clone());
                }
                Ekem1 => {
                    let re1 = self.re1.as_ref().ok_or(NoiseError::OutOfTurn)?;
                    let mut seed = Zeroizing::new([0u8; 32]);
                    rng.fill_bytes(&mut *seed);
                    let (ct, k) = mlkem768::encapsulate(re1, *seed);
                    let k = Zeroizing::new(k);
                    let enc = self.sym.encrypt_and_hash(ct.as_slice());
                    out.extend_from_slice(&enc);
                    self.sym.mix_key(&*k);
                }
                EE | ES | SE | SS => self.mix_dh(t)?,
            }
        }
        out.extend_from_slice(&self.sym.encrypt_and_hash(payload));
        self.next += 1;
        Ok(out)
    }

    /// Прочитать очередное сообщение собеседника; вернуть его полезную нагрузку.
    /// При ошибке состояние испорчено — рукопожатие надо начинать заново.
    pub fn read_message(&mut self, msg: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if self.is_finished() || self.my_turn() {
            return Err(NoiseError::OutOfTurn);
        }
        let mut rest = msg;
        let mut take = |n: usize| -> Result<&[u8], NoiseError> {
            if rest.len() < n {
                return Err(NoiseError::Truncated);
            }
            let (a, b) = rest.split_at(n);
            rest = b;
            Ok(a)
        };
        for &t in self.pattern.messages[self.next] {
            let tag = if self.sym.has_key() { TAGLEN } else { 0 };
            match t {
                E => {
                    let b: [u8; 32] = take(DHLEN)?.try_into().map_err(|_| NoiseError::Truncated)?;
                    self.sym.mix_hash(&b);
                    self.re = Some(PublicKey::from(b));
                }
                S => {
                    let ct = take(DHLEN + tag)?;
                    let pt = self.sym.decrypt_and_hash(ct)?;
                    let b: [u8; 32] = pt.as_slice().try_into().map_err(|_| NoiseError::Truncated)?;
                    self.rs = Some(PublicKey::from(b));
                }
                E1 => {
                    let ct = take(KEM_PK_LEN + tag)?;
                    let pt = self.sym.decrypt_and_hash(ct)?;
                    let b: [u8; KEM_PK_LEN] = pt.as_slice().try_into().map_err(|_| NoiseError::Truncated)?;
                    let pk = MlKem768PublicKey::from(b);
                    // Ключ инкапсуляции от чужого — проверить форму до использования (FIPS 203).
                    if !mlkem768::validate_public_key(&pk) {
                        return Err(NoiseError::BadKey);
                    }
                    self.re1 = Some(pk);
                }
                Ekem1 => {
                    let ct = take(KEM_CT_LEN + tag)?;
                    let pt = self.sym.decrypt_and_hash(ct)?;
                    let b: [u8; KEM_CT_LEN] = pt.as_slice().try_into().map_err(|_| NoiseError::Truncated)?;
                    let sk = self.kem.as_ref().ok_or(NoiseError::OutOfTurn)?;
                    let k = Zeroizing::new(mlkem768::decapsulate(sk, &MlKem768Ciphertext::from(b)));
                    self.sym.mix_key(&*k);
                }
                EE | ES | SE | SS => self.mix_dh(t)?,
            }
        }
        let payload = self.sym.decrypt_and_hash(rest)?;
        self.next += 1;
        Ok(payload)
    }

    /// Ключи транспорта после последнего сообщения шаблона.
    pub fn split(self) -> Result<Split, NoiseError> {
        if !self.is_finished() {
            return Err(NoiseError::OutOfTurn);
        }
        let remote_static = *self.rs.ok_or(NoiseError::OutOfTurn)?.as_bytes();
        let (k1, k2) = hkdf2(&self.sym.ck, &[]);
        let (send, recv) = if self.initiator { (k1, k2) } else { (k2, k1) };
        Ok(Split { send, recv, hash: self.sym.h, remote_static })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// «Случайность», выдающая заранее заданные байты: так эфемерные ключи у нас и
    /// у `snow` совпадают, и сообщения можно сравнить побайтно.
    struct Fixed(Vec<u8>);
    impl RngCore for Fixed {
        fn next_u32(&mut self) -> u32 {
            unimplemented!()
        }
        fn next_u64(&mut self) -> u64 {
            unimplemented!()
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            let n = dest.len();
            dest.copy_from_slice(&self.0[..n]);
            self.0.drain(..n);
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }
    impl CryptoRng for Fixed {}

    const PROLOGUE: &[u8] = b"interlace test prologue";

    /// Классическая часть — байт-в-байт со `snow`: те же статические и эфемерные
    /// ключи, тот же пролог и полезные нагрузки дают те же три сообщения и те же
    /// ключи транспорта.
    #[test]
    fn classic_xx_matches_snow_byte_for_byte() {
        let (si, sr, ei, er) = ([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]);
        let params: snow::params::NoiseParams = XX.name.parse().unwrap();
        let mut a = snow::Builder::new(params.clone())
            .local_private_key(&si)
            .unwrap()
            .fixed_ephemeral_key_for_testing_only(&ei)
            .prologue(PROLOGUE)
            .unwrap()
            .build_initiator()
            .unwrap();
        let mut b = snow::Builder::new(params)
            .local_private_key(&sr)
            .unwrap()
            .fixed_ephemeral_key_for_testing_only(&er)
            .prologue(PROLOGUE)
            .unwrap()
            .build_responder()
            .unwrap();

        let mut mi = HandshakeState::new(&XX, true, PROLOGUE, StaticSecret::from(si));
        let mut mr = HandshakeState::new(&XX, false, PROLOGUE, StaticSecret::from(sr));
        let mut buf = [0u8; 1024];
        let mut tmp = [0u8; 1024];

        let ours = mi.write_message(b"one", &mut Fixed(ei.to_vec())).unwrap();
        let n = a.write_message(b"one", &mut buf).unwrap();
        assert_eq!(ours, buf[..n], "сообщение 1");
        b.read_message(&ours, &mut tmp).unwrap();
        assert_eq!(mr.read_message(&ours).unwrap(), b"one");

        let ours = mr.write_message(b"two", &mut Fixed(er.to_vec())).unwrap();
        let n = b.write_message(b"two", &mut buf).unwrap();
        assert_eq!(ours, buf[..n], "сообщение 2");
        a.read_message(&ours, &mut tmp).unwrap();
        assert_eq!(mi.read_message(&ours).unwrap(), b"two");

        let ours = mi.write_message(b"three", &mut Fixed(Vec::new())).unwrap();
        let n = a.write_message(b"three", &mut buf).unwrap();
        assert_eq!(ours, buf[..n], "сообщение 3");
        b.read_message(&ours, &mut tmp).unwrap();
        assert_eq!(mr.read_message(&ours).unwrap(), b"three");

        assert_eq!(mi.sym.h.as_slice(), a.get_handshake_hash(), "хэш рукопожатия");
        let (ki, kr) = (mi.split().unwrap(), mr.split().unwrap());
        let mut a = a.into_transport_mode().unwrap();
        let n = a.write_message(b"data", &mut buf).unwrap();
        assert_eq!(aead_seal(&ki.send, 0, &[], b"data"), buf[..n], "транспорт инициатора");
        assert_eq!(aead_open(&kr.recv, 0, &[], &buf[..n]).unwrap(), b"data");
        assert_eq!(ki.remote_static, *PublicKey::from(&StaticSecret::from(sr)).as_bytes());
    }

    fn hfs_pair() -> (HandshakeState, HandshakeState) {
        (
            HandshakeState::new(&XX_HFS, true, PROLOGUE, StaticSecret::from([5u8; 32])),
            HandshakeState::new(&XX_HFS, false, PROLOGUE, StaticSecret::from([6u8; 32])),
        )
    }

    #[test]
    fn hybrid_handshake_agrees_on_keys() {
        let mut rng = rand_core::OsRng;
        let (mut i, mut r) = hfs_pair();
        let m1 = i.write_message(&[], &mut rng).unwrap();
        assert_eq!(m1.len(), DHLEN + KEM_PK_LEN, "e + e1 в открытую, без полезной нагрузки");
        r.read_message(&m1).unwrap();
        let m2 = r.write_message(b"resp", &mut rng).unwrap();
        assert_eq!(i.read_message(&m2).unwrap(), b"resp");
        let m3 = i.write_message(b"init", &mut rng).unwrap();
        assert_eq!(r.read_message(&m3).unwrap(), b"init");
        let (ki, kr) = (i.split().unwrap(), r.split().unwrap());
        assert_eq!(*ki.send, *kr.recv);
        assert_eq!(*ki.recv, *kr.send);
        assert_ne!(*ki.send, *ki.recv);
        assert_eq!(ki.hash, kr.hash);
    }

    /// Секрет KEM действительно в ключах транспорта, а не только в хэше: при тех
    /// же ключах X25519 другая случайность инкапсуляции даёт другие ключи. Иначе
    /// гибрид был бы декорацией (шифртекст KEM сам по себе попадает лишь в `h`).
    #[test]
    fn kem_secret_is_mixed_into_keys() {
        let run = |encap_seed: u8| {
            let (mut i, mut r) = hfs_pair();
            let m1 = i.write_message(&[], &mut Fixed([[7u8; 32].as_slice(), &[8u8; 64]].concat())).unwrap();
            r.read_message(&m1).unwrap();
            let m2 = r.write_message(&[], &mut Fixed([[9u8; 32], [encap_seed; 32]].concat())).unwrap();
            i.read_message(&m2).unwrap();
            let m3 = i.write_message(&[], &mut Fixed(Vec::new())).unwrap();
            r.read_message(&m3).unwrap();
            *i.split().unwrap().send
        };
        assert_eq!(run(1), run(1), "детерминированный прогон воспроизводим");
        assert_ne!(run(1), run(2), "секрет KEM не попал в ключи");
    }

    /// Разный пролог — разные стороны разговора: рукопожатие не сходится.
    #[test]
    fn prologue_mismatch_fails() {
        let mut rng = rand_core::OsRng;
        let mut i = HandshakeState::new(&XX_HFS, true, b"a", StaticSecret::from([5u8; 32]));
        let mut r = HandshakeState::new(&XX_HFS, false, b"b", StaticSecret::from([6u8; 32]));
        // e1 шифруется ещё без ключа, так что первое сообщение читается…
        r.read_message(&i.write_message(&[], &mut rng).unwrap()).unwrap();
        // …а второе уже нет: в его AEAD входит h с прологом.
        let m2 = r.write_message(&[], &mut rng).unwrap();
        assert_eq!(i.read_message(&m2).err(), Some(NoiseError::Decrypt));
    }

    #[test]
    fn tampering_and_wrong_turn_are_rejected() {
        let mut rng = rand_core::OsRng;
        let (mut i, mut r) = hfs_pair();
        assert_eq!(r.write_message(&[], &mut rng).err(), Some(NoiseError::OutOfTurn));
        let m1 = i.write_message(&[], &mut rng).unwrap();
        assert_eq!(r.read_message(&m1[..100]).err(), Some(NoiseError::Truncated));
        let (_, mut r) = hfs_pair();
        r.read_message(&m1).unwrap();
        let mut m2 = r.write_message(b"x", &mut rng).unwrap();
        let last = m2.len() - 1;
        m2[last] ^= 1;
        assert_eq!(i.read_message(&m2).err(), Some(NoiseError::Decrypt));
    }

    /// Нулевой эфемерный ключ (точка малого порядка) не даёт «общего секрета».
    #[test]
    fn low_order_ephemeral_is_rejected() {
        let mut rng = rand_core::OsRng;
        let (mut i, mut r) = hfs_pair();
        let mut m1 = i.write_message(&[], &mut rng).unwrap();
        m1[..32].fill(0);
        r.read_message(&m1).unwrap();
        assert_eq!(r.write_message(&[], &mut rng).err(), Some(NoiseError::BadKey));
    }
}
