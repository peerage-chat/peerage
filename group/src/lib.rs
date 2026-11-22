//! peerage-group — групповое E2E-шифрование на MLS (RFC 9420) через openmls.
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
use peerage_core::Identity;
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
    #[error("участника нет в группе")]
    NotMember,
    #[error("нельзя отозвать самого себя (используйте leave)")]
    CannotRemoveSelf,
    /// Рассинхрон эпох: пропущен Commit или секрет прошлой эпохи уже удалён (FS).
    /// Верхний слой должен инициировать resync (re-add), а не глотать ошибку.
    #[error("рассинхрон эпох MLS — нужен resync")]
    EpochMismatch,
    /// Коммит меняет состав (или правила группы), а сделал его не админ — не применён.
    #[error("состав группы меняет только админ")]
    NotAdmin,
}

/// Вправе ли автор применить этот коммит. Ротация своего ключа (Update и пустой
/// путь) — любой участник; Add/Remove — только админ; прочие предложения (PSK,
/// ReInit, правила группы…) мы не используем — их не принимаем ни от кого.
fn commit_allowed(staged: &StagedCommit, committer: &[u8], admin: Option<&[u8]>) -> bool {
    let is_admin = admin == Some(committer);
    staged.queued_proposals().all(|q| match q.proposal() {
        Proposal::Update(_) => true,
        Proposal::Add(_) | Proposal::Remove(_) => is_admin,
        _ => false,
    })
}

fn mls<E: std::fmt::Display>(e: E) -> GroupError {
    GroupError::Mls(e.to_string())
}

/// Результат обработки входящего группового сообщения.
pub enum Incoming {
    /// Прикладное сообщение: отправитель (identity = ed25519-pubkey) + расшифрованные байты.
    Message { sender: Vec<u8>, body: Vec<u8> },
    /// Применено изменение состава группы (Commit). actor — кто его применил;
    /// added / removed — кого коммит добавил и кого убрал (ключи из credential).
    /// Ключ, который и убран, и добавлен (перевыпуск), сюда не попадает.
    MembershipChanged { actor: Vec<u8>, added: Vec<Vec<u8>>, removed: Vec<Vec<u8>> },
    /// Нас удалили из группы (обработали Commit, где нашего leaf-а больше нет).
    /// Верхний слой должен вычистить локальное состояние группы.
    SelfRemoved,
    /// Применён Commit только с ротацией ключа (self-Update, PCS) — состав НЕ менялся.
    /// Отличаем от [`Incoming::MembershipChanged`], чтобы UI не показывал ложное
    /// «изменение состава» на плановой ротации.
    Rekeyed,
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
        // Атомарно: убитый посреди записи процесс не должен оставить обрезанное
        // MLS-состояние — такую группу уже не загрузить.
        peerage_core::fsutil::write_atomic(path, &peerage_core::vault::seal(&state_purpose(), &out)).map_err(mls)?;
        Ok(())
    }

    /// Загрузить состояние группы с диска.
    pub fn load(path: &Path, identity: &Identity) -> Result<Self, GroupError> {
        let raw = std::fs::read(path).map_err(mls)?;
        let data = peerage_core::vault::open(&state_purpose(), &raw).map_err(mls)?;
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

    /// Перевыпустить участника (ADR-004, resync): убрать его устаревший leaf и
    /// добавить по свежему KeyPackage ОДНИМ коммитом. Остальные видят смену эпохи,
    /// а не «вышел и вошёл». Привязка KeyPackage к пиру — как в [`Self::add`].
    pub fn swap(
        &mut self,
        key_package: &[u8],
        expected_identity: &[u8; 32],
    ) -> Result<(Vec<u8>, Vec<u8>), GroupError> {
        let kp_in = KeyPackageIn::tls_deserialize_exact(key_package).map_err(mls)?;
        let kp = kp_in
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(mls)?;
        let leaf = kp.leaf_node();
        let identity = leaf.credential().serialized_content();
        let sig_key = leaf.signature_key().as_slice();
        if identity != expected_identity || sig_key != expected_identity {
            return Err(GroupError::IdentityMismatch);
        }
        let idx = self.leaf_index_of(expected_identity).ok_or(GroupError::NotMember)?;
        let group = self.group.as_mut().ok_or(GroupError::NoGroup)?;
        if idx == group.own_leaf_index() {
            return Err(GroupError::CannotRemoveSelf);
        }
        let out = group
            .swap_members(&self.provider, &self.signer, &[idx], &[kp])
            .map_err(mls)?;
        group.merge_pending_commit(&self.provider).map_err(mls)?;
        Ok((
            out.commit.tls_serialize_detached().map_err(mls)?,
            out.welcome.tls_serialize_detached().map_err(mls)?,
        ))
    }

    /// Leaf-индекс участника по его ed25519-идентичности (credential). None, если
    /// такого участника в группе нет.
    fn leaf_index_of(&self, identity: &[u8; 32]) -> Option<LeafNodeIndex> {
        let group = self.group.as_ref()?;
        group
            .members()
            .find(|m| m.credential.serialized_content() == identity.as_slice())
            .map(|m| m.index)
    }

    /// Отозвать участника по его ed25519-идентичности (ADR-004). Возвращает Commit
    /// для рассылки ОСТАВШИМСЯ участникам (в т.ч. самому удаляемому — он поймёт, что
    /// его удалили, из [`Incoming::SelfRemoved`]). Авторизацию (только админ) проверяет
    /// верхний слой. Своё удаление здесь запрещено — это отдельный «выход» (leave).
    pub fn remove(&mut self, identity: &[u8; 32]) -> Result<Vec<u8>, GroupError> {
        let idx = self.leaf_index_of(identity).ok_or(GroupError::NotMember)?;
        let group = self.group.as_mut().ok_or(GroupError::NoGroup)?;
        if idx == group.own_leaf_index() {
            return Err(GroupError::CannotRemoveSelf);
        }
        let (commit, _welcome, _info) = group
            .remove_members(&self.provider, &self.signer, &[idx])
            .map_err(mls)?;
        group.merge_pending_commit(&self.provider).map_err(mls)?;
        commit.tls_serialize_detached().map_err(mls)
    }

    /// Ротация собственного leaf-ключа (self-Update, PCS по ADR-004): сменяет эпоху,
    /// делая утёкший старый секрет бесполезным. Возвращает Commit для рассылки всем.
    pub fn update_self(&mut self) -> Result<Vec<u8>, GroupError> {
        let group = self.group.as_mut().ok_or(GroupError::NoGroup)?;
        let bundle = group
            .self_update(&self.provider, &self.signer, LeafNodeParameters::default())
            .map_err(mls)?;
        let commit = bundle.commit().clone();
        group.merge_pending_commit(&self.provider).map_err(mls)?;
        commit.tls_serialize_detached().map_err(mls)
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

    /// Обработать входящее групповое сообщение (прикладное или Commit). `admin` —
    /// единственный, чей коммит вправе менять состав (`None` — админ неизвестен:
    /// не вправе никто). Чужой такой коммит не применяется: эпоха остаётся прежней.
    pub fn process(&mut self, message: &[u8], admin: Option<&[u8]>) -> Result<Incoming, GroupError> {
        let msg = MlsMessageIn::tls_deserialize_exact(message).map_err(mls)?;
        let protocol = msg.try_into_protocol_message().map_err(mls)?;
        let group = self.group.as_mut().ok_or(GroupError::NoGroup)?;
        let processed = match group.process_message(&self.provider, protocol) {
            Ok(p) => p,
            // Рассинхрон эпох (ADR-004): пропущен Commit (WrongEpoch) или секрет
            // прошлой эпохи уже удалён forward-secrecy (NoPastEpochData — это и есть
            // «старые группы не лечатся»). Сигналим наверх для resync, не глотаем.
            Err(ProcessMessageError::ValidationError(
                ValidationError::WrongEpoch | ValidationError::NoPastEpochData,
            )) => return Err(GroupError::EpochMismatch),
            Err(e) => return Err(mls(e)),
        };
        // Личность отправителя (= ed25519-pubkey, см. member_identities) доступна
        // до распаковки содержимого — снимаем до into_content(), который её поглощает.
        let sender = processed.credential().serialized_content().to_vec();
        // Внешний вход (External Commit) и внешние отправители не предусмотрены:
        // в группу попадают только через Add от админа.
        if !matches!(processed.sender(), Sender::Member(_)) {
            return Err(GroupError::NotAdmin);
        }
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(app) => {
                Ok(Incoming::Message { sender, body: app.into_bytes() })
            }
            ProcessedMessageContent::StagedCommitMessage(staged) => {
                if !commit_allowed(&staged, &sender, admin) {
                    return Err(GroupError::NotAdmin);
                }
                // Различаем ДО merge: удалили нас / сменился состав / только ротация.
                let self_removed = staged.self_removed();
                let mut added: Vec<Vec<u8>> = staged
                    .add_proposals()
                    .map(|p| {
                        let kp = p.add_proposal().key_package();
                        kp.leaf_node().credential().serialized_content().to_vec()
                    })
                    .collect();
                // Убранных читаем из ТЕКУЩЕГО дерева: после слияния их leaf уже пуст.
                let mut removed: Vec<Vec<u8>> = staged
                    .remove_proposals()
                    .filter_map(|p| group.member(p.remove_proposal().removed()))
                    .map(|c| c.serialized_content().to_vec())
                    .collect();
                // Перевыпуск (тот же ключ убран и добавлен) — не смена состава.
                let both: Vec<Vec<u8>> = added.iter().filter(|k| removed.contains(k)).cloned().collect();
                added.retain(|k| !both.contains(k));
                removed.retain(|k| !both.contains(k));
                group.merge_staged_commit(&self.provider, *staged).map_err(mls)?;
                if self_removed {
                    Ok(Incoming::SelfRemoved)
                } else if !added.is_empty() || !removed.is_empty() {
                    Ok(Incoming::MembershipChanged { actor: sender, added, removed })
                } else {
                    Ok(Incoming::Rekeyed)
                }
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

/// AAD файла MLS-состояния (см. `peerage_core::vault::purpose`). Id группы в него не
/// входит — он лежит внутри файла, и слой групп сверяет его с ожидаемым при загрузке.
fn state_purpose() -> Vec<u8> {
    peerage_core::vault::purpose("mls-group", &[], 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> Identity {
        Identity::from_seed(&[n; 32]).unwrap()
    }

    /// Приём в группе, где админ — создатель id(1) (как во всех тестах ниже).
    fn from_a(m: &mut Member, msg: &[u8]) -> Result<Incoming, GroupError> {
        m.process(msg, Some(&id(1).public_key_bytes()))
    }

    #[test]
    fn roster_change_by_non_admin_is_refused() {
        // Звать и убирать вправе только админ: коммит с Add/Remove от рядового
        // участника остальные не применяют — их эпоха и ростер остаются прежними.
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        let mut c = Member::new(&id(3)).unwrap();
        let d = Member::new(&id(4)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();
        let (commit, w) = a.add(&c.key_package().unwrap(), &id(3).public_key_bytes()).unwrap();
        from_a(&mut b, &commit).unwrap();
        c.join(&w).unwrap();

        // B (не админ) зовёт D и отзывает C.
        let (add_d, _) = b.add(&d.key_package().unwrap(), &id(4).public_key_bytes()).unwrap();
        assert!(matches!(from_a(&mut a, &add_d), Err(GroupError::NotAdmin)));
        assert!(matches!(from_a(&mut c, &add_d), Err(GroupError::NotAdmin)));
        assert_eq!(a.member_count(), 3);
        assert_eq!(c.member_count(), 3);

        // Группа у A и C жива на прежней эпохе.
        let ct = a.encrypt(b"still here").unwrap();
        assert!(matches!(from_a(&mut c, &ct).unwrap(), Incoming::Message { body, .. } if body == b"still here"));
    }

    #[test]
    fn non_admin_may_rotate_own_key() {
        // Ротация своего ключа (PCS) состав не меняет — её может делать любой.
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();
        let commit = b.update_self().unwrap();
        assert!(matches!(from_a(&mut a, &commit).unwrap(), Incoming::Rekeyed));
    }

    #[test]
    fn unknown_admin_refuses_roster_changes() {
        // Админ неизвестен (старая группа) — менять состав не вправе никто.
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        let c = Member::new(&id(3)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();
        let (commit, _) = a.add(&c.key_package().unwrap(), &id(3).public_key_bytes()).unwrap();
        assert!(matches!(b.process(&commit, None), Err(GroupError::NotAdmin)));
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
        match from_a(&mut bob, &ct).unwrap() {
            Incoming::Message { sender, body } => {
                assert_eq!(body, b"hi group");
                assert_eq!(sender, id(1).public_key_bytes()); // отправитель = Alice
            }
            _ => panic!("ожидалось прикладное сообщение"),
        }

        // Bob → группа; Alice расшифровывает.
        let ct = bob.encrypt(b"hello back").unwrap();
        match from_a(&mut alice, &ct).unwrap() {
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
        from_a(&mut b, &commit).unwrap(); // B применяет изменение состава
        c.join(&welcome).unwrap();

        assert_eq!(a.member_count(), 3);
        assert_eq!(b.member_count(), 3);
        assert_eq!(c.member_count(), 3);

        // Сообщение от C видят и A, и B.
        let ct = c.encrypt(b"hey all").unwrap();
        assert!(matches!(from_a(&mut a, &ct).unwrap(), Incoming::Message { body, .. } if body == b"hey all"));
        assert!(matches!(from_a(&mut b, &ct).unwrap(), Incoming::Message { body, .. } if body == b"hey all"));
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
    fn remove_revokes_member() {
        // ADR-004: админ отзывает участника; оставшиеся видят смену состава,
        // отозванный узнаёт об удалении и теряет доступ к новым сообщениям.
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        let mut c = Member::new(&id(3)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();
        let (commit_add_c, w) = a.add(&c.key_package().unwrap(), &id(3).public_key_bytes()).unwrap();
        from_a(&mut b, &commit_add_c).unwrap();
        c.join(&w).unwrap();
        assert_eq!(a.member_count(), 3);

        // Нельзя отозвать себя / несуществующего.
        assert!(matches!(a.remove(&id(1).public_key_bytes()), Err(GroupError::CannotRemoveSelf)));
        assert!(matches!(a.remove(&id(9).public_key_bytes()), Err(GroupError::NotMember)));

        // A отзывает C. Commit идёт ВСЕМ, включая удаляемого.
        let commit = a.remove(&id(3).public_key_bytes()).unwrap();
        assert_eq!(a.member_count(), 2);
        assert!(matches!(from_a(&mut b, &commit).unwrap(), Incoming::MembershipChanged { .. }));
        assert_eq!(b.member_count(), 2);
        assert!(matches!(from_a(&mut c, &commit).unwrap(), Incoming::SelfRemoved));

        // После отзыва: B читает, отозванный C — уже нет.
        let ct = a.encrypt(b"after kick").unwrap();
        assert!(matches!(from_a(&mut b, &ct).unwrap(), Incoming::Message { body, .. } if body == b"after kick"));
        assert!(from_a(&mut c, &ct).is_err());
    }

    #[test]
    fn membership_commit_names_who_was_added_and_removed() {
        // Коммит сообщает не «что-то изменилось», а кого добавили и кого убрали:
        // из этого остальные пишут строки «в группе» / «больше не в группе».
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        let mut c = Member::new(&id(3)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();

        let (commit, w) = a.add(&c.key_package().unwrap(), &id(3).public_key_bytes()).unwrap();
        match from_a(&mut b, &commit).unwrap() {
            Incoming::MembershipChanged { actor, added, removed } => {
                assert_eq!(actor, id(1).public_key_bytes());
                assert_eq!(added, vec![id(3).public_key_bytes().to_vec()]);
                assert!(removed.is_empty());
            }
            _ => panic!("ожидалась смена состава"),
        }
        c.join(&w).unwrap();

        let commit = a.remove(&id(3).public_key_bytes()).unwrap();
        match from_a(&mut b, &commit).unwrap() {
            Incoming::MembershipChanged { added, removed, .. } => {
                assert!(added.is_empty());
                assert_eq!(removed, vec![id(3).public_key_bytes().to_vec()]);
            }
            _ => panic!("ожидалась смена состава"),
        }
    }

    #[test]
    fn swap_reissues_member_without_roster_change_for_others() {
        // Перевыпуск отставшего (ADR-004) — один коммит Remove+Add того же ключа:
        // у остальных это не «вышел и вошёл», а просто новая эпоха.
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        let mut c = Member::new(&id(3)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();
        let (commit, w) = a.add(&c.key_package().unwrap(), &id(3).public_key_bytes()).unwrap();
        from_a(&mut b, &commit).unwrap();
        c.join(&w).unwrap();

        // C отстал: заново — со свежим KeyPackage.
        let mut c2 = Member::new(&id(3)).unwrap();
        let (commit, welcome) = a.swap(&c2.key_package().unwrap(), &id(3).public_key_bytes()).unwrap();
        assert!(matches!(from_a(&mut b, &commit).unwrap(), Incoming::Rekeyed));
        c2.join(&welcome).unwrap();
        assert_eq!(a.member_count(), 3);
        assert_eq!(b.member_count(), 3);

        let ct = a.encrypt(b"after swap").unwrap();
        assert!(matches!(from_a(&mut b, &ct).unwrap(), Incoming::Message { body, .. } if body == b"after swap"));
        assert!(matches!(from_a(&mut c2, &ct).unwrap(), Incoming::Message { body, .. } if body == b"after swap"));
    }

    #[test]
    fn self_update_rekeys_without_membership_change() {
        // ADR-004: ротация ключа (PCS) — не «смена состава», группа продолжает жить.
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();

        let commit = a.update_self().unwrap();
        assert!(matches!(from_a(&mut b, &commit).unwrap(), Incoming::Rekeyed));
        let ct = a.encrypt(b"post-rekey").unwrap();
        assert!(matches!(from_a(&mut b, &ct).unwrap(), Incoming::Message { body, .. } if body == b"post-rekey"));
    }

    #[test]
    fn missed_commit_surfaces_epoch_mismatch() {
        // ADR-004: отставший (пропустил Commit) получает EpochMismatch, а не
        // generic-ошибку — сигнал для resync («старые группы не лечатся»).
        let mut a = Member::new(&id(1)).unwrap();
        let mut b = Member::new(&id(2)).unwrap();
        a.create_group().unwrap();
        let (_, w) = a.add(&b.key_package().unwrap(), &id(2).public_key_bytes()).unwrap();
        b.join(&w).unwrap();

        // A двигает эпоху ротацией — B этот Commit НЕ получает (отстал).
        let _missed = a.update_self().unwrap();
        // Сообщение уже из новой эпохи; B на старой → рассинхрон.
        let ct = a.encrypt(b"from new epoch").unwrap();
        assert!(matches!(from_a(&mut b, &ct), Err(GroupError::EpochMismatch)));
    }

    #[test]
    fn group_state_survives_save_load() {
        let dir = std::env::temp_dir().join(format!("test-grp-{}", std::process::id()));
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
        assert!(matches!(from_a(&mut b, &ct).unwrap(), Incoming::Message { body, .. } if body == b"after reload"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
