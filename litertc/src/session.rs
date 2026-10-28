//! Звонок с нашей стороны как sans-IO автомат (ADR-014 §2).
//!
//! Вызывающий (в Peerage — движок комнаты) держит потоки, кодеки и микширование;
//! `Session` держит всё, что относится к доставке: нумерацию и формат исходящих
//! пакетов, разбор входящих, джиттер-буфер звука и очередь собранных видео-кадров
//! на каждого участника. Своего времени, потоков и очередей с ожиданием нет: время
//! приходит аргументом, исходящие пакеты забираются [`Session::poll_transmit`].
//!
//! Воспроизведение тянет потребитель в своём темпе ([`Session::pull_audio`] по таймеру
//! кадра, [`Session::pull_video`] — декодер), поэтому медиа отдаётся по запросу, а не
//! событиями.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::Instant;

use crate::jitter::{JitterBuffer, JitterOut};
use crate::reassembly::{CompleteFrame, VideoReassembler};
use crate::wire::{
    fragment_video, parse_media, parse_video_frag, write_media_header, MEDIA_HEADER_LEN,
    TRACK_AUDIO, TRACK_VIDEO,
};

/// Предел очереди собранных видео-кадров участника в байтах — поверх предела в
/// кадрах: кадр бывает до 8 МБ (`MAX_VIDEO_FRAGS`), и 60 таких — полгигабайта.
const VIDEO_QUEUE_BYTES: usize = 32 << 20;

/// Предел очереди видео-кадров по умолчанию (≈300 мс при 30 fps), пока вызывающий не
/// уточнит его под разрешение и fps ([`Session::set_video_queue_cap`]). Декодер
/// выводит кадры по своим часам, поэтому бэклог в очереди = постоянная задержка.
const VIDEO_QUEUE_DEFAULT: usize = 9;

/// Параметры звонка.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Частота часов звука (Opus — 48 000): в этих единицах метка времени пакета.
    pub audio_clock_hz: u32,
    /// Сэмплов на кадр звука (20 мс при 48 кГц — 960): шаг метки времени.
    pub audio_frame_samples: u32,
    /// Начальная глубина джиттер-буфера в кадрах; дальше подстраивается по джиттеру.
    pub jitter_depth: usize,
}

/// Исходящий пакет: кому и что. Шифрует и отправляет вызывающий.
#[derive(Debug, PartialEq, Eq)]
pub struct Transmit<P> {
    pub to: P,
    pub data: Vec<u8>,
}

/// Счётчики участника.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeerStats {
    /// Принято полезной нагрузки (без заголовка), байт.
    pub rx_bytes: u64,
}

/// Приём звука от участника: джиттер-буфер и оценка джиттера для его глубины.
struct AudioRx {
    jitter: JitterBuffer,
    last_ts: Option<u32>,
    last_arrival: Option<Instant>,
    jitter_estimate_ms: f64,
}

impl AudioRx {
    fn new(depth: usize) -> Self {
        AudioRx {
            jitter: JitterBuffer::new(depth),
            last_ts: None,
            last_arrival: None,
            jitter_estimate_ms: 30.0,
        }
    }

    fn push(&mut self, now: Instant, seq: u16, ts: u32, payload: Vec<u8>, clock_hz: f64) {
        if let (Some(l_ts), Some(l_arr)) = (self.last_ts, self.last_arrival) {
            let send_diff_samples = ts.wrapping_sub(l_ts) as i32;
            let send_diff_ms = (send_diff_samples as f64) / (clock_hz / 1000.0);
            let recv_diff_ms = now.saturating_duration_since(l_arr).as_secs_f64() * 1000.0;
            let d = (recv_diff_ms - send_diff_ms).abs();
            let alpha = if d > self.jitter_estimate_ms {
                0.25
            } else {
                0.0625
            };
            self.jitter_estimate_ms += (d - self.jitter_estimate_ms) * alpha;

            let target_depth = ((self.jitter_estimate_ms / 20.0).round() as usize + 2).clamp(2, 8);
            self.jitter.set_depth(target_depth);
        }
        self.last_ts = Some(ts);
        self.last_arrival = Some(now);
        self.jitter.push(seq, payload);
    }
}

/// Приём видео от участника: сборка кадров и очередь собранных до декодера.
#[derive(Default)]
struct VideoRx {
    asm: VideoReassembler,
    /// (данные кадра, ключевой).
    queue: VecDeque<(Vec<u8>, bool)>,
}

impl VideoRx {
    fn push(&mut self, frame: CompleteFrame, cap: usize) {
        let q = &mut self.queue;
        q.push_back((frame.data, frame.keyframe));
        // Анти-латентность: при переполнении ПРЫГАЕМ к свежему ключевому кадру (декодер
        // ресинхронизируется чисто), а не тянем устаревшие кадры по одному.
        let cap = cap.max(4);
        if q.len() > cap {
            if let Some(kf) = q.iter().rposition(|(_, k)| *k) {
                if kf > 0 {
                    q.drain(0..kf);
                }
            }
            while q.len() > cap {
                q.pop_front();
            }
        }
        while q.len() > 1 && q.iter().map(|(d, _)| d.len()).sum::<usize>() > VIDEO_QUEUE_BYTES {
            q.pop_front();
        }
    }
}

struct PeerState {
    audio: AudioRx,
    video: VideoRx,
    stats: PeerStats,
}

/// Один звонок с нашей стороны. `P` — идентификатор участника (в Peerage — ключ).
pub struct Session<P> {
    cfg: Config,
    /// Текущие участники в порядке, заданном вызывающим (порядок рассылки).
    peers: Vec<P>,
    state: HashMap<P, PeerState>,
    out: VecDeque<Transmit<P>>,
    audio_seq: u16,
    audio_ts: u32,
    video_seq: u16,
    /// Номер кадра на проводе — u32: 16 бит оборачивались за 36 минут при 30 fps.
    video_frame_id: u32,
    video_queue_cap: usize,
    tx_bytes: u64,
}

impl<P: Copy + Eq + Hash> Session<P> {
    pub fn new(cfg: Config) -> Self {
        Session {
            cfg,
            peers: Vec::new(),
            state: HashMap::new(),
            out: VecDeque::new(),
            audio_seq: 0,
            audio_ts: 0,
            video_seq: 0,
            video_frame_id: 0,
            video_queue_cap: VIDEO_QUEUE_DEFAULT,
            tx_bytes: 0,
        }
    }

    /// Задать участников звонка. Им рассылается наше медиа, и только от них
    /// принимается чужое. Ушедший теряет всё состояние приёма и неотправленные ему
    /// пакеты: вернувшись, он начинает поток заново, с пустого буфера.
    pub fn set_peers(&mut self, peers: &[P]) {
        if self.peers == peers {
            return;
        }
        self.peers = peers.to_vec();
        self.state.retain(|p, _| peers.contains(p));
        self.out.retain(|t| peers.contains(&t.to));
        let depth = self.cfg.jitter_depth;
        for p in peers {
            self.state.entry(*p).or_insert_with(|| PeerState {
                audio: AudioRx::new(depth),
                video: VideoRx::default(),
                stats: PeerStats::default(),
            });
        }
    }

    pub fn peers(&self) -> &[P] {
        &self.peers
    }

    /// Закодированный кадр звука → пакет каждому участнику.
    pub fn send_audio(&mut self, _now: Instant, payload: &[u8]) {
        let (seq, ts) = (self.audio_seq, self.audio_ts);
        self.broadcast(TRACK_AUDIO, seq, ts, payload);
        self.skip_audio();
    }

    /// Кадр звука не отправлен (микрофон выключен, кодер не справился): нумерация
    /// идёт дальше, как будто кадр был, — у приёмника это пропуск, а не сдвиг времени.
    pub fn skip_audio(&mut self) {
        self.audio_seq = self.audio_seq.wrapping_add(1);
        self.audio_ts = self.audio_ts.wrapping_add(self.cfg.audio_frame_samples);
    }

    /// Закодированный видео-кадр → фрагменты каждому участнику.
    pub fn send_video(&mut self, _now: Instant, frame: &[u8], keyframe: bool) {
        let frame_id = self.video_frame_id;
        self.video_frame_id = self.video_frame_id.wrapping_add(1);
        for chunk in fragment_video(frame, frame_id, keyframe) {
            let seq = self.video_seq;
            self.video_seq = self.video_seq.wrapping_add(1);
            self.broadcast(TRACK_VIDEO, seq, 0, &chunk);
        }
    }

    fn broadcast(&mut self, track: u8, seq: u16, ts: u32, payload: &[u8]) {
        let mut packet = vec![0u8; MEDIA_HEADER_LEN + payload.len()];
        write_media_header(&mut packet, track, seq, ts);
        packet[MEDIA_HEADER_LEN..].copy_from_slice(payload);
        for p in &self.peers {
            self.out.push_back(Transmit {
                to: *p,
                data: packet.clone(),
            });
        }
        self.tx_bytes += (payload.len() * self.peers.len()) as u64;
    }

    /// Следующий пакет к отправке.
    pub fn poll_transmit(&mut self) -> Option<Transmit<P>> {
        self.out.pop_front()
    }

    /// Пакет от участника (уже расшифрован, отправитель доказан вызывающим).
    /// Не участник звонка или мусор — пакет молча выбрасывается.
    pub fn handle_datagram(&mut self, now: Instant, from: P, data: &[u8]) {
        let Some(st) = self.state.get_mut(&from) else {
            return;
        };
        let Some(m) = parse_media(data) else { return };
        st.stats.rx_bytes += m.payload.len() as u64;
        match m.track {
            TRACK_VIDEO => {
                let Some(frag) = parse_video_frag(m.payload) else {
                    return;
                };
                if let Some(frame) = st.video.asm.push(frag) {
                    st.video.push(frame, self.video_queue_cap);
                }
            }
            TRACK_AUDIO => {
                let clock = self.cfg.audio_clock_hz as f64;
                st.audio.push(now, m.seq, m.ts, m.payload.to_vec(), clock);
            }
            _ => {}
        }
    }

    /// Следующий кадр звука участника для воспроизведения: `None` — буфер набирается
    /// или пуст (участник молчит), [`JitterOut::Lost`] — кадр потерян, нужен PLC.
    pub fn pull_audio(&mut self, peer: &P) -> Option<JitterOut> {
        self.state.get_mut(peer)?.audio.jitter.pop()
    }

    /// Следующий собранный видео-кадр участника для декодера.
    pub fn pull_video(&mut self, peer: &P) -> Option<Vec<u8>> {
        self.state
            .get_mut(peer)?
            .video
            .queue
            .pop_front()
            .map(|(d, _)| d)
    }

    /// Предел очереди видео-кадров на участника. Политику задаёт вызывающий: он
    /// знает разрешение и fps и даёт тяжёлым режимам больший запас на пачки, а
    /// лёгким — низкую задержку.
    pub fn set_video_queue_cap(&mut self, frames: usize) {
        self.video_queue_cap = frames.clamp(4, 60);
    }

    pub fn stats(&self, peer: &P) -> Option<PeerStats> {
        self.state.get(peer).map(|s| s.stats)
    }

    /// Отправлено полезной нагрузки (без заголовка) всем участникам вместе, байт.
    pub fn tx_bytes(&self) -> u64 {
        self.tx_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{MAX_VIDEO_CHUNK, MEDIA_V1};

    const CFG: Config = Config {
        audio_clock_hz: 48_000,
        audio_frame_samples: 960,
        jitter_depth: 1,
    };

    fn drain(s: &mut Session<u8>) -> Vec<Transmit<u8>> {
        std::iter::from_fn(|| s.poll_transmit()).collect()
    }

    /// Формат на проводе — тот же, что был у движка до переноса: заголовок v1,
    /// нумерация звука идёт и в тишине, видео — с нулевой меткой времени.
    #[test]
    fn packets_match_v1_wire() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[7]);
        s.send_audio(now, &[0xAA]);
        s.skip_audio();
        s.send_audio(now, &[0xBB]);
        s.send_video(now, &[0xCC], true);
        let out = drain(&mut s);
        let data: Vec<&[u8]> = out.iter().map(|t| t.data.as_slice()).collect();
        assert_eq!(data[0], [MEDIA_V1, TRACK_AUDIO, 0, 0, 0, 0, 0, 0, 0xAA]);
        // seq 2, ts 1920 = 0x0780
        assert_eq!(
            data[1],
            [MEDIA_V1, TRACK_AUDIO, 0, 2, 0, 0, 0x07, 0x80, 0xBB]
        );
        assert_eq!(
            data[2],
            [
                MEDIA_V1,
                TRACK_VIDEO,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                1,
                1,
                0xCC
            ]
        );
        assert!(out.iter().all(|t| t.to == 7));
        assert_eq!(
            s.tx_bytes(),
            3 + 9 /* видео-подзаголовок */
        );
    }

    #[test]
    fn sends_to_every_peer_and_only_hears_peers() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1, 2]);
        s.send_audio(now, &[5]);
        let to: Vec<u8> = drain(&mut s).iter().map(|t| t.to).collect();
        assert_eq!(to, [1, 2]);

        let mut pkt = vec![0u8; MEDIA_HEADER_LEN + 1];
        write_media_header(&mut pkt, TRACK_AUDIO, 0, 0);
        s.handle_datagram(now, 3, &pkt); // не участник
        assert_eq!(s.pull_audio(&3), None);
        assert_eq!(s.stats(&3), None);
        s.handle_datagram(now, 1, &pkt);
        assert_eq!(s.pull_audio(&1), Some(JitterOut::Frame(vec![0])));
        assert_eq!(s.stats(&1), Some(PeerStats { rx_bytes: 1 }));
    }

    /// Ушедший участник: ему больше ничего не уходит (даже то, что уже в очереди),
    /// а его состояние приёма сброшено.
    #[test]
    fn removed_peer_loses_queue_and_state() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1, 2]);
        s.send_audio(now, &[5]);
        let mut pkt = vec![0u8; MEDIA_HEADER_LEN + 1];
        write_media_header(&mut pkt, TRACK_AUDIO, 0, 0);
        s.handle_datagram(now, 2, &pkt);
        s.set_peers(&[1]);
        assert!(drain(&mut s).iter().all(|t| t.to == 1));
        assert_eq!(s.stats(&2), None);
        s.set_peers(&[1, 2]);
        assert_eq!(
            s.stats(&2),
            Some(PeerStats::default()),
            "вернулся — с чистого листа"
        );
    }

    /// Очередь видео сверх предела прыгает к свежему ключевому кадру.
    #[test]
    fn video_queue_jumps_to_keyframe() {
        let now = Instant::now();
        let mut tx = Session::new(CFG);
        tx.set_peers(&[9]);
        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        rx.set_video_queue_cap(4);
        for i in 0..6u8 {
            tx.send_video(now, &[i; 10], i == 3);
        }
        for t in drain(&mut tx) {
            rx.handle_datagram(now, 1, &t.data);
        }
        let got: Vec<u8> = std::iter::from_fn(|| rx.pull_video(&1))
            .map(|d| d[0])
            .collect();
        assert_eq!(got, [3, 4, 5]);
    }

    #[test]
    fn large_frame_is_fragmented_and_reassembled() {
        let now = Instant::now();
        let mut tx = Session::new(CFG);
        tx.set_peers(&[9]);
        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        let frame = vec![0x5A; MAX_VIDEO_CHUNK * 3 + 1];
        tx.send_video(now, &frame, true);
        let out = drain(&mut tx);
        assert_eq!(out.len(), 4);
        for t in out.iter().rev() {
            rx.handle_datagram(now, 1, &t.data);
        }
        assert_eq!(rx.pull_video(&1), Some(frame));
    }
}
