//! p2p-messenger — слой сессии: связывает транспорт Yggdrasil ([`p2p_ygg`]) и
//! протокол/крипто ядра ([`p2p_core`]) в рабочий зашифрованный канал сообщений.
//!
//! Обязанности:
//!   * приёмный цикл: читает датаграммы, расшифровывает, диспетчеризует;
//!   * анти-спуфинг: ключ отправителя в конверте обязан совпадать с источником
//!     транспорта (Yggdrasil src key);
//!   * рукопожатие `Hello` в обе стороны (нужно для шифр-сессии ironwood);
//!   * авто-`Ack` на входящий текст (статус «доставлено» ✓✓);
//!   * ответ `Pong` на `Ping`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2p_core::protocol::{decode, encode, Payload, ProtocolError};
use p2p_core::Identity;
use p2p_ygg::{Node, YggError};

/// Публичный ключ ed25519 пира — он же его идентификатор.
pub type PeerKey = [u8; 32];

/// События, доставляемые приложению из приёмного цикла.
#[derive(Debug, Clone)]
pub enum Event {
    /// С пиром установлена сессия (получен его `Hello`).
    Connected { peer: PeerKey },
    /// Входящее текстовое сообщение.
    Text { peer: PeerKey, id: u64, text: String },
    /// Пир подтвердил доставку нашего сообщения с данным id.
    Ack { peer: PeerKey, id: u64 },
}

#[derive(Debug, thiserror::Error)]
pub enum MessengerError {
    #[error("протокол: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("транспорт: {0}")]
    Transport(#[from] YggError),
    #[error("рукопожатие с пиром не завершилось за отведённое время")]
    HandshakeTimeout,
}

/// Разделяемое между API и приёмным потоком состояние.
struct Shared {
    identity: Identity,
    node: Node,
    connected: Mutex<HashSet<PeerKey>>,
    next_id: AtomicU64,
}

impl Shared {
    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn send(&self, peer: &PeerKey, payload: &Payload) -> Result<(), MessengerError> {
        let datagram = encode(&self.identity, peer, payload, Self::now())?;
        self.node.write(peer, &datagram)?;
        Ok(())
    }
}

/// Мессенджер: владеет транспортным узлом и идентичностью, ведёт приёмный цикл.
///
/// Сам по себе `Send + Sync` (поток событий отдаётся отдельно как `Receiver`),
/// поэтому `&Messenger` можно использовать из нескольких потоков.
pub struct Messenger {
    shared: Arc<Shared>,
    // TODO: грациозное завершение — сейчас приёмный поток живёт до остановки
    // узла (read() разблокируется ошибкой); процесс завершается — поток уходит.
    _rx_thread: thread::JoinHandle<()>,
}

impl Messenger {
    /// Запустить мессенджер поверх уже поднятого транспортного узла.
    /// Узел должен быть сконфигурирован (listen/add_peer) до передачи сюда.
    /// Возвращает мессенджер и канал входящих [`Event`].
    pub fn start(identity: Identity, node: Node) -> (Messenger, Receiver<Event>) {
        let shared = Arc::new(Shared {
            identity,
            node,
            connected: Mutex::new(HashSet::new()),
            next_id: AtomicU64::new(1),
        });
        let (tx, rx) = mpsc::channel();
        let s = shared.clone();
        let rx_thread = thread::spawn(move || receive_loop(s, tx));
        (
            Messenger {
                shared,
                _rx_thread: rx_thread,
            },
            rx,
        )
    }

    /// Наш публичный ключ.
    pub fn public_key(&self) -> PeerKey {
        self.shared.identity.public_key_bytes()
    }

    /// Адрес Yggdrasil нашего узла.
    pub fn address(&self) -> String {
        self.shared.node.address()
    }

    /// Установить сессию с пиром: шлём `Hello`, пока не получим его `Hello`
    /// (двунаправленный обмен поднимает шифр-сессию ironwood).
    pub fn connect(&self, peer: &PeerKey, timeout: Duration) -> Result<(), MessengerError> {
        let start = Instant::now();
        loop {
            self.shared.send(peer, &Payload::Hello)?;
            if self.shared.connected.lock().unwrap().contains(peer) {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(MessengerError::HandshakeTimeout);
            }
            thread::sleep(Duration::from_millis(150));
        }
    }

    /// Отправить текстовое сообщение пиру. Возвращает id сообщения (для
    /// сопоставления с будущим [`Event::Ack`]).
    pub fn send_text(&self, peer: &PeerKey, text: &str) -> Result<u64, MessengerError> {
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        self.shared
            .send(peer, &Payload::Text { id, text: text.to_string() })?;
        Ok(id)
    }
}

fn receive_loop(shared: Arc<Shared>, tx: Sender<Event>) {
    let mut buf = vec![0u8; 65535];
    loop {
        let (n, src) = match shared.node.read(&mut buf) {
            Ok(v) => v,
            Err(_) => return, // узел остановлен
        };
        let (from, payload) = match decode(&shared.identity, &buf[..n]) {
            Ok(v) => v,
            Err(_) => continue, // не наш конверт / не расшифровать
        };
        // Анти-спуфинг: ключ в конверте обязан совпадать с источником транспорта.
        if from != src {
            continue;
        }

        match payload {
            Payload::Hello => {
                let first = shared.connected.lock().unwrap().insert(src);
                if first {
                    let _ = tx.send(Event::Connected { peer: src });
                }
            }
            Payload::Text { id, text } => {
                let _ = tx.send(Event::Text { peer: src, id, text });
                let _ = shared.send(&src, &Payload::Ack { id }); // ✓✓ доставлено
            }
            Payload::Ack { id } => {
                let _ = tx.send(Event::Ack { peer: src, id });
            }
            Payload::Ping => {
                let _ = shared.send(&src, &Payload::Pong);
            }
            Payload::Pong => {}
        }
    }
}
