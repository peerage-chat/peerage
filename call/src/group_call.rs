//! Групповой звонок (full mesh): захватываем один раз и шлём КАЖДОМУ участнику;
//! принимаем от всех, держа отдельное состояние (джиттер/декодер) на источник.
//! Аудио всех источников микшуется в один выход; видео отдаётся по участникам
//! (для сетки в UI). Переиспользует кодеки/джиттер из 1:1.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream, StreamConfig};

use litertc::{fragment_video, parse_video_frag, JitterBuffer, JitterOut, VideoReassembler, TRACK_AUDIO, TRACK_SCREEN, TRACK_VIDEO};
use peerage_core::settings::{AudioSettings, VideoSettings};
use peerage_media::{
    AudioDecoder, AudioEncoder, DecodedFrame, VideoDecoder, VideoEncoder,
    VideoSource,
};
use peerage_messenger::Messenger;

use crate::CallError;

pub type PeerKey = [u8; 32];

/// Микс-буфер: суммируем сэмплы источников в i32 (без клиппинга), на выходе клампим.
type MixBuf = Arc<Mutex<VecDeque<i32>>>;

fn mix_into(buf: &MixBuf, pcm: &[i16], cap: usize) {
    let mut q = buf.lock().unwrap();
    for (i, &s) in pcm.iter().enumerate() {
        if i < q.len() {
            q[i] += s as i32; // подмешиваем к уже стоящему в очереди
        } else {
            q.push_back(s as i32);
        }
    }
    while q.len() > cap {
        q.pop_front();
    }
}

/// Активный групповой звонок.
pub struct GroupCall {
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    _in_stream: Option<Stream>,
    _out_stream: Option<Stream>,
    threads: Vec<JoinHandle<()>>,
    frames: Option<Receiver<(PeerKey, DecodedFrame)>>,
}

impl GroupCall {
    /// `members` — другие участники (кому шлём и от кого ждём).
    pub fn start(
        messenger: Arc<Messenger>,
        members: Vec<PeerKey>,
        audio: Option<AudioSettings>,
        video: Option<VideoSettings>,
        source: Option<Box<dyn VideoSource + Send>>,
    ) -> Result<GroupCall, CallError> {
        let running = Arc::new(AtomicBool::new(true));
        let muted = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        let media_rx = messenger.media_channel(2048);
        let (frame_tx, frame_rx) = sync_channel::<(PeerKey, DecodedFrame)>(32);
        let mix: Option<MixBuf> = audio.map(|_| Arc::new(Mutex::new(VecDeque::new())));

        // --- Приём: состояние на каждый источник ---
        {
            let running = running.clone();
            let mix = mix.clone();
            let acfg = audio;
            let allowed = members.clone(); // M2: медиа только от участников группы
            threads.push(thread::spawn(move || {
                let mut a_src: HashMap<PeerKey, (JitterBuffer, AudioDecoder)> = HashMap::new();
                let mut v_src: HashMap<PeerKey, (VideoReassembler, VideoDecoder)> = HashMap::new();
                let cap = acfg.map(|c| c.interleaved_len() * 50).unwrap_or(0);
                while running.load(Ordering::Relaxed) {
                    let Ok(p) = media_rx.recv_timeout(Duration::from_millis(200)) else { continue };
                    if !allowed.contains(&p.src) {
                        continue; // источник не из группы — игнорируем
                    }
                    let Some(f) = p.frame() else { continue };
                    match f.header.track {
                        TRACK_AUDIO => {
                            let (Some(cfg), Some(mix)) = (acfg, mix.as_ref()) else { continue };
                            let entry = a_src.entry(p.src).or_insert_with(|| {
                                (JitterBuffer::new(cfg.jitter_depth), AudioDecoder::new(cfg).unwrap())
                            });
                            entry.0.push(f.header.seq, f.payload.to_vec());
                            while let Some(out) = entry.0.pop() {
                                let pcm = match out {
                                    JitterOut::Frame(pkt) => entry.1.decode(Some(&pkt)),
                                    JitterOut::Lost { .. } => entry.1.decode(None),
                                };
                                if let Ok(pcm) = pcm {
                                    mix_into(mix, &pcm, cap);
                                }
                            }
                        }
                        TRACK_VIDEO | TRACK_SCREEN => {
                            let entry = v_src
                                .entry(p.src)
                                .or_insert_with(|| (VideoReassembler::new(), VideoDecoder::new().unwrap()));
                            if let Some(frag) = parse_video_frag(f.payload) {
                                if let Some(cf) = entry.0.push(frag) {
                                    if let Ok(Some(f)) = entry.1.decode(&cf.data) {
                                        let _ = frame_tx.try_send((p.src, f));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }));
        }

        // --- Аудио: cpal + мультикаст энкода ---
        let mut in_stream = None;
        let mut out_stream = None;
        if let (Some(acfg), Some(mix)) = (audio, mix.clone()) {
            let host = cpal::default_host();
            let input = host.default_input_device().ok_or(CallError::NoInput)?;
            let output = host.default_output_device().ok_or(CallError::NoOutput)?;
            let scfg = StreamConfig {
                channels: acfg.channels,
                sample_rate: SampleRate(acfg.sample_rate),
                buffer_size: cpal::BufferSize::Default,
            };
            let in_ring: Arc<Mutex<VecDeque<i16>>> = Arc::new(Mutex::new(VecDeque::new()));
            let cap = acfg.interleaved_len() * 50;
            let in_cb = in_ring.clone();
            let is = input
                .build_input_stream(
                    &scfg,
                    move |data: &[f32], _: &_| {
                        let mut q = in_cb.lock().unwrap();
                        for &s in data {
                            q.push_back((s.clamp(-1.0, 1.0) * 32767.0) as i16);
                        }
                        while q.len() > cap {
                            q.pop_front();
                        }
                    },
                    |e| eprintln!("[gcall] вход: {e}"),
                    None,
                )
                .map_err(|e| CallError::BuildStream(e.to_string()))?;
            let mix_out = mix.clone();
            let os = output
                .build_output_stream(
                    &scfg,
                    move |data: &mut [f32], _: &_| {
                        let mut q = mix_out.lock().unwrap();
                        for o in data.iter_mut() {
                            *o = q.pop_front().map(|v| v.clamp(-32768, 32767) as f32 / 32768.0).unwrap_or(0.0);
                        }
                    },
                    |e| eprintln!("[gcall] выход: {e}"),
                    None,
                )
                .map_err(|e| CallError::BuildStream(e.to_string()))?;
            is.play().map_err(|e| CallError::BuildStream(e.to_string()))?;
            os.play().map_err(|e| CallError::BuildStream(e.to_string()))?;
            in_stream = Some(is);
            out_stream = Some(os);

            let mut enc = AudioEncoder::new(acfg)?;
            let frame_len = acfg.interleaved_len();
            let fsamps = acfg.frame_samples() as u32;
            let run = running.clone();
            let mute = muted.clone();
            let msgr = messenger.clone();
            let mem = members.clone();
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
                        if let Ok(pkt) = enc.encode(&frame) {
                            for m in &mem {
                                let _ = msgr.send_media(m, TRACK_AUDIO, seq, ts, &pkt);
                            }
                        }
                    }
                    seq = seq.wrapping_add(1);
                    ts = ts.wrapping_add(fsamps);
                }
            }));
        }

        // --- Видео: захват → энкод → фрагменты → мультикаст ---
        if let (Some(vcfg), Some(mut src)) = (video, source) {
            let mut enc = VideoEncoder::new(vcfg)?;
            let run = running.clone();
            let msgr = messenger.clone();
            let mem = members.clone();
            let interval = Duration::from_millis((1000 / vcfg.fps.max(1)) as u64);
            threads.push(thread::spawn(move || {
                let mut frame_id = 0u32;
                let mut seq = 0u16;
                while run.load(Ordering::Relaxed) {
                    let t0 = Instant::now();
                    if let Some(rgb) = src.next_frame() {
                        if let Ok((bs, key)) = enc.encode_rgb(&rgb) {
                            for chunk in fragment_video(&bs, frame_id, litertc::VideoInfo { keyframe: key, ..Default::default() }) {
                                for m in &mem {
                                    let _ = msgr.send_media(m, TRACK_VIDEO, seq, frame_id, &chunk);
                                }
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

        Ok(GroupCall {
            running,
            muted,
            _in_stream: in_stream,
            _out_stream: out_stream,
            threads,
            frames: Some(frame_rx),
        })
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    /// Канал входящих видео-кадров с пометкой источника (для сетки участников).
    pub fn take_frames(&mut self) -> Option<Receiver<(PeerKey, DecodedFrame)>> {
        self.frames.take()
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        for h in self.threads.drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for GroupCall {
    fn drop(&mut self) {
        self.stop();
    }
}
