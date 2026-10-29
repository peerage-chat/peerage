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
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use litertc::{JitterOut, Session, Transmit, VideoFrame, VideoInfo};
use peerage_core::settings::AudioSettings;
use peerage_media::{AudioDecoder, AudioEncoder};
use peerage_messenger::Messenger;
use rand_core::{OsRng, RngCore};

use crate::LockRecover;

type PeerKey = [u8; 32];
type Ring = Arc<Mutex<Mic>>;

/// Захваченный PCM, ещё не отправленный.
#[derive(Default)]
struct Mic {
    pcm: VecDeque<i16>,
    /// Когда записан последний сэмпл: от него считается момент захвата каждого
    /// кадра — метка времени, по которой собеседник сводит звук с видео.
    end: Option<Instant>,
}

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

/// Строка статистики по участникам — для разбора звонков по логу.
fn log_stats(s: &Session<PeerKey>) {
    let pct = |t: Option<litertc::TrackStats>| {
        t.map_or("-".to_string(), |t| {
            format!("{:.1}%", t.fraction_lost * 100.0)
        })
    };
    for p in s.peers() {
        let Some(st) = s.stats(p) else { continue };
        let a = litertc::TRACK_AUDIO as usize;
        let v = litertc::TRACK_VIDEO as usize;
        let jitter = st.inbound[a].map_or("-".to_string(), |t| format!("{:.0}ms", t.jitter_ms));
        let ls = st.lip_sync;
        log::info!(
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
        );
    }
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

/// Активный мультипати звонок комнаты.
pub struct RoomCallEngine {
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    in_ring: Ring,
    in_cap: usize,
    frame_len: usize,
    audio: AudioSettings,
    session: Arc<Mutex<Session<PeerKey>>>,
    /// Декодеры звука по участникам; трогает только [`Self::pull_audio`].
    decoders: Mutex<HashMap<PeerKey, Decoder>>,
    /// Смешанный PCM, ещё не отданный потребителю.
    mix_carry: Mutex<VecDeque<i16>>,
    messenger: Arc<Messenger>,
    members: Arc<Mutex<Vec<PeerKey>>>,
    /// Уровень речи на участника (0..1, сглажен): attack мгновенный, decay плавный.
    /// Считается в [`pull_audio`] по RMS кадра источника. Для active-speaker рамки.
    levels: Mutex<HashMap<PeerKey, f32>>,
    /// Битрейт Opus из настроек звука (бит/с): меняется на лету, send-тред применяет
    /// к энкодеру без пересоздания сессии (чтобы не было дыры в звуке). На узком
    /// канале звук получает меньше ([`audio_bitrate`]).
    target_bitrate: Arc<AtomicI32>,
    kick: Kick,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl RoomCallEngine {
    pub fn start(
        messenger: Arc<Messenger>,
        audio: AudioSettings,
        members: Arc<Mutex<Vec<PeerKey>>>,
    ) -> Arc<RoomCallEngine> {
        let running = Arc::new(AtomicBool::new(true));
        let muted = Arc::new(AtomicBool::new(false));
        let in_ring: Ring = Arc::new(Mutex::new(Mic::default()));
        // Сэмплов (всех каналов) в секунду — для момента захвата кадра.
        let pcm_rate = audio.sample_rate as f64 * audio.channels.max(1) as f64;
        let in_cap = audio.interleaved_len() * 50;
        let frame_len = audio.interleaved_len();
        let session = Arc::new(Mutex::new(Session::new(litertc::Config {
            audio_clock_hz: audio.sample_rate,
            audio_frame_samples: audio.frame_samples() as u32,
            jitter_depth: audio.jitter_depth,
            // Новый на каждый звонок: так собеседник отличит наш перезаход от
            // опоздавших пакетов прошлого раза.
            stream_id: OsRng.next_u32(),
        })));
        let mut threads = Vec::new();

        let media_rx = messenger.media_channel(2048);
        let kick_: Kick = Arc::new((Mutex::new(false), Condvar::new()));

        // --- Приём: датаграммы в сессию; участников сверяет она же ---
        {
            let running = running.clone();
            let session = session.clone();
            let members = members.clone();
            let msgr = messenger.clone();
            let k = kick_.clone();
            threads.push(thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    match media_rx.recv_timeout(RX_WAIT) {
                        Ok(p) => {
                            let out = with_session(&session, &members, |s| {
                                s.handle_datagram(Instant::now(), p.src, &p.data)
                            });
                            send_all(&msgr, out);
                            kick(&k);
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        // Канал отобрали (новый media_channel) — приёма больше не будет,
                        // а крутиться на мгновенно возвращающемся recv значит 100 % CPU.
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            }));
        }

        // --- Таймеры сессии и выпуск отложенного (видео ровным темпом) ---
        {
            let running = running.clone();
            let session = session.clone();
            let members = members.clone();
            let msgr = messenger.clone();
            let k = kick_.clone();
            threads.push(thread::spawn(move || {
                let mut next_log = Instant::now() + STATS_LOG_EVERY;
                while running.load(Ordering::Relaxed) {
                    let now = Instant::now();
                    let out = with_session(&session, &members, |s| {
                        if s.poll_timeout().is_some_and(|t| t <= now) {
                            s.handle_timeout(now);
                        }
                    });
                    send_all(&msgr, out);
                    if now >= next_log {
                        next_log = now + STATS_LOG_EVERY;
                        log_stats(&session.lock_safe());
                    }
                    // Не меньше миллисекунды: точнее выпуск не нужен, а нулевой сон —
                    // это кручение, если таймер почему-то не снялся.
                    let wait = session
                        .lock_safe()
                        .poll_timeout()
                        .map_or(RX_WAIT, |t| t.saturating_duration_since(Instant::now()))
                        .clamp(Duration::from_millis(1), RX_WAIT);
                    wait_kick(&k, wait);
                }
            }));
        }

        let target_bitrate = Arc::new(AtomicI32::new(audio.bitrate));

        // --- Отправка: PCM из Kotlin → Opus → сессия → участникам ---
        {
            let running = running.clone();
            let muted = muted.clone();
            let in_ring = in_ring.clone();
            let session = session.clone();
            let members = members.clone();
            let msgr = messenger.clone();
            let target_bitrate = target_bitrate.clone();
            let k = kick_.clone();
            threads.push(thread::spawn(move || {
                let mut enc = match AudioEncoder::new(audio) {
                    Ok(e) => e,
                    Err(_) => return,
                };
                let mut cur_bitrate = audio.bitrate;
                let mut channel_bitrate = audio.bitrate;
                let mut frame = vec![0i16; frame_len];
                let mut frames_since_loss_check = 0u32;
                while running.load(Ordering::Relaxed) {
                    let captured = {
                        let mut mic = in_ring.lock_safe();
                        let end = mic.end;
                        let q = &mut mic.pcm;
                        if q.len() >= frame_len {
                            // Первый сэмпл кадра записан раньше последнего на всё,
                            // что от него до конца кольца.
                            let behind = Duration::from_secs_f64(q.len() as f64 / pcm_rate);
                            for s in frame.iter_mut() {
                                *s = q.pop_front().unwrap_or(0);
                            }
                            end.map(|e| e.checked_sub(behind).unwrap_or(e))
                        } else {
                            None
                        }
                    };
                    let Some(captured) = captured else {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    frames_since_loss_check += 1;
                    if frames_since_loss_check >= LOSS_CHECK_FRAMES {
                        frames_since_loss_check = 0;
                        let s = session.lock_safe();
                        enc.set_expected_loss(worst_audio_loss(&s));
                        channel_bitrate = audio_bitrate(s.target_bitrate(), i32::MAX);
                    }
                    // Битрейт — из настроек звука (меняются на лету), но не больше, чем
                    // позволяет канал.
                    let want = target_bitrate.load(Ordering::Relaxed).min(channel_bitrate);
                    if want != cur_bitrate {
                        enc.set_bitrate(want);
                        cur_bitrate = want;
                    }
                    let pkt = if muted.load(Ordering::Relaxed) {
                        None
                    } else {
                        enc.encode(&frame).ok()
                    };
                    // Не отправленный кадр (микрофон выключен) номер не тратит, а
                    // метка следующего — по времени его захвата.
                    if let Some(pkt) = pkt {
                        if !running.load(Ordering::Relaxed) {
                            break;
                        }
                        let out = with_session(&session, &members, |s| {
                            s.send_audio(Instant::now(), captured, &pkt)
                        });
                        send_all(&msgr, out);
                        kick(&k);
                    }
                }
            }));
        }

        Arc::new(RoomCallEngine {
            running,
            muted,
            in_ring,
            in_cap,
            frame_len,
            audio,
            session,
            decoders: Mutex::new(HashMap::new()),
            mix_carry: Mutex::new(VecDeque::new()),
            messenger,
            members,
            levels: Mutex::new(HashMap::new()),
            target_bitrate,
            kick: kick_,
            threads: Mutex::new(threads),
        })
    }

    /// Потолок битрейта видео из настроек качества: оценка полосы до участников не
    /// поднимается выше него вместе со звуком.
    pub fn set_video_max_bitrate(&self, bps: u32) {
        let audio = self.target_bitrate.load(Ordering::Relaxed).max(0) as u32;
        self.session
            .lock_safe()
            .set_bitrate_bounds(MIN_BITRATE, bps.saturating_add(audio));
    }

    /// Сколько сейчас отдать видео-кодеру, бит/с: оценка полосы до самого слабого
    /// участника за вычетом звука и заголовков его пакетов.
    pub fn video_target_bitrate(&self) -> u32 {
        let total = self.session.lock_safe().target_bitrate();
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
        let out = with_session(&self.session, &self.members, |s| {
            let now = Instant::now();
            s.send_video(now, now.checked_sub(age).unwrap_or(now), frame, info);
            wanted = s.take_keyframe_request();
        });
        send_all(&self.messenger, out);
        // Остаток кадра выйдет ровным темпом — его выпустит поток таймеров.
        kick(&self.kick);
        wanted
    }

    /// Следующий входящий H264-кадр от peer, если его пора декодировать: LiteRTC
    /// расписывает сроки по меткам захвата (`VideoFrame::play_at`).
    pub fn pull_video_for(&self, peer: &PeerKey) -> Option<VideoFrame> {
        self.session.lock_safe().pull_video(Instant::now(), peer)
    }

    /// Сколько кадр участника идёт через наш видео-декодер (срок → экран): звук
    /// ждёт видео с учётом этого.
    pub fn set_video_render_delay(&self, peer: &PeerKey, delay: Duration) {
        self.session.lock_safe().set_video_render_delay(peer, delay);
    }

    /// Декодер видео участника начат заново — ему нужен ключевой кадр.
    pub fn request_keyframe(&self, peer: &PeerKey) {
        let out = with_session(&self.session, &self.members, |s| {
            s.request_keyframe(Instant::now(), peer)
        });
        send_all(&self.messenger, out);
        kick(&self.kick);
    }

    /// Захваченный PCM с микрофона → в очередь отправки; последний сэмпл записан
    /// `age` назад.
    pub fn push_audio(&self, samples: &[i16], age: Duration) {
        let mut mic = self.in_ring.lock_safe();
        let now = Instant::now();
        mic.end = Some(now.checked_sub(age).unwrap_or(now));
        let q = &mut mic.pcm;
        q.extend(samples.iter().copied());
        while q.len() > self.in_cap {
            q.pop_front();
        }
    }

    /// По кадру звука от каждого участника на один тик микса (без декодирования:
    /// замок сессии держим коротко) и текущий список участников. Кадр будет
    /// слышен в `heard_at`.
    fn pull_frames(&self, heard_at: Instant) -> (Vec<PeerKey>, Vec<(PeerKey, JitterOut)>) {
        let mut s = self.session.lock_safe();
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
                let mut mix = vec![0i32; self.frame_len];
                let mut any = false;
                {
                    let heard_at = now + output + Duration::from_secs_f64(i as f64 / pcm_rate);
                    let (peers, frames) = self.pull_frames(heard_at);
                    let mut decs = self.decoders.lock_safe();
                    decs.retain(|k, _| peers.contains(k));
                    let mut lv = self.levels.lock_safe();
                    // Release: плавное затухание уровня каждого кадра (держит сквозь паузы речи).
                    for v in lv.values_mut() {
                        *v *= 0.82;
                    }
                    for (key, frame) in frames {
                        let dec = match decs.entry(key) {
                            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                            // Декодер не создался — этого собеседника не слышно, но
                            // паника отравила бы мьютексы и убила звук всего звонка.
                            std::collections::hash_map::Entry::Vacant(v) => {
                                match AudioDecoder::new(self.audio) {
                                    Ok(dec) => v.insert(Decoder { dec, plc_run: 0 }),
                                    Err(_) => continue,
                                }
                            }
                        };
                        if let Some(pcm) = dec.decode(frame) {
                            let mut sum = 0f64;
                            for (i, &v) in pcm.iter().enumerate() {
                                sum += (v as f64) * (v as f64);
                                if i < mix.len() {
                                    mix[i] += v as i32;
                                }
                            }
                            let rms = (sum / pcm.len().max(1) as f64).sqrt();
                            // Индикатор «у участника мик ловит звук»: зажигаем при ЛЮБОМ
                            // звуке (порог 0.03 ≈ RMS>160), не только речи — проще и честнее.
                            // Гасит чистую тишину (RMS≈0). Decay держит сквозь паузы.
                            let norm = ((rms / 32768.0 * 6.0).min(1.0)) as f32;
                            let e = lv.entry(key).or_insert(0.0);
                            *e = e.max(norm); // мгновенный attack — зажигаем при малейшем звуке
                            any = true;
                        }
                    }
                }
                if any {
                    for &m in mix.iter() {
                        carry.push_back(m.clamp(-32768, 32767) as i16);
                    }
                } else {
                    starved = true; // никто не говорит — добиваем тишиной
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
        let s = self.session.lock_safe();
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
        self.running.store(false, Ordering::Relaxed);
        self.members.lock_safe().clear();
        self.in_ring.lock_safe().pcm.clear();
        // Неотправленное тоже никому: сессия без участников выбрасывает свою очередь.
        self.session.lock_safe().set_peers(&[]);
        kick(&self.kick);
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
