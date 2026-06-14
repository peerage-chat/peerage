//! p2p-call — движок звонка: аудио и/или видео поверх лёгкого медиа-пути.
//!
//! Один потребитель медиа-канала демультиплексирует по track: аудио →
//! джиттер → Opus → динамики; видео/экран → реассемблер → H.264 → кадры RGB
//! для отрисовки. Захват (микрофон/камера/экран) и параметры — настраиваемы.
//!
//! [`Call`] держит cpal-потоки и НЕ `Send` — используйте на одном потоке.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream, StreamConfig};

use p2p_core::media::{fragment_video, parse_video_frag, VideoReassembler, TRACK_AUDIO, TRACK_SCREEN, TRACK_VIDEO};
use p2p_core::settings::{AudioSettings, VideoSettings};
use p2p_media::{AudioDecoder, AudioEncoder, DecodedFrame, JitterBuffer, JitterOut, VideoDecoder, VideoEncoder, VideoSource};
use p2p_messenger::Messenger;

pub type PeerKey = [u8; 32];

pub use p2p_media::VideoSource as Source;

#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("нет устройства ввода (микрофона)")]
    NoInput,
    #[error("нет устройства вывода (динамиков)")]
    NoOutput,
    #[error("cpal: {0}")]
    BuildStream(String),
    #[error("кодек: {0}")]
    Codec(#[from] p2p_media::CodecError),
    #[error("видео: {0}")]
    Video(#[from] p2p_media::VideoError),
}

type Ring = Arc<Mutex<VecDeque<i16>>>;

fn push_capped(ring: &Ring, samples: &[i16], cap: usize) {
    let mut q = ring.lock().unwrap();
    q.extend(samples.iter().copied());
    while q.len() > cap {
        q.pop_front();
    }
}

/// Активный звонок. Останавливается при `stop()`/`drop`.
pub struct Call {
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    _in_stream: Option<Stream>,
    _out_stream: Option<Stream>,
    threads: Vec<JoinHandle<()>>,
    frames: Option<Receiver<DecodedFrame>>,
}

impl Call {
    /// Запустить звонок.
    /// * `audio` — настройки аудио (Some → захват/воспроизведение через cpal).
    /// * `video` + `source` — если заданы, мы захватываем и шлём видео.
    /// Приём видео работает всегда (входящие кадры доступны через [`take_frames`]).
    pub fn start(
        messenger: Arc<Messenger>,
        peer: PeerKey,
        audio: Option<AudioSettings>,
        video: Option<VideoSettings>,
        source: Option<Box<dyn VideoSource + Send>>,
    ) -> Result<Call, CallError> {
        let running = Arc::new(AtomicBool::new(true));
        let muted = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        let media_rx = messenger.media_channel(1024);
        let (frame_tx, frame_rx) = sync_channel::<DecodedFrame>(8);

        // Кольцо вывода аудио (если аудио включено).
        let out_ring: Option<Ring> = audio.map(|_| Arc::new(Mutex::new(VecDeque::new())));

        // --- Приём: демукс по track ---
        {
            let running = running.clone();
            let out_ring = out_ring.clone();
            let audio_cfg = audio;
            threads.push(thread::spawn(move || {
                let mut audio_dec = audio_cfg.and_then(|c| AudioDecoder::new(c).ok());
                let mut jitter = audio_cfg.map(|c| JitterBuffer::new(c.jitter_depth));
                let mut reasm = VideoReassembler::new();
                let mut video_dec = VideoDecoder::new().ok();
                let cap = audio_cfg.map(|c| c.interleaved_len() * 50).unwrap_or(0);

                while running.load(Ordering::Relaxed) {
                    let Ok(p) = media_rx.recv_timeout(Duration::from_millis(200)) else { continue };
                    match p.track {
                        TRACK_AUDIO => {
                            if let (Some(j), Some(dec), Some(ring)) =
                                (jitter.as_mut(), audio_dec.as_mut(), out_ring.as_ref())
                            {
                                j.push(p.seq, p.payload);
                                while let Some(out) = j.pop() {
                                    let pcm = match out {
                                        JitterOut::Frame(pkt) => dec.decode(Some(&pkt)),
                                        JitterOut::Lost => dec.decode(None),
                                    };
                                    if let Ok(pcm) = pcm {
                                        push_capped(ring, &pcm, cap);
                                    }
                                }
                            }
                        }
                        TRACK_VIDEO | TRACK_SCREEN => {
                            if let Some(frag) = parse_video_frag(&p.payload) {
                                if let Some(cf) = reasm.push(frag) {
                                    if let Some(dec) = video_dec.as_mut() {
                                        if let Ok(Some(frame)) = dec.decode(&cf.data) {
                                            let _ = frame_tx.try_send(frame);
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

        // --- Аудио: cpal захват/вывод + энкод ---
        let mut in_stream = None;
        let mut out_stream = None;
        if let Some(acfg) = audio {
            let host = cpal::default_host();
            let input = host.default_input_device().ok_or(CallError::NoInput)?;
            let output = host.default_output_device().ok_or(CallError::NoOutput)?;
            let scfg = StreamConfig {
                channels: acfg.channels,
                sample_rate: SampleRate(acfg.sample_rate),
                buffer_size: cpal::BufferSize::Default,
            };
            let cap = acfg.interleaved_len() * 50;

            let in_ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
            let in_ring_cb = in_ring.clone();
            let is = input
                .build_input_stream(
                    &scfg,
                    move |data: &[f32], _: &_| {
                        let tmp: Vec<i16> = data.iter().map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
                        push_capped(&in_ring_cb, &tmp, cap);
                    },
                    |e| eprintln!("[call] вход: {e}"),
                    None,
                )
                .map_err(|e| CallError::BuildStream(e.to_string()))?;

            let out_ring = out_ring.clone().unwrap();
            let os = output
                .build_output_stream(
                    &scfg,
                    move |data: &mut [f32], _: &_| {
                        let mut q = out_ring.lock().unwrap();
                        for o in data.iter_mut() {
                            *o = q.pop_front().map(|s| s as f32 / 32768.0).unwrap_or(0.0);
                        }
                    },
                    |e| eprintln!("[call] выход: {e}"),
                    None,
                )
                .map_err(|e| CallError::BuildStream(e.to_string()))?;

            is.play().map_err(|e| CallError::BuildStream(e.to_string()))?;
            os.play().map_err(|e| CallError::BuildStream(e.to_string()))?;
            in_stream = Some(is);
            out_stream = Some(os);

            let mut encoder = AudioEncoder::new(acfg)?;
            let frame_len = acfg.interleaved_len();
            let fsamps = acfg.frame_samples() as u32;
            let run = running.clone();
            let mute = muted.clone();
            let msgr = messenger.clone();
            threads.push(thread::spawn(move || {
                let mut seq = 0u16;
                let mut ts = 0u32;
                let mut frame = vec![0i16; frame_len];
                while run.load(Ordering::Relaxed) {
                    let ready = {
                        let mut q = in_ring.lock().unwrap();
                        if q.len() >= frame_len {
                            for s in frame.iter_mut() { *s = q.pop_front().unwrap(); }
                            true
                        } else { false }
                    };
                    if !ready { thread::sleep(Duration::from_millis(2)); continue; }
                    if !mute.load(Ordering::Relaxed) {
                        if let Ok(pkt) = encoder.encode(&frame) {
                            let _ = msgr.send_media(&peer, TRACK_AUDIO, seq, ts, &pkt);
                        }
                    }
                    seq = seq.wrapping_add(1);
                    ts = ts.wrapping_add(fsamps);
                }
            }));
        }

        // --- Видео: захват → H.264 → фрагменты → send_media ---
        if let (Some(vcfg), Some(mut src)) = (video, source) {
            let mut encoder = VideoEncoder::new(vcfg)?;
            let run = running.clone();
            let msgr = messenger.clone();
            let interval = Duration::from_millis((1000 / vcfg.fps.max(1)) as u64);
            threads.push(thread::spawn(move || {
                let mut frame_id = 0u32;
                let mut seq = 0u16;
                while run.load(Ordering::Relaxed) {
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
        }

        Ok(Call {
            running,
            muted,
            _in_stream: in_stream,
            _out_stream: out_stream,
            threads,
            frames: Some(frame_rx),
        })
    }

    /// Включить/выключить микрофон (mute — не шлём аудио, пир слышит тишину).
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    /// Забрать канал входящих видео-кадров (RGB8) для отрисовки. Один раз.
    pub fn take_frames(&mut self) -> Option<Receiver<DecodedFrame>> {
        self.frames.take()
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        for h in self.threads.drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        self.stop();
    }
}

// Удобный псевдоним отправителя кадров (для тестов/интеграции).
pub type FrameSender = SyncSender<DecodedFrame>;
