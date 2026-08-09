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
    Text {
        text: String,
        /// Идентификатор сообщения в пределах ОТПРАВИТЕЛЯ. Ссылка на сообщение —
        /// пара «автор + id» (ADR-006); у групп автор берётся из MLS, поэтому здесь
        /// достаточно id. `0` — сообщение старого клиента, на такое нельзя ни
        /// ответить, ни отозвать: сослаться не на что.
        #[serde(default)]
        id: u64,
        /// id сообщения, на которое отвечают (0 — обычное).
        #[serde(default)]
        reply_to: u64,
        /// hex автора цитируемого сообщения. В группе собеседников много, поэтому
        /// одного флага «моё/чужое», как в DM, недостаточно.
        #[serde(default)]
        reply_sender: String,
        /// Пересылка: hex автора ИСХОДНОГО сообщения (пусто — своё).
        #[serde(default)]
        fwd_from: String,
        /// «Переслано, автора не называем» — ключ автора не отправляется.
        #[serde(default)]
        fwd_anon: bool,
    },
    #[serde(rename = "media")]
    Media(MediaBody),
    /// Отзыв СВОЕГО сообщения («удалить у всех»). Получатели удаляют строку
    /// `(отправитель этого пакета, target)` — чужое отозвать нельзя, автор берётся
    /// из MLS, а не из тела.
    #[serde(rename = "retract")]
    Retract { target: u64 },
    /// Правка своего сообщения: автор берётся из MLS, поэтому в теле только цель.
    #[serde(rename = "edit")]
    Edit { target: u64, text: String },
    /// Закрепить/открепить сообщение группы. Ссылка — пара «автор + id» (ADR-006).
    /// Право проверяет верхний слой: закрепляет админ, иначе любой участник мог бы
    /// повесить своё сообщение в шапку всем остальным.
    #[serde(rename = "pin")]
    Pin { target: u64, sender: String, on: bool },
    /// Служебное: имя группы. Админ шлёт его новому участнику после add, чтобы тот
    /// увидел настоящее название, а не дефолт (в UI не показывается как сообщение).
    #[serde(rename = "name")]
    Name { name: String },
}

/// Медиа-вложение группового сообщения: ссылка на блоб плюс всё, что нужно
/// карточке «скачать» ДО единого байта содержимого — имя, размер, превью.
///
/// Размер здесь не для красоты: без него получатель не может решить, тянуть ли
/// вложение молча, и чужое видео на 800 МБ приезжает само (в DM это решается
/// такими же полями `MediaMeta`).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MediaBody {
    /// Пересылка: hex автора исходного сообщения (пусто — своё).
    #[serde(default)]
    pub fwd_from: String,
    /// «Переслано, автора не называем».
    #[serde(default)]
    pub fwd_anon: bool,
    /// Ответ вложением: id цитируемого сообщения (0 — обычное). Как у `Text`:
    /// ответить фотографией — обычный случай, и ссылка тут та же (ADR-006).
    #[serde(default)]
    pub reply_to: u64,
    /// hex автора цитируемого сообщения (пусто — обычное).
    #[serde(default)]
    pub reply_sender: String,
    /// Идентификатор в пределах отправителя (см. `AppMsg::Text::id`).
    #[serde(default)]
    pub id: u64,
    pub kind: String,
    /// hex контент-адреса блоба с содержимым.
    pub hash: String,
    pub mime: String,
    pub name: String,
    /// Подпись к вложению. Внутри того же сообщения, а не отдельным текстом:
    /// иначе «фото с подписью» — два MLS-сообщения подряд, и наблюдатель узнаёт
    /// их по характерной паре (то же соображение, что и в DM, см. protocol-spec).
    pub caption: String,
    pub size: u64,
    pub duration_ms: u32,
    pub width: u32,
    pub height: u32,
    /// hex блоба-миниатюры (пусто — нет). Едет отдельным мелким блобом: вшить её
    /// сюда нельзя — тело раздулось бы и вышло из бакета паддинга (M5).
    pub thumb: String,
}

/// Результат обработки входящего группового сообщения — для показа в UI.
pub enum Inbound {
    /// Прикладное сообщение: отправитель + текст (+ ссылка ответа, ADR-006).
    Message {
        sender: PeerKey,
        text: String,
        id: u64,
        reply_to: u64,
        reply_sender: String,
        /// Пересылка: hex автора исходного сообщения (пусто — своё).
        fwd_from: String,
        /// «Переслано, автора не называем».
        fwd_anon: bool,
    },
    /// Отправитель просит удалить СВОЁ сообщение `target` у всех.
    Retract { sender: PeerKey, target: u64 },
    /// Отправитель исправил своё сообщение `target`.
    Edit { sender: PeerKey, target: u64, text: String },
    /// Закрепить/открепить сообщение (`actor` — кто это сделал).
    Pin { actor: PeerKey, target: u64, sender: String, on: bool },
    /// Медиа-сообщение (M2): отправитель + ссылка на блоб с метаданными карточки.
    Media { sender: PeerKey, media: MediaBody },
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

/// Обрезать подпись до `MAX_CAPTION` БАЙТ по границе символа — как в DM. Групповое
/// тело шифруется MLS и едет тем же `CHAN_CHAT`, поэтому потолок общий.
fn truncate_caption(s: &str) -> String {
    if s.len() <= p2p_core::protocol::MAX_CAPTION {
        return s.to_string();
    }
    let mut end = p2p_core::protocol::MAX_CAPTION;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
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
    pub fn send_text(&mut self, text: &str, id: u64) -> Result<(), GroupChatError> {
        self.send_text_reply(text, id, 0, "")
    }

    /// То же ответом на сообщение `reply_to` автора `reply_sender` (hex).
    pub fn send_text_reply(
        &mut self,
        text: &str,
        id: u64,
        reply_to: u64,
        reply_sender: &str,
    ) -> Result<(), GroupChatError> {
        self.send_text_full(text, id, reply_to, reply_sender, "", false)
    }

    /// Общий путь: обычный текст, ответ и пересылка. `fwd_from` — hex автора
    /// исходного сообщения (пусто — своё).
    pub fn send_text_full(
        &mut self,
        text: &str,
        id: u64,
        reply_to: u64,
        reply_sender: &str,
        fwd_from: &str,
        fwd_anon: bool,
    ) -> Result<(), GroupChatError> {
        self.send_app(&AppMsg::Text {
            text: text.to_string(),
            id,
            reply_to,
            reply_sender: reply_sender.to_string(),
            fwd_from: fwd_from.to_string(),
            fwd_anon,
        })
    }

    /// Попросить участников удалить наше сообщение `target` («удалить у всех»).
    pub fn send_retract(&mut self, target: u64) -> Result<(), GroupChatError> {
        self.send_app(&AppMsg::Retract { target })
    }

    /// Отправить медиа-сообщение (M2): ссылка на блоб по хэшу. Байты блоба идут
    /// отдельно (движок решает, пушить их или ждать запроса получателя).
    pub fn send_media(&mut self, media: &MediaBody) -> Result<(), GroupChatError> {
        // Подпись режем здесь, а не в UI: за размер шифруемого тела отвечает этот слой.
        let body = MediaBody { caption: truncate_caption(&media.caption), ..media.clone() };
        self.send_app(&AppMsg::Media(body))
    }

    /// Исправить своё сообщение у всех участников.
    pub fn send_edit(&mut self, target: u64, text: &str) -> Result<(), GroupChatError> {
        self.send_app(&AppMsg::Edit { target, text: text.to_string() })
    }

    /// Закрепить/открепить сообщение у всех участников.
    pub fn send_pin(&mut self, target: u64, sender: &str, on: bool) -> Result<(), GroupChatError> {
        self.send_app(&AppMsg::Pin { target, sender: sender.to_string(), on })
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
                    Ok(AppMsg::Text { text, id, reply_to, reply_sender, fwd_from, fwd_anon }) => {
                        Inbound::Message {
                            sender, text, id, reply_to, reply_sender, fwd_from, fwd_anon,
                        }
                    }
                    Ok(AppMsg::Retract { target }) => Inbound::Retract { sender, target },
                    Ok(AppMsg::Edit { target, text }) => Inbound::Edit { sender, target, text },
                    Ok(AppMsg::Pin { target, sender: who, on }) => {
                        Inbound::Pin { actor: sender, target, sender: who, on }
                    }
                    Ok(AppMsg::Media(media)) => {
                        // Режем и на приёме: отправитель мог быть пропатчен.
                        let caption = truncate_caption(&media.caption);
                        Inbound::Media { sender, media: MediaBody { caption, ..media } }
                    }
                    Ok(AppMsg::Name { name }) => Inbound::Name { name },
                    // Сырое UTF-8 — сообщение клиента до M2: id у него нет.
                    Err(_) => Inbound::Message {
                        sender,
                        text: String::from_utf8_lossy(&body).into_owned(),
                        id: 0,
                        reply_to: 0,
                        reply_sender: String::new(),
                        fwd_from: String::new(),
                        fwd_anon: false,
                    },
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
