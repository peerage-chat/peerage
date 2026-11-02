//! Отчёты о прочтении и индикатор набора (2.10, шаг 7c беты).
//!
//! **По выбору и взаимно.** Оба выключены по умолчанию: метаданные не раскрываются,
//! пока человек сам этого не захотел (`docs/design/app-ui.md`). Выключил отчёты — не
//! шлёшь свои и не видишь чужих; то же для набора. Взаимность держится здесь, а не в
//! клиенте: чужую отметку при выключенных своих движок даже не записывает.
//!
//! **Только личные чаты.** В группе каждая отметка — MLS-сообщение, двигающее
//! ратчет (и `save_group`) у всех N участников, а эфемерный набор поверх
//! чувствительного к порядку MLS — лишний риск рассинхрона ради мелочи.
//!
//! Оба пакета — без id и Ack (бакет CONTROL, M5): потерянную отметку перекроет
//! следующая, а пропавший набор не стоит ретрая.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use peerage_core::store::Chat;
use serde::{Deserialize, Serialize};

use crate::{dto, gate_of, push_event, Engine, Gate, LockRecover};

/// Не чаще одного «печатаю» на собеседника за это время. Получатель гасит
/// индикатор сам чуть позже (клиент — 5 с), так что непрерывный набор не мигает.
const TYPING_EVERY: Duration = Duration::from_secs(3);

/// Переключатели «Профиль → Приватность». Не секрет; `privacy.json` в каталоге данных.
#[derive(Serialize, Deserialize, Default, Clone, Copy)]
pub struct Privacy {
    #[serde(default)]
    pub read_receipts: bool,
    #[serde(default)]
    pub typing: bool,
}

/// Состояние отчётов: переключатели и что уже отправлено.
pub(crate) struct Receipts {
    path: PathBuf,
    prefs: Privacy,
    /// Когда собеседнику ушёл последний «печатаю».
    typing_sent: HashMap<[u8; 32], Instant>,
    /// Последняя отметка «прочитано до» каждому — досылается, когда он вернётся в
    /// сеть: отметка без Ack, и прочитанное при нём офлайн иначе пропало бы.
    read_marks: HashMap<[u8; 32], u64>,
}

impl Receipts {
    pub(crate) fn load(dir: &Path) -> Self {
        let path = dir.join("privacy.json");
        let prefs = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Receipts {
            path,
            prefs,
            typing_sent: HashMap::new(),
            read_marks: HashMap::new(),
        }
    }

    fn save(&self) {
        if let Ok(v) = serde_json::to_vec(&self.prefs) {
            let _ = peerage_core::fsutil::write_atomic(&self.path, v);
        }
    }
}

impl Engine {
    /// Переключатели приватности: `{"read_receipts":bool,"typing":bool}`.
    pub fn privacy_json(&self) -> String {
        serde_json::to_string(&self.receipts.lock_safe().prefs).unwrap_or_default()
    }

    /// Отчёты о прочтении: вкл/выкл (взаимно — выключенные не видны и чужие).
    pub fn set_read_receipts(&self, on: bool) {
        let mut r = self.receipts.lock_safe();
        r.prefs.read_receipts = on;
        if !on {
            r.read_marks.clear();
        }
        r.save();
    }

    /// Индикатор набора: вкл/выкл (взаимно).
    pub fn set_typing_indicator(&self, on: bool) {
        let mut r = self.receipts.lock_safe();
        r.prefs.typing = on;
        r.save();
    }

    /// Личный чат прочитан до последнего входящего: отправить отметку, если отчёты
    /// включены. Клиент зовёт, когда переписка на экране (открыта, приложение на
    /// переднем плане) и когда в открытую приходит новое.
    pub fn mark_read(&self, peer: &[u8; 32]) {
        if !self.receipts.lock_safe().prefs.read_receipts
            || !matches!(gate_of(self, peer), Gate::Contact)
        {
            return;
        }
        let Some(upto) = self.messages.lock_safe().last_incoming_id(Chat::Dm(peer)) else {
            return;
        };
        self.receipts.lock_safe().read_marks.insert(*peer, upto);
        let _ = self.messenger.send_read(peer, upto);
    }

    /// Человек печатает в личном чате. Зовётся на каждое изменение поля — частоту
    /// режет движок. Офлайн-собеседнику не шлётся: ждать доставки набору незачем.
    pub fn typing(&self, peer: &[u8; 32]) {
        let now = Instant::now();
        {
            let mut r = self.receipts.lock_safe();
            if !r.prefs.typing
                || r.typing_sent
                    .get(peer)
                    .is_some_and(|t| now.duration_since(*t) < TYPING_EVERY)
            {
                return;
            }
            if !matches!(gate_of(self, peer), Gate::Contact) || !self.messenger.is_online(peer) {
                return;
            }
            r.typing_sent.insert(*peer, now);
        }
        let _ = self.messenger.send_typing(peer);
    }

    /// Собеседник вернулся в сеть — досылаем последнюю отметку о прочтении.
    pub(crate) fn resend_read_mark(&self, peer: &[u8; 32]) {
        let upto = {
            let r = self.receipts.lock_safe();
            if !r.prefs.read_receipts {
                return;
            }
            r.read_marks.get(peer).copied()
        };
        if let Some(upto) = upto {
            let _ = self.messenger.send_read(peer, upto);
        }
    }

    /// Собеседник прочитал наши сообщения до `upto`.
    pub(crate) fn on_read(&self, peer: &[u8; 32], upto: u64) {
        if !self.receipts.lock_safe().prefs.read_receipts
            || !matches!(gate_of(self, peer), Gate::Contact)
        {
            return;
        }
        let ids = self
            .messages
            .lock_safe()
            .mark_read(Chat::Dm(peer), upto)
            .unwrap_or_default();
        if !ids.is_empty() {
            push_event(
                self,
                dto::UiEvent::Read {
                    peer: hex::encode(peer),
                    ids,
                },
            );
        }
    }

    /// Собеседник печатает.
    pub(crate) fn on_typing(&self, peer: &[u8; 32]) {
        if !self.receipts.lock_safe().prefs.typing || !matches!(gate_of(self, peer), Gate::Contact)
        {
            return;
        }
        push_event(
            self,
            dto::UiEvent::Typing {
                peer: hex::encode(peer),
            },
        );
    }
}
