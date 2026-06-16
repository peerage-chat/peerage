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

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use p2p_core::media::TRACK_AUDIO;
use p2p_core::settings::AudioSettings;
use p2p_media::{AudioDecoder, AudioEncoder, JitterBuffer, JitterOut};
use p2p_messenger::Messenger;

type PeerKey = [u8; 32];
type Ring = Arc<Mutex<VecDeque<i16>>>;

/// Подряд идущих PLC-кадров на источник; дальше — тишина (Opus PLC иначе шумит).
const MAX_PLC: u32 = 3;

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
            self.jitter_estimate_ms += (d - self.jitter_estimate_ms) / 16.0;
            
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
        let in_cap = audio.interleaved_len() * 50;
        let frame_len = audio.interleaved_len();
        let mut threads = Vec::new();

        let media_rx = messenger.media_channel(2048);

        // --- Приём: аудио каждого участника → в его джиттер ---
        {
            let running = running.clone();
            let sources = sources.clone();
            let members = members.clone();
            let sample_rate = audio.sample_rate as f64;
            threads.push(thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    let Ok(p) = media_rx.recv_timeout(Duration::from_millis(200)) else { continue };
                    if p.track != TRACK_AUDIO {
                        continue; // видео в комнате — отдельный шаг
                    }
                    if !members.lock().unwrap().contains(&p.src) {
                        continue; // только от участников комнаты
                    }
                    sources
                        .lock()
                        .unwrap()
                        .entry(p.src)
                        .or_insert_with(|| Source::new(audio).expect("opus decoder"))
                        .push(p.seq, p.ts, p.payload, sample_rate);
                }
            }));
        }

        // --- Отправка: PCM из Kotlin → Opus → мультикаст участникам ---
        {
            let running = running.clone();
            let muted = muted.clone();
            let in_ring = in_ring.clone();
            let members = members.clone();
            let msgr = messenger.clone();
            let fsamps = audio.frame_samples() as u32;
            threads.push(thread::spawn(move || {
                let mut enc = match AudioEncoder::new(audio) {
                    Ok(e) => e,
                    Err(_) => return,
                };
                let mut seq = 0u16;
                let mut ts = 0u32;
                let mut frame = vec![0i16; frame_len];
                while running.load(Ordering::Relaxed) {
                    let ready = {
                        let mut q = in_ring.lock().unwrap();
                        if q.len() >= frame_len {
                            for s in frame.iter_mut() {
                                *s = q.pop_front().unwrap();
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
                    if !muted.load(Ordering::Relaxed) {
                        if let Ok(pkt) = enc.encode(&frame) {
                            for m in members.lock().unwrap().iter() {
                                let _ = msgr.send_media(m, TRACK_AUDIO, seq, ts, &pkt);
                            }
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
            threads: Mutex::new(threads),
        })
    }

    /// Захваченный PCM с микрофона → в очередь отправки.
    pub fn push_audio(&self, samples: &[i16]) {
        let mut q = self.in_ring.lock().unwrap();
        q.extend(samples.iter().copied());
        while q.len() > self.in_cap {
            q.pop_front();
        }
    }

    /// Смешать все источники в `out` ровным темпом потребителя. На каждый
    /// микс-кадр берём по одному кадру с каждого источника и суммируем.
    pub fn pull_audio(&self, out: &mut [i16]) -> usize {
        let mut carry = self.mix_carry.lock().unwrap();
        let mut produced = 0usize;
        let mut starved = false;
        for o in out.iter_mut() {
            if carry.is_empty() && !starved {
                let mut mix = vec![0i32; self.frame_len];
                let mut any = false;
                {
                    let mut srcs = self.sources.lock().unwrap();
                    for s in srcs.values_mut() {
                        if let Some(pcm) = s.next_frame() {
                            for (i, &v) in pcm.iter().enumerate() {
                                if i < mix.len() {
                                    mix[i] += v as i32;
                                }
                            }
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

    pub fn set_muted(&self, m: bool) {
        self.muted.store(m, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        for h in self.threads.lock().unwrap().drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for RoomCallEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
