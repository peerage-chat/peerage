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
//! событиями. Когда показывать видео-кадр, решает `Session` — по метке времени
//! захвата ([`VideoFrame::play_at`], модуль `playout`): потребитель только декодирует
//! и показывает в срок.
//!
//! Метки звука и видео — моменты захвата на одних часах отправителя, поэтому губы
//! сводятся со звуком (модуль `avsync`): потребитель сообщает, когда кадр будет
//! слышен ([`Session::pull_audio`]) и сколько идёт через свой видео-декодер
//! ([`Session::set_video_render_delay`]), а `Session` задерживает тот трек, что
//! спешит.
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

use crate::avsync::AvSync;
use crate::bwe::{ArrivalLog, Bwe};
use crate::jitter::{JitterBuffer, JitterOut};
use crate::nack::{NackList, TxHistory};
use crate::pacer::{Class, Pacer};
use crate::playout::Playout;
use crate::reassembly::{CompleteFrame, VideoReassembler};
use crate::stats::{RxStats, TrackStats};
use crate::wire::{
    fragment_video, parse_keyframe_request, parse_media, parse_nack, parse_report,
    parse_transport_feedback, parse_video_frag, set_tw_seq, write_keyframe_request,
    write_media_header, write_nack, write_report, write_transport_feedback, MediaFrame,
    MediaHeader, Nack, Report, VideoInfo, FEEDBACK_TICK_US, KEYFRAME_REQUEST_V2, MEDIA_HEADER_LEN,
    MEDIA_RTX_V2, MEDIA_V2, NACK_V2, REPORT_V2, TRACK_AUDIO, TRACK_PADDING, TRACK_SCREEN,
    TRACK_VIDEO, TRANSPORT_FEEDBACK_V2, VIDEO_CLOCK_HZ,
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

/// Жёсткий предел очереди в кадрах: сверх мягкого кадры не выбрасываются, пока нет
/// ключевого, к которому можно перескочить (выброшенный P-кадр сломал бы все
/// следующие). Столько набегает, только если кадры никто не забирает (тайл
/// участника не на экране) — тогда очередь выбрасывается целиком. С запасом на
/// секунду задержки показа при 120 fps.
const VIDEO_QUEUE_HARD: usize = 150;

/// Кадр, который пора было показать столько назад, а его не забрали, — потребитель
/// не успевает (декодер встал, тайл не на экране). Задержка копилась бы без конца:
/// очередь перескакивает к свежему ключевому кадру.
const VIDEO_STALL: Duration = Duration::from_millis(500);

/// Потребитель видео считается живым, если забирал кадры не дольше этого назад. Живой,
/// но отстающий получает ключевой кадр для перескока; тайл вне экрана — нет (иначе
/// отправитель слал бы ключевые кадры раз в секунду впустую).
const CONSUMER_ACTIVE: Duration = Duration::from_secs(1);

/// Звук подстраивается под видео по кадру (вставка маскировки или пропуск) не чаще
/// раза в столько кадров: рывки по 20 мс вразбивку не слышны как дрожь.
const AUDIO_ADJUST_EVERY: u32 = 5;

/// Задержка видео-декодера больше этой — сбой замера, не верим.
const MAX_RENDER_DELAY: Duration = Duration::from_secs(2);

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
    /// Метка захвата на часах отправителя, 90 кГц.
    pub ts: u32,
    /// Срок кадра (наши часы): по метке захвата, ровно в темпе камеры отправителя,
    /// с запасом на разброс сети. Это время показа при мгновенном декодере: в срок
    /// кадр подают декодеру, а показывают позже на задержку своего декодера (она у
    /// устройства постоянна — темп не меняется). Опоздавший кадр подают сразу.
    pub play_at: Instant,
}

/// Как губы участника сведены со звуком (ADR-014 §8b).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LipSync {
    /// На сколько видео позже своего звука, мс (< 0 — раньше), до поправки этой
    /// секунды; `None` — одного из треков нет.
    pub error_ms: Option<f32>,
    /// Сколько ждёт звук ради губ.
    pub audio_wait: Duration,
    /// Сколько ждёт видео ради губ.
    pub video_wait: Duration,
    /// Кадров маскировки вставлено (звук ждал) и кадров выброшено (ожидание снято).
    pub audio_holds: u64,
    pub audio_drops: u64,
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
    /// Цель кодера для него, бит/с: оценка, урезанная, пока окно перегрузки полно.
    pub target: u32,
    /// В полёте (без обратной связи) и окно перегрузки, байт.
    pub in_flight: (u64, u64),
    /// Сведение губ со звуком.
    pub lip_sync: LipSync,
}

/// Приём звука от участника: джиттер-буфер и оценка джиттера для его глубины.
struct AudioRx {
    /// Номер потока, к которому относится буфер.
    stream: u32,
    jitter: JitterBuffer,
    last_ts: Option<u32>,
    last_arrival: Option<Instant>,
    jitter_estimate_ms: f64,
    /// Кадров задержки, добавленных ради губ (вставки минус пропуски).
    held: usize,
    /// Кадров с последней вставки или пропуска.
    since_adjust: u32,
}

impl AudioRx {
    fn new(stream: u32, depth: usize) -> Self {
        AudioRx {
            stream,
            jitter: JitterBuffer::new(depth),
            last_ts: None,
            last_arrival: None,
            jitter_estimate_ms: 30.0,
            held: 0,
            since_adjust: 0,
        }
    }

    fn push(&mut self, now: Instant, seq: u16, ts: u32, payload: Vec<u8>, clock_hz: f64) {
        if let (Some(l_ts), Some(l_arr)) = (self.last_ts, self.last_arrival) {
            let send_ms = ts.wrapping_sub(l_ts) as i32 as f64 * 1000.0 / clock_hz;
            let recv_ms = now.saturating_duration_since(l_arr).as_secs_f64() * 1000.0;
            self.jitter_estimate_ms = jitter_estimate(self.jitter_estimate_ms, recv_ms, send_ms);
            self.jitter
                .set_depth(depth_for_jitter(self.jitter_estimate_ms));
        }
        self.last_ts = Some(ts);
        self.last_arrival = Some(now);
        self.jitter.push(seq, ts, payload);
    }
}

/// Пакет `h` (не повтор) — в статистику приёма своего трека; новый номер потока —
/// статистика с начала.
fn count_received(cfg: &Config, st: &mut PeerState, h: &MediaHeader, since_epoch: Duration) {
    let Some(slot) = st.rx.get_mut(h.track as usize) else {
        return;
    };
    let clock = if h.track == TRACK_AUDIO {
        cfg.audio_clock_hz
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

/// Что сделать со звуком перед очередным кадром ради губ.
#[derive(Debug, PartialEq)]
enum LipStep {
    Play,
    /// Кадр маскировки вместо следующего: всё дальше слышно на кадр позже.
    Hold,
    /// Кадр выбросить: всё дальше слышно на кадр раньше.
    Drop,
}

impl AudioRx {
    /// Шаг сведения губ: звук должен отставать на `want` кадров. Не чаще раза в
    /// [`AUDIO_ADJUST_EVERY`] кадров и только пока буфер играет: не хватает — кадр
    /// маскировки, лишнее — выбросить кадр, если в буфере есть хотя бы два.
    fn lip_step(&mut self, want: usize) -> LipStep {
        self.jitter.set_extra(want);
        self.since_adjust = self.since_adjust.saturating_add(1);
        if self.since_adjust < AUDIO_ADJUST_EVERY || !self.jitter.is_playing() {
            return LipStep::Play;
        }
        let step = if self.held < want {
            self.held += 1;
            LipStep::Hold
        } else if self.held > want && self.jitter.len() >= 2 {
            self.held -= 1;
            LipStep::Drop
        } else {
            return LipStep::Play;
        };
        self.since_adjust = 0;
        step
    }
}

/// Оценка джиттера звука, мс: между соседними пакетами пришли через `recv_ms`, а
/// сняты через `send_ms` — разница и есть разброс. Растёт быстро (¼ разницы),
/// спадает медленно (1/16, как в RFC 3550): лучше лишний кадр задержки, чем дыра.
fn jitter_estimate(est_ms: f64, recv_ms: f64, send_ms: f64) -> f64 {
    let d = (recv_ms - send_ms).abs();
    let alpha = if d > est_ms { 0.25 } else { 0.0625 };
    est_ms + (d - est_ms) * alpha
}

/// Глубина буфера звука под джиттер: по кадру (20 мс) на его величину и два сверху,
/// от 2 до 8 кадров.
fn depth_for_jitter(est_ms: f64) -> usize {
    ((est_ms / 20.0).round() as usize + 2).clamp(2, 8)
}

/// Приём видео от участника: пропущенные пакеты, сборка кадров и очередь собранных
/// до декодера.
#[derive(Default)]
struct VideoRx {
    /// Номер потока, к которому относится сборщик.
    stream: Option<u32>,
    nack: NackList,
    asm: VideoReassembler,
    playout: Playout,
    queue: VecDeque<VideoFrame>,
    /// Когда потребитель последний раз забрал кадр.
    last_pull: Option<Instant>,
    /// Когда последний раз просили у него ключевой кадр.
    keyframe_requested: Option<Instant>,
}

impl VideoRx {
    fn push(&mut self, now: Instant, frame: CompleteFrame) {
        let play_at = self.playout.schedule(now, frame.ts);
        let q = &mut self.queue;
        q.push_back(VideoFrame {
            data: frame.data,
            info: frame.info,
            ts: frame.ts,
            play_at,
        });
        // Потребитель не успевает — ПРЫГАЕМ к свежему ключевому кадру (декодер
        // ресинхронизируется чисто). Выбросить P-кадры по одному нельзя: следующие
        // опираются на них.
        if q.front()
            .is_some_and(|f| now.saturating_duration_since(f.play_at) > VIDEO_STALL)
        {
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

    /// Пакет нового потока (отправитель начал заново): прежнее состояние приёма —
    /// мимо.
    fn restart_if_new(&mut self, stream: u32) {
        if self.stream != Some(stream) {
            self.stream = Some(stream);
            self.nack = NackList::default();
            self.asm = VideoReassembler::new();
            self.playout = Playout::default();
        }
    }

    /// Потребитель живой, но не успевает (слабый декодер): старший кадр давно пора
    /// было отдать, а перескочить не к чему — нужен ключевой кадр.
    fn consumer_behind(&self, now: Instant) -> bool {
        self.last_pull
            .is_some_and(|t| now.saturating_duration_since(t) < CONSUMER_ACTIVE)
            && self
                .queue
                .front()
                .is_some_and(|f| now.saturating_duration_since(f.play_at) > VIDEO_STALL)
            && !self.queue.iter().any(|f| f.info.keyframe)
    }
}

struct PeerState {
    /// `None` — звука от участника ещё не было.
    audio: Option<AudioRx>,
    video: VideoRx,
    /// Сведение губ со звуком.
    sync: AvSync,
    /// Сколько кадр идёт через видео-декодер потребителя (срок → экран).
    render_delay: Duration,
    stats: PeerStats,
    /// Счётчики приёма по трекам — для отчётов ему.
    rx: [Option<RxStats>; TRACKS],
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
    /// Обратная связь о пришедшем, если есть о чём и пора (раз в
    /// [`FEEDBACK_INTERVAL`]).
    fn take_feedback(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.arrivals.is_empty() || self.next_feedback.is_some_and(|t| t > now) {
            return None;
        }
        self.next_feedback = Some(now + FEEDBACK_INTERVAL);
        self.arrivals
            .take()
            .as_ref()
            .and_then(write_transport_feedback)
    }

    /// Отчёт о приёме, если пора (раз в [`REPORT_INTERVAL`]): эхо последнего
    /// отчёта участника (для его RTT) и блоки по нашим входящим трекам; снимки этих
    /// блоков — в статистику.
    fn take_report(&mut self, now: Instant, now_ms: u32) -> Option<Vec<u8>> {
        if self.next_report.is_some_and(|t| t > now) {
            return None;
        }
        self.next_report = Some(now + REPORT_INTERVAL);
        let echo = self
            .last_report
            .map(|(ts, at)| (ts, now.saturating_duration_since(at).as_millis() as u32));
        let mut blocks = Vec::new();
        for (track, slot) in self.rx.iter_mut().enumerate() {
            if let Some(rx) = slot {
                blocks.push(rx.report_block(track as u8));
                self.stats.inbound[track] = Some(rx.snapshot());
            }
        }
        Some(write_report(&Report {
            ts_ms: now_ms,
            echo,
            blocks,
        }))
    }

    fn new(min_bps: u32, max_bps: u32) -> Self {
        PeerState {
            audio: None,
            video: VideoRx::default(),
            sync: AvSync::default(),
            render_delay: Duration::ZERO,
            stats: PeerStats::default(),
            rx: Default::default(),
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
    /// Сквозной номер следующего пакета каждому участнику (по всем трекам). Живёт
    /// всю сессию, а не с [`PeerState`]: забыв участника (не дошли маячки), мы не
    /// забываем, что он нас слышал, — его журнал прихода ждёт продолжения, а номера
    /// с нуля счёл бы опоздавшими, и обратная связь замолчала бы.
    tw_seq: HashMap<P, u16>,
    pacer: Pacer<P>,
    min_bitrate: u32,
    max_bitrate: u32,
    audio_seq: u16,
    video_seq: u16,
    /// Номер кадра на проводе — u32: 16 бит оборачивались за 36 минут при 30 fps.
    video_frame_id: u32,
    /// Начало отсчёта меток времени — общее у звука и видео (первый захват): метки
    /// обоих треков — моменты на одних часах, по ним приёмник сводит губы со звуком.
    media_epoch: Option<Instant>,
    /// Отправленное видео — для повторов по NACK.
    video_history: TxHistory,
    /// Кто-то из участников попросил ключевой кадр, а кодер его ещё не выдал.
    keyframe_wanted: bool,
    tx_bytes: u64,
    /// Начало наших часов для отчётов и времени прихода (первый увиденный `now`).
    epoch: Option<Instant>,
    /// Последний увиденный `now` — для [`Session::poll_timeout`].
    last_now: Option<Instant>,
    /// Сколько кадров видео встало в очередь показа за всю сессию.
    video_queued: u64,
}

impl<P: Copy + Eq + Hash> Session<P> {
    pub fn new(cfg: Config) -> Self {
        Session {
            cfg,
            peers: Vec::new(),
            state: HashMap::new(),
            tw_seq: HashMap::new(),
            pacer: Pacer::new(DEFAULT_MAX_BITRATE as f64),
            min_bitrate: DEFAULT_MIN_BITRATE,
            max_bitrate: DEFAULT_MAX_BITRATE,
            audio_seq: 0,
            video_seq: 0,
            video_frame_id: 0,
            media_epoch: None,
            video_history: TxHistory::default(),
            keyframe_wanted: false,
            tx_bytes: 0,
            epoch: None,
            last_now: None,
            video_queued: 0,
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
            .map(|st| st.bwe.target() as u32)
            .min()
            .unwrap_or(self.max_bitrate)
    }

    /// Темп выпуска: каждому участнику — его копия, поэтому худшая оценка на их
    /// число. От оценки, а не от урезанной цели кодера: придержанное окном должно
    /// уходить, как только оно откроется.
    fn update_pacing(&mut self) {
        let worst = self
            .peers
            .iter()
            .filter_map(|p| self.state.get(p))
            .map(|st| st.bwe.rate())
            .reduce(f64::min)
            .unwrap_or(self.max_bitrate as f64);
        self.pacer.set_rate(worst * self.peers.len().max(1) as f64);
    }

    pub fn peers(&self) -> &[P] {
        &self.peers
    }

    /// Метка времени момента захвата `captured` в единицах `clock_hz`: от общего для
    /// звука и видео начала отсчёта, по кругу u32 (при 90 кГц — 13 часов).
    fn media_ts(&mut self, captured: Instant, clock_hz: u32) -> u32 {
        let epoch = *self.media_epoch.get_or_insert(captured);
        let micros = captured.saturating_duration_since(epoch).as_micros();
        (micros * clock_hz as u128 / 1_000_000) as u32
    }

    /// Закодированный кадр звука → пакет каждому участнику. `captured` — когда
    /// записан его первый сэмпл. Номер считает только отправленные пакеты, а метка —
    /// время: выключенный микрофон приёмник не примет за потери (и в отчётах, и в
    /// джиттер-буфере после включения — первые ~0,4 с речи терялись).
    pub fn send_audio(&mut self, now: Instant, captured: Instant, payload: &[u8]) {
        self.tick(now);
        let seq = self.audio_seq;
        let ts = self.media_ts(captured, self.cfg.audio_clock_hz);
        self.broadcast(TRACK_AUDIO, seq, ts, payload);
        self.audio_seq = self.audio_seq.wrapping_add(1);
    }

    /// Закодированный видео-кадр → фрагменты каждому участнику. `captured` — момент
    /// съёмки кадра (камера и кодер добавляют к нему десятки мс, и без этой поправки
    /// губы отставали бы от звука).
    pub fn send_video(&mut self, now: Instant, captured: Instant, frame: &[u8], info: VideoInfo) {
        self.tick(now);
        let ts = self.media_ts(captured, VIDEO_CLOCK_HZ);
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
            if class == Class::Video {
                st.bwe.start_probing();
            }
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
        let state = &self.state;
        let held = |p: &P| state.get(p).is_some_and(|st| st.bwe.congested());
        self.pacer.drop_stale(now, held);
        let (mut t, probe) = match self.pacer.pop(now, held) {
            Some(t) => (t, false),
            None => (self.probe(now)?, true),
        };
        let media = matches!(t.data.first(), Some(&MEDIA_V2 | &MEDIA_RTX_V2));
        if let (true, Some(st), Some(epoch)) = (media, self.state.get_mut(&t.to), self.epoch) {
            let next = self.tw_seq.entry(t.to).or_default();
            let seq = *next;
            *next = next.wrapping_add(1);
            set_tw_seq(&mut t.data, seq);
            let at_ms = now.saturating_duration_since(epoch).as_secs_f64() * 1000.0;
            let size = t.data.len() as u32;
            st.bwe.on_sent(seq, at_ms, size);
            if probe {
                st.bwe.on_probe_sent(seq, at_ms, size);
            }
        }
        Some(t)
    }

    /// Пакет пробы полосы ([`crate::probe`]), если кому-то пора: пустой, на треке
    /// заполнения.
    fn probe(&self, now: Instant) -> Option<Transmit<P>> {
        let at_ms = now.saturating_duration_since(self.epoch?).as_secs_f64() * 1000.0;
        let (to, size) = self.peers.iter().find_map(|p| {
            let st = self.state.get(p)?;
            let (due, size) = st.bwe.probe_due()?;
            (due <= at_ms).then_some((*p, size))
        })?;
        let header = MediaHeader {
            track: TRACK_PADDING,
            stream_id: self.cfg.stream_id,
            seq: 0,
            tw_seq: 0,
            ts: 0,
        };
        Some(Transmit {
            to,
            data: media_packet(&header, &vec![0; size], false),
        })
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
            }
            Some(&NACK_V2) => {
                if let Some(n) = parse_nack(data) {
                    self.retransmit(from, n);
                }
            }
            Some(&TRANSPORT_FEEDBACK_V2) => {
                if let Some(fb) = parse_transport_feedback(data) {
                    let now_ms = since_epoch.as_secs_f64() * 1000.0;
                    let rtt_ms = st.rtt().as_secs_f64() * 1000.0;
                    st.bwe.on_feedback(now_ms, &fb, rtt_ms);
                    self.update_pacing();
                }
            }
            Some(&KEYFRAME_REQUEST_V2) => self.handle_keyframe_request(now, from, data),
            _ => {
                if let Some(m) = parse_media(data) {
                    self.handle_media(now, since_epoch, from, &m);
                }
            }
        }
    }

    /// Участник просит у нас ключевой кадр нашего видео — выдать, но не чаще
    /// [`KEYFRAME_REQUEST_MIN_INTERVAL`]: по запросу на каждый потерянный пакет кодер
    /// выдавал бы одни ключевые кадры.
    fn handle_keyframe_request(&mut self, now: Instant, from: P, data: &[u8]) {
        let Some(st) = self.state.get_mut(&from) else {
            return;
        };
        let ours = (TRACK_VIDEO, self.cfg.stream_id);
        let due = st
            .keyframe_honoured
            .is_none_or(|t| now.saturating_duration_since(t) >= KEYFRAME_REQUEST_MIN_INTERVAL);
        if parse_keyframe_request(data) == Some(ours) && due {
            st.keyframe_honoured = Some(now);
            self.keyframe_wanted = true;
        }
    }

    /// Медиа-пакет участника: в журнал прихода (обратная связь), в статистику, в
    /// приём своего трека.
    fn handle_media(&mut self, now: Instant, since_epoch: Duration, from: P, m: &MediaFrame<'_>) {
        let Some(st) = self.state.get_mut(&from) else {
            return;
        };
        let h = &m.header;
        st.arrivals.on_packet(
            h.stream_id,
            h.tw_seq,
            since_epoch.as_micros() as u64 / FEEDBACK_TICK_US,
        );
        // Заполнение проб полосы — только для обратной связи.
        if h.track == TRACK_PADDING {
            return;
        }
        st.stats.rx_bytes += m.payload.len() as u64;
        // Повторы в статистику приёма не идут: потери в отчётах — потери первой
        // отправки, по ним судят о канале.
        if !m.retransmit {
            count_received(&self.cfg, st, h, since_epoch);
        }
        // Новый номер потока — отправитель начал заново (перезашёл в комнату): старое
        // состояние приёма этого трека выбрасываем, а не ждём, пока нумерация догонит.
        match h.track {
            TRACK_VIDEO => {
                let Some(frag) = parse_video_frag(m.payload) else {
                    return;
                };
                st.video.restart_if_new(h.stream_id);
                st.video.nack.on_packet(now, h.seq);
                st.video.asm.push(frag, h.ts);
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
                st.bwe
                    .on_rtt(since_epoch.as_secs_f64() * 1000.0, rtt as f64);
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
        v.playout.set_sync_delay(st.sync.video_delay());
        while let Some(frame) = v.asm.pop(now, wait) {
            v.push(now, frame);
            self.video_queued += 1;
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
        let ask = (v.asm.wants_keyframe() || v.consumer_behind(now))
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
                let probe = st
                    .bwe
                    .probe_due()
                    .zip(self.epoch)
                    .map(|((at_ms, _), e)| e + Duration::from_secs_f64(at_ms / 1000.0));
                [
                    st.next_report.or(self.last_now),
                    v.asm.deadline(max_frame_wait(rtt)),
                    v.nack.next_due(nack_retry(rtt)),
                    keyframe,
                    feedback,
                    probe,
                ]
            })
            .flatten()
            .chain(self.last_now.and_then(|now| {
                self.pacer.next_release(now, |p| {
                    self.state.get(p).is_some_and(|st| st.bwe.congested())
                })
            }))
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
            let due = [st.take_feedback(now), st.take_report(now, now_ms)];
            for data in due.into_iter().flatten() {
                self.pacer
                    .push(Class::Control, now, Transmit { to: *p, data });
            }
        }
        for p in self.peers.clone() {
            self.service_video(now, p);
        }
    }

    /// Следующий кадр звука участника; он будет слышен в `heard_at` (сейчас плюс
    /// задержка вывода — по ней звук сводится с видео). `None` — буфер набирается
    /// или пуст (участник молчит), [`JitterOut::Lost`] — кадр потерян: восстановить из
    /// FEC следующего пакета, если он уже есть, иначе маскировать (PLC);
    /// [`JitterOut::Hold`] — звук ждёт отстающее видео: маскировать.
    pub fn pull_audio(&mut self, heard_at: Instant, peer: &P) -> Option<JitterOut> {
        let clock = self.cfg.audio_clock_hz;
        let frame_s = self.cfg.audio_frame_samples as f64 / clock as f64;
        let st = self.state.get_mut(peer)?;
        let a = st.audio.as_mut()?;
        let want = (st.sync.audio_delay().as_secs_f64() / frame_s).round() as usize;
        match a.lip_step(want) {
            LipStep::Hold => {
                st.stats.lip_sync.audio_holds += 1;
                return Some(JitterOut::Hold);
            }
            LipStep::Drop => {
                st.stats.lip_sync.audio_drops += 1;
                a.jitter.pop();
            }
            LipStep::Play => {}
        }
        let out = a.jitter.pop();
        if let (Some(JitterOut::Frame(_)), Some(ts)) = (&out, a.jitter.played_ts()) {
            st.sync.observe_audio(heard_at, ts, clock);
        }
        out
    }

    /// Следующий видео-кадр участника, если его срок ([`VideoFrame::play_at`]) не
    /// позже `due` (обычно — сейчас).
    pub fn pull_video(&mut self, due: Instant, peer: &P) -> Option<VideoFrame> {
        let st = self.state.get_mut(peer)?;
        let v = &mut st.video;
        if v.queue.front()?.play_at > due {
            return None;
        }
        v.last_pull = Some(due);
        let f = v.queue.pop_front()?;
        st.sync.observe_video(f.play_at + st.render_delay, f.ts);
        Some(f)
    }

    /// Сколько кадров видео встало в очередь показа за всю сессию (от всех). Вырос —
    /// потребителю, спящему до [`Session::next_video_at`], есть что пересмотреть.
    pub fn video_frames_queued(&self) -> u64 {
        self.video_queued
    }

    /// Срок старшего кадра участника в очереди показа: раньше него
    /// [`Session::pull_video`] ничего не отдаст. `None` — очередь пуста.
    pub fn next_video_at(&self, peer: &P) -> Option<Instant> {
        Some(self.state.get(peer)?.video.queue.front()?.play_at)
    }

    /// Сколько кадр участника идёт через видео-декодер потребителя: от срока
    /// ([`VideoFrame::play_at`]) до экрана. Видео на столько позже своего срока, и
    /// звук ждёт его с учётом этого.
    pub fn set_video_render_delay(&mut self, peer: &P, delay: Duration) {
        if let Some(st) = self.state.get_mut(peer) {
            st.render_delay = delay.min(MAX_RENDER_DELAY);
        }
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

    pub fn stats(&self, peer: &P) -> Option<PeerStats> {
        self.state.get(peer).map(|s| PeerStats {
            bandwidth: s.bwe.rate() as u32,
            bandwidth_cuts: (s.bwe.cuts_delay, s.bwe.cuts_loss),
            target: s.bwe.target() as u32,
            in_flight: s.bwe.flight(),
            lip_sync: LipSync {
                error_ms: s.sync.error_ms().map(|e| e as f32),
                audio_wait: s.sync.audio_delay(),
                video_wait: s.sync.video_delay(),
                ..s.stats.lip_sync
            },
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
        s.send_audio(t0, t0, &[0xAA]);
        // Кадр в 20 мс не отправлен (микрофон выключен).
        s.send_audio(t0, t0 + Duration::from_millis(40), &[0xBB]);
        s.send_video(t0, t0, &[0xCC], key());
        s.send_video(
            t0,
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
        s.send_audio(now, now, &[1]);
        s.set_peers(&[1, 2]);
        s.send_audio(now, now, &[2]);
        let tw: Vec<(u8, u16)> = drain(&mut s)
            .iter()
            .map(|t| (t.to, parse_media(&t.data).unwrap().header.tw_seq))
            .collect();
        assert_eq!(tw, [(1, 0), (1, 1), (2, 0)]);
    }

    /// Пошло видео — пошли пробы полосы: пустые пакеты на треке заполнения в общей
    /// сквозной нумерации. Приёмник отмечает их приход для обратной связи, и только.
    /// Звук сам по себе проб не зовёт.
    #[test]
    fn video_starts_bandwidth_probes() {
        let t0 = Instant::now();
        let mut tx = Session::new(CFG);
        tx.set_peers(&[9]);
        tx.send_audio(t0, t0, &[1]);
        assert!(drain(&mut tx).iter().all(|t| t.data[1] == TRACK_AUDIO));
        tx.send_video(t0, t0, &[2; 10], key());
        let mut out = Vec::new();
        for ms in 0..100 {
            let now = t0 + Duration::from_millis(ms);
            tx.handle_timeout(now);
            out.extend(drain(&mut tx));
        }
        let probes: Vec<&Transmit<u8>> =
            out.iter().filter(|t| t.data[1] == TRACK_PADDING).collect();
        assert!(probes.len() >= 10, "пакетов проб {}", probes.len());
        let tw: Vec<u16> = out
            .iter()
            .filter_map(|t| parse_media(&t.data))
            .map(|m| m.header.tw_seq)
            .collect();
        assert_eq!(tw, (1..=tw.len() as u16).collect::<Vec<_>>());

        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        for t in probes {
            rx.handle_datagram(t0, 1, &t.data);
        }
        assert_eq!(rx.stats(&1).unwrap().rx_bytes, 0);
        rx.handle_timeout(t0);
        assert!(drain(&mut rx)
            .iter()
            .any(|t| t.data[0] == TRANSPORT_FEEDBACK_V2));
    }

    /// Забытого и вернувшегося участника нумеруем дальше: он нас не забывал, и
    /// номера с нуля его журнал прихода счёл бы опоздавшими — обратная связь
    /// замолчала бы, а окно перегрузки навсегда заперло бы видео.
    #[test]
    fn readded_peer_continues_tw_seq() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1]);
        s.send_audio(now, now, &[1]);
        let mut out = drain(&mut s);
        s.set_peers(&[]);
        s.set_peers(&[1]);
        s.send_audio(now, now, &[2]);
        out.extend(drain(&mut s));
        let tw: Vec<u16> = out
            .iter()
            .map(|t| parse_media(&t.data).unwrap().header.tw_seq)
            .collect();
        assert_eq!(tw, [0, 1]);
    }

    #[test]
    fn sends_to_every_peer_and_only_hears_peers() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1, 2]);
        s.send_audio(now, now, &[5]);
        let to: Vec<u8> = drain(&mut s).iter().map(|t| t.to).collect();
        assert_eq!(to, [1, 2]);

        let pkt = audio_pkt(1, 0, 0);
        s.handle_datagram(now, 3, &pkt); // не участник
        assert_eq!(s.pull_audio(now, &3), None);
        assert_eq!(s.stats(&3), None);
        s.handle_datagram(now, 1, &pkt);
        assert_eq!(s.pull_audio(now, &1), Some(JitterOut::Frame(vec![0])));
        assert_eq!(s.stats(&1).map(|st| st.rx_bytes), Some(1));
    }

    /// Ушедший участник: ему больше ничего не уходит (даже то, что уже в очереди),
    /// а его состояние приёма сброшено.
    #[test]
    fn removed_peer_loses_queue_and_state() {
        let now = Instant::now();
        let mut s = Session::new(CFG);
        s.set_peers(&[1, 2]);
        s.send_audio(now, now, &[5]);
        s.handle_datagram(now, 2, &audio_pkt(1, 0, 0));
        s.set_peers(&[1]);
        assert!(drain(&mut s).iter().all(|t| t.to == 1));
        assert_eq!(s.stats(&2), None);
        s.set_peers(&[1, 2]);
        assert_eq!(
            s.stats(&2).map(|st| PeerStats {
                bandwidth: 0,
                target: 0,
                in_flight: (st.in_flight.0, 0),
                ..st
            }),
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
            assert!(s.pull_audio(now, &1).is_some());
        }
        s.handle_datagram(now, 1, &audio_pkt(200, 0, 2));
        assert_eq!(s.pull_audio(now, &1), Some(JitterOut::Frame(vec![2])));
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
        old.send_video(now, now, &[1; 10], key());
        let mut new = Session::new(Config {
            stream_id: 2,
            ..CFG
        });
        new.set_peers(&[9]);
        new.send_video(now, now, &[2; 10], key());
        for t in drain(&mut old).into_iter().chain(drain(&mut new)) {
            rx.handle_datagram(now, 1, &t.data);
        }
        let got: Vec<u8> = std::iter::from_fn(|| rx.pull_video(now, &1))
            .map(|f| f.data[0])
            .collect();
        assert_eq!(got, [1, 2]);
    }

    /// Кадры давно пора показать, а их не забирают — очередь прыгает к свежему
    /// ключевому кадру, а не копит задержку.
    #[test]
    fn stalled_video_queue_jumps_to_keyframe() {
        let t0 = Instant::now();
        let mut tx = Session::new(CFG);
        tx.set_peers(&[9]);
        tx.set_bitrate_bounds(1_000_000_000, 1_000_000_000);
        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        let mut now = t0;
        for i in 0..6u8 {
            now = t0 + Duration::from_millis(i as u64 * 200);
            let info = VideoInfo {
                keyframe: i == 0 || i == 3,
                ..key()
            };
            tx.send_video(now, now, &[i; 10], info);
            for t in drain(&mut tx) {
                rx.handle_datagram(now, 1, &t.data);
            }
        }
        // Ключевой кадр 3 пришёл, когда кадр 0 ждал показа уже 600 мс: перескок к 3.
        let got: Vec<u8> = std::iter::from_fn(|| rx.pull_video(now, &1))
            .map(|f| f.data[0])
            .collect();
        assert_eq!(got, [3, 4, 5]);
    }

    /// Потребитель забирает кадры вдвое медленнее, чем они идут: очередь отстаёт, и
    /// у отправителя просят ключевой кадр, к которому можно перескочить. Тайл, который
    /// кадров не забирает вовсе, ключевых кадров не просит.
    #[test]
    fn slow_consumer_gets_a_keyframe_but_an_idle_one_does_not() {
        for consume in [true, false] {
            let t0 = Instant::now();
            let mut tx = Session::new(CFG);
            tx.set_peers(&[9]);
            tx.set_bitrate_bounds(1_000_000_000, 1_000_000_000);
            let mut rx = Session::new(CFG);
            rx.set_peers(&[1]);
            let mut asked = false;
            for i in 0..90u64 {
                let now = t0 + Duration::from_millis(i * 33);
                let info = VideoInfo {
                    keyframe: i == 0,
                    ..key()
                };
                tx.send_video(now, now, &[i as u8; 10], info);
                for t in drain(&mut tx) {
                    rx.handle_datagram(now, 1, &t.data);
                }
                if consume && i % 2 == 0 {
                    rx.pull_video(now, &1);
                }
                asked |= drain(&mut rx)
                    .iter()
                    .any(|t| t.data[0] == crate::wire::KEYFRAME_REQUEST_V2);
            }
            assert_eq!(asked, consume, "потребитель забирает кадры: {consume}");
        }
    }

    /// Кадр отдаётся не раньше, чем его показ подошёл к `due`.
    #[test]
    fn video_is_released_shortly_before_its_time() {
        let t0 = Instant::now();
        let mut tx = Session::new(CFG);
        tx.set_peers(&[9]);
        tx.set_bitrate_bounds(1_000_000_000, 1_000_000_000);
        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        // Оба кадра пришли разом (первый задержался): второй показывать через 33 мс
        // после первого, по меткам захвата.
        tx.send_video(t0, t0, &[0; 10], key());
        let first = drain(&mut tx);
        let t1 = t0 + Duration::from_millis(33);
        tx.send_video(t1, t1, &[1; 10], VideoInfo::default());
        let second = drain(&mut tx);
        let at = t0 + Duration::from_millis(300);
        for t in first.iter().chain(&second) {
            rx.handle_datagram(at, 1, &t.data);
        }
        let f0 = rx.pull_video(at, &1).expect("первый — пора");
        assert_eq!(f0.play_at, at);
        // Второй показывать через 33 мс после первого: к `at` ещё рано.
        assert_eq!(rx.pull_video(at, &1), None);
        let f1 = rx
            .pull_video(at + Duration::from_millis(33), &1)
            .expect("второй — к своему показу");
        assert_eq!(f1.play_at - f0.play_at, Duration::from_millis(33));
    }

    /// Потребитель видео не опрашивает сессию: по счётчику видно, что пришёл кадр, а
    /// по сроку старшего — до когда спать.
    #[test]
    fn consumer_knows_when_the_next_frame_is_due() {
        let t0 = Instant::now();
        let mut tx = Session::new(CFG);
        tx.set_peers(&[9]);
        tx.set_bitrate_bounds(1_000_000_000, 1_000_000_000);
        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        assert_eq!(rx.video_frames_queued(), 0);
        assert_eq!(rx.next_video_at(&1), None);
        tx.send_video(t0, t0, &[0; 10], key());
        let first = drain(&mut tx);
        let t1 = t0 + Duration::from_millis(33);
        tx.send_video(t1, t1, &[1; 10], VideoInfo::default());
        let at = t0 + Duration::from_millis(300);
        for t in first.iter().chain(&drain(&mut tx)) {
            rx.handle_datagram(at, 1, &t.data);
        }
        assert_eq!(rx.video_frames_queued(), 2);
        assert_eq!(rx.next_video_at(&1), Some(at));
        rx.pull_video(at, &1).unwrap();
        assert_eq!(rx.next_video_at(&1), Some(at + Duration::from_millis(33)));
        assert_eq!(rx.next_video_at(&2), None);
        assert_eq!(
            rx.video_frames_queued(),
            2,
            "счётчик — пришедшие, не лежащие"
        );
    }

    /// Ждём недостающий кадр две попытки NACK (2,5 RTT и 50 мс сверху), от 0,1 до
    /// 0,8 с; NACK повторяем через RTT и 30 мс, не чаще 50 мс.
    #[test]
    fn video_waits_scale_with_rtt() {
        let ms = Duration::from_millis;
        assert_eq!(max_frame_wait(ms(100)), ms(300));
        assert_eq!(max_frame_wait(ms(10)), ms(100));
        assert_eq!(max_frame_wait(ms(1000)), ms(800));
        assert_eq!(nack_retry(ms(100)), ms(130));
        assert_eq!(nack_retry(ms(5)), ms(50));
    }

    /// Джиттер звука растёт на четверть разницы, спадает на шестнадцатую; глубина
    /// буфера — кадр на 20 мс джиттера плюс два, в пределах 2…8.
    #[test]
    fn audio_depth_follows_jitter() {
        assert_eq!(jitter_estimate(10.0, 70.0, 20.0), 20.0);
        assert_eq!(jitter_estimate(26.0, 20.0, 20.0), 24.375);
        assert_eq!(jitter_estimate(10.0, 0.0, 20.0), 12.5);
        assert_eq!(depth_for_jitter(0.0), 2);
        assert_eq!(depth_for_jitter(29.0), 3);
        assert_eq!(depth_for_jitter(31.0), 4);
        assert_eq!(depth_for_jitter(500.0), 8);
    }

    /// Тик воспроизведения как в [`Session::pull_audio`]: пришёл кадр `seq`, шаг
    /// губ, затем кадр в вывод.
    fn tick(a: &mut AudioRx, seq: &mut u16, want: usize) -> LipStep {
        a.jitter.push(*seq, 0, vec![0]);
        *seq += 1;
        let step = a.lip_step(want);
        if step != LipStep::Hold {
            a.jitter.pop();
        }
        step
    }

    /// Звук должен отстать на 2 кадра: кадр маскировки раз в 5 кадров, пока не
    /// наберётся; задержку сняли — по кадру выбрасываем с тем же темпом.
    #[test]
    fn lip_sync_holds_and_drops_one_frame_at_a_time() {
        let mut a = AudioRx::new(1, 1);
        let mut seq = 0;
        let at = |steps: Vec<LipStep>, want: LipStep| -> Vec<usize> {
            (0..steps.len()).filter(|&i| steps[i] == want).collect()
        };
        let steps = (0..15).map(|_| tick(&mut a, &mut seq, 2)).collect();
        assert_eq!(at(steps, LipStep::Hold), [4, 9]);
        let steps = (0..15).map(|_| tick(&mut a, &mut seq, 0)).collect();
        // С прошлой подстройки прошло больше 5 кадров — первый выброс сразу.
        assert_eq!(at(steps, LipStep::Drop), [0, 5]);
        assert_eq!(a.held, 0);
    }

    /// Не играет — подстраивать нечего; выбрасывать последний кадр — значит
    /// сделать дыру.
    #[test]
    fn lip_sync_waits_for_playback_and_keeps_the_last_frame() {
        let mut a = AudioRx::new(1, 3);
        a.jitter.push(0, 0, vec![0]);
        assert!((0..10).all(|_| a.lip_step(2) == LipStep::Play), "не играет");
        // Играет, лишнего три кадра, но в буфере всего один — не выбрасываем.
        let mut a = AudioRx::new(1, 1);
        a.held = 3;
        let mut seq = 0;
        let steps: Vec<LipStep> = (0..10).map(|_| tick(&mut a, &mut seq, 0)).collect();
        assert!(!steps.contains(&LipStep::Drop));
    }

    /// Пакеты звука, пришедшие с разбросом, углубляют буфер: до начала игры он
    /// копит больше кадров.
    #[test]
    fn jittery_audio_gets_a_deeper_buffer() {
        let t0 = Instant::now();
        let mut a = AudioRx::new(1, 2);
        // 20 мс кадры (960 сэмплов при 48 кГц), приходят то сразу, то через 80 мс.
        for i in 0..40u16 {
            let at = t0 + Duration::from_millis(i as u64 * 20 + if i % 2 == 0 { 0 } else { 80 });
            a.push(at, i, i as u32 * 960, vec![i as u8], 48_000.0);
        }
        assert!(a.jitter_estimate_ms > 50.0, "{}", a.jitter_estimate_ms);
        let mut b = AudioRx::new(1, 2);
        for i in 0..40u16 {
            let at = t0 + Duration::from_millis(i as u64 * 20);
            b.push(at, i, i as u32 * 960, vec![i as u8], 48_000.0);
        }
        // Спадает на 1/16 за пакет: с 30 мс за 40 ровных пакетов — до 2,4.
        assert!(b.jitter_estimate_ms < 2.5, "{}", b.jitter_estimate_ms);
    }

    #[test]
    fn large_frame_is_fragmented_and_reassembled() {
        let now = Instant::now();
        let mut tx = Session::new(CFG);
        tx.set_peers(&[9]);
        // Без ровного темпа: кадр нужен целиком в один момент.
        tx.set_bitrate_bounds(1_000_000_000, 1_000_000_000);
        let mut rx = Session::new(CFG);
        rx.set_peers(&[1]);
        let frame = vec![0x5A; MAX_VIDEO_CHUNK * 3 + 1];
        tx.send_video(now, now, &frame, key());
        let out = drain(&mut tx);
        assert_eq!(out.len(), 4);
        for t in out.iter().rev() {
            rx.handle_datagram(now, 1, &t.data);
        }
        assert_eq!(
            rx.pull_video(now, &1),
            Some(VideoFrame {
                data: frame,
                info: key(),
                ts: 0,
                play_at: now,
            })
        );
    }
}
