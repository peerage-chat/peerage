//! p2p-call — движок 1:1 аудио-звонка: захват с микрофона → Opus → лёгкий
//! медиа-путь Yggdrasil; приём → джиттер → Opus → вывод на динамики.
//!
//! Параметры — из пользовательских [`AudioSettings`]. Сигнализация (invite/
//! accept/end) и UI — вне этого крейта (в приложении), здесь только медиа-движок.
//!
//! ВНИМАНИЕ: [`AudioCall`] держит cpal-потоки и НЕ `Send` — используйте его на
//! одном потоке (в десктопе — на UI-потоке).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream, StreamConfig};

use p2p_core::media::TRACK_AUDIO;
use p2p_core::settings::AudioSettings;
use p2p_media::{AudioDecoder, AudioEncoder, JitterBuffer, JitterOut};
use p2p_messenger::Messenger;

pub type PeerKey = [u8; 32];

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
}

/// Кольцевой буфер сэмплов с ограничением (роняем старое при переполнении —
/// реалтайм важнее полноты).
type Ring = Arc<Mutex<VecDeque<i16>>>;

fn push_capped(ring: &Ring, samples: &[i16], cap: usize) {
    let mut q = ring.lock().unwrap();
    q.extend(samples.iter().copied());
    while q.len() > cap {
        q.pop_front();
    }
}

/// Активный аудио-звонок. Останавливается при `stop()` или `drop`.
pub struct AudioCall {
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    _in_stream: Stream,
    _out_stream: Stream,
    encode_thread: Option<JoinHandle<()>>,
    decode_thread: Option<JoinHandle<()>>,
}

impl AudioCall {
    /// Запустить полнодуплексный аудио-звонок с пиром.
    pub fn start(
        messenger: Arc<Messenger>,
        peer: PeerKey,
        settings: AudioSettings,
    ) -> Result<AudioCall, CallError> {
        let host = cpal::default_host();
        let input = host.default_input_device().ok_or(CallError::NoInput)?;
        let output = host.default_output_device().ok_or(CallError::NoOutput)?;

        let cfg = StreamConfig {
            channels: settings.channels,
            sample_rate: SampleRate(settings.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let running = Arc::new(AtomicBool::new(true));
        let muted = Arc::new(AtomicBool::new(false));
        let frame_len = settings.interleaved_len();
        // ~1с буфера-предохранителя.
        let cap = settings.interleaved_len() * 50;

        // --- ВВОД: микрофон → input_ring ---
        let in_ring: Ring = Arc::new(Mutex::new(VecDeque::with_capacity(cap)));
        let in_ring_cb = in_ring.clone();
        let in_stream = input
            .build_input_stream(
                &cfg,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut tmp = Vec::with_capacity(data.len());
                    for &s in data {
                        tmp.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
                    }
                    push_capped(&in_ring_cb, &tmp, cap);
                },
                |e| eprintln!("[call] ошибка ввода: {e}"),
                None,
            )
            .map_err(|e| CallError::BuildStream(e.to_string()))?;

        // --- ВЫВОД: output_ring → динамики ---
        let out_ring: Ring = Arc::new(Mutex::new(VecDeque::with_capacity(cap)));
        let out_ring_cb = out_ring.clone();
        let out_stream = output
            .build_output_stream(
                &cfg,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut q = out_ring_cb.lock().unwrap();
                    for o in data.iter_mut() {
                        *o = match q.pop_front() {
                            Some(s) => s as f32 / 32768.0,
                            None => 0.0, // underrun → тишина
                        };
                    }
                },
                |e| eprintln!("[call] ошибка вывода: {e}"),
                None,
            )
            .map_err(|e| CallError::BuildStream(e.to_string()))?;

        in_stream.play().map_err(|e| CallError::BuildStream(e.to_string()))?;
        out_stream.play().map_err(|e| CallError::BuildStream(e.to_string()))?;

        // --- ЗАХВАТ/ЭНКОД: input_ring → Opus → send_media ---
        let mut encoder = AudioEncoder::new(settings)?;
        let enc_running = running.clone();
        let enc_muted = muted.clone();
        let enc_msgr = messenger.clone();
        let encode_thread = thread::spawn(move || {
            let mut seq: u16 = 0;
            let mut ts: u32 = 0;
            let mut frame = vec![0i16; frame_len];
            while enc_running.load(Ordering::Relaxed) {
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
                if !enc_muted.load(Ordering::Relaxed) {
                    if let Ok(pkt) = encoder.encode(&frame) {
                        let _ = enc_msgr.send_media(&peer, TRACK_AUDIO, seq, ts, &pkt);
                    }
                }
                seq = seq.wrapping_add(1);
                ts = ts.wrapping_add(settings.frame_samples() as u32);
            }
        });

        // --- ПРИЁМ/ДЕКОД: media → джиттер → Opus → output_ring ---
        let media_rx = messenger.media_channel(512);
        let mut decoder = AudioDecoder::new(settings)?;
        let mut jitter = JitterBuffer::new(settings.jitter_depth);
        let dec_running = running.clone();
        let decode_thread = thread::spawn(move || {
            while dec_running.load(Ordering::Relaxed) {
                if let Ok(p) = media_rx.recv_timeout(Duration::from_millis(200)) {
                    if p.track == TRACK_AUDIO {
                        jitter.push(p.seq, p.payload);
                    }
                }
                while let Some(out) = jitter.pop() {
                    let pcm = match out {
                        JitterOut::Frame(pkt) => decoder.decode(Some(&pkt)),
                        JitterOut::Lost => decoder.decode(None),
                    };
                    if let Ok(pcm) = pcm {
                        push_capped(&out_ring, &pcm, cap);
                    }
                }
            }
        });

        Ok(AudioCall {
            running,
            muted,
            _in_stream: in_stream,
            _out_stream: out_stream,
            encode_thread: Some(encode_thread),
            decode_thread: Some(decode_thread),
        })
    }

    /// Включить/выключить микрофон (при mute мы не шлём пакеты — пир слышит тишину).
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    /// Остановить звонок (останавливает потоки и аудио-устройства).
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.encode_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.decode_thread.take() {
            let _ = h.join();
        }
    }
}

impl Drop for AudioCall {
    fn drop(&mut self) {
        self.stop();
    }
}
