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
use serde::{Deserialize, Serialize};

pub type PeerKey = [u8; 32];

/// Прикладное тело группового сообщения (M2): текст или ссылка на медиа-блоб.
/// Сериализуется в JSON и шифруется MLS. Старое сырое-UTF-8 (до M2) распознаётся
/// как текст фолбэком в [`GroupChat::handle`] — группы пересоздавать не обязательно.
#[derive(Serialize, Deserialize)]
#[serde(tag = "t")]
enum AppMsg {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "media")]
    Media { kind: String, hash: String, mime: String, name: String },
    /// Служебное: имя группы. Админ шлёт его новому участнику после add, чтобы тот
    /// увидел настоящее название, а не дефолт (в UI не показывается как сообщение).
    #[serde(rename = "name")]
    Name { name: String },
}

/// Результат обработки входящего группового сообщения — для показа в UI.
pub enum Inbound {
    /// Прикладное сообщение: отправитель + текст.
    Message { sender: PeerKey, text: String },
    /// Медиа-сообщение (M2): отправитель + ссылка на блоб по хэшу.
    Media { sender: PeerKey, kind: String, hash: String, mime: String, name: String },
    /// Имя группы (служебное): верхний слой обновляет название, без показа в чате.
    Name { name: String },
    /// Изменён состав группы (применён Commit), его применил actor.
    Membership { actor: PeerKey },
    /// Нас удалили из группы (ADR-004) — верхний слой забывает группу локально.
    SelfRemoved,
    /// Мы отстали (пропущен Commit / удалён секрет эпохи) — верхний слой должен
    /// инициировать resync (re-add). См. ADR-004.
    NeedsResync,
    /// Нечего показывать (вступление по Welcome, ротация ключа, служебное).
    None,
}

fn to_key(id: Vec<u8>) -> PeerKey {
    <[u8; 32]>::try_from(id.as_slice()).unwrap_or([0u8; 32])
}

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

    /// Администратор отзывает участника (ADR-004). Commit рассылается ВСЕМ текущим
    /// участникам, **включая удаляемого** (список снимаем ДО удаления) — тот поймёт,
    /// что его удалили, по [`Inbound::SelfRemoved`]. Авторизацию (мы — админ)
    /// проверяет верхний слой.
    pub fn remove_member(&mut self, peer: PeerKey) -> Result<(), GroupChatError> {
        let recipients = self.others(); // до удаления — ещё включает peer
        let commit = self.member.remove(&peer)?;
        for pk in recipients {
            let _ = self.messenger.send_group(&pk, &commit);
        }
        Ok(())
    }

    /// Ротация собственного ключа (PCS, ADR-004): Commit рассылается всем — сменяет
    /// эпоху, обесценивая утёкший старый секрет. Периодический вызов даёт «катящийся»
    /// PCS всей группе.
    pub fn rekey_self(&mut self) -> Result<(), GroupChatError> {
        let commit = self.member.update_self()?;
        for pk in self.others() {
            let _ = self.messenger.send_group(&pk, &commit);
        }
        Ok(())
    }

    /// Отправить текст в группу (рассылается всем текущим участникам).
    pub fn send_text(&mut self, text: &str) -> Result<(), GroupChatError> {
        self.send_app(&AppMsg::Text { text: text.to_string() })
    }

    /// Отправить медиа-сообщение (M2): ссылка на блоб по хэшу. Байты блоба идут
    /// отдельно (движок пушит их каждому участнику). `hash` — hex.
    pub fn send_media(&mut self, kind: &str, hash: &str, mime: &str, name: &str) -> Result<(), GroupChatError> {
        self.send_app(&AppMsg::Media {
            kind: kind.to_string(), hash: hash.to_string(),
            mime: mime.to_string(), name: name.to_string(),
        })
    }

    /// Разослать имя группы участникам (служебное). Админ зовёт после добавления
    /// участника — тот увидит настоящее название вместо дефолта.
    pub fn send_name(&mut self, name: &str) -> Result<(), GroupChatError> {
        self.send_app(&AppMsg::Name { name: name.to_string() })
    }

    /// Зашифровать прикладное тело (JSON) MLS и разослать участникам.
    fn send_app(&mut self, msg: &AppMsg) -> Result<(), GroupChatError> {
        let body = serde_json::to_vec(msg).unwrap_or_default();
        let ct = self.member.encrypt(&body)?;
        for pk in self.others() {
            let _ = self.messenger.send_group(&pk, &ct);
        }
        Ok(())
    }

    /// Обработать входящее групповое сообщение ([`p2p_messenger::Event::Group`]).
    /// Если мы ещё не в группе — это Welcome (вступаем). Возвращает отправителя
    /// и текст для прикладных сообщений либо актора смены состава.
    pub fn handle(&mut self, data: &[u8]) -> Result<Inbound, GroupChatError> {
        if !self.member.is_joined() {
            self.member.join(data)?;
            return Ok(Inbound::None);
        }
        match self.member.process(data) {
            Ok(Incoming::Message { sender, body }) => {
                let sender = to_key(sender);
                // M2: тело — JSON-envelope (текст/медиа); старое сырое-UTF-8 → текст.
                Ok(match serde_json::from_slice::<AppMsg>(&body) {
                    Ok(AppMsg::Text { text }) => Inbound::Message { sender, text },
                    Ok(AppMsg::Media { kind, hash, mime, name }) => {
                        Inbound::Media { sender, kind, hash, mime, name }
                    }
                    Ok(AppMsg::Name { name }) => Inbound::Name { name },
                    Err(_) => Inbound::Message { sender, text: String::from_utf8_lossy(&body).into_owned() },
                })
            }
            Ok(Incoming::MembershipChanged { actor }) => {
                Ok(Inbound::Membership { actor: to_key(actor) })
            }
            Ok(Incoming::SelfRemoved) => Ok(Inbound::SelfRemoved),
            // Ротация ключа и служебное — молча.
            Ok(Incoming::Rekeyed | Incoming::Other) => Ok(Inbound::None),
            // Рассинхрон эпох (ADR-004): не ошибка, а сигнал наверх для resync.
            Err(GroupError::EpochMismatch) => Ok(Inbound::NeedsResync),
            Err(e) => Err(e.into()),
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
