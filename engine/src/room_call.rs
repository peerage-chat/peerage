//! Мультипати аудио-движок комнаты (Discord-style): один захват с микрофона →
//! мультикаст ВСЕМ текущим участникам; приём от каждого с отдельным
//! джиттером/декодером и миксом в один выход. Темп воспроизведения задаёт
//! потребитель (`AudioTrack` через [`pull_audio`]) — как в 1:1 ([`crate::call`]).
//!
//! Видео в комнате — следующий шаг (H1.1b): захват мультикастится так же, а
//! приём пойдёт по источникам в отдельные очереди (для сетки H1.2).
//!
//! Список участников (`members`) динамический: его обновляет presence-слой
//! (lib.rs) по мере входа/выхода. Движок `Send` — живёт в `Arc`, зовётся из JNI.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use peerage_core::media::{
    fragment_video, parse_video_frag, CompleteFrame, VideoReassembler,
    TRACK_AUDIO, TRACK_VIDEO,
};
use peerage_core::settings::AudioSettings;
use peerage_media::{AudioDecoder, AudioEncoder, JitterBuffer, JitterOut};
use peerage_messenger::Messenger;

use crate::LockRecover;

type PeerKey = [u8; 32];
type Ring = Arc<Mutex<VecDeque<i16>>>;

/// Подряд идущих PLC-кадров на источник; дальше — тишина (Opus PLC иначе шумит).
const MAX_PLC: u32 = 4;

/// Дефолтный кап видео-очереди в кадрах (≈300мс при 30fps), пока Kotlin не уточнит
/// под разрешение/fps через [`RoomCallEngine::set_video_cap`]. Декодер пейсит вывод
/// по wall-clock, поэтому бэклог в очереди = постоянная задержка.
const VID_CAP_DEFAULT: usize = 9;

/// Предел входящей видео-очереди участника в байтах — поверх предела в кадрах:
/// кадр бывает до 8 МБ (`MAX_VIDEO_FRAGS`), и 60 таких — полгигабайта.
const VID_QUEUE_BYTES: usize = 32 << 20;

/// Состояние приёма одного участника: джиттер + декодер + счётчик PLC + расчет джиттера.
struct Source {
    dec: AudioDecoder,
    jitter: JitterBuffer,
    plc_run: u32,
    last_ts: Option<u32>,
    last_arrival: Option<std::time::Instant>,
    jitter_estimate_ms: f64,
}

impl Source {
    fn new(audio: AudioSettings) -> Option<Source> {
        Some(Source {
            dec: AudioDecoder::new(audio).ok()?,
            jitter: JitterBuffer::new(audio.jitter_depth),
            plc_run: 0,
            last_ts: None,
            last_arrival: None,
            jitter_estimate_ms: 30.0,
        })
    }

    fn push(&mut self, seq: u16, ts: u32, payload: Vec<u8>, sample_rate: f64) {
        let now = std::time::Instant::now();
        if let (Some(l_ts), Some(l_arr)) = (self.last_ts, self.last_arrival) {
            let send_diff_samples = ts.wrapping_sub(l_ts) as i32;
            let send_diff_ms = (send_diff_samples as f64) / (sample_rate / 1000.0);
            let recv_diff_ms = now.saturating_duration_since(l_arr).as_secs_f64() * 1000.0;
            let d = (recv_diff_ms - send_diff_ms).abs();
            let alpha = if d > self.jitter_estimate_ms { 0.25 } else { 0.0625 };
            self.jitter_estimate_ms += (d - self.jitter_estimate_ms) * alpha;
            
            let target_depth = ((self.jitter_estimate_ms / 20.0).round() as usize + 2).clamp(2, 8);
            self.jitter.set_depth(target_depth);
        }
        self.last_ts = Some(ts);
        self.last_arrival = Some(now);
        self.jitter.push(seq, payload);
    }

    /// Один кадр PCM этого источника на текущий тик микса (или `None`, если в
    /// буфере пусто — источник вносит тишину).
    fn next_frame(&mut self) -> Option<Vec<i16>> {
        match self.jitter.pop()? {
            JitterOut::Frame(pkt) => {
                self.plc_run = 0;
                self.dec.decode(Some(&pkt)).ok()
            }
            JitterOut::Lost => {
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

/// Активный мультипати аудио-звонок комнаты.
pub struct RoomCallEngine {
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    in_ring: Ring,
    in_cap: usize,
    frame_len: usize,
    /// Приёмное состояние на каждого участника.
    sources: Arc<Mutex<HashMap<PeerKey, Source>>>,
    /// Смешанный PCM, ещё не отданный потребителю.
    mix_carry: Mutex<VecDeque<i16>>,
    /// Входящие собранные H264-кадры per-peer (после реассемблера): (данные, keyframe).
    vid_sources: Arc<Mutex<HashMap<PeerKey, VecDeque<(Vec<u8>, bool)>>>>,
    /// Для push_video: мультикаст исходящих H264-кадров.
    messenger: Arc<Messenger>,
    members: Arc<Mutex<Vec<PeerKey>>>,
    vid_seq: AtomicU16,
    /// Номер кадра на проводе — u32: 16 бит оборачивались за 36 минут при 30 fps.
    vid_frame_id: AtomicU32,
    /// Кап видео-очереди в кадрах (зависит от fps, см. [`set_video_fps`]).
    vid_cap: Arc<AtomicUsize>,
    /// Уровень речи на участника (0..1, сглажен): attack мгновенный, decay плавный.
    /// Считается в [`pull_audio`] по RMS кадра источника. Для active-speaker рамки.
    levels: Mutex<HashMap<PeerKey, f32>>,
    /// Целевой битрейт Opus (бит/с): меняется на лету из настроек звука, send-тред
    /// применяет к энкодеру без пересоздания сессии (чтобы не было дыры в звуке).
    target_bitrate: Arc<AtomicI32>,
    /// Накопленные байты приёма НА УЧАСТНИКА (аудио+видео) — для тех-инфо трафика.
    rx_bytes: Arc<Mutex<HashMap<PeerKey, u64>>>,
    /// Накопленные байты отправки (всего по участникам) — «свой» исходящий трафик.
    tx_bytes: Arc<AtomicU64>,
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
        let in_ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
        let sources: Arc<Mutex<HashMap<PeerKey, Source>>> = Arc::new(Mutex::new(HashMap::new()));
        let vid_sources: Arc<Mutex<HashMap<PeerKey, VecDeque<(Vec<u8>, bool)>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let in_cap = audio.interleaved_len() * 50;
        let frame_len = audio.interleaved_len();
        // Дефолт; уточняется set_video_cap() из Kotlin при старте видео.
        let vid_cap = Arc::new(AtomicUsize::new(VID_CAP_DEFAULT));
        let rx_bytes: Arc<Mutex<HashMap<PeerKey, u64>>> = Arc::new(Mutex::new(HashMap::new()));
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let mut threads = Vec::new();

        let media_rx = messenger.media_channel(2048);

        // --- Приём: аудио → джиттер; видео → реассемблер → per-peer очередь ---
        {
            let running = running.clone();
            let sources = sources.clone();
            let vid_sources = vid_sources.clone();
            let members = members.clone();
            let vid_cap = vid_cap.clone();
            let rx_bytes = rx_bytes.clone();
            let sample_rate = audio.sample_rate as f64;
            threads.push(thread::spawn(move || {
                // VideoReassembler per-peer: собирает фрагменты в целые H264-кадры.
                let mut vid_asm: HashMap<PeerKey, VideoReassembler> = HashMap::new();
                while running.load(Ordering::Relaxed) {
                    let p = match media_rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(p) => p,
                        Err(RecvTimeoutError::Timeout) => continue,
                        // Канал отобрали (новый media_channel) — приёма больше не будет,
                        // а крутиться на мгновенно возвращающемся recv значит 100 % CPU.
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    if !members.lock_safe().contains(&p.src) {
                        continue;
                    }
                    *rx_bytes.lock_safe().entry(p.src).or_insert(0) += p.payload.len() as u64;
                    if p.track == TRACK_VIDEO {
                        if let Some(frag) = parse_video_frag(&p.payload) {
                            let asm = vid_asm.entry(p.src).or_insert_with(VideoReassembler::new);
                            if let Some(CompleteFrame { data, keyframe, .. }) = asm.push(frag) {
                                let mut vs = vid_sources.lock_safe();
                                let q = vs.entry(p.src).or_default();
                                q.push_back((data, keyframe));
                                // Анти-латентность: декодер пейсит вывод по wall-clock от
                                // первого кадра, поэтому бэклог в очереди = постоянная
                                // задержка. Держим кап (≈300мс, см. VID_BUDGET_MS); при
                                // переполнении ПРЫГАЕМ к свежему keyframe (decoder
                                // ресинкается чисто), а не тянем устаревшие кадры по одному.
                                let cap = vid_cap.load(Ordering::Relaxed).max(4);
                                if q.len() > cap {
                                    if let Some(kf) = q.iter().rposition(|(_, k)| *k) {
                                        if kf > 0 { q.drain(0..kf); }
                                    }
                                    while q.len() > cap { q.pop_front(); }
                                }
                                while q.len() > 1 && q.iter().map(|(d, _)| d.len()).sum::<usize>() > VID_QUEUE_BYTES {
                                    q.pop_front();
                                }
                            }
                        }
                        continue;
                    }
                    if p.track != TRACK_AUDIO { continue; }
                    let mut srcs = sources.lock_safe();
                    let src = match srcs.entry(p.src) {
                        Entry::Occupied(e) => e.into_mut(),
                        // Декодер не создался — этого собеседника не слышно, но
                        // паника отравила бы мьютексы и убила звук всего звонка.
                        Entry::Vacant(v) => match Source::new(audio) {
                            Some(src) => v.insert(src),
                            None => continue,
                        },
                    };
                    src.push(p.seq, p.ts, p.payload, sample_rate);
                }
            }));
        }

        let target_bitrate = Arc::new(AtomicI32::new(audio.bitrate));

        // --- Отправка: PCM из Kotlin → Opus → мультикаст участникам ---
        {
            let running = running.clone();
            let muted = muted.clone();
            let in_ring = in_ring.clone();
            let members = members.clone();
            let msgr = messenger.clone();
            let fsamps = audio.frame_samples() as u32;
            let target_bitrate = target_bitrate.clone();
            let tx_bytes = tx_bytes.clone();
            threads.push(thread::spawn(move || {
                let mut enc = match AudioEncoder::new(audio) {
                    Ok(e) => e,
                    Err(_) => return,
                };
                let mut cur_bitrate = audio.bitrate;
                let mut seq = 0u16;
                let mut ts = 0u32;
                let mut frame = vec![0i16; frame_len];
                while running.load(Ordering::Relaxed) {
                    let ready = {
                        let mut q = in_ring.lock_safe();
                        if q.len() >= frame_len {
                            for s in frame.iter_mut() {
                                *s = q.pop_front().unwrap_or(0);
                            }
                            true
                        } else {
                            false
                        }
                    };
                    if !ready {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    // Битрейт сменили в настройках звука → применяем на лету.
                    let want = target_bitrate.load(Ordering::Relaxed);
                    if want != cur_bitrate {
                        enc.set_bitrate(want);
                        cur_bitrate = want;
                    }
                    if !muted.load(Ordering::Relaxed) {
                        if let Ok(pkt) = enc.encode(&frame) {
                            if !running.load(Ordering::Relaxed) {
                                break;
                            }
                            let ms = members.lock_safe();
                            for m in ms.iter() {
                                let _ = msgr.send_media(m, TRACK_AUDIO, seq, ts, &pkt);
                            }
                            tx_bytes.fetch_add((pkt.len() * ms.len()) as u64, Ordering::Relaxed);
                        }
                    }
                    seq = seq.wrapping_add(1);
                    ts = ts.wrapping_add(fsamps);
                }
            }));
        }

        Arc::new(RoomCallEngine {
            running,
            muted,
            in_ring,
            in_cap,
            frame_len,
            sources,
            mix_carry: Mutex::new(VecDeque::new()),
            vid_sources,
            messenger,
            members,
            vid_seq: AtomicU16::new(0),
            vid_frame_id: AtomicU32::new(0),
            vid_cap,
            levels: Mutex::new(HashMap::new()),
            target_bitrate,
            rx_bytes,
            tx_bytes,
            threads: Mutex::new(threads),
        })
    }

    /// Задать кап входящей видео-очереди (в кадрах). Политику считает Kotlin: он
    /// знает разрешение/fps и даёт тяжёлым режимам (1080p) больший запас на пачки,
    /// а лёгким — низкую задержку.
    pub fn set_video_cap(&self, frames: usize) {
        self.vid_cap.store(frames.clamp(4, 60), Ordering::Relaxed);
    }

    /// Закодированный H264 access-unit → фрагментация + мультикаст всем участникам.
    pub fn push_video(&self, frame: &[u8], keyframe: bool) {
        let frame_id = self.vid_frame_id.fetch_add(1, Ordering::Relaxed);
        let fragments = fragment_video(frame, frame_id, keyframe);
        let members = self.members.lock_safe();
        for chunk in &fragments {
            let seq = self.vid_seq.fetch_add(1, Ordering::Relaxed);
            for m in members.iter() {
                let _ = self.messenger.send_media(m, TRACK_VIDEO, seq, 0, chunk);
            }
            self.tx_bytes.fetch_add((chunk.len() * members.len()) as u64, Ordering::Relaxed);
        }
    }

    /// Следующий входящий H264-пакет от peer, или None если очередь пуста.
    pub fn pull_video_for(&self, peer: &PeerKey) -> Option<Vec<u8>> {
        self.vid_sources.lock_safe().get_mut(peer)?.pop_front().map(|(d, _)| d)
    }

    /// Захваченный PCM с микрофона → в очередь отправки.
    pub fn push_audio(&self, samples: &[i16]) {
        let mut q = self.in_ring.lock_safe();
        q.extend(samples.iter().copied());
        while q.len() > self.in_cap {
            q.pop_front();
        }
    }

    /// Смешать все источники в `out` ровным темпом потребителя. На каждый
    /// микс-кадр берём по одному кадру с каждого источника и суммируем.
    pub fn pull_audio(&self, out: &mut [i16]) -> usize {
        let mut carry = self.mix_carry.lock_safe();
        let mut produced = 0usize;
        let mut starved = false;
        for o in out.iter_mut() {
            if carry.is_empty() && !starved {
                let mut mix = vec![0i32; self.frame_len];
                let mut any = false;
                {
                    let mut srcs = self.sources.lock_safe();
                    let mut lv = self.levels.lock_safe();
                    // Release: плавное затухание уровня каждого кадра (держит сквозь паузы речи).
                    for v in lv.values_mut() {
                        *v *= 0.82;
                    }
                    for (key, s) in srcs.iter_mut() {
                        if let Some(pcm) = s.next_frame() {
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
                            let e = lv.entry(*key).or_insert(0.0);
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
        let arr: Vec<String> =
            lv.iter().filter(|(_, v)| **v > 0.03).map(|(k, _)| hex::encode(k)).collect();
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
        let rx = self.rx_bytes.lock_safe();
        let mut m: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
        for (k, v) in rx.iter() {
            m.insert(hex::encode(k), *v);
        }
        // Ключ "self" — свой исходящий трафик: клиенту он нужен рядом с чужими,
        // чтобы показать ↓/↑ одной строкой.
        m.insert("self".into(), self.tx_bytes.load(Ordering::Relaxed));
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
        self.in_ring.lock_safe().clear();
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
