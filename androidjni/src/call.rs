//! Движок 1:1 звонка для Android. Аудио ввод/вывод приходят ИЗВНЕ из Kotlin
//! (`AudioRecord`/`AudioTrack`), кодек аудио — Opus в Rust. Видео — **без кодека
//! в Rust**: H.264 кодируется/декодируется аппаратно через MediaCodec на стороне
//! Kotlin, а Rust лишь фрагментирует исходящие access-unit'ы и собирает входящие
//! (тот же формат фрагментации, что у десктопа — совместимость по сети).
//!
//! Движок `Send` (нет cpal/!Send-объектов), живёт в Arc, доступен из JNI-потоков.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use p2p_core::media::{
    fragment_video, parse_video_frag, VideoReassembler, TRACK_AUDIO, TRACK_SCREEN, TRACK_VIDEO,
};
use p2p_core::settings::AudioSettings;
use p2p_media::{AudioDecoder, AudioEncoder, JitterBuffer, JitterOut};
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

/// Счётчики фрагментации исходящего видео.
struct VideoTx {
    frame_id: u32,
    seq: u16,
}

/// Активный звонок. Останавливается при [`stop`](CallEngine::stop)/`drop`.
pub struct CallEngine {
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    in_ring: Ring,
    out_ring: Ring,
    in_cap: usize,
    messenger: Arc<Messenger>,
    peer: PeerKey,
    video_tx: Mutex<VideoTx>,
    /// Входящие собранные H.264 access-unit'ы (для MediaCodec-декодера в Kotlin).
    video_in: Arc<Mutex<VecDeque<Vec<u8>>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl CallEngine {
    pub fn start(messenger: Arc<Messenger>, peer: PeerKey, audio: AudioSettings) -> Arc<CallEngine> {
        let running = Arc::new(AtomicBool::new(true));
        let muted = Arc::new(AtomicBool::new(false));
        let in_ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
        let out_ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
        let video_in = Arc::new(Mutex::new(VecDeque::new()));
        let cap = audio.interleaved_len() * 50;
        let mut threads = Vec::new();

        let media_rx = messenger.media_channel(1024);

        // --- Приём: демукс по track ---
        {
            let running = running.clone();
            let out_ring = out_ring.clone();
            let video_in = video_in.clone();
            let call_peer = peer;
            threads.push(thread::spawn(move || {
                let mut dec = AudioDecoder::new(audio).ok();
                let mut jitter = JitterBuffer::new(audio.jitter_depth);
                let mut reasm = VideoReassembler::new();
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
                            // Собираем фрагменты в полный H.264 access-unit и кладём
                            // в очередь для аппаратного декодера (MediaCodec/Kotlin).
                            if let Some(frag) = parse_video_frag(&p.payload) {
                                if let Some(cf) = reasm.push(frag) {
                                    let mut q = video_in.lock().unwrap();
                                    q.push_back(cf.data);
                                    while q.len() > 5 {
                                        q.pop_front(); // реалтайм — держим свежие
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

        Arc::new(CallEngine {
            running,
            muted,
            in_ring,
            out_ring,
            in_cap: cap,
            messenger,
            peer,
            video_tx: Mutex::new(VideoTx { frame_id: 0, seq: 0 }),
            video_in,
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

    /// H.264 access-unit от аппаратного энкодера (Kotlin/MediaCodec) →
    /// фрагментация → отправка по сети (TRACK_VIDEO).
    pub fn send_video(&self, nal: &[u8], keyframe: bool) {
        let mut tx = self.video_tx.lock().unwrap();
        let frame_id = tx.frame_id;
        for chunk in fragment_video(nal, frame_id, keyframe) {
            let _ = self.messenger.send_media(&self.peer, TRACK_VIDEO, tx.seq, frame_id, &chunk);
            tx.seq = tx.seq.wrapping_add(1);
        }
        tx.frame_id = tx.frame_id.wrapping_add(1);
    }

    /// Забрать собранный входящий H.264 access-unit (для MediaCodec-декодера).
    pub fn pull_video_packet(&self) -> Option<Vec<u8>> {
        self.video_in.lock().unwrap().pop_front()
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
