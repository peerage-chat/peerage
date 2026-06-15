//! Движок 1:1 звонка для Android. Аудио ввод/вывод приходят ИЗВНЕ из Kotlin
//! (`AudioRecord`/`AudioTrack`), кодек аудио — Opus в Rust. Видео — **без кодека
//! в Rust**: H.264 кодируется/декодируется аппаратно через MediaCodec на стороне
//! Kotlin, а Rust лишь фрагментирует исходящие access-unit'ы и собирает входящие
//! (тот же формат фрагментации, что у десктопа — совместимость по сети).
//!
//! Движок `Send` (нет cpal/!Send-объектов), живёт в Arc, доступен из JNI-потоков.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
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

/// Состояние воспроизведения аудио. Приёмный поток только КЛАДЁТ пакеты в
/// `jitter`; декодирование, PLC и выдача PCM происходят в [`pull_audio`] —
/// ровным темпом потребителя (`AudioTrack`). Это даёт реальное сглаживание
/// джиттера и не даёт видео-нагрузке сбить аудио-конвейер.
struct Playout {
    dec: Option<AudioDecoder>,
    jitter: JitterBuffer,
    /// Декодированные, но ещё не отданные сэмплы (хвост кадра между вызовами).
    carry: VecDeque<i16>,
    /// Подряд идущих PLC-кадров. Opus PLC хорош на 1-2 кадра; дольше — уходит в
    /// шум, поэтому после [`MAX_PLC`] выдаём тишину.
    plc_run: u32,
}

/// Активный звонок. Останавливается при [`stop`](CallEngine::stop)/`drop`.
pub struct CallEngine {
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    in_ring: Ring,
    in_cap: usize,
    /// Длина interleaved-кадра (сэмплов) — для тишины/PLC при потерях.
    frame_len: usize,
    /// Аудио-воспроизведение (джиттер + декодер), под общим замком.
    playout: Arc<Mutex<Playout>>,
    /// Исходящие H.264 access-unit'ы (nal, keyframe). Bounded — при заторе сети
    /// (блокирующий WriteTo) новые кадры ДРОПАЮТСЯ, чтобы не застопорить дренаж
    /// энкодера/камеру. Фрагментация и отправка — в выделенном потоке.
    video_out: SyncSender<(Vec<u8>, bool)>,
    /// Входящие собранные H.264 access-unit'ы (для MediaCodec-декодера в Kotlin).
    video_in: Arc<Mutex<VecDeque<Vec<u8>>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl CallEngine {
    pub fn start(messenger: Arc<Messenger>, peer: PeerKey, audio: AudioSettings) -> Arc<CallEngine> {
        let running = Arc::new(AtomicBool::new(true));
        let muted = Arc::new(AtomicBool::new(false));
        let in_ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
        let video_in = Arc::new(Mutex::new(VecDeque::new()));
        let cap = audio.interleaved_len() * 50;
        let frame_len = audio.interleaved_len();
        let playout = Arc::new(Mutex::new(Playout {
            dec: AudioDecoder::new(audio).ok(),
            jitter: JitterBuffer::new(audio.jitter_depth),
            carry: VecDeque::new(),
            plc_run: 0,
        }));
        let mut threads = Vec::new();

        let media_rx = messenger.media_channel(1024);

        // --- Приём: демукс по track ---
        {
            let running = running.clone();
            let playout = playout.clone();
            let video_in = video_in.clone();
            let call_peer = peer;
            threads.push(thread::spawn(move || {
                let mut reasm = VideoReassembler::new();
                while running.load(Ordering::Relaxed) {
                    let Ok(p) = media_rx.recv_timeout(Duration::from_millis(200)) else { continue };
                    if p.src != call_peer {
                        continue; // принимаем медиа только от собеседника
                    }
                    match p.track {
                        TRACK_AUDIO => {
                            // Только в джиттер — декод/PLC делает pull_audio ровным
                            // темпом AudioTrack (видео-нагрузка не сбивает аудио).
                            playout.lock().unwrap().jitter.push(p.seq, p.payload);
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

        // --- Видео-отправка: bounded-очередь → фрагментация → send_media ---
        // Выделенный поток: блокирующий WriteTo (backpressure Yggdrasil) тормозит
        // ТОЛЬКО его, а bounded-канал (cap 4) дропает лишние кадры. Так дренаж
        // энкодера в Kotlin (callSendVideo→try_send) никогда не блокируется →
        // энкодер дренится → камера не таймаутит.
        let (video_out, video_rx) = sync_channel::<(Vec<u8>, bool)>(4);
        {
            let running = running.clone();
            let msgr = messenger.clone();
            threads.push(thread::spawn(move || {
                let mut frame_id = 0u32;
                let mut seq = 0u16;
                while running.load(Ordering::Relaxed) {
                    let (nal, key) = match video_rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(v) => v,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    for chunk in fragment_video(&nal, frame_id, key) {
                        let _ = msgr.send_media(&peer, TRACK_VIDEO, seq, frame_id, &chunk);
                        seq = seq.wrapping_add(1);
                    }
                    frame_id = frame_id.wrapping_add(1);
                }
            }));
        }

        Arc::new(CallEngine {
            running,
            muted,
            in_ring,
            in_cap: cap,
            frame_len,
            playout,
            video_out,
            video_in,
            threads: Mutex::new(threads),
        })
    }

    /// Захваченный PCM (interleaved i16) с микрофона → в очередь отправки.
    pub fn push_audio(&self, samples: &[i16]) {
        push_capped(&self.in_ring, samples, self.in_cap);
    }

    /// Заполнить буфер декодированным PCM для воспроизведения. Возвращает число
    /// реальных сэмплов (хвост — тишина, если данных нет).
    ///
    /// Декод и PLC происходят ЗДЕСЬ, ровным темпом потребителя (`AudioTrack`),
    /// а не в приёмном потоке. Это даёт настоящее джиттер-сглаживание (глубина
    /// учитывается) и устойчивость к видео-нагрузке. При длительной потере
    /// выдаём тишину, а не растущий PLC (который у Opus уходит в шум).
    pub fn pull_audio(&self, out: &mut [i16]) -> usize {
        const MAX_PLC: u32 = 3; // ~60мс маскировки, дальше — тишина
        let mut p = self.playout.lock().unwrap();
        let mut produced = 0usize;
        let mut starved = false;
        for o in out.iter_mut() {
            if p.carry.is_empty() && !starved {
                match p.jitter.pop() {
                    Some(JitterOut::Frame(pkt)) => {
                        p.plc_run = 0;
                        if let Some(d) = p.dec.as_mut() {
                            if let Ok(pcm) = d.decode(Some(&pkt)) {
                                p.carry.extend(pcm);
                            }
                        }
                    }
                    Some(JitterOut::Lost) => {
                        if p.plc_run < MAX_PLC {
                            p.plc_run += 1;
                            if let Some(d) = p.dec.as_mut() {
                                if let Ok(pcm) = d.decode(None) {
                                    p.carry.extend(pcm);
                                }
                            }
                        } else {
                            // длительная потеря — тишина вместо PLC-шума
                            for _ in 0..self.frame_len {
                                p.carry.push_back(0);
                            }
                        }
                    }
                    None => starved = true, // буфер набирается/пуст — добиваем тишиной
                }
            }
            match p.carry.pop_front() {
                Some(s) => {
                    *o = s;
                    produced += 1;
                }
                None => *o = 0,
            }
        }
        produced
    }

    /// H.264 access-unit от аппаратного энкодера (Kotlin/MediaCodec) → в очередь
    /// отправки. НЕ блокирует: при заторе сети кадр дропается (реалтайм).
    pub fn send_video(&self, nal: &[u8], keyframe: bool) {
        let _ = self.video_out.try_send((nal.to_vec(), keyframe));
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
