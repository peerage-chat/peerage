//! Резервная копия ДАННЫХ: что кладём в архив и что достаём обратно.
//!
//! Формат контейнера — [`peerage_core::backup`]; здесь только состав и правила слияния.
//!
//! **Пользователь выбирает состав.** Причина не в гибкости ради гибкости: части
//! отличаются на порядки по объёму и по чувствительности. Переписка — мегабайты,
//! вложения — гигабайты; копию с вложениями некуда положить, а без них она влезает
//! куда угодно. Журнал звонков локален и никем не синхронизируется, и человек может
//! осознанно не хотеть его в облаке. Решать за пользователя тут нечего.
//!
//! **Чего в копии НЕТ и почему:**
//!
//! * **Ключа личности.** Он в отдельной копии (24 слова). Положив его сюда, мы
//!   сделали бы один файл достаточным, чтобы стать вами, — и цена утечки копии
//!   выросла бы с «прочитали переписку» до «забрали личность».
//! * **Групп (MLS-состояния).** Восстановленный снимок ратчета устарел: группа ушла
//!   вперёд, читать новые сообщения им нельзя, а два устройства с одним состоянием
//!   ломают группу целиком. Честная граница: копия возвращает то, что ЧИТАТЬ, а не
//!   то, ЧЕМ расшифровывать. После восстановления в группы приглашают заново.
//! * **Транспортных ключей контактов** (адресов): протухают, см.
//!   [`peerage_core::store::ContactStore::import_contact`].
//!
//! **Восстановление — слияние, а не замена.** То, что пришло после снятия копии,
//! новее; затирать его архивом нельзя. Ключи записей одинаковы на обоих концах,
//! поэтому повторный импорт того же файла ничего не задваивает.

use std::path::Path;

use peerage_core::backup::{BackupError, Frame, Reader, Writer};
use peerage_core::store::{CallRecord, Chat, Contact, StoredMessage};
use serde::{Deserialize, Serialize};

use crate::{dto, push_event, Engine, LockRecover};

/// Сколько сообщений в одной записи архива. Компромисс: слишком мелкие записи —
/// оверхед на кадр (nonce+tag), слишком крупные — пик памяти на телефоне.
const MSG_BATCH: usize = 500;

/// Потолок на одно вложение в копии. Кадр контейнера ограничен 16 МиБ, и держать
/// файл целиком в памяти телефона всё равно нельзя. Крупные вложения контент-адресны
/// и качаются у собеседника заново, поэтому их пропуск — потеря удобства, а не данных;
/// сколько пропущено, сообщается пользователю числом, а не молчанием.
const MAX_BLOB: u64 = 15 * 1024 * 1024;

/// Что положить в копию. Все части независимы.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Parts {
    /// Контакты: имена, отпечатки, признак верификации. Вместе с ними едут аватары
    /// (мелкие блобы) — без них список контактов выглядит сломанным.
    #[serde(default)]
    pub contacts: bool,
    /// Переписка личных чатов.
    #[serde(default)]
    pub messages: bool,
    /// Переписка групп. Отдельным флажком, потому что это чужие сообщения тоже, и
    /// решение «класть ли их в файл» человек принимает осознанно. Само членство в
    /// группе не восстанавливается: MLS-состояние в копию не входит (ADR-004).
    #[serde(default)]
    pub groups: bool,
    /// Заметки себе («Сохранённое») — это чат с самим собой, поэтому отдельным флагом.
    #[serde(default)]
    pub notes: bool,
    /// Журнал звонков.
    #[serde(default)]
    pub calls: bool,
    /// Файлы вложений. Тяжёлая часть: обычно это весь объём копии.
    #[serde(default)]
    pub attachments: bool,
}

/// Сколько чего вошло (или войдёт — при оценке до создания копии).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub contacts: u64,
    pub messages: u64,
    #[serde(default)]
    pub group_messages: u64,
    pub notes: u64,
    pub calls: u64,
    pub attachments: u64,
    /// Байты вложений (у оценки — сумма файлов на диске).
    pub attachment_bytes: u64,
    /// Вложения, пропущенные как слишком крупные (> [`MAX_BLOB`]).
    pub skipped_big: u64,
}

/// Запись архива.
#[derive(Serialize, Deserialize)]
#[serde(tag = "rec", rename_all = "snake_case")]
enum Rec {
    /// Идёт первой: по ней читатель узнаёт состав, не разбирая весь файл.
    Manifest {
        version: u32,
        created: i64,
        me: String,
        parts: Parts,
    },
    // Именованные поля, а не newtype: внутренне тегированный enum (`tag = "rec"`)
    // не умеет сериализовать вариант-обёртку над списком.
    Contacts {
        items: Vec<Contact>,
    },
    Messages {
        items: Vec<MsgRow>,
    },
    Calls {
        items: Vec<CallRow>,
    },
    /// Названия групп. Без них восстановленная переписка называлась бы hex-строкой:
    /// членство в копию не входит, и спросить имя будет не у кого.
    Chats {
        items: Vec<ChatRow>,
    },
    /// Реакции — с той перепиской, к которой относятся; снятые тоже (надгробия).
    Reactions {
        items: Vec<ReactRow>,
    },
    /// Своё имя и фото (шаг 7g): без них после переноса на новый телефон профиль пустой.
    /// Всегда в составе — это не переписка и не объём. Фото — блобом, как вложения.
    Profile {
        name: String,
        avatar_id: String,
        avatar_mime: String,
    },
}

#[derive(Serialize, Deserialize)]
struct ReactRow {
    chat: String,
    scope: i64,
    msg_id: u64,
    msg_sender: String,
    who: String,
    emoji: String,
    at: i64,
}

#[derive(Serialize, Deserialize)]
struct ChatRow {
    chat: String,
    scope: i64,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct MsgRow {
    /// hex чата: ключ собеседника или gid группы. `peer` — как это поле звалось в
    /// копиях до объединения хранилищ; читаем и такие.
    #[serde(alias = "peer")]
    chat: String,
    /// 0 — личный чат (в т.ч. заметки себе), 1 — группа. В старых копиях групп не
    /// было вовсе, поэтому отсутствие поля значит «личный».
    #[serde(default)]
    scope: i64,
    #[serde(flatten)]
    msg: StoredMessage,
}

#[derive(Serialize, Deserialize)]
struct CallRow {
    peer: String,
    gid: String,
    incoming: bool,
    outcome: String,
    started: i64,
    duration: i64,
    /// Видеозвонок; в копиях до 2.8б поля нет — значит голосовой.
    #[serde(default)]
    video: bool,
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// Закрыта ли копия `path` паролем — по заголовку, до расшифровки: восстановление
/// спрашивает пароль только тогда.
pub fn backup_needs_password(path: &Path) -> Result<bool, BackupError> {
    peerage_core::backup::needs_password(std::fs::File::open(path)?)
}

impl Engine {
    /// Оценка объёма ДО создания копии: чтобы выбор «класть вложения или нет» делался
    /// по цифрам, а не вслепую.
    pub fn backup_estimate(&self) -> Stats {
        let me = self.me_pub;
        let msgs = self.messages.lock_safe();
        let (mut messages, mut notes, mut group_messages) = (0u64, 0u64, 0u64);
        let mut after = 0i64;
        loop {
            let page = msgs.export_page(after, MSG_BATCH);
            if page.is_empty() {
                break;
            }
            for (rowid, chat, scope, _) in &page {
                after = *rowid;
                if *scope == 1 {
                    group_messages += 1;
                } else if chat.as_slice() == me.as_slice() {
                    notes += 1;
                } else {
                    messages += 1;
                }
            }
        }
        drop(msgs);

        // Вложения считаем по РЕАЛЬНЫМ файлам: сообщение может ссылаться на ещё не
        // скачанный блоб, и обещать его в копии нельзя.
        let (mut attachments, mut attachment_bytes, mut skipped_big) = (0u64, 0u64, 0u64);
        let mut seen = std::collections::HashSet::new();
        for id in self.backup_blob_ids() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(meta) = std::fs::metadata(self.blob_dir.join(&id)) else {
                continue;
            };
            if meta.len() > MAX_BLOB {
                skipped_big += 1;
                continue;
            }
            attachments += 1;
            attachment_bytes += meta.len();
        }

        Stats {
            contacts: self.contacts.lock_safe().list().len() as u64,
            messages,
            group_messages,
            notes,
            calls: self.calls.lock_safe().count(),
            attachments,
            attachment_bytes,
            skipped_big,
        }
    }

    /// То же для FFI: JSON со статистикой.
    pub fn backup_estimate_json(&self) -> String {
        serde_json::to_string(&self.backup_estimate()).unwrap_or_else(|_| "{}".into())
    }

    /// Создать копию; `parts_json` — объект вида `{"contacts":true,...}`.
    /// Возвращает JSON статистики либо `{"error":"..."}` — текст ошибки показывается
    /// пользователю как есть, поэтому он на русском и без внутренних терминов.
    pub fn backup_export_json(&self, path: &str, parts_json: &str, password: &str) -> String {
        let parts: Parts = match serde_json::from_str(parts_json) {
            Ok(p) => p,
            Err(e) => return err_json(&format!("неверный состав копии: {e}")),
        };
        match self.backup_export(
            Path::new(path),
            parts,
            Some(password).filter(|p| !p.is_empty()),
        ) {
            Ok(s) => serde_json::to_string(&s).unwrap_or_else(|_| "{}".into()),
            Err(e) => err_json(&e.to_string()),
        }
    }

    /// Восстановить из копии. Возвращает JSON ДОБАВЛЕННОГО либо `{"error":"..."}`.
    /// Пустой `password` — без пароля.
    pub fn backup_import_json(&self, path: &str, password: &str) -> String {
        match self.backup_import(Path::new(path), Some(password).filter(|p| !p.is_empty())) {
            Ok(s) => serde_json::to_string(&s).unwrap_or_else(|_| "{}".into()),
            Err(e) => err_json(&e.to_string()),
        }
    }

    /// id блобов, на которые ссылается история (вложения и их миниатюры).
    fn backup_blob_ids(&self) -> Vec<String> {
        let mut out = Vec::new();
        let msgs = self.messages.lock_safe();
        let mut after = 0i64;
        loop {
            let page = msgs.export_page(after, MSG_BATCH);
            if page.is_empty() {
                break;
            }
            for (rowid, _, _, m) in &page {
                after = *rowid;
                if !m.blob_id.is_empty() {
                    out.push(m.blob_id.clone());
                }
                if !m.thumb_id.is_empty() {
                    out.push(m.thumb_id.clone());
                }
            }
        }
        out
    }

    /// Ключ копий этой личности — из 24 слов (шаг 7k).
    fn backup_key(&self) -> zeroize::Zeroizing<[u8; 32]> {
        zeroize::Zeroizing::new(
            self.identity
                .derive_subkey(peerage_core::backup::SUBKEY_CONTEXT),
        )
    }

    /// Создать копию в `path`: ключом личности, с `password` — ещё и им.
    pub fn backup_export(
        &self,
        path: &Path,
        parts: Parts,
        password: Option<&str>,
    ) -> Result<Stats, BackupError> {
        let file = std::fs::File::create(path)?;
        let mut w = Writer::create(std::io::BufWriter::new(file), &self.backup_key(), password)?;
        let mut stats = Stats::default();

        w.json(&Rec::Manifest {
            version: 1,
            created: crate::now(),
            me: hex::encode(self.me_pub),
            parts,
        })?;

        let me = self.load_profile_meta();
        w.json(&Rec::Profile {
            name: me.name,
            avatar_id: me.avatar_id.clone(),
            avatar_mime: me.avatar_mime,
        })?;

        // Аватары едут вместе с контактами: без них список выглядит сломанным, а весят
        // они единицы килобайт. Своё фото — всегда, вместе с именем.
        let mut blobs: Vec<String> = Vec::new();
        if !me.avatar_id.is_empty() {
            blobs.push(me.avatar_id);
        }
        if parts.contacts {
            let contacts: Vec<Contact> = self
                .contacts
                .lock_safe()
                .list()
                .into_iter()
                .cloned()
                .collect();
            stats.contacts = contacts.len() as u64;
            for c in &contacts {
                if !c.avatar_id.is_empty() {
                    blobs.push(c.avatar_id.clone());
                }
            }
            w.json(&Rec::Contacts { items: contacts })?;
        }

        if parts.messages || parts.notes || parts.groups {
            let me = self.me_pub;
            let mut after = 0i64;
            loop {
                let page = self.messages.lock_safe().export_page(after, MSG_BATCH);
                if page.is_empty() {
                    break;
                }
                let mut batch = Vec::with_capacity(page.len());
                for (rowid, chat, scope, m) in page {
                    after = rowid;
                    let is_group = scope == 1;
                    let is_note = !is_group && chat.as_slice() == me.as_slice();
                    let wanted = if is_group {
                        parts.groups
                    } else if is_note {
                        parts.notes
                    } else {
                        parts.messages
                    };
                    if !wanted {
                        continue;
                    }
                    if is_group {
                        stats.group_messages += 1;
                    } else if is_note {
                        stats.notes += 1;
                    } else {
                        stats.messages += 1;
                    }
                    if parts.attachments {
                        if !m.blob_id.is_empty() {
                            blobs.push(m.blob_id.clone());
                        }
                        if !m.thumb_id.is_empty() {
                            blobs.push(m.thumb_id.clone());
                        }
                    }
                    batch.push(MsgRow {
                        chat: hex::encode(chat),
                        scope,
                        msg: m,
                    });
                }
                if !batch.is_empty() {
                    w.json(&Rec::Messages { items: batch })?;
                }
            }
        }

        // Реакции — по тому же выбору, что их переписка.
        if parts.messages || parts.notes || parts.groups {
            let me = self.me_pub;
            let items: Vec<ReactRow> = self
                .messages
                .lock_safe()
                .export_reactions()
                .into_iter()
                .filter(|r| {
                    if r.scope == 1 {
                        parts.groups
                    } else if r.chat.as_slice() == me.as_slice() {
                        parts.notes
                    } else {
                        parts.messages
                    }
                })
                .map(|r| ReactRow {
                    chat: hex::encode(&r.chat),
                    scope: r.scope,
                    msg_id: r.msg_id,
                    msg_sender: r.msg_sender,
                    who: r.who,
                    emoji: r.emoji,
                    at: r.at,
                })
                .collect();
            if !items.is_empty() {
                w.json(&Rec::Reactions { items })?;
            }
        }

        // Имена групп кладём вместе с их перепиской — это часть того же выбора.
        if parts.groups {
            let items: Vec<ChatRow> = self
                .messages
                .lock_safe()
                .chat_titles(1)
                .into_iter()
                .map(|(gid, name)| ChatRow {
                    chat: hex::encode(gid),
                    scope: 1,
                    name,
                })
                .collect();
            if !items.is_empty() {
                w.json(&Rec::Chats { items })?;
            }
        }

        if parts.calls {
            let rows: Vec<CallRow> = self
                .calls
                .lock_safe()
                .list(usize::MAX)
                .into_iter()
                .map(|c| CallRow {
                    peer: hex::encode(c.peer),
                    gid: hex::encode(&c.gid),
                    incoming: c.incoming,
                    outcome: c.outcome,
                    started: c.started,
                    duration: c.duration,
                    video: c.video,
                })
                .collect();
            stats.calls = rows.len() as u64;
            w.json(&Rec::Calls { items: rows })?;
        }

        let mut written = std::collections::HashSet::new();
        for id in blobs {
            if !written.insert(id.clone()) {
                continue;
            }
            let Ok(meta) = std::fs::metadata(self.blob_dir.join(&id)) else {
                continue;
            };
            if meta.len() > MAX_BLOB {
                stats.skipped_big += 1;
                continue;
            }
            let Some(bytes) = self.blob_read(&id) else {
                continue;
            };
            stats.attachment_bytes += bytes.len() as u64;
            stats.attachments += 1;
            w.blob(&id, &bytes)?;
        }

        w.finish()?;
        Ok(stats)
    }

    /// Восстановить из копии. Возвращает, сколько записей реально ДОБАВЛЕНО (уже
    /// известные не считаются) — иначе пользователь не отличил бы «восстановилось»
    /// от «импортировал тот же файл второй раз».
    pub fn backup_import(&self, path: &Path, password: Option<&str>) -> Result<Stats, BackupError> {
        let file = std::fs::File::open(path)?;
        let mut r = Reader::open(std::io::BufReader::new(file), &self.backup_key(), password)?;
        let mut stats = Stats::default();
        let me = self.me_pub;
        // Чья это копия. Заметки себе — чат с САМИМ СОБОЙ, то есть ключ автора; если
        // копию разворачивают на другой личности, старый ключ там ничей, и заметки
        // осели бы в чате с несуществующим собеседником. Поэтому переадресуем их на
        // текущее «я»: авторство заметки и так наше, менять нечего.
        let mut src_me: Option<[u8; 32]> = None;
        // Своё имя и фото из копии — применяются после обхода: фото-блоб идёт в конце.
        let mut profile: Option<(String, String, String)> = None;

        while let Some(frame) = r.next()? {
            match frame {
                Frame::Json(bytes) => {
                    let rec: Rec = serde_json::from_slice(&bytes)
                        .map_err(|e| BackupError::Parse(e.to_string()))?;
                    match rec {
                        Rec::Manifest { me: src, .. } => {
                            src_me = hex::decode(&src)
                                .ok()
                                .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok());
                        }
                        Rec::Profile {
                            name,
                            avatar_id,
                            avatar_mime,
                        } => profile = Some((name, avatar_id, avatar_mime)),
                        Rec::Contacts { items: list } => {
                            let mut added: Vec<[u8; 32]> = Vec::new();
                            {
                                let mut store = self.contacts.lock_safe();
                                for c in &list {
                                    if store.import_contact(c).unwrap_or(false) {
                                        stats.contacts += 1;
                                        added.extend(crate::parse_key(&c.key));
                                    }
                                }
                            }
                            // Восстановленный по 24 словам телефон — на новой эпохе:
                            // контактам из копии он сообщит новый адрес (шаг 7g).
                            self.announce_later(&added);
                        }
                        Rec::Messages { items: list } => {
                            let store = self.messages.lock_safe();
                            for row in &list {
                                let Ok(raw) = hex::decode(&row.chat) else {
                                    continue;
                                };
                                if row.scope == 1 {
                                    // Группа: идентификатор — gid, он одинаков у всех
                                    // участников и переадресовывать его некуда.
                                    if store
                                        .import_message(Chat::Group(&raw), &row.msg)
                                        .unwrap_or(false)
                                    {
                                        stats.group_messages += 1;
                                    }
                                    continue;
                                }
                                let Ok(peer) = <[u8; 32]>::try_from(raw.as_slice()) else {
                                    continue;
                                };
                                let is_note = peer == me || Some(peer) == src_me;
                                let peer = if is_note { me } else { peer };
                                if store
                                    .import_message(Chat::Dm(&peer), &row.msg)
                                    .unwrap_or(false)
                                {
                                    if is_note {
                                        stats.notes += 1;
                                    } else {
                                        stats.messages += 1;
                                    }
                                }
                            }
                        }
                        Rec::Chats { items: list } => {
                            let store = self.messages.lock_safe();
                            for row in &list {
                                let Ok(raw) = hex::decode(&row.chat) else {
                                    continue;
                                };
                                if row.scope != 1 {
                                    continue;
                                }
                                let _ = store.set_chat_title(Chat::Group(&raw), &row.name);
                            }
                        }
                        Rec::Reactions { items: list } => {
                            // Последнее слово по времени (`react`): импорт — слияние,
                            // более свежее на этом устройстве копия не перебьёт.
                            let store = self.messages.lock_safe();
                            for row in &list {
                                let Ok(raw) = hex::decode(&row.chat) else {
                                    continue;
                                };
                                let (r, s) = (row.msg_id, row.msg_sender.as_str());
                                let (who, e, at) = (row.who.as_str(), row.emoji.as_str(), row.at);
                                // Копия — чужой файл: эмодзи проверяем, как пришедший по сети.
                                if !e.is_empty() && !peerage_core::protocol::reaction_ok(e) {
                                    continue;
                                }
                                if row.scope == 1 {
                                    let _ = store.react(Chat::Group(&raw), r, s, who, e, at);
                                    continue;
                                }
                                let Ok(peer) = <[u8; 32]>::try_from(raw.as_slice()) else {
                                    continue;
                                };
                                // Заметки переадресуются на текущее «я», как и сами сообщения.
                                let is_note = peer == me || Some(peer) == src_me;
                                let peer = if is_note { me } else { peer };
                                let _ = store.react(Chat::Dm(&peer), r, s, who, e, at);
                            }
                            // Из копии — не новость: капсула «❤» не должна звать к старым реакциям.
                            let _ = store.mark_all_reactions_seen();
                        }
                        Rec::Calls { items: list } => {
                            let store = self.calls.lock_safe();
                            for row in &list {
                                let Ok(raw) = hex::decode(&row.peer) else {
                                    continue;
                                };
                                let Ok(peer) = <[u8; 32]>::try_from(raw.as_slice()) else {
                                    continue;
                                };
                                let rec = CallRecord {
                                    id: 0,
                                    peer,
                                    gid: hex::decode(&row.gid).unwrap_or_default(),
                                    incoming: row.incoming,
                                    outcome: row.outcome.clone(),
                                    started: row.started,
                                    duration: row.duration,
                                    video: row.video,
                                };
                                if store.import_row(&rec) {
                                    stats.calls += 1;
                                }
                            }
                        }
                    }
                }
                Frame::Blob { id, data } => {
                    // Контент-адресный: если байты уже есть, второй раз не пишем.
                    if self.blob_exists(&id) {
                        continue;
                    }
                    let Ok(raw) = hex::decode(&id) else { continue };
                    let Ok(hash) = <[u8; 32]>::try_from(raw.as_slice()) else {
                        continue;
                    };
                    // Адрес обязан быть хэшем байтов: иначе подложенная копия кладёт
                    // под чужой адрес что угодно, и мы раздаём это другим как тот блоб.
                    if peerage_blob::blob_id(&data) != hash {
                        log::warn!("backup: блоб {id} не совпал со своим хэшем — пропущен");
                        continue;
                    }
                    stats.attachment_bytes += data.len() as u64;
                    stats.attachments += 1;
                    self.store_blob(hash, &data);
                }
            }
        }
        if !r.complete() {
            return Err(BackupError::Truncated);
        }
        if let Some((name, avatar_id, avatar_mime)) = profile {
            self.apply_backup_profile(&name, &avatar_id, &avatar_mime);
        }
        // Контакты и чаты поменялись под ногами у UI — пусть перечитает список.
        // Отдельного события «восстановлено» не заводим: клиенту важен факт «данные
        // изменились», а не причина.
        push_event(self, dto::UiEvent::ContactsChanged);
        Ok(stats)
    }

    /// Своё имя и фото из копии — только если на устройстве их ещё нет: введённое
    /// человеком не затирается. Что-то применилось — профиль уходит контактам.
    fn apply_backup_profile(&self, name: &str, avatar_id: &str, avatar_mime: &str) {
        let mut m = self.load_profile_meta();
        let mut changed = false;
        if m.name.trim().is_empty() && !name.trim().is_empty() {
            m.name = name.trim().to_string();
            changed = true;
        }
        if m.avatar_id.is_empty() && !avatar_id.is_empty() && self.blob_exists(avatar_id) {
            m.avatar_id = avatar_id.to_string();
            m.avatar_mime = avatar_mime.to_string();
            changed = true;
        }
        if changed {
            self.save_profile_meta(&m);
            self.broadcast_profile();
        }
    }
}
