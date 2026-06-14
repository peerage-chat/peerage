//! p2p-group — групповое E2E-шифрование на MLS (RFC 9420) через openmls.
//!
//! Один участник = [`Member`]: держит MLS-провайдер, ключ подписи и состояние
//! группы. Идентичность участника (credential) — наш публичный ключ ed25519.
//!
//! Что даёт MLS: общий групповой ключ (одна операция шифрования на всех),
//! Forward Secrecy и Post-Compromise Security при сменах состава (Commit).
//!
//! Здесь — только крипто-слой и формат сообщений. Распространение по сети и
//! УПОРЯДОЧИВАНИЕ Commit (при одновременных изменениях состава) — отдельная
//! задача (нужен delivery service, напр. ntfy из Фазы D).

use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use tls_codec::{Deserialize as _, Serialize as _};

/// Шифрсьют MLS на X25519/AES128-GCM/SHA256/Ed25519.
const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

#[derive(Debug, thiserror::Error)]
pub enum GroupError {
    #[error("mls: {0}")]
    Mls(String),
    #[error("группа ещё не создана/не присоединена")]
    NoGroup,
    #[error("ожидалось welcome-сообщение")]
    NotWelcome,
}

fn mls<E: std::fmt::Display>(e: E) -> GroupError {
    GroupError::Mls(e.to_string())
}

/// Результат обработки входящего группового сообщения.
pub enum Incoming {
    /// Прикладное сообщение (расшифрованный текст/данные).
    Message(Vec<u8>),
    /// Применено изменение состава группы (Commit) — данных нет.
    MembershipChanged,
    /// Прочее (предложение и т.п.) — игнорируем на уровне приложения.
    Other,
}

/// Участник MLS-группы.
pub struct Member {
    provider: OpenMlsRustCrypto,
    signer: SignatureKeyPair,
    credential: CredentialWithKey,
    group: Option<MlsGroup>,
}

impl Member {
    /// Создать участника с идентичностью = публичный ключ ed25519 (32 байта).
    pub fn new(identity: &[u8; 32]) -> Result<Self, GroupError> {
        let provider = OpenMlsRustCrypto::default();
        let signer = SignatureKeyPair::new(SignatureScheme::ED25519).map_err(mls)?;
        signer.store(provider.storage()).map_err(mls)?;
        let credential = CredentialWithKey {
            credential: BasicCredential::new(identity.to_vec()).into(),
            signature_key: signer.public().into(),
        };
        Ok(Self { provider, signer, credential, group: None })
    }

    /// Сериализованный KeyPackage — отдаётся тому, кто будет нас добавлять.
    pub fn key_package(&self) -> Result<Vec<u8>, GroupError> {
        let bundle = KeyPackage::builder()
            .build(CIPHERSUITE, &self.provider, &self.signer, self.credential.clone())
            .map_err(mls)?;
        bundle.key_package().tls_serialize_detached().map_err(mls)
    }

    /// Создать новую группу (мы — единственный участник).
    pub fn create_group(&mut self) -> Result<(), GroupError> {
        let cfg = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .build();
        let group = MlsGroup::new(&self.provider, &self.signer, &cfg, self.credential.clone())
            .map_err(mls)?;
        self.group = Some(group);
        Ok(())
    }

    /// Добавить участника по его KeyPackage. Возвращает (commit, welcome) —
    /// commit рассылается существующим участникам, welcome — новому.
    pub fn add(&mut self, key_package: &[u8]) -> Result<(Vec<u8>, Vec<u8>), GroupError> {
        let kp_in = KeyPackageIn::tls_deserialize_exact(key_package).map_err(mls)?;
        let kp = kp_in
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(mls)?;
        let group = self.group.as_mut().ok_or(GroupError::NoGroup)?;
        let (commit, welcome, _info) = group
            .add_members(&self.provider, &self.signer, &[kp])
            .map_err(mls)?;
        group.merge_pending_commit(&self.provider).map_err(mls)?;
        Ok((
            commit.tls_serialize_detached().map_err(mls)?,
            welcome.tls_serialize_detached().map_err(mls)?,
        ))
    }

    /// Присоединиться к группе по welcome-сообщению.
    pub fn join(&mut self, welcome: &[u8]) -> Result<(), GroupError> {
        let msg = MlsMessageIn::tls_deserialize_exact(welcome).map_err(mls)?;
        let welcome = match msg.extract() {
            MlsMessageBodyIn::Welcome(w) => w,
            _ => return Err(GroupError::NotWelcome),
        };
        let cfg = MlsGroupJoinConfig::builder()
            .use_ratchet_tree_extension(true)
            .build();
        // ratchet_tree = None: дерево приходит в self-extension welcome-сообщения.
        let staged = StagedWelcome::new_from_welcome(&self.provider, &cfg, welcome, None).map_err(mls)?;
        let group = staged.into_group(&self.provider).map_err(mls)?;
        self.group = Some(group);
        Ok(())
    }

    /// Зашифровать прикладное сообщение для группы.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, GroupError> {
        let group = self.group.as_mut().ok_or(GroupError::NoGroup)?;
        let out = group
            .create_message(&self.provider, &self.signer, plaintext)
            .map_err(mls)?;
        out.tls_serialize_detached().map_err(mls)
    }

    /// Обработать входящее групповое сообщение (прикладное или Commit).
    pub fn process(&mut self, message: &[u8]) -> Result<Incoming, GroupError> {
        let msg = MlsMessageIn::tls_deserialize_exact(message).map_err(mls)?;
        let protocol = msg.try_into_protocol_message().map_err(mls)?;
        let group = self.group.as_mut().ok_or(GroupError::NoGroup)?;
        let processed = group.process_message(&self.provider, protocol).map_err(mls)?;
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(app) => {
                Ok(Incoming::Message(app.into_bytes()))
            }
            ProcessedMessageContent::StagedCommitMessage(staged) => {
                group.merge_staged_commit(&self.provider, *staged).map_err(mls)?;
                Ok(Incoming::MembershipChanged)
            }
            _ => Ok(Incoming::Other),
        }
    }

    /// Число участников группы.
    pub fn member_count(&self) -> usize {
        self.group.as_ref().map(|g| g.members().count()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_members_exchange_encrypted_messages() {
        let mut alice = Member::new(&[1u8; 32]).unwrap();
        let mut bob = Member::new(&[2u8; 32]).unwrap();

        alice.create_group().unwrap();
        let bob_kp = bob.key_package().unwrap();
        let (_commit, welcome) = alice.add(&bob_kp).unwrap();
        bob.join(&welcome).unwrap();

        assert_eq!(alice.member_count(), 2);
        assert_eq!(bob.member_count(), 2);

        // Alice → группа; Bob расшифровывает.
        let ct = alice.encrypt(b"hi group").unwrap();
        match bob.process(&ct).unwrap() {
            Incoming::Message(m) => assert_eq!(m, b"hi group"),
            _ => panic!("ожидалось прикладное сообщение"),
        }

        // Bob → группа; Alice расшифровывает.
        let ct = bob.encrypt(b"hello back").unwrap();
        match alice.process(&ct).unwrap() {
            Incoming::Message(m) => assert_eq!(m, b"hello back"),
            _ => panic!("ожидалось прикладное сообщение"),
        }
    }

    #[test]
    fn third_member_added_via_commit() {
        let mut a = Member::new(&[1u8; 32]).unwrap();
        let mut b = Member::new(&[2u8; 32]).unwrap();
        let mut c = Member::new(&[3u8; 32]).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap()).unwrap();
        b.join(&w).unwrap();

        // A добавляет C: commit идёт B, welcome — C.
        let (commit, welcome) = a.add(&c.key_package().unwrap()).unwrap();
        b.process(&commit).unwrap(); // B применяет изменение состава
        c.join(&welcome).unwrap();

        assert_eq!(a.member_count(), 3);
        assert_eq!(b.member_count(), 3);
        assert_eq!(c.member_count(), 3);

        // Сообщение от C видят и A, и B.
        let ct = c.encrypt(b"hey all").unwrap();
        assert!(matches!(a.process(&ct).unwrap(), Incoming::Message(m) if m == b"hey all"));
        assert!(matches!(b.process(&ct).unwrap(), Incoming::Message(m) if m == b"hey all"));
    }
}
