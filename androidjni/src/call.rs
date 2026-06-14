//! Движок 1:1 звонка для Android. В отличие от десктопного `p2p-call` (cpal),
//! здесь аудио ввод/вывод и захват видео приходят ИЗВНЕ — из Kotlin:
//! `AudioRecord` → [`CallEngine::push_audio`], [`CallEngine::pull_audio`] →
//! `AudioTrack`; камера/экран → [`CallEngine::push_video`]. Так движок не зависит
//! от платформенных устройств и `Send` (cpal-поток был бы `!Send`).
//!
//! Сетевой путь и кодеки — те же примитивы, что у десктопа (`p2p-media` +
//! `p2p-core::media`): Opus + джиттер для аудио, H.264 + фрагментация для видео.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use p2p_core::media::{
    fragment_video, parse_video_frag, VideoReassembler, TRACK_AUDIO, TRACK_SCREEN, TRACK_VIDEO,
};
use p2p_core::settings::{AudioSettings, VideoSettings};
use p2p_media::{
    push_source, AudioDecoder, AudioEncoder, DecodedFrame, FrameFeeder, JitterBuffer, JitterOut,
    VideoDecoder, VideoEncoder,
};
use p2p_messenger::Messenger;

type PeerKey = [u8; 32];
type Ring = Arc<Mutex<VecDeque<i16>>>;

fn push_capped(ring: &Ring, samples: &[i16], cap: usize) {
    let mut q = ring.lock().unwrap();
    q.extend(samples.iter().copied());
    while q.len() > cap {
        q.pop_front();
    }
}

/// Активный звонок. Останавливается при [`stop`](CallEngine::stop)/`drop`.
pub struct CallEngine {
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    in_ring: Ring,
    out_ring: Ring,
    in_cap: usize,
    feeder: Option<FrameFeeder>,
    frames: Arc<Mutex<VecDeque<DecodedFrame>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl CallEngine {
    /// Запустить движок. `video` + (`vw`,`vh`) — если шлём видео (камера/экран).
    pub fn start(
        messenger: Arc<Messenger>,
        peer: PeerKey,
        audio: AudioSettings,
        video: Option<VideoSettings>,
    ) -> Arc<CallEngine> {
        let running = Arc::new(AtomicBool::new(true));
        let muted = Arc::new(AtomicBool::new(false));
        let in_ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
        let out_ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
        let frames = Arc::new(Mutex::new(VecDeque::new()));
        let cap = audio.interleaved_len() * 50;
        let mut threads = Vec::new();

        let media_rx = messenger.media_channel(1024);

        // --- Приём: демукс по track ---
        {
            let running = running.clone();
            let out_ring = out_ring.clone();
            let frames = frames.clone();
            let call_peer = peer;
            threads.push(thread::spawn(move || {
                let mut dec = AudioDecoder::new(audio).ok();
                let mut jitter = JitterBuffer::new(audio.jitter_depth);
                let mut reasm = VideoReassembler::new();
                let mut video_dec = VideoDecoder::new().ok();
                while running.load(Ordering::Relaxed) {
                    let Ok(p) = media_rx.recv_timeout(Duration::from_millis(200)) else { continue };
                    if p.src != call_peer {
                        continue; // принимаем медиа только от собеседника
                    }
                    match p.track {
                        TRACK_AUDIO => {
                            if let Some(d) = dec.as_mut() {
                                jitter.push(p.seq, p.payload);
                                while let Some(out) = jitter.pop() {
                                    let pcm = match out {
                                        JitterOut::Frame(pkt) => d.decode(Some(&pkt)),
                                        JitterOut::Lost => d.decode(None),
                                    };
                                    if let Ok(pcm) = pcm {
                                        push_capped(&out_ring, &pcm, cap);
                                    }
                                }
                            }
                        }
                        TRACK_VIDEO | TRACK_SCREEN => {
                            if let Some(frag) = parse_video_frag(&p.payload) {
                                if let Some(cf) = reasm.push(frag) {
                                    if let Some(d) = video_dec.as_mut() {
                                        if let Ok(Some(frame)) = d.decode(&cf.data) {
                                            let mut q = frames.lock().unwrap();
                                            q.push_back(frame);
                                            while q.len() > 3 {
                                                q.pop_front(); // держим только свежие кадры
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }));
        }

        // --- Аудио-отправка: PCM из Kotlin → Opus → send_media ---
        {
            let running = running.clone();
            let muted = muted.clone();
            let in_ring = in_ring.clone();
            let msgr = messenger.clone();
            let frame_len = audio.interleaved_len();
            let fsamps = audio.frame_samples() as u32;
            threads.push(thread::spawn(move || {
                let mut encoder = match AudioEncoder::new(audio) {
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
                        if let Ok(pkt) = encoder.encode(&frame) {
                            let _ = msgr.send_media(&peer, TRACK_AUDIO, seq, ts, &pkt);
                        }
                    }
                    seq = seq.wrapping_add(1);
                    ts = ts.wrapping_add(fsamps);
                }
            }));
        }

        // --- Видео-отправка: RGB-кадры из Kotlin → H.264 → фрагменты → send_media ---
        let feeder = if let Some(vcfg) = video {
            let (feeder, mut src) = push_source(vcfg.width, vcfg.height);
            let running = running.clone();
            let msgr = messenger.clone();
            let interval = Duration::from_millis((1000 / vcfg.fps.max(1)) as u64);
            threads.push(thread::spawn(move || {
                use p2p_media::VideoSource;
                let mut encoder = match VideoEncoder::new(vcfg) {
                    Ok(e) => e,
                    Err(_) => return,
                };
                let mut frame_id = 0u32;
                let mut seq = 0u16;
                while running.load(Ordering::Relaxed) {
                    let t0 = Instant::now();
                    if let Some(rgb) = src.next_frame() {
                        if let Ok((bs, key)) = encoder.encode_rgb(&rgb) {
                            for chunk in fragment_video(&bs, frame_id, key) {
                                let _ = msgr.send_media(&peer, TRACK_VIDEO, seq, frame_id, &chunk);
                                seq = seq.wrapping_add(1);
                            }
                            frame_id = frame_id.wrapping_add(1);
                        }
                    }
                    if let Some(rem) = interval.checked_sub(t0.elapsed()) {
                        thread::sleep(rem);
                    }
                }
            }));
            Some(feeder)
        } else {
            None
        };

        Arc::new(CallEngine {
            running,
            muted,
            in_ring,
            out_ring,
            in_cap: cap,
            feeder,
            frames,
            threads: Mutex::new(threads),
        })
    }

    /// Захваченный PCM (interleaved i16) с микрофона → в очередь отправки.
    pub fn push_audio(&self, samples: &[i16]) {
        push_capped(&self.in_ring, samples, self.in_cap);
    }

    /// Заполнить буфер декодированным PCM для воспроизведения. Возвращает число
    /// записанных сэмплов (хвост — тишина, если данных нет).
    pub fn pull_audio(&self, out: &mut [i16]) -> usize {
        let mut q = self.out_ring.lock().unwrap();
        let mut n = 0;
        for o in out.iter_mut() {
            match q.pop_front() {
                Some(s) => {
                    *o = s;
                    n += 1;
                }
                None => *o = 0,
            }
        }
        n
    }

    /// Свежий RGB8-кадр с камеры/экрана → в очередь видео-отправки.
    pub fn push_video(&self, rgb: Vec<u8>) {
        if let Some(f) = &self.feeder {
            f.push(rgb);
        }
    }

    /// Забрать декодированный входящий видео-кадр (RGB8) для отрисовки.
    pub fn pull_frame(&self) -> Option<DecodedFrame> {
        self.frames.lock().unwrap().pop_front()
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        for h in self.threads.lock().unwrap().drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for CallEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
