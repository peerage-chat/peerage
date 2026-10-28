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
//!
//! Таймеры (отчёты получателя, повторы NACK, ожидание недостающего кадра, запрос
//! ключевого): вызывающий зовёт [`Session::handle_timeout`] не позже
//! [`Session::poll_timeout`].
//!
//! Восстановление видео (ADR-014 §5): приёмник просит повторить пропущенные пакеты
//! (NACK), а если не дождался — ключевой кадр; отправитель повторяет из истории в
//! пределах бюджета и сообщает вызывающему, что кодеру нужен ключевой кадр
//! ([`Session::take_keyframe_request`]).

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};

use crate::bwe::{ArrivalLog, Bwe};
use crate::jitter::{JitterBuffer, JitterOut};
use crate::nack::{NackList, TxHistory};
use crate::pacer::{Class, Pacer};
use crate::reassembly::{CompleteFrame, VideoReassembler};
use crate::stats::{RxStats, TrackStats};
use crate::wire::{
    fragment_video, parse_keyframe_request, parse_media, parse_nack, parse_report,
    parse_transport_feedback, parse_video_frag, set_tw_seq, write_keyframe_request,
    write_media_header, write_nack, write_report, write_transport_feedback, MediaHeader, Nack,
    Report, VideoInfo, FEEDBACK_TICK_US, KEYFRAME_REQUEST_V2, MEDIA_HEADER_LEN, MEDIA_RTX_V2,
    MEDIA_V2, NACK_V2, REPORT_V2, TRACK_AUDIO, TRACK_SCREEN, TRACK_VIDEO, TRANSPORT_FEEDBACK_V2,
    VIDEO_CLOCK_HZ,
};

/// Как часто каждый участник получает наш отчёт о приёме.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// RTT больше этого — мусор (часы отправителя отчёта или подделка), не учитываем.
const MAX_RTT_MS: i32 = 60_000;

/// Треков три: звук, камера, экран — индексы в массивах статистики.
const TRACKS: usize = TRACK_SCREEN as usize + 1;

/// RTT, пока отчётов ещё не было.
const DEFAULT_RTT: Duration = Duration::from_millis(200);

/// Как часто сообщать участнику, когда пришли его пакеты (для его оценки полосы).
const FEEDBACK_INTERVAL: Duration = Duration::from_millis(100);

/// Пределы оценки полосы по умолчанию, бит/с, пока вызывающий не задаст свои
/// ([`Session::set_bitrate_bounds`]).
const DEFAULT_MIN_BITRATE: u32 = 100_000;
const DEFAULT_MAX_BITRATE: u32 = 10_000_000;

/// Повторы участнику — не больше этой доли того, что ему отправлено впервые
/// (ADR-014 §8: иначе один участник NACK-ами заставил бы нас заливать канал)...
const RTX_SHARE: f64 = 0.25;
/// ...с запасом на пачку: потерянный кусок ключевого кадра.
const RTX_BURST: f64 = 256.0 * 1024.0;

/// Запрос ключевого кадра от участника выполняем не чаще: иначе он заставил бы кодер
/// выдавать одни ключевые. Чуть меньше секунды, чтобы повтор запроса приёмником раз
/// в секунду ([`KEYFRAME_REQUEST_RETRY`]) не отклонялся из-за дрожания времени.
const KEYFRAME_REQUEST_MIN_INTERVAL: Duration = Duration::from_millis(900);
/// Повтор нашего запроса ключевого кадра, пока он не пришёл.
const KEYFRAME_REQUEST_RETRY: Duration = Duration::from_secs(1);

/// Сколько ждать недостающий видео-кадр, когда более новые уже собраны: две попытки
/// NACK — первая сразу, вторая через [`nack_retry`], и каждая возвращается за RTT.
/// Одной мало: при 5 % потерь в обе стороны каждый десятый повтор не доходит, и это
/// ключевой кадр. Дольше двух — картинка стоит, дешевле ключевой кадр.
fn max_frame_wait(rtt: Duration) -> Duration {
    (rtt * 5 / 2 + Duration::from_millis(50))
        .clamp(Duration::from_millis(100), Duration::from_millis(800))
}

/// Через сколько повторять NACK: повтор не пришёл за RTT с запасом на разброс.
fn nack_retry(rtt: Duration) -> Duration {
    (rtt + Duration::from_millis(30)).max(Duration::from_millis(50))
}

/// Предел очереди собранных видео-кадров участника в байтах — поверх предела в
/// кадрах: кадр бывает до 8 МБ (`MAX_VIDEO_FRAGS`), и 60 таких — полгигабайта.
const VIDEO_QUEUE_BYTES: usize = 32 << 20;

/// Предел очереди видео-кадров по умолчанию (≈300 мс при 30 fps), пока вызывающий не
/// уточнит его под разрешение и fps ([`Session::set_video_queue_cap`]). Декодер
/// выводит кадры по своим часам, поэтому бэклог в очереди = постоянная задержка.
const VIDEO_QUEUE_DEFAULT: usize = 9;

/// Жёсткий предел очереди в кадрах: сверх мягкого кадры не выбрасываются, пока нет
/// ключевого, к которому можно перескочить (выброшенный P-кадр сломал бы все
/// следующие). Столько набегает, только если кадры никто не забирает (тайл
/// участника не на экране) — тогда очередь выбрасывается целиком.
const VIDEO_QUEUE_HARD: usize = 64;

/// Параметры звонка.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Частота часов звука (Opus — 48 000): в этих единицах метка времени пакета.
    pub audio_clock_hz: u32,
    /// Сэмплов на кадр звука (20 мс при 48 кГц — 960): шаг метки времени.
    pub audio_frame_samples: u32,
    /// Начальная глубина джиттер-буфера в кадрах; дальше подстраивается по джиттеру.
    pub jitter_depth: usize,
    /// Номер нашего потока — случайный на каждый звонок (его выбирает вызывающий:
    /// у LiteRTC нет своего источника случайности). По нему собеседник отличает наш
    /// новый поток после перезахода от опоздавших пакетов старого.
    pub stream_id: u32,
}

/// Исходящий пакет: кому и что. Шифрует и отправляет вызывающий.
#[derive(Debug, PartialEq, Eq)]
pub struct Transmit<P> {
    pub to: P,
    pub data: Vec<u8>,
}

/// Собранный видео-кадр участника для декодера.
#[derive(Debug, PartialEq, Eq)]
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub info: VideoInfo,
}

/// Статистика по участнику.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PeerStats {
    /// Принято полезной нагрузки (без заголовка), байт.
    pub rx_bytes: u64,
    /// Время туда-обратно (сглаженное), из эха в отчётах.
    pub rtt: Option<Duration>,
    /// Как мы принимаем его потоки, по трекам (`TRACK_*`).
    pub inbound: [Option<TrackStats>; TRACKS],
    /// Как он принимает наши потоки — из его отчётов.
    pub outbound: [Option<TrackStats>; TRACKS],
    /// Пакетов повторили ему по его NACK.
    pub retransmitted: u64,
    /// Ключевых кадров попросили у него.
    pub keyframe_requests: u64,
    /// Оценка полосы до него, бит/с.
    pub bandwidth: u32,
    /// Сколько раз оценку сбавляли: из-за роста задержки и из-за потерь.
    pub bandwidth_cuts: (u64, u64),
}

/// Приём звука от участника: джиттер-буфер и оценка джиттера для его глубины.
struct AudioRx {
    /// Номер потока, к которому относится буфер.
    stream: u32,
    jitter: JitterBuffer,
    last_ts: Option<u32>,
    last_arrival: Option<Instant>,
    jitter_estimate_ms: f64,
}

impl AudioRx {
    fn new(stream: u32, depth: usize) -> Self {
        AudioRx {
            stream,
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

/// Приём видео от участника: пропущенные пакеты, сборка кадров и очередь собранных
/// до декодера.
#[derive(Default)]
struct VideoRx {
    /// Номер потока, к которому относится сборщик.
    stream: Option<u32>,
    nack: NackList,
    asm: VideoReassembler,
    queue: VecDeque<VideoFrame>,
    /// Когда последний раз просили у него ключевой кадр.
    keyframe_requested: Option<Instant>,
}

impl VideoRx {
    fn push(&mut self, frame: CompleteFrame, cap: usize) {
        let q = &mut self.queue;
        q.push_back(VideoFrame {
            data: frame.data,
            info: frame.info,
        });
        // Анти-латентность: при переполнении ПРЫГАЕМ к свежему ключевому кадру (декодер
        // ресинхронизируется чисто). Выбросить P-кадры по одному нельзя: следующие
        // опираются на них.
        if q.len() > cap.max(4) {
            if let Some(kf) = q.iter().rposition(|f| f.info.keyframe) {
                q.drain(0..kf);
            }
        }
        let bytes = q.iter().map(|f| f.data.len()).sum::<usize>();
        if q.len() > VIDEO_QUEUE_HARD || bytes > VIDEO_QUEUE_BYTES {
            q.clear();
            self.asm.skip_to_keyframe(false);
        }
    }
}

struct PeerState {
    /// `None` — звука от участника ещё не было.
    audio: Option<AudioRx>,
    video: VideoRx,
    stats: PeerStats,
    /// Счётчики приёма по трекам — для отчётов ему.
    rx: [Option<RxStats>; TRACKS],
    /// Сквозной номер следующего пакета этому участнику (по всем трекам).
    tw_seq: u16,
    /// Когда слать ему следующий отчёт; `None` — при первой возможности.
    next_report: Option<Instant>,
    /// `ts_ms` последнего его отчёта и когда он пришёл — для эха.
    last_report: Option<(u32, Instant)>,
    /// Бюджет повторов ему, байт ([`RTX_SHARE`]).
    rtx_budget: f64,
    /// Когда последний раз выполнили его запрос ключевого кадра.
    keyframe_honoured: Option<Instant>,
    /// Оценка полосы до него — по его обратной связи о наших пакетах.
    bwe: Bwe,
    /// Когда пришли его пакеты — для обратной связи ему.
    arrivals: ArrivalLog,
    next_feedback: Option<Instant>,
}

impl PeerState {
    fn new(min_bps: u32, max_bps: u32) -> Self {
        PeerState {
            audio: None,
            video: VideoRx::default(),
            stats: PeerStats::default(),
            rx: Default::default(),
            tw_seq: 0,
            next_report: None,
            last_report: None,
            // Полный с начала: первые потери звонка тоже чинятся повтором.
            rtx_budget: RTX_BURST,
            keyframe_honoured: None,
            bwe: Bwe::new(min_bps as f64, max_bps as f64),
            arrivals: ArrivalLog::default(),
            next_feedback: None,
        }
    }

    fn rtt(&self) -> Duration {
        self.stats.rtt.unwrap_or(DEFAULT_RTT)
    }
}

/// Медиа-пакет: заголовок и payload; `retransmit` — повтор по NACK.
fn media_packet(header: &MediaHeader, payload: &[u8], retransmit: bool) -> Vec<u8> {
    let mut packet = vec![0u8; MEDIA_HEADER_LEN + payload.len()];
    write_media_header(&mut packet, header);
    if retransmit {
        packet[0] = MEDIA_RTX_V2;
    }
    packet[MEDIA_HEADER_LEN..].copy_from_slice(payload);
    packet
}

/// Один звонок с нашей стороны. `P` — идентификатор участника (в Peerage — ключ).
pub struct Session<P> {
    cfg: Config,
    /// Текущие участники в порядке, заданном вызывающим (порядок рассылки).
    peers: Vec<P>,
    state: HashMap<P, PeerState>,
    pacer: Pacer<P>,
    min_bitrate: u32,
    max_bitrate: u32,
    audio_seq: u16,
    audio_ts: u32,
    video_seq: u16,
    /// Номер кадра на проводе — u32: 16 бит оборачивались за 36 минут при 30 fps.
    video_frame_id: u32,
    /// Начало отсчёта времени захвата видео (первый отправленный кадр).
    video_epoch: Option<Instant>,
    /// Отправленное видео — для повторов по NACK.
    video_history: TxHistory,
    /// Кто-то из участников попросил ключевой кадр, а кодер его ещё не выдал.
    keyframe_wanted: bool,
    video_queue_cap: usize,
    tx_bytes: u64,
    /// Начало наших часов для отчётов и времени прихода (первый увиденный `now`).
    epoch: Option<Instant>,
    /// Последний увиденный `now` — для [`Session::poll_timeout`].
    last_now: Option<Instant>,
}

impl<P: Copy + Eq + Hash> Session<P> {
    pub fn new(cfg: Config) -> Self {
        Session {
            cfg,
            peers: Vec::new(),
            state: HashMap::new(),
            pacer: Pacer::new(DEFAULT_MAX_BITRATE as f64),
            min_bitrate: DEFAULT_MIN_BITRATE,
            max_bitrate: DEFAULT_MAX_BITRATE,
            audio_seq: 0,
            audio_ts: 0,
            video_seq: 0,
            video_frame_id: 0,
            video_epoch: None,
            video_history: TxHistory::default(),
            keyframe_wanted: false,
            video_queue_cap: VIDEO_QUEUE_DEFAULT,
            tx_bytes: 0,
            epoch: None,
            last_now: None,
        }
    }

    /// Запомнить время; вернуть его от начала наших часов.
    fn tick(&mut self, now: Instant) -> Duration {
        self.last_now = Some(now);
        now.saturating_duration_since(*self.epoch.get_or_insert(now))
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
        self.pacer.retain(|t| peers.contains(&t.to));
        let (min, max) = (self.min_bitrate, self.max_bitrate);
        for p in peers {
            self.state
                .entry(*p)
                .or_insert_with(|| PeerState::new(min, max));
        }
        self.update_pacing();
    }

    /// Пределы оценки полосы до каждого участника, бит/с: снизу — без чего звонок
    /// бессмыслен, сверху — больше кодеры всё равно не выдадут.
    pub fn set_bitrate_bounds(&mut self, min: u32, max: u32) {
        let max = max.max(min);
        (self.min_bitrate, self.max_bitrate) = (min, max);
        for st in self.state.values_mut() {
            st.bwe.set_bounds(min as f64, max as f64);
        }
        self.update_pacing();
    }

    /// Сколько можно слать каждому участнику, бит/с, — всё наше медиа вместе. Кодер
    /// один на всех, поэтому это худшая из оценок до участников (ADR-014 §6).
    pub fn target_bitrate(&self) -> u32 {
        self.peers
            .iter()
            .filter_map(|p| self.state.get(p))
            .map(|st| st.bwe.rate() as u32)
            .min()
            .unwrap_or(self.max_bitrate)
    }

    /// Темп выпуска: каждому участнику — его копия, поэтому цель на их число.
    fn update_pacing(&mut self) {
        let total = self.target_bitrate() as f64 * self.peers.len().max(1) as f64;
        self.pacer.set_rate(total);
    }

    pub fn peers(&self) -> &[P] {
        &self.peers
    }

    /// Закодированный кадр звука → пакет каждому участнику.
    pub fn send_audio(&mut self, now: Instant, payload: &[u8]) {
        self.tick(now);
        let (seq, ts) = (self.audio_seq, self.audio_ts);
        self.broadcast(TRACK_AUDIO, seq, ts, payload);
        self.audio_seq = self.audio_seq.wrapping_add(1);
        self.audio_ts = self.audio_ts.wrapping_add(self.cfg.audio_frame_samples);
    }

    /// Кадр звука не отправлен (микрофон выключен, кодер не справился): время идёт
    /// дальше, номер — нет. Номер считает отправленные пакеты, иначе приёмник принял
    /// бы выключенный микрофон за потери (и в отчётах, и в джиттер-буфере после
    /// включения — первые ~0,4 с речи терялись).
    pub fn skip_audio(&mut self) {
        self.audio_ts = self.audio_ts.wrapping_add(self.cfg.audio_frame_samples);
    }

    /// Закодированный видео-кадр → фрагменты каждому участнику. `now` — момент
    /// захвата (точнее, выхода из кодера: его задержка почти постоянна).
    pub fn send_video(&mut self, now: Instant, frame: &[u8], info: VideoInfo) {
        self.tick(now);
        let epoch = *self.video_epoch.get_or_insert(now);
        let micros = now.saturating_duration_since(epoch).as_micros();
        // Метка времени идёт по кругу u32 (13 часов при 90 кГц), как и у звука.
        let ts = (micros * VIDEO_CLOCK_HZ as u128 / 1_000_000) as u32;
        let frame_id = self.video_frame_id;
        self.video_frame_id = self.video_frame_id.wrapping_add(1);
        if info.keyframe {
            self.keyframe_wanted = false;
        }
        for chunk in fragment_video(frame, frame_id, info) {
            let seq = self.video_seq;
            self.video_seq = self.video_seq.wrapping_add(1);
            self.video_history.push(now, seq, ts, &chunk);
            self.broadcast(TRACK_VIDEO, seq, ts, &chunk);
        }
    }

    /// Участник просил ключевой кадр, и кодеру пора его выдать. Сбрасывается этим
    /// вызовом и любым отправленным ключевым кадром.
    pub fn take_keyframe_request(&mut self) -> bool {
        std::mem::take(&mut self.keyframe_wanted)
    }

    fn broadcast(&mut self, track: u8, seq: u16, ts: u32, payload: &[u8]) {
        let Some(now) = self.last_now else { return };
        let class = if track == TRACK_AUDIO {
            Class::Audio
        } else {
            Class::Video
        };
        for p in &self.peers {
            let Some(st) = self.state.get_mut(p) else {
                continue;
            };
            // Сквозной номер — при выпуске (poll_transmit).
            let header = MediaHeader {
                track,
                stream_id: self.cfg.stream_id,
                seq,
                tw_seq: 0,
                ts,
            };
            st.rtx_budget = (st.rtx_budget + payload.len() as f64 * RTX_SHARE).min(RTX_BURST);
            let data = media_packet(&header, payload, false);
            self.pacer.push(class, now, Transmit { to: *p, data });
        }
        self.tx_bytes += (payload.len() * self.peers.len()) as u64;
    }

    /// NACK участника: повторить ему то, что ещё есть в истории, пока хватает бюджета.
    fn retransmit(&mut self, to: P, nack: Nack) {
        if nack.track != TRACK_VIDEO || nack.stream_id != self.cfg.stream_id {
            return;
        }
        let Some(now) = self.last_now else { return };
        let Some(st) = self.state.get_mut(&to) else {
            return;
        };
        for seq in nack.seqs {
            let Some((ts, payload)) = self.video_history.get(seq) else {
                continue;
            };
            if st.rtx_budget < payload.len() as f64 {
                break;
            }
            st.rtx_budget -= payload.len() as f64;
            let header = MediaHeader {
                track: TRACK_VIDEO,
                stream_id: self.cfg.stream_id,
                seq,
                tw_seq: 0,
                ts,
            };
            st.stats.retransmitted += 1;
            self.tx_bytes += payload.len() as u64;
            let data = media_packet(&header, payload, true);
            self.pacer
                .push(Class::Retransmit, now, Transmit { to, data });
        }
    }

    /// Следующий пакет, который пора отправить. Видео выходит ровным темпом по оценке
    /// полосы, поэтому после `None` очередь может быть не пуста: когда звать снова,
    /// говорит [`Session::poll_timeout`].
    pub fn poll_transmit(&mut self) -> Option<Transmit<P>> {
        let now = self.last_now?;
        let mut t = self.pacer.pop(now)?;
        let media = matches!(t.data.first(), Some(&MEDIA_V2 | &MEDIA_RTX_V2));
        if let (true, Some(st), Some(epoch)) = (media, self.state.get_mut(&t.to), self.epoch) {
            let seq = st.tw_seq;
            st.tw_seq = st.tw_seq.wrapping_add(1);
            set_tw_seq(&mut t.data, seq);
            let at_ms = now.saturating_duration_since(epoch).as_secs_f64() * 1000.0;
            st.bwe.on_sent(seq, at_ms, t.data.len() as u32);
        }
        Some(t)
    }

    /// Пакет от участника (уже расшифрован, отправитель доказан вызывающим).
    /// Не участник звонка или мусор — пакет молча выбрасывается.
    pub fn handle_datagram(&mut self, now: Instant, from: P, data: &[u8]) {
        let since_epoch = self.tick(now);
        let Some(st) = self.state.get_mut(&from) else {
            return;
        };
        match data.first() {
            Some(&REPORT_V2) => {
                if let Some(r) = parse_report(data) {
                    Self::handle_report(&self.cfg, st, now, since_epoch, r);
                }
                return;
            }
            Some(&NACK_V2) => {
                if let Some(n) = parse_nack(data) {
                    self.retransmit(from, n);
                }
                return;
            }
            Some(&TRANSPORT_FEEDBACK_V2) => {
                if let Some(fb) = parse_transport_feedback(data) {
                    let now_ms = since_epoch.as_secs_f64() * 1000.0;
                    let rtt_ms = st.rtt().as_secs_f64() * 1000.0;
                    st.bwe.on_feedback(now_ms, &fb, rtt_ms);
                    self.update_pacing();
                }
                return;
            }
            Some(&KEYFRAME_REQUEST_V2) => {
                let ours = (TRACK_VIDEO, self.cfg.stream_id);
                let due = st.keyframe_honoured.is_none_or(|t| {
                    now.saturating_duration_since(t) >= KEYFRAME_REQUEST_MIN_INTERVAL
                });
                if parse_keyframe_request(data) == Some(ours) && due {
                    st.keyframe_honoured = Some(now);
                    self.keyframe_wanted = true;
                }
                return;
            }
            _ => {}
        }
        let Some(m) = parse_media(data) else { return };
        let h = m.header;
        st.stats.rx_bytes += m.payload.len() as u64;
        st.arrivals
            .on_packet(h.tw_seq, since_epoch.as_micros() as u64 / FEEDBACK_TICK_US);
        // Повторы в статистику приёма не идут: потери в отчётах — потери первой
        // отправки, по ним судят о канале.
        if let Some(slot) = st.rx.get_mut(h.track as usize).filter(|_| !m.retransmit) {
            let clock = if h.track == TRACK_AUDIO {
                self.cfg.audio_clock_hz
            } else {
                VIDEO_CLOCK_HZ
            };
            let rx = match slot {
                Some(r) if r.stream == h.stream_id => r,
                slot => slot.insert(RxStats::new(h.stream_id, h.seq, clock)),
            };
            let arrival = (since_epoch.as_micros() * clock as u128 / 1_000_000) as i64;
            rx.update(h.seq, h.ts, arrival);
        }
        // Новый номер потока — отправитель начал заново (перезашёл в комнату): старое
        // состояние приёма этого трека выбрасываем, а не ждём, пока нумерация догонит.
        match h.track {
            TRACK_VIDEO => {
                let Some(frag) = parse_video_frag(m.payload) else {
                    return;
                };
                if st.video.stream != Some(h.stream_id) {
                    st.video.stream = Some(h.stream_id);
                    st.video.nack = NackList::default();
                    st.video.asm = VideoReassembler::new();
                }
                st.video.nack.on_packet(now, h.seq);
                st.video.asm.push(frag);
                self.service_video(now, from);
            }
            TRACK_AUDIO => {
                let depth = self.cfg.jitter_depth;
                let audio = match &mut st.audio {
                    Some(a) if a.stream == h.stream_id => a,
                    slot => slot.insert(AudioRx::new(h.stream_id, depth)),
                };
                let clock = self.cfg.audio_clock_hz as f64;
                audio.push(now, h.seq, h.ts, m.payload.to_vec(), clock);
            }
            _ => {}
        }
    }

    /// Отчёт участника о том, как он принимает нас: RTT из эха, его статистика по
    /// нашим текущим потокам (блоки о чужих номерах потока — о прошлом звонке, мимо).
    fn handle_report(
        cfg: &Config,
        st: &mut PeerState,
        now: Instant,
        since_epoch: Duration,
        r: Report,
    ) {
        st.last_report = Some((r.ts_ms, now));
        if let Some((echo_ts, echo_delay)) = r.echo {
            let now_ms = since_epoch.as_millis() as u32;
            let rtt = now_ms.wrapping_sub(echo_ts).wrapping_sub(echo_delay) as i32;
            if (0..=MAX_RTT_MS).contains(&rtt) {
                let sample = Duration::from_millis(rtt as u64);
                // Сглаживание как у TCP (RFC 6298): 7/8 старого + 1/8 нового.
                st.stats.rtt = Some(st.stats.rtt.map_or(sample, |old| (old * 7 + sample) / 8));
            }
        }
        for b in r.blocks.iter().filter(|b| b.stream_id == cfg.stream_id) {
            let clock = if b.track == TRACK_AUDIO {
                cfg.audio_clock_hz
            } else {
                VIDEO_CLOCK_HZ
            };
            if let Some(slot) = st.stats.outbound.get_mut(b.track as usize) {
                *slot = Some(TrackStats::from_block(b, clock));
            }
        }
    }

    /// Видео участника: отдать декодеру готовые кадры, попросить повторить
    /// пропущенное и, если без ключевого кадра дальше не пойти, попросить его.
    fn service_video(&mut self, now: Instant, peer: P) {
        let Some(st) = self.state.get_mut(&peer) else {
            return;
        };
        let rtt = st.rtt();
        let wait = max_frame_wait(rtt);
        let v = &mut st.video;
        while let Some(frame) = v.asm.pop(now, wait) {
            v.push(frame, self.video_queue_cap);
        }
        let Some(stream_id) = v.stream else { return };
        let nack = Nack {
            track: TRACK_VIDEO,
            stream_id,
            seqs: v.nack.due(now, nack_retry(rtt), wait),
        };
        if let Some(data) = write_nack(&nack) {
            self.pacer
                .push(Class::Control, now, Transmit { to: peer, data });
        }
        let ask = v.asm.wants_keyframe()
            && v.keyframe_requested
                .is_none_or(|t| now.saturating_duration_since(t) >= KEYFRAME_REQUEST_RETRY);
        if ask {
            v.keyframe_requested = Some(now);
            st.stats.keyframe_requests += 1;
            let data = write_keyframe_request(TRACK_VIDEO, stream_id);
            self.pacer
                .push(Class::Control, now, Transmit { to: peer, data });
        }
    }

    /// Когда звать [`Session::handle_timeout`]. `None` — участников нет, таймеров тоже.
    pub fn poll_timeout(&self) -> Option<Instant> {
        self.peers
            .iter()
            .filter_map(|p| self.state.get(p))
            .flat_map(|st| {
                let rtt = st.rtt();
                let v = &st.video;
                let keyframe = v
                    .keyframe_requested
                    .filter(|_| v.asm.wants_keyframe())
                    .map(|t| t + KEYFRAME_REQUEST_RETRY);
                let feedback = (!st.arrivals.is_empty())
                    .then(|| st.next_feedback.or(self.last_now))
                    .flatten();
                [
                    st.next_report.or(self.last_now),
                    v.asm.deadline(max_frame_wait(rtt)),
                    v.nack.next_due(nack_retry(rtt)),
                    keyframe,
                    feedback,
                ]
            })
            .flatten()
            .chain(self.last_now.and_then(|now| self.pacer.next_release(now)))
            .min()
    }

    /// Сработали таймеры: разослать отчёты получателя тем, кому пора; повторить
    /// NACK и запросы ключевого кадра; отдать кадры, которые больше не ждут.
    pub fn handle_timeout(&mut self, now: Instant) {
        let now_ms = self.tick(now).as_millis() as u32;
        for p in &self.peers {
            let Some(st) = self.state.get_mut(p) else {
                continue;
            };
            if !st.arrivals.is_empty() && st.next_feedback.is_none_or(|t| t <= now) {
                st.next_feedback = Some(now + FEEDBACK_INTERVAL);
                if let Some(data) = st
                    .arrivals
                    .take()
                    .as_ref()
                    .and_then(write_transport_feedback)
                {
                    self.pacer
                        .push(Class::Control, now, Transmit { to: *p, data });
                }
            }
            if st.next_report.is_some_and(|t| t > now) {
                continue;
            }
            st.next_report = Some(now + REPORT_INTERVAL);
            let echo = st
                .last_report
                .map(|(ts, at)| (ts, now.saturating_duration_since(at).as_millis() as u32));
            let mut blocks = Vec::new();
            for (track, slot) in st.rx.iter_mut().enumerate() {
                if let Some(rx) = slot {
                    blocks.push(rx.report_block(track as u8));
                    st.stats.inbound[track] = Some(rx.snapshot());
                }
            }
            let data = write_report(&Report {
                ts_ms: now_ms,
                echo,
                blocks,
            });
            self.pacer
                .push(Class::Control, now, Transmit { to: *p, data });
        }
        for p in self.peers.clone() {
            self.service_video(now, p);
        }
    }

    /// Следующий кадр звука участника для воспроизведения: `None` — буфер набирается
    /// или пуст (участник молчит), [`JitterOut::Lost`] — кадр потерян: восстановить из
    /// FEC следующего пакета, если он уже есть, иначе маскировать (PLC).
    pub fn pull_audio(&mut self, peer: &P) -> Option<JitterOut> {
        self.state.get_mut(peer)?.audio.as_mut()?.jitter.pop()
    }

    /// Следующий собранный видео-кадр участника для декодера.
    pub fn pull_video(&mut self, peer: &P) -> Option<VideoFrame> {
        self.state.get_mut(peer)?.video.queue.pop_front()
    }

    /// Декодер видео участника начат заново (тайл появился на экране, декодер
    /// пересоздан после ошибки): ему нужен ключевой кадр. Если очередь с него и
    /// начинается — ничего не нужно; иначе очередь выбрасывается, а ключевой кадр
    /// просим у отправителя (не чаще раза в секунду).
    pub fn request_keyframe(&mut self, now: Instant, peer: &P) {
        self.tick(now);
        let Some(st) = self.state.get_mut(peer) else {
            return;
        };
        let v = &mut st.video;
        if v.queue.front().is_some_and(|f| f.info.keyframe) || v.stream.is_none() {
            return;
        }
        v.queue.clear();
        v.asm.skip_to_keyframe(true);
        self.service_video(now, *peer);
    }

    /// Предел очереди видео-кадров на участника. Политику задаёт вызывающий: он
    /// знает разрешение и fps и даёт тяжёлым режимам больший запас на пачки, а
    /// лёгким — низкую задержку.
    pub fn set_video_queue_cap(&mut self, frames: usize) {
        self.video_queue_cap = frames.clamp(4, 60);
    }

    pub fn stats(&self, peer: &P) -> Option<PeerStats> {
        self.state.get(peer).map(|s| PeerStats {
            bandwidth: s.bwe.rate() as u32,
            bandwidth_cuts: (s.bwe.cuts_delay, s.bwe.cuts_loss),
            ..s.stats
        })
    }

    /// Отправлено полезной нагрузки (без заголовка) всем участникам вместе, байт.
    pub fn tx_bytes(&self) -> u64 {
        self.tx_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{MAX_VIDEO_CHUNK, MEDIA_V2};
    use std::time::Duration;

    const CFG: Config = Config {
        audio_clock_hz: 48_000,
        audio_frame_samples: 960,
        jitter_depth: 1,
        stream_id: 0x0A0B_0C0D,
    };

    fn drain(s: &mut Session<u8>) -> Vec<Transmit<u8>> {
        std::iter::from_fn(|| s.poll_transmit()).collect()
    }

    fn key() -> VideoInfo {
        VideoInfo {
            keyframe: true,
            rotation: 1,
            fps: 30,
        }
    }

    fn audio_pkt(stream_id: u32, seq: u16, byte: u8) -> Vec<u8> {
        let mut pkt = vec![byte; MEDIA_HEADER_LEN + 1];
        let h = MediaHeader {
            track: TRACK_AUDIO,
            stream_id,
            seq,
            tw_seq: 0,
            ts: 0,
        };
        write_media_header(&mut pkt, &h);
        pkt
    }

    /// Пакеты v2: номер потока, сквозной номер на получателя по всем трекам; в
    /// тишине у звука идёт время, но не номер; видео — со временем захвата в 90 кГц.
    #[test]
    fn packets_match_v2_wire() {
        let t0 = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[7]);
        s.send_audio(t0, &[0xAA]);
        s.skip_audio();
        s.send_audio(t0, &[0xBB]);
        s.send_video(t0, &[0xCC], key());
        s.send_video(
            t0 + Duration::from_millis(100),
            &[0xDD],
            VideoInfo::default(),
        );
        let out = drain(&mut s);
        let data: Vec<&[u8]> = out.iter().map(|t| t.data.as_slice()).collect();
        let sid = [0x0A, 0x0B, 0x0C, 0x0D];
        let head = |track: u8| [MEDIA_V2, track, sid[0], sid[1], sid[2], sid[3]];
        assert_eq!(
            data[0],
            [&head(TRACK_AUDIO)[..], &[0, 0, 0, 0, 0, 0, 0, 0, 0xAA]].concat()
        );
        // seq 1 (тишина номер не тратит), tw 1, ts 1920 = 0x0780
        assert_eq!(
            data[1],
            [
                &head(TRACK_AUDIO)[..],
                &[0, 1, 0, 1, 0, 0, 0x07, 0x80, 0xBB]
            ]
            .concat()
        );
        // видео: seq 0, tw 2, ts 0; фрагмент: кадр 0, 0 из 1, ключевой, поворот 1, 30 fps
        let frag = [0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 30, 0xCC];
        assert_eq!(
            data[2],
            [&head(TRACK_VIDEO)[..], &[0, 0, 0, 2, 0, 0, 0, 0], &frag].concat()
        );
        // второй кадр через 100 мс: seq 1, tw 3, ts 9000 = 0x2328
        assert_eq!(data[3][6..14], [0, 1, 0, 3, 0, 0, 0x23, 0x28]);
        assert!(out.iter().all(|t| t.to == 7));
        assert_eq!(s.tx_bytes(), 2 + 2 * (11 + 1));
    }

    /// Сквозной номер — свой у каждого получателя.
    #[test]
    fn tw_seq_is_per_recipient() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1]);
        s.send_audio(now, &[1]);
        s.set_peers(&[1, 2]);
        s.send_audio(now, &[2]);
        let tw: Vec<(u8, u16)> = drain(&mut s)
            .iter()
            .map(|t| (t.to, parse_media(&t.data).unwrap().header.tw_seq))
            .collect();
        assert_eq!(tw, [(1, 0), (1, 1), (2, 0)]);
    }

    #[test]
    fn sends_to_every_peer_and_only_hears_peers() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1, 2]);
        s.send_audio(now, &[5]);
        let to: Vec<u8> = drain(&mut s).iter().map(|t| t.to).collect();
        assert_eq!(to, [1, 2]);

        let pkt = audio_pkt(1, 0, 0);
        s.handle_datagram(now, 3, &pkt); // не участник
        assert_eq!(s.pull_audio(&3), None);
        assert_eq!(s.stats(&3), None);
        s.handle_datagram(now, 1, &pkt);
        assert_eq!(s.pull_audio(&1), Some(JitterOut::Frame(vec![0])));
        assert_eq!(s.stats(&1).map(|st| st.rx_bytes), Some(1));
    }

    /// Ушедший участник: ему больше ничего не уходит (даже то, что уже в очереди),
    /// а его состояние приёма сброшено.
    #[test]
    fn removed_peer_loses_queue_and_state() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1, 2]);
        s.send_audio(now, &[5]);
        s.handle_datagram(now, 2, &audio_pkt(1, 0, 0));
        s.set_peers(&[1]);
        assert!(drain(&mut s).iter().all(|t| t.to == 1));
        assert_eq!(s.stats(&2), None);
        s.set_peers(&[1, 2]);
        assert_eq!(
            s.stats(&2).map(|st| PeerStats { bandwidth: 0, ..st }),
            Some(PeerStats::default()),
            "вернулся — с чистого листа"
        );
    }

    /// Собеседник перезашёл (новый номер потока) и нумерует звук с нуля, пока мы
    /// его ещё не вычеркнули: новый поток слышен сразу (R-108).
    #[test]
    fn audio_restart_with_new_stream_is_heard() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1]);
        for seq in 0..30_000u16 {
            s.handle_datagram(now, 1, &audio_pkt(100, seq, 1));
            assert!(s.pull_audio(&1).is_some());
        }
        s.handle_datagram(now, 1, &audio_pkt(200, 0, 2));
        assert_eq!(s.pull_audio(&1), Some(JitterOut::Frame(vec![2])));
    }

    /// То же для видео: новый поток с нулевого кадра принимается сразу.
    #[test]
    fn video_restart_with_new_stream_is_accepted() {
        let now = Instant::now();
        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        let mut old = Session::new(Config {
            stream_id: 1,
            ..CFG
        });
        old.set_peers(&[9]);
        old.video_frame_id = 50_000;
        old.send_video(now, &[1; 10], key());
        let mut new = Session::new(Config {
            stream_id: 2,
            ..CFG
        });
        new.set_peers(&[9]);
        new.send_video(now, &[2; 10], key());
        for t in drain(&mut old).into_iter().chain(drain(&mut new)) {
            rx.handle_datagram(now, 1, &t.data);
        }
        let got: Vec<u8> = std::iter::from_fn(|| rx.pull_video(&1))
            .map(|f| f.data[0])
            .collect();
        assert_eq!(got, [1, 2]);
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
            tx.send_video(
                now,
                &[i; 10],
                VideoInfo {
                    keyframe: i == 0 || i == 3,
                    ..key()
                },
            );
        }
        for t in drain(&mut tx) {
            rx.handle_datagram(now, 1, &t.data);
        }
        let got: Vec<u8> = std::iter::from_fn(|| rx.pull_video(&1))
            .map(|f| f.data[0])
            .collect();
        assert_eq!(got, [3, 4, 5]);
    }

    #[test]
    fn large_frame_is_fragmented_and_reassembled() {
        let now = Instant::now();
        let mut tx = Session::new(CFG);
        tx.set_peers(&[9]);
        // Без ровного темпа: кадр нужен целиком в один момент.
        tx.set_bitrate_bounds(100_000, 1_000_000_000);
        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        let frame = vec![0x5A; MAX_VIDEO_CHUNK * 3 + 1];
        tx.send_video(now, &frame, key());
        let out = drain(&mut tx);
        assert_eq!(out.len(), 4);
        for t in out.iter().rev() {
            rx.handle_datagram(now, 1, &t.data);
        }
        assert_eq!(
            rx.pull_video(&1),
            Some(VideoFrame {
                data: frame,
                info: key()
            })
        );
    }
}
