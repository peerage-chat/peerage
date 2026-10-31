//! Мультипати движок комнаты (Discord-style): один захват с микрофона и камеры →
//! всем текущим участникам; приём от каждого с отдельным декодером и миксом звука в
//! один выход. Темп воспроизведения задаёт потребитель (`AudioTrack` через
//! [`RoomCallEngine::pull_audio`], видео-декодер через
//! [`RoomCallEngine::pull_video_for`]).
//!
//! Доставку — формат пакетов, нумерацию, джиттер-буфер, сборку видео-кадров, оценку
//! полосы и ровный темп отправки — делает `litertc::Session` (ADR-014). Здесь
//! остаются потоки, кодек звука, микс, уровни речи и остановка звонка.
//!
//! Потоки: приём (датаграммы → сессия), отправка звука (PCM → Opus → сессия) и
//! таймеры. Сессия выпускает видео ровным темпом, поэтому после отправки кадра в её
//! очереди остаются пакеты — их выпускает поток таймеров, когда приходит их время;
//! кто ставит пакеты в очередь, будит его ([`Kick`]).
//!
//! Список участников (`members`) динамический: его обновляет presence-слой
//! (lib.rs) по мере входа/выхода. Движок `Send` — живёт в `Arc`, зовётся из JNI.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use litertc::{JitterOut, Session, Transmit, VideoFrame, VideoInfo};
use peerage_core::settings::AudioSettings;
use peerage_media::{AudioDecoder, AudioEncoder};
use peerage_messenger::{MediaPacket, Messenger};
use rand_core::{OsRng, RngCore};

use crate::LockRecover;

type PeerKey = [u8; 32];
/// Захваченный PCM, ещё не отправленный. Поток отправки ждёт на нём кадр — спит,
/// пока его не разбудит [`MicQueue::push`] (R-113: раньше проверял каждые 2 мс).
struct MicQueue {
    mic: Mutex<Mic>,
    ready: Condvar,
    /// Больше стольких сэмплов не держим: отправка отстала — шлём свежее.
    cap: usize,
    /// Сэмплов (всех каналов) в секунду — для момента захвата кадра.
    pcm_rate: f64,
}

#[derive(Default)]
struct Mic {
    pcm: VecDeque<i16>,
    /// Когда записан последний сэмпл: от него считается момент захвата каждого
    /// кадра — метка времени, по которой собеседник сводит звук с видео.
    end: Option<Instant>,
    /// Звонок кончился: кадров больше не будет.
    closed: bool,
}

impl MicQueue {
    fn new(cap: usize, pcm_rate: f64) -> Self {
        MicQueue {
            mic: Mutex::new(Mic::default()),
            ready: Condvar::new(),
            cap,
            pcm_rate,
        }
    }

    /// Захваченные сэмплы; последний записан в `end`.
    fn push(&self, samples: &[i16], end: Instant) {
        let mut mic = self.mic.lock_safe();
        if mic.closed {
            return;
        }
        mic.end = Some(end);
        mic.pcm.extend(samples.iter().copied());
        let over = mic.pcm.len().saturating_sub(self.cap);
        mic.pcm.drain(..over);
        drop(mic);
        self.ready.notify_one();
    }

    /// Звонок кончился: ждущий кадр поток просыпается ни с чем.
    fn close(&self) {
        let mut mic = self.mic.lock_safe();
        mic.closed = true;
        mic.pcm.clear();
        drop(mic);
        self.ready.notify_all();
    }

    /// Следующий кадр в `frame`, если наберётся за `wait`; возвращает, когда записан
    /// его первый сэмпл. `None` — не набрался или звонок кончился.
    fn next_frame(&self, frame: &mut [i16], wait: Duration) -> Option<Instant> {
        let mic = self.mic.lock_safe();
        let (mut mic, _) = self
            .ready
            .wait_timeout_while(mic, wait, |m| !m.closed && m.pcm.len() < frame.len())
            .unwrap_or_else(|e| e.into_inner());
        if mic.closed || mic.pcm.len() < frame.len() {
            return None;
        }
        // Первый сэмпл кадра записан раньше последнего на всё, что от него до конца.
        let behind = Duration::from_secs_f64(mic.pcm.len() as f64 / self.pcm_rate);
        let end = mic.end?;
        let n = frame.len();
        for (s, v) in frame.iter_mut().zip(mic.pcm.drain(..n)) {
            *s = v;
        }
        Some(end.checked_sub(behind).unwrap_or(end))
    }
}

/// Прибавить `pcm` к миксу; вернуть уровень этого звука для индикатора 0..1.
/// Индикатор «у участника мик ловит звук» зажигается при ЛЮБОМ звуке (0,03 ≈ RMS 160),
/// не только речи — проще и честнее; чистую тишину гасит.
fn add_into(mix: &mut [i32], pcm: &[i16]) -> f32 {
    for (m, &v) in mix.iter_mut().zip(pcm) {
        *m += v as i32;
    }
    let sum: f64 = pcm.iter().map(|&v| v as f64 * v as f64).sum();
    let rms = (sum / pcm.len().max(1) as f64).sqrt();
    (rms / 32768.0 * 6.0).min(1.0) as f32
}

/// Пришли новые кадры видео — будит потребителей, спящих в [`wait_video`].
#[derive(Default)]
struct VideoReady {
    lock: Mutex<()>,
    cv: Condvar,
}

impl VideoReady {
    fn notify(&self) {
        let _g = self.lock.lock_safe();
        self.cv.notify_all();
    }
}

/// Следующий кадр `peer`, как только его пора декодировать; ждёт не дольше `wait`.
/// Спит до срока старшего кадра или до прихода новых ([`VideoReady::notify`]) — декодер
/// не опрашивает сессию (R-113: раньше каждые 4 мс на плитку). Замок `ready` держится
/// от проверки до сна: уведомление между ними не потеряется.
fn wait_video(
    session: &Mutex<Session<PeerKey>>,
    ready: &VideoReady,
    running: &AtomicBool,
    peer: &PeerKey,
    wait: Duration,
) -> Option<VideoFrame> {
    let deadline = Instant::now() + wait;
    let mut guard = ready.lock.lock_safe();
    loop {
        let now = Instant::now();
        let next = {
            let mut s = session.lock_safe();
            if let Some(f) = s.pull_video(now, peer) {
                return Some(f);
            }
            s.next_video_at(peer)
        };
        if now >= deadline || !running.load(Ordering::Relaxed) {
            return None;
        }
        let until = next.map_or(deadline, |t| t.min(deadline));
        guard = ready
            .cv
            .wait_timeout(guard, until.saturating_duration_since(now))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

/// Дольше поток отправки звука не ждёт кадра — и проверяет, идёт ли ещё звонок.
const MIC_WAIT: Duration = Duration::from_millis(200);

/// Подряд идущих PLC-кадров на источник; дальше — тишина (Opus PLC иначе шумит).
const MAX_PLC: u32 = 4;

/// Дольше этого потоки приёма и таймеров не спят: проверяют флаг остановки.
const RX_WAIT: Duration = Duration::from_millis(200);

/// Нижняя граница оценки полосы: меньше звонок со звуком не живёт.
const MIN_BITRATE: u32 = 100_000;

/// Звуку — не меньше этого, даже на очень узком канале (Opus ещё разборчив).
const MIN_AUDIO_BITRATE: i32 = 16_000;

/// Битрейт звука при общей цели `total` (бит/с): настройка пользователя, пока канал
/// широкий; на узком — четверть канала, остальное видео.
fn audio_bitrate(total: u32, user: i32) -> i32 {
    user.min((total / 4) as i32)
        .max(MIN_AUDIO_BITRATE.min(user))
}

/// Будильник потока таймеров: кто поставил пакеты в очередь сессии, будит его, чтобы
/// он выпустил их в свой срок, а не через [`RX_WAIT`].
type Kick = Arc<(Mutex<bool>, Condvar)>;

fn kick(k: &Kick) {
    *k.0.lock_safe() = true;
    k.1.notify_one();
}

/// Спать до `dur` или до пинка.
fn wait_kick(k: &Kick, dur: Duration) {
    let mut kicked = k.0.lock_safe();
    if !*kicked {
        kicked =
            k.1.wait_timeout(kicked, dur)
                .unwrap_or_else(|e| e.into_inner())
                .0;
    }
    *kicked = false;
}

/// Как часто писать в лог статистику звонка (потери, джиттер, RTT по участникам).
const STATS_LOG_EVERY: Duration = Duration::from_secs(5);

/// Раз в столько кадров звука (50 × 20 мс = 1 с, как отчёты) кодер узнаёт потери.
const LOSS_CHECK_FRAMES: u32 = 50;

/// Худшая доля потерь нашего звука среди участников, %, по их отчётам. Кодер один на
/// всех, поэтому FEC подстраивается под самого слабого.
fn worst_audio_loss(s: &Session<PeerKey>) -> i32 {
    let worst = s
        .peers()
        .iter()
        .filter_map(|p| s.stats(p)?.outbound[litertc::TRACK_AUDIO as usize])
        .map(|t| t.fraction_lost)
        .fold(0.0f32, f32::max);
    (worst * 100.0).ceil() as i32
}

/// Статистика по участникам — в лог, для разбора звонков.
fn log_stats(s: &Session<PeerKey>) {
    for p in s.peers() {
        if let Some(st) = s.stats(p) {
            log::info!("{}", stats_line(p, &st));
        }
    }
}

/// Строка статистики участника `p`: потери, джиттер, RTT, полоса, сведение губ.
fn stats_line(p: &PeerKey, st: &litertc::PeerStats) -> String {
    let pct = |t: Option<litertc::TrackStats>| {
        t.map_or("-".to_string(), |t| {
            format!("{:.1}%", t.fraction_lost * 100.0)
        })
    };
    let a = litertc::TRACK_AUDIO as usize;
    let v = litertc::TRACK_VIDEO as usize;
    let jitter = st.inbound[a].map_or("-".to_string(), |t| format!("{:.0}ms", t.jitter_ms));
    let ls = st.lip_sync;
    format!(
        "room-call {}: rtt={} in: audio {} jitter {} video {} | out: audio {} video {} | rtx {} keyreq {} | bw {}k target {}k (cuts: delay {} loss {}) flight {}/{}k | lips {} wait audio {}ms video {}ms (hold {} drop {})",
        hex::encode(&p[..4]),
        st.rtt.map_or("-".to_string(), |r| format!("{}ms", r.as_millis())),
        pct(st.inbound[a]),
        jitter,
        pct(st.inbound[v]),
        pct(st.outbound[a]),
        pct(st.outbound[v]),
        st.retransmitted,
        st.keyframe_requests,
        st.bandwidth / 1000,
        st.target / 1000,
        st.bandwidth_cuts.0,
        st.bandwidth_cuts.1,
        st.in_flight.0 / 1000,
        st.in_flight.1 / 1000,
        ls.error_ms.map_or("-".to_string(), |e| format!("{e:+.0}ms")),
        ls.audio_wait.as_millis(),
        ls.video_wait.as_millis(),
        ls.audio_holds,
        ls.audio_drops,
    )
}

/// Декодер звука одного участника и счётчик подряд идущих PLC.
struct Decoder {
    dec: AudioDecoder,
    plc_run: u32,
}

impl Decoder {
    /// Кадр PCM из того, что отдал джиттер-буфер.
    fn decode(&mut self, out: JitterOut) -> Option<Vec<i16>> {
        match out {
            JitterOut::Frame(pkt) => {
                self.plc_run = 0;
                self.dec.decode(Some(&pkt)).ok()
            }
            // Следующий пакет уже здесь — кадр восстанавливается из его FEC-копии.
            // Это настоящий звук, а не маскировка: счётчик PLC не растёт.
            JitterOut::Lost { next: Some(next) } => {
                self.plc_run = 0;
                self.dec.decode_fec(&next).ok()
            }
            // Звук ждёт отстающее видео (ADR-014 §8b): маскировка, как при потере.
            JitterOut::Lost { next: None } | JitterOut::Hold => {
                if self.plc_run < MAX_PLC {
                    self.plc_run += 1;
                    self.dec.decode(None).ok()
                } else {
                    None // длительная потеря — тишина вместо растущего PLC
                }
            }
        }
    }
}

/// Сверить участников с presence-слоем и выполнить `f` над сессией; готовые пакеты
/// забираются здесь же, а отправляются — уже без замка (шифрование и запись в
/// транспорт не должны держать приём и микс).
fn with_session(
    session: &Mutex<Session<PeerKey>>,
    members: &Mutex<Vec<PeerKey>>,
    f: impl FnOnce(&mut Session<PeerKey>),
) -> Vec<Transmit<PeerKey>> {
    let mut s = session.lock_safe();
    s.set_peers(&members.lock_safe());
    f(&mut s);
    std::iter::from_fn(|| s.poll_transmit()).collect()
}

fn send_all(messenger: &Messenger, out: Vec<Transmit<PeerKey>>) {
    for t in out {
        let _ = messenger.send_media_packet(&t.to, &t.data);
    }
}

/// Общее для потоков звонка и его методов.
#[derive(Clone)]
struct Shared {
    running: Arc<AtomicBool>,
    session: Arc<Mutex<Session<PeerKey>>>,
    members: Arc<Mutex<Vec<PeerKey>>>,
    messenger: Arc<Messenger>,
    kick: Kick,
    video_ready: Arc<VideoReady>,
}

impl Shared {
    /// `f` над сессией (см. [`with_session`]); собрались новые кадры видео (`seen` —
    /// сколько их было) — разбудить потребителей.
    fn step(
        &self,
        seen: &mut u64,
        f: impl FnOnce(&mut Session<PeerKey>),
    ) -> Vec<Transmit<PeerKey>> {
        let mut queued = *seen;
        let out = with_session(&self.session, &self.members, |s| {
            f(s);
            queued = s.video_frames_queued();
        });
        if queued != *seen {
            *seen = queued;
            self.video_ready.notify();
        }
        out
    }
}

/// Приём: датаграммы участников — в сессию; участников сверяет она же.
fn rx_loop(sh: Shared, media_rx: Receiver<MediaPacket>) {
    let mut seen = 0;
    while sh.running.load(Ordering::Relaxed) {
        match media_rx.recv_timeout(RX_WAIT) {
            Ok(p) => {
                let out = sh.step(&mut seen, |s| {
                    s.handle_datagram(Instant::now(), p.src, &p.data)
                });
                send_all(&sh.messenger, out);
                kick(&sh.kick);
            }
            Err(RecvTimeoutError::Timeout) => {}
            // Канал отобрали (новый media_channel) — приёма больше не будет, а
            // крутиться на мгновенно возвращающемся recv значит 100 % CPU.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Таймеры сессии и выпуск отложенного (видео ровным темпом), статистика в лог.
fn timer_loop(sh: Shared) {
    let mut next_log = Instant::now() + STATS_LOG_EVERY;
    let mut seen = 0;
    while sh.running.load(Ordering::Relaxed) {
        let now = Instant::now();
        // Кадр мог собраться и по таймеру (дождались недостающего).
        let out = sh.step(&mut seen, |s| {
            if s.poll_timeout().is_some_and(|t| t <= now) {
                s.handle_timeout(now);
            }
        });
        send_all(&sh.messenger, out);
        if now >= next_log {
            next_log = now + STATS_LOG_EVERY;
            log_stats(&sh.session.lock_safe());
        }
        let wait = timer_wait(&sh.session.lock_safe(), Instant::now());
        wait_kick(&sh.kick, wait);
    }
}

/// Сколько потоку таймеров спать: до ближайшего таймера сессии, но не меньше
/// миллисекунды (точнее выпуск не нужен, а нулевой сон — это кручение, если таймер
/// почему-то не снялся) и не больше [`RX_WAIT`] (проверить, идёт ли звонок).
fn timer_wait(s: &Session<PeerKey>, now: Instant) -> Duration {
    s.poll_timeout()
        .map_or(RX_WAIT, |t| t.saturating_duration_since(now))
        .clamp(Duration::from_millis(1), RX_WAIT)
}

/// Кодер звука и его битрейт: настройка пользователя, но не больше, чем позволяет
/// канал.
struct AudioTx {
    enc: AudioEncoder,
    /// Битрейт, выставленный кодеру.
    bitrate: i32,
    /// Предел канала: [`audio_bitrate`] от оценки полосы.
    channel: i32,
    /// Кадров с прошлой сверки с каналом.
    frames: u32,
}

impl AudioTx {
    fn new(enc: AudioEncoder, bitrate: i32) -> Self {
        AudioTx {
            enc,
            bitrate,
            channel: bitrate,
            frames: 0,
        }
    }

    /// Ещё кадр; раз в [`LOSS_CHECK_FRAMES`] — сверка с каналом: FEC под худшие
    /// потери, предел битрейта по оценке полосы.
    fn follow_channel(&mut self, session: &Mutex<Session<PeerKey>>) {
        self.frames += 1;
        if self.frames < LOSS_CHECK_FRAMES {
            return;
        }
        self.frames = 0;
        let s = session.lock_safe();
        self.enc.set_expected_loss(worst_audio_loss(&s));
        self.channel = audio_bitrate(s.target_bitrate(), i32::MAX);
    }

    /// Битрейт из настроек `user` (меняются на лету), но не выше предела канала.
    fn apply_bitrate(&mut self, user: i32) {
        let want = user.min(self.channel);
        if want != self.bitrate {
            self.enc.set_bitrate(want);
            self.bitrate = want;
        }
    }
}

/// Отправка: PCM с микрофона → Opus → сессия → участникам. Спит до кадра; конец
/// звонка будит сразу (halt → close).
fn send_loop(
    sh: Shared,
    mic: Arc<MicQueue>,
    muted: Arc<AtomicBool>,
    target_bitrate: Arc<AtomicI32>,
    audio: AudioSettings,
) {
    let Ok(enc) = AudioEncoder::new(audio) else {
        return;
    };
    let mut tx = AudioTx::new(enc, audio.bitrate);
    let mut frame = vec![0i16; audio.interleaved_len()];
    while sh.running.load(Ordering::Relaxed) {
        let Some(captured) = mic.next_frame(&mut frame, MIC_WAIT) else {
            continue;
        };
        tx.follow_channel(&sh.session);
        tx.apply_bitrate(target_bitrate.load(Ordering::Relaxed));
        // Не отправленный кадр (микрофон выключен) номер не тратит, а метка
        // следующего — по времени его захвата.
        if muted.load(Ordering::Relaxed) {
            continue;
        }
        let Ok(pkt) = tx.enc.encode(&frame) else {
            continue;
        };
        if !sh.running.load(Ordering::Relaxed) {
            break;
        }
        let out = with_session(&sh.session, &sh.members, |s| {
            s.send_audio(Instant::now(), captured, &pkt)
        });
        send_all(&sh.messenger, out);
        kick(&sh.kick);
    }
}

/// Активный мультипати звонок комнаты.
pub struct RoomCallEngine {
    sh: Shared,
    muted: Arc<AtomicBool>,
    mic: Arc<MicQueue>,
    frame_len: usize,
    audio: AudioSettings,
    /// Декодеры звука по участникам; трогает только [`Self::pull_audio`].
    decoders: Mutex<HashMap<PeerKey, Decoder>>,
    /// Смешанный PCM, ещё не отданный потребителю.
    mix_carry: Mutex<VecDeque<i16>>,
    /// Уровень речи на участника (0..1, сглажен): attack мгновенный, decay плавный.
    /// Считается в [`pull_audio`] по RMS кадра источника. Для active-speaker рамки.
    levels: Mutex<HashMap<PeerKey, f32>>,
    /// Битрейт Opus из настроек звука (бит/с): меняется на лету, send-тред применяет
    /// к энкодеру без пересоздания сессии (чтобы не было дыры в звуке). На узком
    /// канале звук получает меньше ([`audio_bitrate`]).
    target_bitrate: Arc<AtomicI32>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl RoomCallEngine {
    pub fn start(
        messenger: Arc<Messenger>,
        audio: AudioSettings,
        members: Arc<Mutex<Vec<PeerKey>>>,
    ) -> Arc<RoomCallEngine> {
        // Сэмплов (всех каналов) в секунду — для момента захвата кадра; держим до
        // секунды несработанного.
        let pcm_rate = audio.sample_rate as f64 * audio.channels.max(1) as f64;
        let mic = Arc::new(MicQueue::new(audio.interleaved_len() * 50, pcm_rate));
        let muted = Arc::new(AtomicBool::new(false));
        let target_bitrate = Arc::new(AtomicI32::new(audio.bitrate));
        let media_rx = messenger.media_channel(2048);
        let sh = Shared {
            running: Arc::new(AtomicBool::new(true)),
            session: Arc::new(Mutex::new(Session::new(litertc::Config {
                audio_clock_hz: audio.sample_rate,
                audio_frame_samples: audio.frame_samples() as u32,
                jitter_depth: audio.jitter_depth,
                // Новый на каждый звонок: так собеседник отличит наш перезаход от
                // опоздавших пакетов прошлого раза.
                stream_id: OsRng.next_u32(),
            }))),
            members,
            messenger,
            kick: Arc::new((Mutex::new(false), Condvar::new())),
            video_ready: Arc::new(VideoReady::default()),
        };
        let threads = vec![
            {
                let sh = sh.clone();
                thread::spawn(move || rx_loop(sh, media_rx))
            },
            {
                let sh = sh.clone();
                thread::spawn(move || timer_loop(sh))
            },
            {
                let (sh, mic, muted) = (sh.clone(), mic.clone(), muted.clone());
                let target_bitrate = target_bitrate.clone();
                thread::spawn(move || send_loop(sh, mic, muted, target_bitrate, audio))
            },
        ];
        Arc::new(RoomCallEngine {
            sh,
            muted,
            mic,
            frame_len: audio.interleaved_len(),
            audio,
            decoders: Mutex::new(HashMap::new()),
            mix_carry: Mutex::new(VecDeque::new()),
            levels: Mutex::new(HashMap::new()),
            target_bitrate,
            threads: Mutex::new(threads),
        })
    }

    /// Потолок битрейта видео из настроек качества: оценка полосы до участников не
    /// поднимается выше него вместе со звуком.
    pub fn set_video_max_bitrate(&self, bps: u32) {
        let audio = self.target_bitrate.load(Ordering::Relaxed).max(0) as u32;
        self.sh
            .session
            .lock_safe()
            .set_bitrate_bounds(MIN_BITRATE, bps.saturating_add(audio));
    }

    /// Сколько сейчас отдать видео-кодеру, бит/с: оценка полосы до самого слабого
    /// участника за вычетом звука и заголовков его пакетов.
    pub fn video_target_bitrate(&self) -> u32 {
        let total = self.sh.session.lock_safe().target_bitrate();
        let user = self.target_bitrate.load(Ordering::Relaxed);
        // ~50 пакетов звука в секунду по 14 байт заголовка LiteRTC — около 6 кбит/с.
        let audio = audio_bitrate(total, user).max(0) as u32 + 6_000;
        (total.saturating_sub(audio) / 100 * 95).max(MIN_BITRATE / 2)
    }

    /// Закодированный H264 access-unit → всем участникам; кадр снят `age` назад
    /// (камера и кодер). `true` — кто-то из участников просит ключевой кадр: пусть
    /// кодер выдаст его следующим.
    pub fn push_video(&self, frame: &[u8], info: VideoInfo, age: Duration) -> bool {
        let mut wanted = false;
        let out = with_session(&self.sh.session, &self.sh.members, |s| {
            let now = Instant::now();
            s.send_video(now, now.checked_sub(age).unwrap_or(now), frame, info);
            wanted = s.take_keyframe_request();
        });
        send_all(&self.sh.messenger, out);
        // Остаток кадра выйдет ровным темпом — его выпустит поток таймеров.
        kick(&self.sh.kick);
        wanted
    }

    /// Следующий входящий H264-кадр от peer, если его пора декодировать: LiteRTC
    /// расписывает сроки по меткам захвата (`VideoFrame::play_at`).
    /// Ждёт не дольше `wait`: декодеру без кадров незачем крутиться.
    pub fn pull_video_for(&self, peer: &PeerKey, wait: Duration) -> Option<VideoFrame> {
        wait_video(
            &self.sh.session,
            &self.sh.video_ready,
            &self.sh.running,
            peer,
            wait,
        )
    }

    /// Сколько кадр участника идёт через наш видео-декодер (срок → экран): звук
    /// ждёт видео с учётом этого.
    pub fn set_video_render_delay(&self, peer: &PeerKey, delay: Duration) {
        self.sh
            .session
            .lock_safe()
            .set_video_render_delay(peer, delay);
    }

    /// Декодер видео участника начат заново — ему нужен ключевой кадр.
    pub fn request_keyframe(&self, peer: &PeerKey) {
        let out = with_session(&self.sh.session, &self.sh.members, |s| {
            s.request_keyframe(Instant::now(), peer)
        });
        send_all(&self.sh.messenger, out);
        kick(&self.sh.kick);
    }

    /// Захваченный PCM с микрофона → в очередь отправки; последний сэмпл записан
    /// `age` назад.
    pub fn push_audio(&self, samples: &[i16], age: Duration) {
        let now = Instant::now();
        self.mic.push(samples, now.checked_sub(age).unwrap_or(now));
    }

    /// По кадру звука от каждого участника на один тик микса (без декодирования:
    /// замок сессии держим коротко) и текущий список участников. Кадр будет
    /// слышен в `heard_at`.
    fn pull_frames(&self, heard_at: Instant) -> (Vec<PeerKey>, Vec<(PeerKey, JitterOut)>) {
        let mut s = self.sh.session.lock_safe();
        let peers = s.peers().to_vec();
        let frames = peers
            .iter()
            .filter_map(|p| s.pull_audio(heard_at, p).map(|o| (*p, o)))
            .collect();
        (peers, frames)
    }

    /// Смешать все источники в `out` ровным темпом потребителя. На каждый
    /// микс-кадр берём по одному кадру с каждого источника и суммируем. `output` —
    /// сколько звук идёт от `out` до динамика (буфер вывода): по нему звук
    /// сводится с видео.
    pub fn pull_audio(&self, out: &mut [i16], output: Duration) -> usize {
        let mut carry = self.mix_carry.lock_safe();
        let mut produced = 0usize;
        let mut starved = false;
        let now = Instant::now();
        let pcm_rate = self.audio.sample_rate as f64 * self.audio.channels.max(1) as f64;
        for (i, o) in out.iter_mut().enumerate() {
            if carry.is_empty() && !starved {
                let heard_at = now + output + Duration::from_secs_f64(i as f64 / pcm_rate);
                match self.mix_frame(heard_at) {
                    Some(mix) => carry.extend(mix.iter().map(|&m| m.clamp(-32768, 32767) as i16)),
                    None => starved = true, // никто не говорит — добиваем тишиной
                }
            }
            match carry.pop_front() {
                Some(s) => {
                    *o = s;
                    produced += 1;
                }
                None => *o = 0,
            }
        }
        produced
    }

    /// Один микс-кадр, который будет слышен в `heard_at`: по кадру с каждого
    /// источника, декодированные и сложенные; заодно уровни речи. `None` — ни у кого
    /// нет звука.
    fn mix_frame(&self, heard_at: Instant) -> Option<Vec<i32>> {
        let (peers, frames) = self.pull_frames(heard_at);
        let mut decs = self.decoders.lock_safe();
        decs.retain(|k, _| peers.contains(k));
        let mut lv = self.levels.lock_safe();
        // Release: плавное затухание уровня каждого кадра (держит сквозь паузы речи).
        for v in lv.values_mut() {
            *v *= 0.82;
        }
        let mut mix = vec![0i32; self.frame_len];
        let mut any = false;
        for (key, frame) in frames {
            let dec = match decs.entry(key) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                // Декодер не создался — этого собеседника не слышно, но паника
                // отравила бы мьютексы и убила звук всего звонка.
                std::collections::hash_map::Entry::Vacant(v) => match AudioDecoder::new(self.audio)
                {
                    Ok(dec) => v.insert(Decoder { dec, plc_run: 0 }),
                    Err(_) => continue,
                },
            };
            if let Some(pcm) = dec.decode(frame) {
                let e = lv.entry(key).or_insert(0.0);
                *e = e.max(add_into(&mut mix, &pcm)); // мгновенный attack
                any = true;
            }
        }
        any.then_some(mix)
    }

    /// Hex-ключи участников, которые сейчас говорят (уровень > порога) — для
    /// active-speaker рамки. Уровень считается в [`pull_audio`] по RMS кадра.
    pub fn speaking_json(&self) -> String {
        let lv = self.levels.lock_safe();
        let arr: Vec<String> = lv
            .iter()
            .filter(|(_, v)| **v > 0.03)
            .map(|(k, _)| hex::encode(k))
            .collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
    }

    /// Сменить целевой Opus-битрейт на лету (применит send-тред). Режим (Voip/Audio)
    /// менять live нельзя — он на старте сессии.
    pub fn set_bitrate(&self, bitrate: i32) {
        self.target_bitrate.store(bitrate, Ordering::Relaxed);
    }

    /// Накопленные байты трафика: {hex_участника: rx_всего, "self": tx_всего}. Клиент
    /// считает скорость (kbps) по дельте между опросами. Для тех-инфо чипа.
    pub fn traffic_json(&self) -> String {
        let s = self.sh.session.lock_safe();
        let mut m: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
        for p in s.peers() {
            if let Some(st) = s.stats(p) {
                m.insert(hex::encode(p), st.rx_bytes);
            }
        }
        // Ключ "self" — свой исходящий трафик: клиенту он нужен рядом с чужими,
        // чтобы показать ↓/↑ одной строкой.
        m.insert("self".into(), s.tx_bytes());
        serde_json::to_string(&m).unwrap_or_else(|_| "{}".into())
    }

    pub fn set_muted(&self, m: bool) {
        self.muted.store(m, Ordering::Relaxed);
    }

    /// Немедленно прекратить отправку и приём, не дожидаясь потоков: адресатов
    /// больше нет, а поток отправки выходит на следующем кадре. Это и есть
    /// «положить трубку» — оно не стоит ни в какой очереди за входящим трафиком
    /// (класс Signal RingRTC CVE-2026-113380). Дождаться потоков — [`stop`].
    pub fn halt(&self) {
        self.sh.running.store(false, Ordering::Relaxed);
        self.sh.members.lock_safe().clear();
        self.mic.close();
        self.sh.video_ready.notify();
        // Неотправленное тоже никому: сессия без участников выбрасывает свою очередь.
        self.sh.session.lock_safe().set_peers(&[]);
        kick(&self.sh.kick);
    }

    pub fn stop(&self) {
        self.halt();
        for h in self.threads.lock_safe().drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for RoomCallEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 сэмплов в кадре, 1000 сэмплов в секунду — кадр = 10 мс.
    fn queue(cap: usize) -> Arc<MicQueue> {
        Arc::new(MicQueue::new(cap, 1000.0))
    }

    /// Поток отправки спит, пока кадр не набран, и просыпается, как только набран:
    /// без опроса по таймеру. Момент захвата кадра — по последнему сэмплу минус всё,
    /// что до конца очереди.
    #[test]
    fn frame_is_handed_over_as_soon_as_it_is_full() {
        let q = queue(1000);
        let waiter = {
            let q = q.clone();
            thread::spawn(move || {
                let mut frame = [0i16; 10];
                let got = q.next_frame(&mut frame, Duration::from_secs(5));
                (got, frame, Instant::now())
            })
        };
        let end = Instant::now();
        q.push(&[1; 5], end);
        thread::sleep(Duration::from_millis(50));
        assert!(!waiter.is_finished(), "полкадра — ещё не кадр");
        q.push(&[2; 5], end);
        let pushed = Instant::now();
        let (got, frame, woke) = waiter.join().unwrap();
        assert_eq!(got, Some(end - Duration::from_millis(10)));
        assert_eq!(frame, [1, 1, 1, 1, 1, 2, 2, 2, 2, 2]);
        assert!(woke - pushed < Duration::from_millis(500));
    }

    #[test]
    fn empty_queue_waits_no_longer_than_asked() {
        let q = queue(1000);
        let t = Instant::now();
        assert_eq!(q.next_frame(&mut [0; 10], Duration::from_millis(30)), None);
        assert!(t.elapsed() >= Duration::from_millis(30));
    }

    /// Источники складываются в микс; уровень — RMS×6 от полной шкалы, не выше 1:
    /// тишина — 0, громкий звук — 1, тихий — пропорционально.
    #[test]
    fn mix_adds_sources_and_measures_level() {
        let mut mix = vec![0i32; 3];
        assert_eq!(add_into(&mut mix, &[100, -200, 300, 400]), {
            let rms =
                ((100f64.powi(2) + 200f64.powi(2) + 300f64.powi(2) + 400f64.powi(2)) / 4.0).sqrt();
            (rms / 32768.0 * 6.0) as f32
        });
        assert_eq!(
            add_into(&mut mix, &[1, 1, 1]),
            (1.0f64 / 32768.0 * 6.0) as f32
        );
        assert_eq!(mix, [101, -199, 301]);
        assert_eq!(add_into(&mut mix, &[0; 3]), 0.0);
        assert_eq!(add_into(&mut mix, &[i16::MAX; 3]), 1.0);
        assert_eq!(add_into(&mut [], &[]), 0.0);
    }

    /// Не набрался за ожидание — неполный кадр не отдаётся, и набранное не теряется.
    #[test]
    fn partial_frame_waits_for_the_rest() {
        let q = queue(1000);
        q.push(&[1; 5], Instant::now());
        let mut frame = [0i16; 10];
        assert_eq!(q.next_frame(&mut frame, Duration::from_millis(10)), None);
        q.push(&[2; 5], Instant::now());
        assert!(q.next_frame(&mut frame, Duration::ZERO).is_some());
        assert_eq!(frame, [1, 1, 1, 1, 1, 2, 2, 2, 2, 2]);
    }

    /// Конец звонка будит ждущий поток сразу, и кадров больше нет.
    #[test]
    fn close_wakes_the_waiter() {
        let q = queue(1000);
        let waiter = {
            let q = q.clone();
            thread::spawn(move || q.next_frame(&mut [0; 10], Duration::from_secs(10)))
        };
        thread::sleep(Duration::from_millis(20));
        let t = Instant::now();
        q.close();
        assert_eq!(waiter.join().unwrap(), None);
        assert!(t.elapsed() < Duration::from_secs(1));
        q.push(&[1; 10], Instant::now());
        assert_eq!(q.next_frame(&mut [0; 10], Duration::ZERO), None);
    }

    const A: PeerKey = [1; 32];
    const B: PeerKey = [2; 32];

    fn session(peer: PeerKey) -> Session<PeerKey> {
        let mut s = Session::new(litertc::Config {
            audio_clock_hz: 48_000,
            audio_frame_samples: 960,
            jitter_depth: 1,
            stream_id: 7,
        });
        s.set_peers(&[peer]);
        s.set_bitrate_bounds(1_000_000_000, 1_000_000_000);
        s
    }

    /// Два кадра от A (ключевой и следующий, сняты с разницей 33 мс), как их увидит B.
    fn two_frames() -> Vec<Vec<u8>> {
        let mut tx = session(B);
        let t0 = Instant::now();
        let key = VideoInfo {
            keyframe: true,
            ..Default::default()
        };
        tx.send_video(t0, t0, &[5; 100], key);
        let t1 = t0 + Duration::from_millis(33);
        tx.send_video(t1, t1, &[6; 100], VideoInfo::default());
        std::iter::from_fn(|| tx.poll_transmit())
            .map(|t| t.data)
            .collect()
    }

    /// Декодер спит в ожидании кадра и просыпается, когда тот пришёл и подошёл его
    /// срок, — не раньше срока и без опроса.
    #[test]
    fn video_consumer_sleeps_until_a_frame_is_due() {
        let rx = Arc::new(Mutex::new(session(A)));
        let ready = Arc::new(VideoReady::default());
        let running = Arc::new(AtomicBool::new(true));
        let waiter = {
            let (rx, ready, running) = (rx.clone(), ready.clone(), running.clone());
            thread::spawn(move || {
                let f = wait_video(&rx, &ready, &running, &A, Duration::from_secs(5));
                (f, Instant::now())
            })
        };
        thread::sleep(Duration::from_millis(30));
        assert!(!waiter.is_finished(), "кадров нет — ждёт");
        let sent = Instant::now();
        {
            let mut s = rx.lock_safe();
            for p in two_frames() {
                s.handle_datagram(Instant::now(), A, &p);
            }
        }
        ready.notify();
        let (f, woke) = waiter.join().unwrap();
        let f = f.expect("кадр пришёл");
        assert!(woke >= f.play_at, "отдан раньше срока");
        assert!(woke - sent < Duration::from_secs(2));
        // Второй уже лежит, но его показ — через 33 мс: проснуться к сроку, без
        // уведомлений.
        let f = wait_video(&rx, &ready, &running, &A, Duration::from_secs(5)).unwrap();
        assert!(Instant::now() >= f.play_at, "отдан раньше срока");
        assert!(Instant::now() - woke >= Duration::from_millis(25));
    }

    #[test]
    fn video_consumer_gives_up_after_wait_or_on_halt() {
        let rx = Mutex::new(session(A));
        let ready = Arc::new(VideoReady::default());
        let running = Arc::new(AtomicBool::new(true));
        let t = Instant::now();
        assert!(wait_video(&rx, &ready, &running, &A, Duration::from_millis(30)).is_none());
        assert!(t.elapsed() >= Duration::from_millis(30));
        let rx = Arc::new(rx);
        let waiter = {
            let (rx, ready, running) = (rx.clone(), ready.clone(), running.clone());
            thread::spawn(move || wait_video(&rx, &ready, &running, &A, Duration::from_secs(10)))
        };
        thread::sleep(Duration::from_millis(20));
        let t = Instant::now();
        running.store(false, Ordering::Relaxed);
        ready.notify();
        assert!(waiter.join().unwrap().is_none());
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    /// Сверх ёмкости выбрасывается самое старое: отправка отстала — шлём свежее.
    #[test]
    fn overflow_drops_the_oldest() {
        let q = queue(20);
        let pcm: Vec<i16> = (0..25).collect();
        q.push(&pcm, Instant::now());
        let mut frame = [0i16; 10];
        assert!(q.next_frame(&mut frame, Duration::ZERO).is_some());
        assert_eq!(frame[0], 5);
    }

    /// Отчёт участника о нашем звуке: доля потерь `fraction` из 256.
    fn report_about_audio(fraction: u8) -> Vec<u8> {
        litertc::write_report(&litertc::Report {
            ts_ms: 0,
            echo: None,
            blocks: vec![litertc::ReportBlock {
                track: litertc::TRACK_AUDIO,
                fraction_lost: fraction,
                stream_id: 7,
                ..Default::default()
            }],
        })
    }

    /// Кодер один на всех — FEC под самого слабого: худшие потери звука среди
    /// участников, в процентах с округлением вверх.
    #[test]
    fn worst_audio_loss_is_the_weakest_peer() {
        let mut s = session(A);
        s.set_peers(&[A, B]);
        assert_eq!(worst_audio_loss(&s), 0, "отчётов нет");
        let now = Instant::now();
        s.handle_datagram(now, A, &report_about_audio(13)); // 5,08 %
        assert_eq!(worst_audio_loss(&s), 6);
        s.handle_datagram(now, B, &report_about_audio(64)); // 25 %
        assert_eq!(worst_audio_loss(&s), 25);
    }

    /// Строка статистики для разбора звонка по логу: чего ещё нет — «-», есть — в
    /// процентах.
    #[test]
    fn stats_line_shows_missing_as_dash() {
        let mut s = session(A);
        let line = stats_line(&A, &s.stats(&A).unwrap());
        assert!(
            line.starts_with(
                "room-call 01010101: rtt=- in: audio - jitter - video - | out: audio - video -"
            ),
            "{line}"
        );
        s.handle_datagram(Instant::now(), A, &report_about_audio(64));
        let line = stats_line(&A, &s.stats(&A).unwrap());
        assert!(line.contains("out: audio 25.0% video -"), "{line}");
    }

    /// Потеря без FEC — маскировка, но не дольше [`MAX_PLC`] кадров подряд (дальше
    /// Opus PLC шумит) — потом тишина; настоящий или восстановленный из FEC кадр
    /// счёт сбрасывает.
    #[test]
    fn concealment_is_capped_and_reset_by_real_audio() {
        let audio = AudioSettings::default();
        let n = audio.interleaved_len();
        let pkt = AudioEncoder::new(audio)
            .unwrap()
            .encode(&vec![1000; n])
            .unwrap();
        let mut d = Decoder {
            dec: AudioDecoder::new(audio).unwrap(),
            plc_run: 0,
        };
        let frame = d.decode(JitterOut::Frame(pkt.clone()));
        assert_eq!(frame.map(|p| p.len()), Some(n));
        let lost: Vec<bool> = (0..MAX_PLC + 2)
            .map(|_| d.decode(JitterOut::Lost { next: None }).is_some())
            .collect();
        assert_eq!(lost, [true, true, true, true, false, false]);
        let fec = d.decode(JitterOut::Lost {
            next: Some(pkt.clone()),
        });
        assert!(fec.is_some(), "из FEC следующего");
        assert!(d.decode(JitterOut::Hold).is_some(), "счёт сброшен");
    }

    /// Таймер сессии через 50 мс — спать 50; уже прошёл — миллисекунду, не ноль;
    /// таймеров нет или далеко — не дольше RX_WAIT.
    #[test]
    fn timer_sleeps_until_the_session_timer_within_limits() {
        let mut s = session(A);
        let now = Instant::now();
        assert_eq!(timer_wait(&s, now), RX_WAIT, "нет ни одного таймера");
        s.handle_timeout(now);
        let due = s.poll_timeout().unwrap();
        let wait = timer_wait(&s, due - Duration::from_millis(50));
        assert_eq!(wait, Duration::from_millis(50).min(RX_WAIT));
        assert_eq!(
            timer_wait(&s, due + Duration::from_secs(1)),
            Duration::from_millis(1)
        );
        assert_eq!(timer_wait(&s, due - Duration::from_secs(5)), RX_WAIT);
    }

    /// Битрейт звука — из настроек, но раз в секунду (50 кадров) сверяется с каналом:
    /// на узком — четверть оценки полосы.
    #[test]
    fn audio_bitrate_follows_the_channel_once_a_second() {
        let audio = AudioSettings::default();
        let mut tx = AudioTx::new(AudioEncoder::new(audio).unwrap(), audio.bitrate);
        let s = Mutex::new(session(A));
        s.lock_safe().set_bitrate_bounds(100_000, 100_000);
        let narrow = (s.lock_safe().target_bitrate() / 4) as i32;
        assert!(narrow < audio.bitrate);
        for _ in 1..LOSS_CHECK_FRAMES {
            tx.follow_channel(&s);
        }
        tx.apply_bitrate(audio.bitrate);
        assert_eq!(tx.bitrate, audio.bitrate, "ещё не сверялся");
        tx.follow_channel(&s);
        tx.apply_bitrate(audio.bitrate);
        assert_eq!(tx.bitrate, narrow);
        tx.apply_bitrate(16_000);
        assert_eq!(tx.bitrate, 16_000, "настройка ниже предела канала");
    }
}
