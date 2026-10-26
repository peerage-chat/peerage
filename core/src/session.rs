//! Сессии 1:1 v2 (ADR-012): рукопожатие XXhfs, транспорт, смена ключей.
//!
//! Чистая логика без сети и часов: время и случайность приходят снаружи, наружу
//! уходят готовые датаграммы. Мессенджер держит одну таблицу на узел.
//!
//! Ключи сессий живут только в памяти: перезапуск процесса — это просто новое
//! рукопожатие (модель WireGuard). Сессию заменяет новая каждые [`REKEY_AFTER`],
//! старая принимает ещё до [`REJECT_AFTER`] — запоздавшие и переставленные пакеты.
//!
//! Провод (после байта канала `CHAN_CHAT`), индексы — little-endian:
//!
//! ```text
//! msg1  sender_idx(4) | e(32) | e1(1184) | mac1(16)                    | случайная добивка
//! msg2  receiver_idx(4) | sender_idx(4) | e(32) | ekem1(1104) | s(48) | auth(112) | добивка
//! msg3  receiver_idx(4) | s(48) | auth(112)                            | добивка
//! data  receiver_idx(4) | counter(8) | AEAD(текст с паддингом бакета)
//! ```
//!
//! Открытого поля «тип» нет: получатель узнаёт пакет по своему индексу в первых
//! четырёх байтах, а msg1 — по размеру и `mac1`. msg1 и msg2 добиты до размера
//! прикладного пакета данных, msg3 — до управляющего (M5: по размеру рукопожатие
//! не отличить от текста и ack). `auth` — ключ личности и её подпись над
//! статическим ключом сессий: так статический ключ связан с личностью, а сама
//! личность едет только внутри шифрования.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use rand_core::{CryptoRng, RngCore};
use x25519_dalek::StaticSecret;
use zeroize::Zeroizing;

use crate::identity::{verify_session_static, Identity};
use crate::noise::{aead_open, aead_seal, HandshakeState, KEM_CT_LEN, KEM_PK_LEN, XX_HFS};
use crate::protocol::{APP_BUCKET, CONTROL_BUCKET};

pub type PeerKey = [u8; 32];

/// Возраст сессии, после которого отправка запускает новое рукопожатие.
pub const REKEY_AFTER: Duration = Duration::from_secs(120);
/// Возраст, после которого сессия не годится ни для отправки, ни для приёма.
pub const REJECT_AFTER: Duration = Duration::from_secs(180);
/// Сколько ждём ответа на рукопожатие (и подтверждения новой сессии).
pub const REKEY_TIMEOUT: Duration = Duration::from_secs(5);
/// Раньше этого новое рукопожатие с тем же собеседником не начинаем. Прежнее при
/// этом живо до [`REKEY_TIMEOUT`]: ответ на любое из них годится. Так потерянный
/// msg1/msg2 стоит секунду, а не пять, а к офлайн-собеседнику уходит не больше
/// одного msg1 в секунду (и не больше, чем его зовут `send`/`tick`).
const HANDSHAKE_RETRY: Duration = Duration::from_secs(1);
/// Шлём, а собеседник молчит столько — вероятно, он перезапустился и сессию
/// потерял: начинаем новую, не дожидаясь [`REKEY_AFTER`] (WireGuard: 10 + 5 с).
pub const UNANSWERED_AFTER: Duration = Duration::from_secs(15);
/// Сколько данные ждут рукопожатия. Надёжные сообщения дошлёт ретрай, а
/// устаревший маячок присутствия лучше не слать вовсе.
const QUEUE_TTL: Duration = Duration::from_secs(10);
const QUEUE_CAP: usize = 64;
/// Как часто инициатор повторяет msg3, пока сессия не подтверждена. Потерянный
/// msg3 иначе стоил бы целого нового рукопожатия, а ответчик ждёт его
/// [`REKEY_TIMEOUT`].
const MSG3_RESEND: Duration = Duration::from_secs(1);
/// Сколько незаконченных рукопожатий, где мы отвечаем, держим одновременно.
/// Предел памяти против чужого потока msg1; число самих msg1 режет бюджет пиров.
const MAX_RESPONDING: usize = 256;
const MAX_LIVE_PER_PEER: usize = 4;
/// Потолок счётчика сообщений сессии (как в WireGuard): дальше — только новая.
const REJECT_AFTER_MESSAGES: u64 = u64::MAX - (1 << 13);

const PROLOGUE: &[u8] = b"dev.p2p.dm-session.v2";
const TAG: usize = 16;
const AUTH_LEN: usize = 32 + 64;
const MAC_LEN: usize = 16;
const MSG1_LEN: usize = 4 + 32 + KEM_PK_LEN + MAC_LEN;
const MSG2_LEN: usize = 8 + 32 + KEM_CT_LEN + TAG + 32 + TAG + AUTH_LEN + TAG;
const MSG3_LEN: usize = 4 + 32 + TAG + AUTH_LEN + TAG;
/// Заголовок и тег пакета данных.
pub const DATA_OVERHEAD: usize = 4 + 8 + TAG;
/// Пакет данных прикладного бакета — и размер msg1/msg2.
pub const APP_PACKET: usize = DATA_OVERHEAD + APP_BUCKET;
/// Пакет данных управляющего бакета — и размер msg3.
pub const CONTROL_PACKET: usize = DATA_OVERHEAD + CONTROL_BUCKET;
const _: () = assert!(MSG1_LEN <= APP_PACKET && MSG2_LEN <= APP_PACKET && MSG3_LEN <= CONTROL_PACKET);

/// Ключ `mac1` для msg1, адресованного личности `identity`: посчитать его может
/// только тот, кто эту личность уже знает. Без верного `mac1` мы молчим, и новый
/// адрес нельзя «спросить, чей он» (иначе смена адреса из ADR-007 обесценилась бы).
fn mac1_key(identity: &PeerKey) -> [u8; 32] {
    blake3::derive_key("dev.p2p.session-mac1.v1", identity)
}

fn mac1(key: &[u8; 32], msg: &[u8]) -> [u8; MAC_LEN] {
    let mut out = [0u8; MAC_LEN];
    out.copy_from_slice(&blake3::keyed_hash(key, msg).as_bytes()[..MAC_LEN]);
    out
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn index_at(pkt: &[u8], at: usize) -> Option<u32> {
    let b = pkt.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Добить случайными байтами: нули в хвосте отличали бы рукопожатие от шифртекста.
fn pad_random<R: RngCore>(mut v: Vec<u8>, len: usize, rng: &mut R) -> Vec<u8> {
    let n = v.len();
    v.resize(len.max(n), 0);
    rng.fill_bytes(&mut v[n..]);
    v
}

/// Окно повторов (как в WireGuard, RFC 6479): принимаем каждый номер один раз и
/// не старше [`WINDOW`] от самого нового.
#[cfg(not(kani))]
const WINDOW: u64 = 2048;
/// Под Kani окно меньше: код не зависит от размера (годится любое кратное 64), а
/// цикл сдвига окна верификатор разворачивает целиком.
#[cfg(kani)]
const WINDOW: u64 = 128;

#[derive(Clone)]
struct ReplayWindow {
    /// Самый новый принятый номер + 1 (0 — ещё ничего не было).
    top: u64,
    bits: [u64; (WINDOW / 64) as usize],
}

impl ReplayWindow {
    fn new() -> Self {
        ReplayWindow { top: 0, bits: [0; (WINDOW / 64) as usize] }
    }

    fn slot(n: u64) -> (usize, u64) {
        let i = n % WINDOW;
        ((i / 64) as usize, 1 << (i % 64))
    }

    fn check(&self, n: u64) -> bool {
        if n >= REJECT_AFTER_MESSAGES {
            return false;
        }
        if n >= self.top {
            return true;
        }
        if self.top - n > WINDOW {
            return false;
        }
        let (w, b) = Self::slot(n);
        self.bits[w] & b == 0
    }

    /// Отметить номер принятым. Только после успешной расшифровки: иначе чужой
    /// мусор с большим номером сдвинул бы окно и отрезал настоящие пакеты.
    fn update(&mut self, n: u64) {
        if n >= self.top {
            if n + 1 - self.top >= WINDOW {
                self.bits = [0; (WINDOW / 64) as usize];
            } else {
                for i in self.top..=n {
                    let (w, b) = Self::slot(i);
                    self.bits[w] &= !b;
                }
            }
            self.top = n + 1;
        }
        let (w, b) = Self::slot(n);
        self.bits[w] |= b;
    }
}

/// Доказательства Kani для окна повторов: для ЛЮБЫХ номеров пакетов, а не для
/// выбранных в тестах. Ошибка здесь — либо повтор чужого пакета принят (атака),
/// либо настоящий пакет отвергнут (молча пропавшее сообщение).
#[cfg(kani)]
mod proofs {
    use super::*;

    /// Принять номер, как это делает сессия: сначала `check`, потом `update`.
    fn accept(w: &mut ReplayWindow, n: u64) -> bool {
        if !w.check(n) {
            return false;
        }
        w.update(n);
        true
    }

    /// Два произвольных пакета подряд: один номер не принимается дважды, а после
    /// приёма повтор любого из принятых отвергается. (Вариант с тремя пакетами не
    /// укладывается в 15 минут верификатора, а нового свойства не добавляет.)
    #[kani::proof]
    #[kani::unwind(130)]
    fn no_number_accepted_twice() {
        let mut w = ReplayWindow::new();
        let (a, b): (u64, u64) = (kani::any(), kani::any());
        let got_a = accept(&mut w, a);
        let got_b = accept(&mut w, b);
        assert!(!(got_a && got_b && a == b));
        if got_a {
            assert!(!w.check(a));
        }
        if got_b {
            assert!(!w.check(b));
        }
    }

    /// Нет ложных отказов: номер, который ещё не приходил и лежит внутри окна (или
    /// новее всех), принимается — переупорядочивание сети не теряет пакеты.
    #[kani::proof]
    #[kani::unwind(130)]
    fn fresh_number_in_window_is_accepted() {
        let mut w = ReplayWindow::new();
        let (a, b, c): (u64, u64, u64) = (kani::any(), kani::any(), kani::any());
        let got_a = accept(&mut w, a);
        let got_b = accept(&mut w, b);
        kani::assume(c < REJECT_AFTER_MESSAGES);
        kani::assume(!(got_a && c == a) && !(got_b && c == b));
        kani::assume(c >= w.top || w.top - c <= WINDOW);
        assert!(w.check(c));
    }
}

#[derive(Clone)]
struct Session {
    peer: PeerKey,
    /// Индекс сессии у собеседника — им начинается каждый наш пакет.
    remote: u32,
    send: Zeroizing<[u8; 32]>,
    recv: Zeroizing<[u8; 32]>,
    created: Instant,
    next: u64,
    window: ReplayWindow,
    /// Медиа-канал (SEC-4): свои ключи, счётчик и окно повторов.
    media_send: Zeroizing<[u8; 32]>,
    media_recv: Zeroizing<[u8; 32]>,
    media_next: u64,
    media_window: ReplayWindow,
    /// Собеседник точно держит эту сессию: мы от него по ней что-то приняли (или
    /// мы отвечали, и его msg3 это доказал). Неподтверждённой шлём, только если
    /// подтверждённой нет: потерянный msg3 иначе молча съел бы всё отправленное.
    confirmed: bool,
    /// Наш msg3 (мы инициатор) и когда он ушёл последний раз — повторяем, пока
    /// сессия не подтверждена.
    msg3: Option<(Vec<u8>, Instant)>,
}

impl Session {
    fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let n = self.next;
        self.next += 1;
        let mut out = Vec::with_capacity(DATA_OVERHEAD + plaintext.len());
        out.extend_from_slice(&self.remote.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        let ct = aead_seal(&self.send, n, &out, plaintext);
        out.extend_from_slice(&ct);
        out
    }

    fn usable(&self, now: Instant) -> bool {
        now.duration_since(self.created) < REJECT_AFTER && self.next < REJECT_AFTER_MESSAGES
    }
}

#[derive(Clone)]
enum Slot {
    /// Мы начали рукопожатие с `peer` и ждём msg2.
    Initiating { peer: PeerKey, hs: HandshakeState, started: Instant },
    /// Мы ответили на msg1 и ждём msg3; кто на том конце — ещё не знаем.
    Responding { hs: HandshakeState, remote: u32, started: Instant },
    Live(Session),
}

#[derive(Default, Clone)]
struct Peer {
    /// Индексы живых сессий, старые первыми.
    live: Vec<u32>,
    /// Последнее начатое нами рукопожатие и когда.
    initiating: Option<(u32, Instant)>,
    /// Открытый текст, ждущий рукопожатия.
    queue: VecDeque<(Instant, Vec<u8>)>,
    /// Отправленное по ещё не подтверждённой сессии: если она не подтвердится,
    /// это уйдёт снова — с повтором msg3 или новым рукопожатием.
    unconfirmed: VecDeque<(Instant, Vec<u8>)>,
    /// С какого момента шлём, ничего не получая в ответ.
    unanswered_since: Option<Instant>,
}

/// Что вышло из принятого пакета.
#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    /// Данные от подлинного собеседника `peer` (личность) — текст с паддингом.
    Data { peer: PeerKey, plaintext: Vec<u8> },
    /// Пустой пакет подтверждения сессии: собеседник жив, нести нечего.
    Keepalive { peer: PeerKey },
    /// Ответ на рукопожатие (msg2) — отправить туда, откуда пришёл msg1.
    Reply(Vec<u8>),
    /// Рукопожатие с `peer` завершено; `out` отправить на его адрес.
    Established { peer: PeerKey, out: Vec<Vec<u8>> },
    /// Индекс нам не знаком: например, мы перезапустились, а собеседник ещё шлёт
    /// по старой сессии. Мессенджер может сам начать рукопожатие со своим.
    UnknownIndex,
    /// Подделка, повтор, обрубок или просроченная сессия.
    Rejected,
}

/// Все сессии 1:1 одного узла. `Clone` — для фаззинга (снимок состояния на
/// каждом шаге рукопожатия).
#[derive(Clone)]
pub struct Sessions {
    s: StaticSecret,
    /// Своя личность и подпись над статическим ключом — payload msg2/msg3.
    auth: [u8; AUTH_LEN],
    mac1_mine: [u8; 32],
    slots: HashMap<u32, Slot>,
    peers: HashMap<PeerKey, Peer>,
    /// Индексы ответных рукопожатий по возрасту (для вытеснения).
    responding: VecDeque<u32>,
    last_sweep: Option<Instant>,
}

impl Sessions {
    pub fn new(me: &Identity) -> Self {
        let s = StaticSecret::from(me.session_static_secret());
        let static_pub = x25519_dalek::PublicKey::from(&s).to_bytes();
        let identity = me.public_key_bytes();
        let mut auth = [0u8; AUTH_LEN];
        auth[..32].copy_from_slice(&identity);
        auth[32..].copy_from_slice(&me.sign_session_static(&static_pub));
        Sessions {
            s,
            auth,
            mac1_mine: mac1_key(&identity),
            slots: HashMap::new(),
            peers: HashMap::new(),
            responding: VecDeque::new(),
            last_sweep: None,
        }
    }

    /// Есть ли сессия, по которой прямо сейчас можно слать `peer`.
    pub fn has_session(&self, peer: &PeerKey, now: Instant) -> bool {
        self.sending_session(peer, now).is_some()
    }

    /// Зашифровать `plaintext` (уже с паддингом бакета) для `peer`. Вернуть
    /// датаграммы на адрес `peer`: пакет данных и/или msg1 нового рукопожатия.
    ///
    /// Без живой сессии `hold = true` кладёт текст в очередь — он уйдёт сразу за
    /// рукопожатием (разовые пакеты: маячок, манифест, ack). `hold = false` —
    /// только начать рукопожатие, а текст выбросить: так шлются надёжные
    /// сообщения, их дошлёт сам мессенджер. Держать их и здесь значило бы
    /// доставить и то, что он уже снял (положенная трубка, истёкший звонок).
    pub fn send<R: RngCore + CryptoRng>(
        &mut self,
        peer: &PeerKey,
        plaintext: &[u8],
        hold: bool,
        now: Instant,
        rng: &mut R,
    ) -> Vec<Vec<u8>> {
        self.sweep(now);
        let mut out = Vec::new();
        let need_handshake = match self.sending_session(peer, now) {
            Some(idx) => {
                let Some(Slot::Live(s)) = self.slots.get_mut(&idx) else { return out };
                out.push(s.seal(plaintext));
                let p = self.peers.entry(*peer).or_default();
                if !s.confirmed {
                    push_bounded(&mut p.unconfirmed, (now, plaintext.to_vec()));
                }
                let since = *p.unanswered_since.get_or_insert(now);
                let patience = if s.confirmed { UNANSWERED_AFTER } else { REKEY_TIMEOUT };
                now.duration_since(s.created) >= REKEY_AFTER || now.duration_since(since) >= patience
            }
            None => {
                if hold {
                    let p = self.peers.entry(*peer).or_default();
                    push_bounded(&mut p.queue, (now, plaintext.to_vec()));
                }
                true
            }
        };
        if need_handshake && !self.handshaking(peer, now) {
            if let Some(m1) = self.initiate(peer, now, rng) {
                out.push(m1);
            }
        }
        out
    }

    /// Зашифровать медиа-кадр для `peer` (SEC-4): ключом медиа текущей сессии,
    /// тем же форматом, что пакет данных. Без живой сессии — `None`: медиа не
    /// ждёт (опоздавший кадр бесполезен), рукопожатие начинает мессенджер.
    pub fn seal_media(&mut self, peer: &PeerKey, plaintext: &[u8], now: Instant) -> Option<Vec<u8>> {
        let idx = self.sending_session(peer, now)?;
        let Some(Slot::Live(s)) = self.slots.get_mut(&idx) else { return None };
        if s.media_next >= REJECT_AFTER_MESSAGES {
            return None;
        }
        let n = s.media_next;
        s.media_next += 1;
        let mut out = Vec::with_capacity(DATA_OVERHEAD + plaintext.len());
        out.extend_from_slice(&s.remote.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        let ct = aead_seal(&s.media_send, n, &out, plaintext);
        out.extend_from_slice(&ct);
        Some(out)
    }

    /// Расшифровать медиа-пакет: (кто прислал — личность из рукопожатия, кадр).
    /// Подделка, повтор и пакет без живой сессии — `None`.
    pub fn open_media(&mut self, pkt: &[u8], now: Instant) -> Option<(PeerKey, Vec<u8>)> {
        let idx = index_at(pkt, 0)?;
        let Some(Slot::Live(s)) = self.slots.get_mut(&idx) else { return None };
        if pkt.len() < DATA_OVERHEAD || now.duration_since(s.created) >= REJECT_AFTER {
            return None;
        }
        let mut c = [0u8; 8];
        c.copy_from_slice(&pkt[4..12]);
        let counter = u64::from_le_bytes(c);
        if !s.media_window.check(counter) {
            return None;
        }
        let plaintext = aead_open(&s.media_recv, counter, &pkt[..12], &pkt[12..]).ok()?;
        s.media_window.update(counter);
        Some((s.peer, plaintext))
    }

    /// Таймеры рукопожатий (зовётся периодически, раз в полсекунды): повторить
    /// msg3 неподтверждённых сессий, бросить те, что так и не подтвердились, и
    /// начать рукопожатие с теми, для кого ждут данные. Без этого первый пакет на
    /// холодной сессии зависел бы от того, позовёт ли кто-нибудь `send` ещё раз, —
    /// а маячки, манифесты блобов и профиль шлются один раз. Вернуть
    /// (кому, датаграмма) — слать на адрес этого собеседника.
    pub fn tick<R: RngCore + CryptoRng>(&mut self, now: Instant, rng: &mut R) -> Vec<(PeerKey, Vec<u8>)> {
        self.sweep(now);
        let mut out = Vec::new();
        let mut dead = Vec::new();
        for (idx, slot) in self.slots.iter_mut() {
            let Slot::Live(s) = slot else { continue };
            let Some((m3, sent)) = &mut s.msg3 else { continue };
            if now.duration_since(s.created) >= REKEY_TIMEOUT {
                dead.push((*idx, s.peer));
            } else if now.duration_since(*sent) >= MSG3_RESEND {
                *sent = now;
                out.push((s.peer, m3.clone()));
                if let Some(p) = self.peers.get(&s.peer) {
                    let resend: Vec<Vec<u8>> = p.unconfirmed.iter().map(|(_, pt)| pt.clone()).collect();
                    for pt in resend {
                        out.push((s.peer, s.seal(&pt)));
                    }
                }
            }
        }
        // Сессия так и не подтвердилась (потерялись msg3 или ответ на него):
        // собеседник её, скорее всего, не держит. Всё, что ушло по ней, — снова в
        // очередь, впереди более нового.
        for (idx, peer) in dead {
            self.slots.remove(&idx);
            if let Some(p) = self.peers.get_mut(&peer) {
                p.live.retain(|i| *i != idx);
                while let Some(e) = p.unconfirmed.pop_back() {
                    p.queue.push_front(e);
                }
                p.queue.truncate(QUEUE_CAP);
            }
        }
        // Таймер повторяет рукопожатие реже, чем `send`: за данными, которые
        // ждут, никто больше не придёт, а собеседник может быть просто офлайн —
        // msg1 в секунду (4 КБ) ему все 10 с жизни очереди был бы расточительством.
        let waiting: Vec<PeerKey> = self
            .peers
            .iter()
            .filter(|(k, p)| !p.queue.is_empty() && !self.started_within(k, now, REKEY_TIMEOUT))
            .map(|(k, _)| *k)
            .collect();
        for peer in waiting {
            match self.sending_session(&peer, now) {
                Some(idx) => out.extend(self.flush(peer, idx, now).into_iter().map(|d| (peer, d))),
                None => {
                    if let Some(m1) = self.initiate(&peer, now, rng) {
                        out.push((peer, m1));
                    }
                }
            }
        }
        out
    }

    /// Разобрать пакет чат-канала.
    pub fn receive<R: RngCore + CryptoRng>(&mut self, pkt: &[u8], now: Instant, rng: &mut R) -> Received {
        self.sweep(now);
        let Some(idx) = index_at(pkt, 0) else { return Received::Rejected };
        match self.slots.get(&idx) {
            Some(Slot::Live(_)) => self.recv_data(idx, pkt, now),
            Some(Slot::Initiating { .. }) => self.recv_msg2(idx, pkt, now, rng),
            Some(Slot::Responding { .. }) => self.recv_msg3(idx, pkt, now),
            None if pkt.len() == APP_PACKET => self.recv_msg1(pkt, now, rng),
            None => Received::UnknownIndex,
        }
    }

    /// Недавно начатое рукопожатие ещё ждёт ответа — новое пока не начинаем.
    fn handshaking(&self, peer: &PeerKey, now: Instant) -> bool {
        self.started_within(peer, now, HANDSHAKE_RETRY)
    }

    fn started_within(&self, peer: &PeerKey, now: Instant, d: Duration) -> bool {
        self.peers.get(peer).and_then(|p| p.initiating).is_some_and(|(_, t)| now.duration_since(t) < d)
    }

    /// Новейшая подтверждённая живая сессия, иначе новейшая живая.
    fn sending_session(&self, peer: &PeerKey, now: Instant) -> Option<u32> {
        let p = self.peers.get(peer)?;
        let live = |i: &&u32| matches!(self.slots.get(i), Some(Slot::Live(s)) if s.usable(now));
        let confirmed = |i: &&u32| matches!(self.slots.get(i), Some(Slot::Live(s)) if s.confirmed);
        let newest_confirmed = p.live.iter().rev().filter(live).find(confirmed);
        newest_confirmed.or_else(|| p.live.iter().rev().find(live)).copied()
    }

    fn new_index<R: RngCore>(&self, rng: &mut R) -> u32 {
        loop {
            let i = rng.next_u32();
            if !self.slots.contains_key(&i) {
                return i;
            }
        }
    }

    fn initiate<R: RngCore + CryptoRng>(&mut self, peer: &PeerKey, now: Instant, rng: &mut R) -> Option<Vec<u8>> {
        let idx = self.new_index(rng);
        let mut hs = HandshakeState::new(&XX_HFS, true, PROLOGUE, self.s.clone());
        let noise = hs.write_message(&[], rng).ok()?;
        let mut m = Vec::with_capacity(APP_PACKET);
        m.extend_from_slice(&idx.to_le_bytes());
        m.extend_from_slice(&noise);
        let mac = mac1(&mac1_key(peer), &m);
        m.extend_from_slice(&mac);
        self.slots.insert(idx, Slot::Initiating { peer: *peer, hs, started: now });
        self.peers.entry(*peer).or_default().initiating = Some((idx, now));
        Some(pad_random(m, APP_PACKET, rng))
    }

    fn recv_msg1<R: RngCore + CryptoRng>(&mut self, pkt: &[u8], now: Instant, rng: &mut R) -> Received {
        let (body, mac) = pkt[..MSG1_LEN].split_at(MSG1_LEN - MAC_LEN);
        if !ct_eq(&mac1(&self.mac1_mine, body), mac) {
            // Не нам или не от того, кто нас знает. Тем же размером бывает и
            // пакет данных потерянной сессии — отсюда «незнакомый индекс».
            return Received::UnknownIndex;
        }
        let Some(remote) = index_at(body, 0) else { return Received::Rejected };
        let mut hs = HandshakeState::new(&XX_HFS, false, PROLOGUE, self.s.clone());
        if hs.read_message(&body[4..]).is_err() {
            return Received::Rejected;
        }
        let Ok(noise) = hs.write_message(&self.auth, rng) else { return Received::Rejected };
        let idx = self.new_index(rng);
        let mut m = Vec::with_capacity(APP_PACKET);
        m.extend_from_slice(&remote.to_le_bytes());
        m.extend_from_slice(&idx.to_le_bytes());
        m.extend_from_slice(&noise);
        self.slots.insert(idx, Slot::Responding { hs, remote, started: now });
        self.responding.push_back(idx);
        while self.responding.len() > MAX_RESPONDING {
            if let Some(old) = self.responding.pop_front() {
                self.slots.remove(&old);
            }
        }
        Received::Reply(pad_random(m, APP_PACKET, rng))
    }

    fn recv_msg2<R: RngCore + CryptoRng>(&mut self, idx: u32, pkt: &[u8], now: Instant, rng: &mut R) -> Received {
        let Some(Slot::Initiating { peer, hs, .. }) = self.slots.get(&idx) else { return Received::Rejected };
        if pkt.len() < MSG2_LEN {
            return Received::Rejected;
        }
        // Читаем копию: подделка с подсмотренным индексом не должна сорвать
        // настоящее рукопожатие, а неудачное чтение портит состояние Noise.
        let (peer, mut hs) = (*peer, hs.clone());
        let Some(remote) = index_at(pkt, 4) else { return Received::Rejected };
        let Ok(auth) = hs.read_message(&pkt[8..MSG2_LEN]) else { return Received::Rejected };
        // Отвечать обязан именно тот, кому мы звонили.
        if check_auth(&auth, &hs, Some(&peer)).is_none() {
            return Received::Rejected;
        }
        let Ok(noise) = hs.write_message(&self.auth, rng) else { return Received::Rejected };
        let Ok(keys) = hs.split() else { return Received::Rejected };
        let mut m3 = Vec::with_capacity(CONTROL_PACKET);
        m3.extend_from_slice(&remote.to_le_bytes());
        m3.extend_from_slice(&noise);
        let m3 = pad_random(m3, CONTROL_PACKET, rng);
        let mut out = vec![m3.clone()];
        let session = Session {
            peer,
            remote,
            send: keys.send,
            recv: keys.recv,
            created: now,
            next: 0,
            window: ReplayWindow::new(),
            media_send: keys.media_send,
            media_recv: keys.media_recv,
            media_next: 0,
            media_window: ReplayWindow::new(),
            confirmed: false,
            msg3: Some((m3, now)),
        };
        self.slots.insert(idx, Slot::Live(session));
        let p = self.peers.entry(peer).or_default();
        if p.initiating.is_some_and(|(i, _)| i == idx) {
            p.initiating = None;
        }
        out.extend(self.adopt(peer, idx, now));
        Received::Established { peer, out }
    }

    fn recv_msg3(&mut self, idx: u32, pkt: &[u8], now: Instant) -> Received {
        let Some(Slot::Responding { hs, remote, .. }) = self.slots.get(&idx) else { return Received::Rejected };
        if pkt.len() < MSG3_LEN {
            return Received::Rejected;
        }
        let (mut hs, remote) = (hs.clone(), *remote);
        let Ok(auth) = hs.read_message(&pkt[4..MSG3_LEN]) else { return Received::Rejected };
        let Some(peer) = check_auth(&auth, &hs, None) else { return Received::Rejected };
        let Ok(keys) = hs.split() else { return Received::Rejected };
        self.responding.retain(|i| *i != idx);
        let session = Session {
            peer,
            remote,
            send: keys.send,
            recv: keys.recv,
            created: now,
            next: 0,
            window: ReplayWindow::new(),
            media_send: keys.media_send,
            media_recv: keys.media_recv,
            media_next: 0,
            media_window: ReplayWindow::new(),
            confirmed: true,
            msg3: None,
        };
        self.slots.insert(idx, Slot::Live(session));
        let mut out = self.adopt(peer, idx, now);
        if out.is_empty() {
            // Подтвердить сессию инициатору сразу, даже если сказать нечего:
            // пока он не получил от нас ни пакета, он шлёт по старой.
            let keepalive = crate::pad::pad(&[], CONTROL_BUCKET);
            if let Some(Slot::Live(s)) = self.slots.get_mut(&idx) {
                out.push(s.seal(&keepalive));
            }
        }
        Received::Established { peer, out }
    }

    fn recv_data(&mut self, idx: u32, pkt: &[u8], now: Instant) -> Received {
        let Some(Slot::Live(s)) = self.slots.get_mut(&idx) else { return Received::Rejected };
        if pkt.len() < DATA_OVERHEAD || now.duration_since(s.created) >= REJECT_AFTER {
            return Received::Rejected;
        }
        let mut c = [0u8; 8];
        c.copy_from_slice(&pkt[4..12]);
        let counter = u64::from_le_bytes(c);
        if !s.window.check(counter) {
            return Received::Rejected;
        }
        let Ok(plaintext) = aead_open(&s.recv, counter, &pkt[..12], &pkt[12..]) else {
            return Received::Rejected;
        };
        s.window.update(counter);
        let newly_confirmed = !s.confirmed;
        s.confirmed = true;
        s.msg3 = None;
        let peer = s.peer;
        if let Some(p) = self.peers.get_mut(&peer) {
            p.unanswered_since = None;
            if newly_confirmed {
                p.unconfirmed.clear();
            }
        }
        if crate::pad::unpad(&plaintext).is_some_and(|c| c.is_empty()) {
            return Received::Keepalive { peer };
        }
        Received::Data { peer, plaintext }
    }

    /// Новая живая сессия `idx` с `peer`: записать, лишние старые выбросить,
    /// очередь ожидания зашифровать по ней.
    fn adopt(&mut self, peer: PeerKey, idx: u32, now: Instant) -> Vec<Vec<u8>> {
        let p = self.peers.entry(peer).or_default();
        p.unanswered_since = None;
        p.live.push(idx);
        while p.live.len() > MAX_LIVE_PER_PEER {
            let old = p.live.remove(0);
            self.slots.remove(&old);
        }
        self.flush(peer, idx, now)
    }

    /// Зашифровать очередь ожидания `peer` по сессии `idx`.
    fn flush(&mut self, peer: PeerKey, idx: u32, now: Instant) -> Vec<Vec<u8>> {
        let Some(p) = self.peers.get_mut(&peer) else { return Vec::new() };
        let Some(Slot::Live(s)) = self.slots.get_mut(&idx) else { return Vec::new() };
        let mut out = Vec::new();
        while let Some((t, pt)) = p.queue.pop_front() {
            if now.duration_since(t) >= QUEUE_TTL {
                continue;
            }
            out.push(s.seal(&pt));
            if !s.confirmed {
                push_bounded(&mut p.unconfirmed, (t, pt));
            }
        }
        out
    }

    /// Выбросить просроченное. Дёшево и не чаще раза в секунду.
    fn sweep(&mut self, now: Instant) {
        if self.last_sweep.is_some_and(|t| now.duration_since(t) < Duration::from_secs(1)) {
            return;
        }
        self.last_sweep = Some(now);
        self.slots.retain(|_, s| match s {
            Slot::Initiating { started, .. } | Slot::Responding { started, .. } => {
                now.duration_since(*started) < REKEY_TIMEOUT
            }
            Slot::Live(s) => now.duration_since(s.created) < REJECT_AFTER,
        });
        let slots = &self.slots;
        self.responding.retain(|i| slots.contains_key(i));
        self.peers.retain(|_, p| {
            p.live.retain(|i| slots.contains_key(i));
            if p.initiating.is_some_and(|(i, _)| !slots.contains_key(&i)) {
                p.initiating = None;
            }
            p.queue.retain(|(t, _)| now.duration_since(*t) < QUEUE_TTL);
            p.unconfirmed.retain(|(t, _)| now.duration_since(*t) < QUEUE_TTL);
            !p.live.is_empty() || p.initiating.is_some() || !p.queue.is_empty()
        });
    }
}

fn push_bounded(q: &mut VecDeque<(Instant, Vec<u8>)>, e: (Instant, Vec<u8>)) {
    if q.len() >= QUEUE_CAP {
        q.pop_front();
    }
    q.push_back(e);
}

/// Проверить `auth` собеседника: подпись его личности над статическим ключом,
/// который он только что доказал рукопожатием. `expect` — кого мы ждём.
fn check_auth(auth: &[u8], hs: &HandshakeState, expect: Option<&PeerKey>) -> Option<PeerKey> {
    if auth.len() != AUTH_LEN {
        return None;
    }
    let mut id = [0u8; 32];
    id.copy_from_slice(&auth[..32]);
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&auth[32..]);
    if expect.is_some_and(|e| *e != id) {
        return None;
    }
    let rs = hs.remote_static()?;
    verify_session_static(&id, &rs, &sig).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    struct Node {
        id: PeerKey,
        s: Sessions,
    }

    fn node() -> Node {
        let i = Identity::generate();
        Node { id: i.public_key_bytes(), s: Sessions::new(&i) }
    }

    fn text(s: &str) -> Vec<u8> {
        crate::pad::pad(s.as_bytes(), APP_BUCKET)
    }

    fn unpadded(p: &[u8]) -> String {
        String::from_utf8(crate::pad::unpad(p).unwrap()).unwrap()
    }

    /// Прогнать полное рукопожатие: `a` шлёт `b` первое сообщение. Вернуть, что
    /// `b` получил данными.
    fn handshake(a: &mut Node, b: &mut Node, first: &str, now: Instant) -> Vec<String> {
        let out = a.s.send(&b.id, &text(first), true, now, &mut OsRng);
        assert_eq!(out.len(), 1, "без сессии уходит только msg1");
        assert_eq!(out[0].len(), APP_PACKET, "msg1 размером с прикладной пакет");
        let Received::Reply(m2) = b.s.receive(&out[0], now, &mut OsRng) else { panic!("msg2") };
        assert_eq!(m2.len(), APP_PACKET);
        let Received::Established { peer, out } = a.s.receive(&m2, now, &mut OsRng) else { panic!("msg3") };
        assert_eq!(peer, b.id);
        assert_eq!(out[0].len(), CONTROL_PACKET, "msg3 размером с управляющий пакет");
        let mut got = Vec::new();
        for (i, pkt) in out.iter().enumerate() {
            match b.s.receive(pkt, now, &mut OsRng) {
                Received::Established { peer, out: back } => {
                    assert_eq!(i, 0);
                    assert_eq!(peer, a.id);
                    for p in back {
                        assert!(matches!(a.s.receive(&p, now, &mut OsRng), Received::Keepalive { .. } | Received::Data { .. }));
                    }
                }
                Received::Data { peer, plaintext } => {
                    assert_eq!(peer, a.id);
                    got.push(unpadded(&plaintext));
                }
                r => panic!("неожиданно: {r:?}"),
            }
        }
        got
    }

    fn deliver(from: &mut Node, to: &mut Node, msg: &str, now: Instant) -> Vec<Received> {
        from.s
            .send(&to.id, &text(msg), true, now, &mut OsRng)
            .iter()
            .map(|p| to.s.receive(p, now, &mut OsRng))
            .collect()
    }

    #[test]
    fn handshake_then_data_both_ways() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        assert_eq!(handshake(&mut a, &mut b, "привет", t), ["привет"], "очередь ушла вслед за msg3");
        assert!(a.s.has_session(&b.id, t) && b.s.has_session(&a.id, t));
        let r = deliver(&mut b, &mut a, "и тебе", t);
        assert!(matches!(&r[..], [Received::Data { peer, plaintext }] if *peer == b.id && unpadded(plaintext) == "и тебе"));
        let r = deliver(&mut a, &mut b, "ещё", t);
        assert!(matches!(&r[..], [Received::Data { .. }]), "{r:?}");
    }

    /// Медиа (SEC-4): кадр доходит, отправитель — личность из рукопожатия;
    /// повтор, подделка и пакет чата на месте медиа не проходят; без сессии
    /// медиа не шифруется вовсе.
    #[test]
    fn media_is_sealed_with_its_own_keys() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        assert!(a.s.seal_media(&b.id, b"frame", t).is_none(), "без сессии медиа не ждёт");
        handshake(&mut a, &mut b, "x", t);
        let m = a.s.seal_media(&b.id, b"frame", t).unwrap();
        assert_eq!(m.len(), DATA_OVERHEAD + 5);
        assert_eq!(b.s.open_media(&m, t), Some((a.id, b"frame".to_vec())));
        assert_eq!(b.s.open_media(&m, t), None, "повтор");
        let mut bad = a.s.seal_media(&b.id, b"frame", t).unwrap();
        bad[DATA_OVERHEAD] ^= 1;
        assert_eq!(b.s.open_media(&bad, t), None, "подделка");
        // Пакет чата по ключу медиа не открывается — и наоборот.
        let chat = a.s.send(&b.id, &text("чат"), true, t, &mut OsRng).remove(0);
        assert_eq!(b.s.open_media(&chat, t), None);
        let media = a.s.seal_media(&b.id, &text("кадр"), t).unwrap();
        assert_eq!(b.s.receive(&media, t, &mut OsRng), Received::Rejected);
        // В обратную сторону — свой ключ.
        let back = b.s.seal_media(&a.id, b"answer", t).unwrap();
        assert_eq!(a.s.open_media(&back, t), Some((b.id, b"answer".to_vec())));
    }

    #[test]
    fn data_packets_have_bucket_sizes() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        handshake(&mut a, &mut b, "x", t);
        let app = a.s.send(&b.id, &text("да"), true, t, &mut OsRng);
        let ctl = a.s.send(&b.id, &crate::pad::pad(b"ack", CONTROL_BUCKET), true, t, &mut OsRng);
        assert_eq!(app[0].len(), APP_PACKET);
        assert_eq!(ctl[0].len(), CONTROL_PACKET);
    }

    #[test]
    fn replay_is_rejected_reorder_is_not() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        handshake(&mut a, &mut b, "x", t);
        let p1 = a.s.send(&b.id, &text("1"), true, t, &mut OsRng).remove(0);
        let p2 = a.s.send(&b.id, &text("2"), true, t, &mut OsRng).remove(0);
        assert!(matches!(b.s.receive(&p2, t, &mut OsRng), Received::Data { .. }));
        assert!(matches!(b.s.receive(&p1, t, &mut OsRng), Received::Data { .. }), "переставленный принят");
        assert_eq!(b.s.receive(&p1, t, &mut OsRng), Received::Rejected, "повтор отброшен");
        assert_eq!(b.s.receive(&p2, t, &mut OsRng), Received::Rejected);
        let mut bad = a.s.send(&b.id, &text("3"), true, t, &mut OsRng).remove(0);
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert_eq!(b.s.receive(&bad, t, &mut OsRng), Received::Rejected);
    }

    #[test]
    fn replay_window_edges() {
        let mut w = ReplayWindow::new();
        assert!(w.check(0));
        w.update(0);
        assert!(!w.check(0));
        w.update(5000);
        assert!(!w.check(5000 - WINDOW), "старше окна");
        assert!(w.check(5000 - WINDOW + 1));
        assert!(w.check(4999));
        w.update(4999);
        assert!(!w.check(4999));
        assert!(!w.check(REJECT_AFTER_MESSAGES));
    }

    /// Без знания нашей личности рукопожатие не начать: msg1 к другому адресату
    /// (mac1 под чужую личность) — молчание, никакого состояния и ответа.
    #[test]
    fn msg1_without_our_identity_gets_no_answer() {
        let (mut a, mut b, mut c) = (node(), node(), node());
        let t = Instant::now();
        let m1 = a.s.send(&b.id, &text("x"), true, t, &mut OsRng).remove(0);
        assert_eq!(c.s.receive(&m1, t, &mut OsRng), Received::UnknownIndex);
        assert!(c.s.slots.is_empty());
        let mut broken = m1.clone();
        broken[MSG1_LEN - 1] ^= 1;
        assert_eq!(b.s.receive(&broken, t, &mut OsRng), Received::UnknownIndex);
        assert!(matches!(b.s.receive(&m1, t, &mut OsRng), Received::Reply(_)));
    }

    /// Ответить вместо адресата нельзя, даже зная его личность (и потому `mac1`):
    /// подпись личности над статическим ключом подделать нечем.
    #[test]
    fn impostor_cannot_answer_for_someone_else() {
        let (mut a, b, mut eve) = (node(), node(), node());
        eve.s.mac1_mine = mac1_key(&b.id);
        let t = Instant::now();
        let m1 = a.s.send(&b.id, &text("секрет"), true, t, &mut OsRng).remove(0);
        let Received::Reply(m2) = eve.s.receive(&m1, t, &mut OsRng) else { panic!() };
        assert_eq!(a.s.receive(&m2, t, &mut OsRng), Received::Rejected);
        assert!(!a.s.has_session(&b.id, t));
        // Подставить личность b в auth Евы — подпись не сойдётся.
        let mut forged = Sessions::new(&Identity::generate());
        forged.mac1_mine = mac1_key(&b.id);
        forged.auth[..32].copy_from_slice(&b.id);
        let m1 = a.s.send(&b.id, &text("секрет"), true, t + REKEY_TIMEOUT * 2, &mut OsRng).pop().unwrap();
        let Received::Reply(m2) = forged.receive(&m1, t, &mut OsRng) else { panic!() };
        assert_eq!(a.s.receive(&m2, t + REKEY_TIMEOUT * 2, &mut OsRng), Received::Rejected);
    }

    /// Мусор с подсмотренным индексом не срывает настоящее рукопожатие.
    #[test]
    fn garbage_with_known_index_does_not_break_handshake() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        let m1 = a.s.send(&b.id, &text("x"), true, t, &mut OsRng).remove(0);
        let Received::Reply(m2) = b.s.receive(&m1, t, &mut OsRng) else { panic!() };
        let mut fake = m2.clone();
        fake[100] ^= 0xff;
        assert_eq!(a.s.receive(&fake, t, &mut OsRng), Received::Rejected);
        assert!(matches!(a.s.receive(&m2, t, &mut OsRng), Received::Established { .. }));
    }

    #[test]
    fn rekey_after_two_minutes_keeps_old_session_for_receiving() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        handshake(&mut a, &mut b, "x", t);
        let late = t + REKEY_AFTER + Duration::from_secs(1);
        let old_pkt = b.s.send(&a.id, &text("в пути"), true, late, &mut OsRng);
        // b отвечал на рукопожатие, но смена — по возрасту у любой стороны.
        assert_eq!(old_pkt.len(), 2, "данные по старой сессии + msg1 новой");
        let out = a.s.send(&b.id, &text("старой ещё можно"), true, late, &mut OsRng);
        assert_eq!(out.len(), 2);
        // Довести рукопожатие a → b.
        let Received::Reply(m2) = b.s.receive(&out[1], late, &mut OsRng) else { panic!() };
        let Received::Established { out, .. } = a.s.receive(&m2, late, &mut OsRng) else { panic!() };
        let Received::Established { out: back, .. } = b.s.receive(&out[0], late, &mut OsRng) else { panic!() };
        assert!(matches!(a.s.receive(&back[0], late, &mut OsRng), Received::Keepalive { .. }));
        // Пакет по старой сессии всё ещё принимается.
        assert!(matches!(a.s.receive(&old_pkt[0], late, &mut OsRng), Received::Data { .. }));
        // После REJECT_AFTER от создания старая не годится, новая — да.
        let later = t + REJECT_AFTER + Duration::from_secs(1);
        let r = deliver(&mut a, &mut b, "по новой", later);
        assert!(matches!(&r[..], [Received::Data { .. }]), "{r:?}");
    }

    #[test]
    fn expired_session_rejects_and_restarts() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        handshake(&mut a, &mut b, "x", t);
        let p = a.s.send(&b.id, &text("1"), true, t, &mut OsRng).remove(0);
        let dead = t + REJECT_AFTER;
        assert_eq!(b.s.receive(&p, dead, &mut OsRng), Received::UnknownIndex, "выметено");
        assert!(!a.s.has_session(&b.id, dead));
        assert_eq!(handshake(&mut a, &mut b, "снова", dead), ["снова"]);
    }

    /// Собеседник перезапустился и всё забыл: его ответов нет, и через
    /// UNANSWERED_AFTER мы сами начинаем новую сессию — не ждём двух минут.
    #[test]
    fn peer_restart_is_detected_by_silence() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        handshake(&mut a, &mut b, "x", t);
        let _ = deliver(&mut b, &mut a, "подтверждаю", t);
        b.s = Sessions::new(&Identity::generate()); // «перезапуск»: память пуста
        let p = a.s.send(&b.id, &text("ау"), true, t, &mut OsRng);
        assert_eq!(p.len(), 1);
        assert_eq!(b.s.receive(&p[0], t, &mut OsRng), Received::UnknownIndex);
        let p = a.s.send(&b.id, &text("ау?"), true, t + UNANSWERED_AFTER, &mut OsRng);
        assert_eq!(p.len(), 2, "молчание → новое рукопожатие");
        assert_eq!(p[1].len(), APP_PACKET);
    }

    /// Оба начали одновременно: обе сессии достраиваются, данные идут.
    #[test]
    fn simultaneous_initiation_converges() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        let a1 = a.s.send(&b.id, &text("от a"), true, t, &mut OsRng).remove(0);
        let b1 = b.s.send(&a.id, &text("от b"), true, t, &mut OsRng).remove(0);
        let Received::Reply(b2) = b.s.receive(&a1, t, &mut OsRng) else { panic!() };
        let Received::Reply(a2) = a.s.receive(&b1, t, &mut OsRng) else { panic!() };
        let Received::Established { out: a3, .. } = a.s.receive(&b2, t, &mut OsRng) else { panic!() };
        let Received::Established { out: b3, .. } = b.s.receive(&a2, t, &mut OsRng) else { panic!() };
        let mut got_b = Vec::new();
        for p in &a3 {
            match b.s.receive(p, t, &mut OsRng) {
                Received::Data { plaintext, .. } => got_b.push(unpadded(&plaintext)),
                Received::Established { out, .. } => out.iter().for_each(|p| {
                    a.s.receive(p, t, &mut OsRng);
                }),
                r => panic!("{r:?}"),
            }
        }
        let mut got_a = Vec::new();
        for p in &b3 {
            match a.s.receive(p, t, &mut OsRng) {
                Received::Data { plaintext, .. } => got_a.push(unpadded(&plaintext)),
                Received::Established { out, .. } => out.iter().for_each(|p| {
                    b.s.receive(p, t, &mut OsRng);
                }),
                r => panic!("{r:?}"),
            }
        }
        assert_eq!(got_b, ["от a"]);
        assert_eq!(got_a, ["от b"]);
        for _ in 0..3 {
            assert!(matches!(&deliver(&mut a, &mut b, "a→b", t)[..], [Received::Data { .. }]));
            assert!(matches!(&deliver(&mut b, &mut a, "b→a", t)[..], [Received::Data { .. }]));
        }
    }

    /// msg3 потерян: инициатор не шлёт в неподтверждённую сессию вечно — через
    /// REKEY_TIMEOUT молчания начинает заново.
    #[test]
    fn lost_msg3_recovers() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        let m1 = a.s.send(&b.id, &text("1"), true, t, &mut OsRng).remove(0);
        let Received::Reply(m2) = b.s.receive(&m1, t, &mut OsRng) else { panic!() };
        let Received::Established { .. } = a.s.receive(&m2, t, &mut OsRng) else { panic!() };
        // msg3 и данные не дошли.
        let p = a.s.send(&b.id, &text("2"), true, t, &mut OsRng);
        assert_eq!(p.len(), 1);
        let p = a.s.send(&b.id, &text("3"), true, t + REKEY_TIMEOUT, &mut OsRng);
        assert_eq!(p.len(), 2, "неподтверждённая сессия молчит → заново");
    }

    /// Потерян msg3 вместе с данными за ним: таймер повторяет msg3 и данные, и
    /// они доходят без нового рукопожатия и без повторного `send`.
    #[test]
    fn lost_msg3_is_resent_with_its_data() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        let m1 = a.s.send(&b.id, &text("манифест"), true, t, &mut OsRng).remove(0);
        let Received::Reply(m2) = b.s.receive(&m1, t, &mut OsRng) else { panic!() };
        let Received::Established { out, .. } = a.s.receive(&m2, t, &mut OsRng) else { panic!() };
        assert_eq!(out.len(), 2, "msg3 + данные — оба потеряны");
        assert!(a.s.tick(t, &mut OsRng).is_empty(), "рано");
        let again = a.s.tick(t + MSG3_RESEND, &mut OsRng);
        assert_eq!(again.len(), 2, "msg3 и данные повторены");
        let mut got = Vec::new();
        for (to, p) in &again {
            assert_eq!(*to, b.id);
            match b.s.receive(p, t + MSG3_RESEND, &mut OsRng) {
                Received::Established { out, .. } => {
                    for k in out {
                        assert!(matches!(a.s.receive(&k, t + MSG3_RESEND, &mut OsRng), Received::Keepalive { .. }));
                    }
                }
                Received::Data { plaintext, .. } => got.push(unpadded(&plaintext)),
                r => panic!("{r:?}"),
            }
        }
        assert_eq!(got, ["манифест"]);
        assert!(a.s.tick(t + MSG3_RESEND * 2, &mut OsRng).is_empty(), "подтверждено — больше не повторяем");
    }

    /// msg1 потерян, а `send` больше никто не зовёт: таймер сам начинает новое
    /// рукопожатие, пока данные ждут.
    #[test]
    fn lost_msg1_is_retried_by_timer() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        let _lost = a.s.send(&b.id, &text("маячок"), true, t, &mut OsRng);
        assert!(a.s.tick(t + HANDSHAKE_RETRY, &mut OsRng).is_empty(), "таймер не частит");
        let retry = a.s.tick(t + REKEY_TIMEOUT, &mut OsRng);
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].1.len(), APP_PACKET, "новый msg1");
        let now = t + REKEY_TIMEOUT;
        let Received::Reply(m2) = b.s.receive(&retry[0].1, now, &mut OsRng) else { panic!() };
        let Received::Established { out, .. } = a.s.receive(&m2, now, &mut OsRng) else { panic!() };
        let got: Vec<String> = out
            .iter()
            .filter_map(|p| match b.s.receive(p, now, &mut OsRng) {
                Received::Data { plaintext, .. } => Some(unpadded(&plaintext)),
                _ => None,
            })
            .collect();
        assert_eq!(got, ["маячок"]);
    }

    /// Сессия так и не подтвердилась за REKEY_TIMEOUT: её бросают, отправленное по
    /// ней возвращается в очередь и уходит после нового рукопожатия.
    #[test]
    fn unconfirmed_session_is_dropped_and_data_requeued() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        let m1 = a.s.send(&b.id, &text("1"), true, t, &mut OsRng).remove(0);
        let Received::Reply(m2) = b.s.receive(&m1, t, &mut OsRng) else { panic!() };
        let _lost = a.s.receive(&m2, t, &mut OsRng);
        let late = t + REKEY_TIMEOUT;
        let out = a.s.tick(late, &mut OsRng);
        assert_eq!(out.len(), 1, "только новый msg1: {}", out.len());
        let Received::Reply(m2) = b.s.receive(&out[0].1, late, &mut OsRng) else { panic!() };
        let Received::Established { out, .. } = a.s.receive(&m2, late, &mut OsRng) else { panic!() };
        let got: Vec<String> = out
            .iter()
            .filter_map(|p| match b.s.receive(p, late, &mut OsRng) {
                Received::Data { plaintext, .. } => Some(unpadded(&plaintext)),
                _ => None,
            })
            .collect();
        assert_eq!(got, ["1"]);
    }

    /// Надёжное (hold = false) без сессии не копится: только рукопожатие. Иначе
    /// после него ушло бы и то, что мессенджер уже снял.
    #[test]
    fn unheld_payload_is_not_queued() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        let m1 = a.s.send(&b.id, &text("звонок"), false, t, &mut OsRng);
        assert_eq!(m1.len(), 1, "рукопожатие началось");
        assert!(a.s.send(&b.id, &text("звонок"), false, t, &mut OsRng).is_empty(), "второе не начинаем сразу");
        let Received::Reply(m2) = b.s.receive(&m1[0], t, &mut OsRng) else { panic!() };
        let Received::Established { out, .. } = a.s.receive(&m2, t, &mut OsRng) else { panic!() };
        assert_eq!(out.len(), 1, "только msg3, без данных");
    }

    /// Ответ на прежнее рукопожатие годится, даже если уже начато новое.
    #[test]
    fn answer_to_earlier_handshake_still_completes() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        let first = a.s.send(&b.id, &text("1"), true, t, &mut OsRng).remove(0);
        let second = a.s.send(&b.id, &text("2"), true, t + HANDSHAKE_RETRY, &mut OsRng);
        assert_eq!(second.len(), 1, "через секунду — новое рукопожатие");
        let now = t + HANDSHAKE_RETRY;
        let Received::Reply(m2) = b.s.receive(&first, now, &mut OsRng) else { panic!() };
        assert!(matches!(a.s.receive(&m2, now, &mut OsRng), Received::Established { .. }));
    }

    #[test]
    fn queue_is_bounded_and_expires() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        for i in 0..(QUEUE_CAP + 10) {
            a.s.send(&b.id, &text(&i.to_string()), true, t, &mut OsRng);
        }
        assert_eq!(a.s.peers[&b.id].queue.len(), QUEUE_CAP);
        // Через QUEUE_TTL очередь пуста, рукопожатие начинается заново.
        let later = t + QUEUE_TTL;
        let got = handshake(&mut a, &mut b, "свежее", later);
        assert_eq!(got, ["свежее"]);
    }

    /// Обрубки и мусор любой длины не роняют разбор (фаззер — отдельно).
    #[test]
    fn garbage_never_panics() {
        let (mut a, mut b) = (node(), node());
        let t = Instant::now();
        handshake(&mut a, &mut b, "x", t);
        let live = *a.s.slots.keys().next().unwrap();
        for len in [0, 1, 3, 4, 11, 12, 27, 28, 100, CONTROL_PACKET, MSG2_LEN - 1, APP_PACKET, 9000] {
            let mut p = vec![0x5a; len];
            if len >= 4 {
                p[..4].copy_from_slice(&live.to_le_bytes());
            }
            let _ = a.s.receive(&p, t, &mut OsRng);
            let _ = b.s.receive(&p, t, &mut OsRng);
        }
    }
}
