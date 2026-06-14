//! p2p-groupchat — групповой чат поверх Yggdrasil, БЕЗ сервера.
//!
//! Связывает MLS-группу ([`p2p_group`]) с транспортом ([`p2p_messenger`]):
//! рассылает MLS-сообщения участникам напрямую через mesh; список получателей
//! берётся из самой группы (MLS знает участников по их публичным ключам).
//!
//! Порядок изменений состава (Commit) — через **админ-секвенсора**: добавляет/
//! удаляет участников один (создатель группы), что исключает одновременные
//! Commit. Прикладные сообщения шлёт любой (MLS упорядочивает по эпохам).

use std::path::Path;
use std::sync::Arc;

use p2p_core::Identity;
use p2p_group::{GroupError, Incoming, Member};
use p2p_messenger::Messenger;

pub type PeerKey = [u8; 32];

#[derive(Debug, thiserror::Error)]
pub enum GroupChatError {
    #[error(transparent)]
    Group(#[from] GroupError),
}

/// Один участник группового чата на узле.
pub struct GroupChat {
    member: Member,
    messenger: Arc<Messenger>,
    self_pub: PeerKey,
}

impl GroupChat {
    /// Создать новую группу (мы — администратор/секвенсор).
    pub fn create(identity: &Identity, messenger: Arc<Messenger>) -> Result<Self, GroupChatError> {
        let mut member = Member::new(identity)?;
        member.create_group()?;
        Ok(Self { member, messenger, self_pub: identity.public_key_bytes() })
    }

    /// Подготовиться к вступлению: создаёт участника и отдаёт его KeyPackage
    /// (его передают администратору, например, по 1:1-чату).
    pub fn pending(
        identity: &Identity,
        messenger: Arc<Messenger>,
    ) -> Result<(Self, Vec<u8>), GroupChatError> {
        let member = Member::new(identity)?;
        let kp = member.key_package()?;
        Ok((Self { member, messenger, self_pub: identity.public_key_bytes() }, kp))
    }

    fn others(&self) -> Vec<PeerKey> {
        self.member
            .member_identities()
            .into_iter()
            .filter_map(|id| <[u8; 32]>::try_from(id.as_slice()).ok())
            .filter(|pk| *pk != self.self_pub)
            .collect()
    }

    /// Администратор приглашает контакт: просит его KeyPackage (контакт ответит
    /// [`p2p_messenger::Event::GroupKeyReply`], после чего вызываем [`add_member`]).
    pub fn invite(&self, peer: PeerKey) {
        let _ = self.messenger.send_group_key_request(&peer);
    }

    /// Администратор добавляет участника по его KeyPackage: рассылает Commit
    /// существующим участникам и Welcome — новому.
    pub fn add_member(&mut self, new_peer: PeerKey, key_package: &[u8]) -> Result<(), GroupChatError> {
        let existing = self.others(); // до добавления
        // H1: KeyPackage привязывается к аутентифицированному пиру (new_peer).
        let (commit, welcome) = self.member.add(key_package, &new_peer)?;
        for pk in existing {
            let _ = self.messenger.send_group(&pk, &commit);
        }
        let _ = self.messenger.send_group(&new_peer, &welcome);
        Ok(())
    }

    /// Отправить текст в группу (рассылается всем текущим участникам).
    pub fn send_text(&mut self, text: &str) -> Result<(), GroupChatError> {
        let ct = self.member.encrypt(text.as_bytes())?;
        for pk in self.others() {
            let _ = self.messenger.send_group(&pk, &ct);
        }
        Ok(())
    }

    /// Обработать входящее групповое сообщение ([`p2p_messenger::Event::Group`]).
    /// Если мы ещё не в группе — это Welcome (вступаем). Возвращает текст для
    /// прикладных сообщений.
    pub fn handle(&mut self, data: &[u8]) -> Result<Option<String>, GroupChatError> {
        if !self.member.is_joined() {
            self.member.join(data)?;
            return Ok(None);
        }
        match self.member.process(data)? {
            Incoming::Message(b) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
            _ => Ok(None),
        }
    }

    pub fn member_count(&self) -> usize {
        self.member.member_count()
    }

    /// Идентификатор группы (hex-able байты), если в группе.
    pub fn group_id(&self) -> Option<Vec<u8>> {
        self.member.group_id()
    }

    /// Ключи других участников (для группового звонка).
    pub fn members(&self) -> Vec<PeerKey> {
        self.others()
    }

    /// Сохранить состояние группы на диск.
    pub fn save(&self, path: &Path) -> Result<(), GroupChatError> {
        self.member.save(path)?;
        Ok(())
    }

    /// Загрузить группу с диска.
    pub fn load(
        path: &Path,
        identity: &Identity,
        messenger: Arc<Messenger>,
    ) -> Result<Self, GroupChatError> {
        let member = Member::load(path, identity)?;
        Ok(Self { member, messenger, self_pub: identity.public_key_bytes() })
    }
}
