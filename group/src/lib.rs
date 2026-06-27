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

use std::path::Path;

use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{MemoryStorage, OpenMlsRustCrypto};
use p2p_core::Identity;
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
    #[error("идентичность KeyPackage не совпадает с ожидаемой (попытка подмены)")]
    IdentityMismatch,
}

fn mls<E: std::fmt::Display>(e: E) -> GroupError {
    GroupError::Mls(e.to_string())
}

/// Результат обработки входящего группового сообщения.
pub enum Incoming {
    /// Прикладное сообщение: отправитель (identity = ed25519-pubkey) + расшифрованные байты.
    Message { sender: Vec<u8>, body: Vec<u8> },
    /// Применено изменение состава группы (Commit). actor — кто его применил.
    MembershipChanged { actor: Vec<u8> },
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
    /// Создать участника. MLS-ключ подписи = наша идентичность ed25519
    /// (детерминированно из seed), credential = публичный ключ. Один и тот же
    /// идентификатор, что в контактах/звонках.
    pub fn new(identity: &Identity) -> Result<Self, GroupError> {
        let provider = OpenMlsRustCrypto::default();
        let signer = Self::signer_from(identity);
        signer.store(provider.storage()).map_err(mls)?;
        let credential = Self::credential(identity, &signer);
        Ok(Self { provider, signer, credential, group: None })
    }

    fn signer_from(identity: &Identity) -> SignatureKeyPair {
        SignatureKeyPair::from_raw(
            SignatureScheme::ED25519,
            identity.seed().to_vec(),
            identity.public_key_bytes().to_vec(),
        )
    }
    fn credential(identity: &Identity, signer: &SignatureKeyPair) -> CredentialWithKey {
        CredentialWithKey {
            credential: BasicCredential::new(identity.public_key_bytes().to_vec()).into(),
            signature_key: signer.public().into(),
        }
    }

    /// Сохранить состояние группы на диск (MLS-хранилище + group_id).
    pub fn save(&self, path: &Path) -> Result<(), GroupError> {
        let group = self.group.as_ref().ok_or(GroupError::NoGroup)?;
        let gid = group.group_id().as_slice().to_vec();
        let mut storage = Vec::new();
        self.provider.storage().serialize(&mut storage).map_err(mls)?;
        let mut out = Vec::with_capacity(4 + gid.len() + storage.len());
        out.extend_from_slice(&(gid.len() as u32).to_be_bytes());
        out.extend_from_slice(&gid);
        out.extend_from_slice(&storage);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(mls)?;
            }
        }
        std::fs::write(path, p2p_core::vault::seal(&out)).map_err(mls)?;
        Ok(())
    }

    /// Загрузить состояние группы с диска.
    pub fn load(path: &Path, identity: &Identity) -> Result<Self, GroupError> {
        let raw = std::fs::read(path).map_err(mls)?;
        let data = p2p_core::vault::open(&raw).map_err(mls)?;
        if data.len() < 4 {
            return Err(GroupError::Mls("повреждённый файл группы".into()));
        }
        let glen = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if 4 + glen > data.len() {
            return Err(GroupError::Mls("повреждённый файл группы".into()));
        }
        let gid = &data[4..4 + glen];
        let storage_bytes = &data[4 + glen..];

        let restored = MemoryStorage::deserialize(&mut &storage_bytes[..]).map_err(mls)?;
        let provider = OpenMlsRustCrypto::default();
        *provider.storage().values.write().unwrap() = restored.values.into_inner().unwrap();

        let signer = Self::signer_from(identity);
        let credential = Self::credential(identity, &signer);
        let group = MlsGroup::load(provider.storage(), &GroupId::from_slice(gid))
            .map_err(mls)?
            .ok_or(GroupError::NoGroup)?;
        Ok(Self { provider, signer, credential, group: Some(group) })
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

    /// Добавить участника по его KeyPackage. `expected_identity` — публичный ключ
    /// пира, которого мы РЕАЛЬНО приглашали (аутентифицированный транспортом).
    /// Возвращает (commit, welcome): commit — существующим, welcome — новому.
    ///
    /// H1: привязываем KeyPackage к аутентифицированной идентичности — иначе пир
    /// мог бы вступить под чужим публичным ключом (подмена в группе).
    pub fn add(
        &mut self,
        key_package: &[u8],
        expected_identity: &[u8; 32],
    ) -> Result<(Vec<u8>, Vec<u8>), GroupError> {
        let kp_in = KeyPackageIn::tls_deserialize_exact(key_package).map_err(mls)?;
        let kp = kp_in
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(mls)?;
        // Идентичность credential, ключ подписи и ожидаемый пир обязаны совпадать.
        let leaf = kp.leaf_node();
        let identity = leaf.credential().serialized_content();
        let sig_key = leaf.signature_key().as_slice();
        if identity != expected_identity || sig_key != expected_identity {
            return Err(GroupError::IdentityMismatch);
        }
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
        // Личность отправителя (= ed25519-pubkey, см. member_identities) доступна
        // до распаковки содержимого — снимаем до into_content(), который её поглощает.
        let sender = processed.credential().serialized_content().to_vec();
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(app) => {
                Ok(Incoming::Message { sender, body: app.into_bytes() })
            }
            ProcessedMessageContent::StagedCommitMessage(staged) => {
                group.merge_staged_commit(&self.provider, *staged).map_err(mls)?;
                Ok(Incoming::MembershipChanged { actor: sender })
            }
            _ => Ok(Incoming::Other),
        }
    }

    /// Число участников группы.
    pub fn member_count(&self) -> usize {
        self.group.as_ref().map(|g| g.members().count()).unwrap_or(0)
    }

    /// Идентичности всех участников (publicKey ed25519 = credential identity).
    pub fn member_identities(&self) -> Vec<Vec<u8>> {
        self.group
            .as_ref()
            .map(|g| {
                g.members()
                    .map(|m| m.credential.serialized_content().to_vec())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// В группе ли мы (создана/присоединена).
    pub fn is_joined(&self) -> bool {
        self.group.is_some()
    }

    /// Идентификатор группы (если в группе).
    pub fn group_id(&self) -> Option<Vec<u8>> {
        self.group.as_ref().map(|g| g.group_id().as_slice().to_vec())
    }
}

/// Узнать group_id входящего MLS-сообщения (для маршрутизации между группами).
/// Возвращает None для Welcome и нечитаемых сообщений.
pub fn peek_group_id(message: &[u8]) -> Option<Vec<u8>> {
    let msg = MlsMessageIn::tls_deserialize_exact(message).ok()?;
    let protocol = msg.try_into_protocol_message().ok()?;
    Some(protocol.group_id().as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> Identity {
        Identity::from_seed(&[n; 32]).unwrap()
    }

    #[test]
    fn two_members_exchange_encrypted_messages() {
        let mut alice = Member::new(&id(1)).unwrap();
        let mut bob = Member::new(&id(2)).unwrap();

        alice.create_group().unwrap();
        let bob_kp = bob.key_package().unwrap();
        let (_commit, welcome) = alice.add(&bob_kp, &id(2).public_key_bytes()).unwrap();
        bob.join(&welcome).unwrap();

        assert_eq!(alice.member_count(), 2);
        assert_eq!(bob.member_count(), 2);

        // Alice → группа; Bob расшифровывает.
        let ct = alice.encrypt(b"hi group").unwrap();
        match bob.process(&ct).unwrap() {
            Incoming::Message { sender, body } => {
                assert_eq!(body, b"hi group");
                assert_eq!(sender, id(1).public_key_bytes()); // отправитель = Alice
            }
            _ => panic!("ожидалось прикладное сообщение"),
        }

        // Bob → группа; Alice расшифровывает.
        let ct = bob.encrypt(b"hello back").unwrap();
        match alice.process(&ct).unwrap() {
            Incoming::Message { sender, body } => {
                assert_eq!(body, b"hello back");
                assert_eq!(sender, id(2).public_key_bytes()); // отправитель = Bob
            }
            _ => panic!("ожидалось прикладное сообщение"),
        }
    }

    #[test]
    fn third_member_added_via_commit() {
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        let mut c = Member::new(&id(3)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();

        // A добавляет C: commit идёт B, welcome — C.
        let (commit, welcome) = a.add(&c.key_package().unwrap(), &id(3).public_key_bytes()).unwrap();
        b.process(&commit).unwrap(); // B применяет изменение состава
        c.join(&welcome).unwrap();

        assert_eq!(a.member_count(), 3);
        assert_eq!(b.member_count(), 3);
        assert_eq!(c.member_count(), 3);

        // Сообщение от C видят и A, и B.
        let ct = c.encrypt(b"hey all").unwrap();
        assert!(matches!(a.process(&ct).unwrap(), Incoming::Message { body, .. } if body == b"hey all"));
        assert!(matches!(b.process(&ct).unwrap(), Incoming::Message { body, .. } if body == b"hey all"));
    }

    #[test]
    fn add_rejects_identity_mismatch() {
        // H1: KeyPackage с идентичностью, не равной ожидаемому пиру, отвергается.
        let mut a = Member::new(&id(1)).unwrap();
        a.create_group().unwrap();
        let b = Member::new(&id(2)).unwrap();
        let kp = b.key_package().unwrap();
        // Ожидаем «не того» (id(3)) — должно быть отклонено.
        assert!(matches!(
            a.add(&kp, &id(3).public_key_bytes()),
            Err(GroupError::IdentityMismatch)
        ));
        // С правильной идентичностью — ок.
        assert!(a.add(&kp, &id(2).public_key_bytes()).is_ok());
    }

    #[test]
    fn group_state_survives_save_load() {
        let dir = std::env::temp_dir().join(format!("p2p-grp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.group");

        let a_id = id(1);
        let mut a = Member::new(&a_id).unwrap();
        a.create_group().unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        let (_c, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();

        a.save(&path).unwrap();
        drop(a);

        // Перезагрузка с диска — группа жива.
        let mut a2 = Member::load(&path, &a_id).unwrap();
        assert_eq!(a2.member_count(), 2);
        let ct = a2.encrypt(b"after reload").unwrap();
        assert!(matches!(b.process(&ct).unwrap(), Incoming::Message { body, .. } if body == b"after reload"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
