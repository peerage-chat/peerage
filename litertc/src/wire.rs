//! Формат медиа-пакета (внутри запечатанной датаграммы) и видео-фрагмента — v2.
//! Контракт описан в `docs/architecture/litertc-wire.md`; тесты `golden_*` держат
//! его байт в байт.
//!
//! Шифрование, аутентичность отправителя и байт канала снаружи — забота
//! вызывающего (в Peerage — `messenger` и сессия 1:1, SEC-4). Сюда приходит уже
//! расшифрованный пакет:
//! ```text
//! байт 0       : версия и тип (0x20 — v2, медиа)
//! байт 1       : track (тип потока: аудио/видео/экран)
//! байты 2..6   : stream_id (u32) — случайный номер потока отправителя
//! байты 6..8   : seq (u16) — номер пакета в треке
//! байты 8..10  : tw_seq (u16) — сквозной номер пакета этому получателю, по всем трекам
//! байты 10..14 : ts (u32) — время захвата: звук — 48 кГц, видео — 90 кГц
//! байты 14..   : payload (кадр Opus или видео-фрагмент)
//! ```
//! Все целые — big-endian.

/// Байт 0: версия 2 (старшие 4 бита), тип 0 — медиа (младшие). v1 начинался с
/// 0x02 — такой пакет не разбирается, старые и новые сборки друг друга не слышат.
pub const MEDIA_V2: u8 = 0x20;

/// Байт 0 повторно отправленного медиа-пакета (по NACK): v2, тип 4. Остальное — как у
/// [`MEDIA_V2`], с исходными `seq` и `ts`. Отдельный тип нужен статистике: потери в
/// отчётах — потери первой отправки, иначе оценка полосы не увидела бы перегрузку.
pub const MEDIA_RTX_V2: u8 = 0x24;

/// Длина медиа-заголовка.
pub const MEDIA_HEADER_LEN: usize = 14;

/// Лимит payload-а медиа-пакета. Сеть (в Peerage — Yggdrasil) сама фрагментирует на IP-уровне,
/// поэтому мы можем отдавать крупные куски и снижать cgo-оверхед на пакет.
/// 60000 байт = ~15-25 фрагментов на FHD-кадр вместо ~150+ при старом MTU.
pub const MAX_MEDIA_PAYLOAD: usize = 60_000;

/// Идентификаторы потоков (track).
pub const TRACK_AUDIO: u8 = 0;
pub const TRACK_VIDEO: u8 = 1;
pub const TRACK_SCREEN: u8 = 2;
/// Заполнение проб полосы: пустой payload, приёмник только отмечает приход для
/// обратной связи. Приёмники до него и так поступали с неизвестным треком так же.
pub const TRACK_PADDING: u8 = 3;

/// Частота часов видео: метка времени видео-пакета — время захвата в этих единицах.
pub const VIDEO_CLOCK_HZ: u32 = 90_000;

/// Поля медиа-заголовка.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaHeader {
    pub track: u8,
    /// Случайный номер потока: меняется, когда отправитель начинает заново
    /// (перезашёл в комнату). По нему приёмник отличает новый поток от опоздавших
    /// пакетов старого.
    pub stream_id: u32,
    pub seq: u16,
    pub tw_seq: u16,
    pub ts: u32,
}

/// Разобранный медиа-пакет (заголовок + срез payload без копирования).
#[derive(Debug, PartialEq, Eq)]
pub struct MediaFrame<'a> {
    pub header: MediaHeader,
    /// Повторная отправка по NACK ([`MEDIA_RTX_V2`]).
    pub retransmit: bool,
    pub payload: &'a [u8],
}

/// Записать медиа-заголовок в начало `out` (нужно `out.len() >= MEDIA_HEADER_LEN`).
#[inline]
pub fn write_media_header(out: &mut [u8], h: &MediaHeader) {
    out[0] = MEDIA_V2;
    out[1] = h.track;
    out[2..6].copy_from_slice(&h.stream_id.to_be_bytes());
    out[6..8].copy_from_slice(&h.seq.to_be_bytes());
    out[8..10].copy_from_slice(&h.tw_seq.to_be_bytes());
    out[10..14].copy_from_slice(&h.ts.to_be_bytes());
}

/// Вписать сквозной номер в готовый медиа-пакет: он назначается в момент выпуска в
/// сеть, а не постановки в очередь, — по нему отправитель знает время отправки.
#[inline]
pub fn set_tw_seq(packet: &mut [u8], tw_seq: u16) {
    packet[8..10].copy_from_slice(&tw_seq.to_be_bytes());
}

/// Разобрать медиа-пакет (первичный или повторный). `None` — не v2-медиа или короче
/// заголовка.
#[inline]
pub fn parse_media(dg: &[u8]) -> Option<MediaFrame<'_>> {
    if dg.len() < MEDIA_HEADER_LEN || (dg[0] != MEDIA_V2 && dg[0] != MEDIA_RTX_V2) {
        return None;
    }
    let header = MediaHeader {
        track: dg[1],
        stream_id: u32::from_be_bytes([dg[2], dg[3], dg[4], dg[5]]),
        seq: u16::from_be_bytes([dg[6], dg[7]]),
        tw_seq: u16::from_be_bytes([dg[8], dg[9]]),
        ts: u32::from_be_bytes([dg[10], dg[11], dg[12], dg[13]]),
    };
    Some(MediaFrame {
        header,
        retransmit: dg[0] == MEDIA_RTX_V2,
        payload: &dg[MEDIA_HEADER_LEN..],
    })
}

// ---- Видео: фрагментация кадров (кадр > MTU) ----
//
// Видео-кадр (H.264) обычно больше одной датаграммы, поэтому режется на чанки.
// Каждый чанк передаётся как медиа-payload (track = VIDEO/SCREEN) со своим
// суб-заголовком:
// ```text
// байты 0..4 : frame_id (u32) — номер кадра (группирует фрагменты)
// байты 4..6 : frag_idx (u16) — индекс фрагмента в кадре
// байты 6..8 : frag_count (u16) — всего фрагментов в кадре
// байт  8    : flags (бит 0 — ключевой кадр)
// байт  9    : rotation — на сколько четвертей оборота по часовой довернуть кадр
// байт  10   : fps отправителя (0 — неизвестен)
// байты 11.. : кусок данных кадра
// ```

/// Длина видео-суб-заголовка.
pub const VIDEO_HDR: usize = 11;
/// Флаг ключевого кадра.
pub const VIDEO_KEYFRAME: u8 = 0x01;

/// Максимум данных кадра в одном фрагменте. Сеть сама фрагментирует на IP-уровне,
/// поэтому берём крупные куски для снижения cgo-оверхеда. 8000 байт ≈ 6-7 IP-пакетов.
pub const MAX_VIDEO_CHUNK: usize = 8_000;

/// Больше фрагментов в кадре не принимаем: 8 МБ при [`MAX_VIDEO_CHUNK`] — с запасом
/// на ключевой кадр 4K. Без предела участник звонка объявил бы кадр в 65 535
/// фрагментов, и мы копили бы его до полугигабайта.
pub const MAX_VIDEO_FRAGS: u16 = 1024;

/// Свойства видео-кадра, которые едут в протоколе рядом с ним.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoInfo {
    pub keyframe: bool,
    /// Четверти оборота по часовой (0..=3), на которые приёмник доворачивает кадр:
    /// камера телефона снимает в ориентации сенсора, а не экрана.
    pub rotation: u8,
    /// Кадров в секунду, заявленных кодеру отправителя, — для сведения: показ идёт по
    /// меткам времени захвата (настоящий темп камеры бывает ниже).
    pub fps: u8,
}

/// Разбить кадр на медиа-payload-ы (суб-заголовок + кусок), готовые к отправке
/// как медиа-датаграммы (track = VIDEO/SCREEN). Пустой кадр → один пустой фрагмент.
pub fn fragment_video(frame: &[u8], frame_id: u32, info: VideoInfo) -> Vec<Vec<u8>> {
    let flags = if info.keyframe { VIDEO_KEYFRAME } else { 0 };
    let chunks: Vec<&[u8]> = if frame.is_empty() {
        vec![&[]]
    } else {
        frame.chunks(MAX_VIDEO_CHUNK).collect()
    };
    let count = chunks.len() as u16;
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            let mut out = Vec::with_capacity(VIDEO_HDR + chunk.len());
            out.extend_from_slice(&frame_id.to_be_bytes());
            out.extend_from_slice(&(i as u16).to_be_bytes());
            out.extend_from_slice(&count.to_be_bytes());
            out.push(flags);
            out.push(info.rotation % 4);
            out.push(info.fps);
            out.extend_from_slice(chunk);
            out
        })
        .collect()
}

/// Разобранный видео-фрагмент (из медиа-payload).
#[derive(Debug)]
pub struct VideoFrag<'a> {
    pub frame_id: u32,
    pub frag_idx: u16,
    pub frag_count: u16,
    pub info: VideoInfo,
    pub data: &'a [u8],
}

/// Разобрать видео-фрагмент из медиа-payload (то, что вернул [`parse_media`] как payload).
pub fn parse_video_frag(payload: &[u8]) -> Option<VideoFrag<'_>> {
    if payload.len() < VIDEO_HDR {
        return None;
    }
    let frame_id = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let frag_idx = u16::from_be_bytes([payload[4], payload[5]]);
    let frag_count = u16::from_be_bytes([payload[6], payload[7]]);
    let info = VideoInfo {
        keyframe: payload[8] & VIDEO_KEYFRAME != 0,
        rotation: payload[9] % 4,
        fps: payload[10],
    };
    Some(VideoFrag {
        frame_id,
        frag_idx,
        frag_count,
        info,
        data: &payload[VIDEO_HDR..],
    })
}

// ---- Отчёт получателя (обратная связь, как RTCP RR) ----
//
// Каждый участник раз в секунду шлёт каждому отчёт о том, как он принимает его
// потоки, и эхо для RTT. Идёт по тому же CHAN_MEDIA: реальное время, потеря
// допустима (следующий отчёт через секунду).
// ```text
// байт 0       : 0x21 — v2, тип 1 (отчёт)
// байты 1..5   : ts_ms (u32) — время отправки по часам отправителя отчёта, мс
// байты 5..9   : echo_ts_ms (u32) — ts_ms последнего отчёта, полученного от адресата
// байты 9..13  : echo_delay_ms (u32) — сколько мс прошло с его получения; u32::MAX — эха нет
// байт  13     : n — число блоков (0..=3)
// байты 14..   : n блоков по 18 байт:
//   track u8, fraction_lost u8 (доля потерь за интервал × 256), stream_id u32,
//   highest_seq u32 (расширенный: циклы × 2¹⁶ + seq), cumulative_lost u32,
//   jitter u32 (в единицах часов трека)
// ```

/// Байт 0 отчёта: v2, тип 1.
pub const REPORT_V2: u8 = 0x21;
/// Длина заголовка отчёта.
pub const REPORT_HEADER_LEN: usize = 14;
/// Длина блока отчёта.
pub const REPORT_BLOCK_LEN: usize = 18;
/// Больше блоков не бывает: по одному на трек.
pub const MAX_REPORT_BLOCKS: usize = 3;
/// `echo_delay_ms` без эха.
pub const NO_ECHO: u32 = u32::MAX;

/// Как получатель принимает один поток отправителя.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReportBlock {
    pub track: u8,
    /// Доля потерь с прошлого отчёта, × 256.
    pub fraction_lost: u8,
    pub stream_id: u32,
    pub highest_seq: u32,
    pub cumulative_lost: u32,
    /// Межпакетный джиттер (RFC 3550, §6.4.1) в единицах часов трека.
    pub jitter: u32,
}

/// Отчёт получателя.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub ts_ms: u32,
    /// (ts_ms из отчёта адресата, мс с его получения) — для RTT.
    pub echo: Option<(u32, u32)>,
    pub blocks: Vec<ReportBlock>,
}

/// Собрать отчёт. Блоков больше [`MAX_REPORT_BLOCKS`] не бывает (лишние отброшены).
pub fn write_report(r: &Report) -> Vec<u8> {
    let blocks = &r.blocks[..r.blocks.len().min(MAX_REPORT_BLOCKS)];
    let mut out = Vec::with_capacity(REPORT_HEADER_LEN + blocks.len() * REPORT_BLOCK_LEN);
    out.push(REPORT_V2);
    out.extend_from_slice(&r.ts_ms.to_be_bytes());
    let (echo_ts, echo_delay) = r.echo.unwrap_or((0, NO_ECHO));
    out.extend_from_slice(&echo_ts.to_be_bytes());
    out.extend_from_slice(&echo_delay.to_be_bytes());
    out.push(blocks.len() as u8);
    for b in blocks {
        out.push(b.track);
        out.push(b.fraction_lost);
        out.extend_from_slice(&b.stream_id.to_be_bytes());
        out.extend_from_slice(&b.highest_seq.to_be_bytes());
        out.extend_from_slice(&b.cumulative_lost.to_be_bytes());
        out.extend_from_slice(&b.jitter.to_be_bytes());
    }
    out
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Разобрать отчёт. `None` — не отчёт, обрезан или блоков больше допустимого.
pub fn parse_report(dg: &[u8]) -> Option<Report> {
    if dg.len() < REPORT_HEADER_LEN || dg[0] != REPORT_V2 {
        return None;
    }
    let n = dg[13] as usize;
    if n > MAX_REPORT_BLOCKS || dg.len() != REPORT_HEADER_LEN + n * REPORT_BLOCK_LEN {
        return None;
    }
    let echo_delay = be32(&dg[9..]);
    let blocks = dg[REPORT_HEADER_LEN..]
        .chunks_exact(REPORT_BLOCK_LEN)
        .map(|b| ReportBlock {
            track: b[0],
            fraction_lost: b[1],
            stream_id: be32(&b[2..]),
            highest_seq: be32(&b[6..]),
            cumulative_lost: be32(&b[10..]),
            jitter: be32(&b[14..]),
        })
        .collect();
    Some(Report {
        ts_ms: be32(&dg[1..]),
        echo: (echo_delay != NO_ECHO).then(|| (be32(&dg[5..]), echo_delay)),
        blocks,
    })
}

// ---- NACK: просьба повторить видео-пакеты (как RFC 4585 Generic NACK) ----
// ```text
// байт 0      : 0x22 — v2, тип 2
// байт 1      : track
// байты 2..6  : stream_id — о каком потоке адресата
// байт 6      : n — число пар (1..=128)
// байты 7..   : n пар (pid u16, blp u16): потерян pid, и pid+i+1 — если бит i в blp
// ```

/// Байт 0 NACK.
pub const NACK_V2: u8 = 0x22;
/// Длина заголовка NACK до пар.
pub const NACK_HEADER_LEN: usize = 7;
/// Больше пар не бывает: 128 × 17 номеров — с запасом на любую дыру, которую ещё
/// стоит чинить повтором, а не ключевым кадром.
pub const MAX_NACK_ITEMS: usize = 128;

/// Просьба повторить пакеты трека.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nack {
    pub track: u8,
    pub stream_id: u32,
    /// Номера пакетов; в проводе — пары (pid, blp).
    pub seqs: Vec<u16>,
}

/// Собрать NACK. Номера упаковываются по порядку, как переданы: каждый следующий в
/// пределах 16 после начала пары ложится в её маску. Пары сверх [`MAX_NACK_ITEMS`]
/// отброшены. `None` — номеров нет.
pub fn write_nack(n: &Nack) -> Option<Vec<u8>> {
    let mut items: Vec<(u16, u16)> = Vec::new();
    for &seq in &n.seqs {
        if let Some((pid, blp)) = items.last_mut() {
            let d = seq.wrapping_sub(*pid);
            if (1..=16).contains(&d) {
                *blp |= 1 << (d - 1);
                continue;
            }
        }
        if items.len() == MAX_NACK_ITEMS {
            break;
        }
        items.push((seq, 0));
    }
    if items.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(NACK_HEADER_LEN + 4 * items.len());
    out.push(NACK_V2);
    out.push(n.track);
    out.extend_from_slice(&n.stream_id.to_be_bytes());
    out.push(items.len() as u8);
    for (pid, blp) in items {
        out.extend_from_slice(&pid.to_be_bytes());
        out.extend_from_slice(&blp.to_be_bytes());
    }
    Some(out)
}

/// Разобрать NACK. `None` — не NACK, длина не сходится, пар 0 или больше предела.
pub fn parse_nack(dg: &[u8]) -> Option<Nack> {
    if dg.len() < NACK_HEADER_LEN || dg[0] != NACK_V2 {
        return None;
    }
    let n = dg[6] as usize;
    if n == 0 || n > MAX_NACK_ITEMS || dg.len() != NACK_HEADER_LEN + 4 * n {
        return None;
    }
    let mut seqs = Vec::new();
    for item in dg[NACK_HEADER_LEN..].chunks_exact(4) {
        let pid = u16::from_be_bytes([item[0], item[1]]);
        let blp = u16::from_be_bytes([item[2], item[3]]);
        seqs.push(pid);
        for i in 0..16u16 {
            if blp & (1 << i) != 0 {
                seqs.push(pid.wrapping_add(i + 1));
            }
        }
    }
    Some(Nack {
        track: dg[1],
        stream_id: be32(&dg[2..]),
        seqs,
    })
}

// ---- Запрос ключевого кадра (как PLI, RFC 4585) ----
// ```text
// байт 0      : 0x23 — v2, тип 3
// байт 1      : track
// байты 2..6  : stream_id — о каком потоке адресата
// ```

/// Байт 0 запроса ключевого кадра.
pub const KEYFRAME_REQUEST_V2: u8 = 0x23;
/// Длина запроса ключевого кадра.
pub const KEYFRAME_REQUEST_LEN: usize = 6;

/// Собрать запрос ключевого кадра.
pub fn write_keyframe_request(track: u8, stream_id: u32) -> Vec<u8> {
    let mut out = vec![KEYFRAME_REQUEST_V2, track];
    out.extend_from_slice(&stream_id.to_be_bytes());
    out
}

/// Разобрать запрос ключевого кадра: (track, stream_id).
pub fn parse_keyframe_request(dg: &[u8]) -> Option<(u8, u32)> {
    if dg.len() != KEYFRAME_REQUEST_LEN || dg[0] != KEYFRAME_REQUEST_V2 {
        return None;
    }
    Some((dg[1], be32(&dg[2..])))
}

// ---- Транспортная обратная связь: когда пришёл каждый пакет (как transport-cc) ----
//
// По ней отправитель оценивает полосу по задержке: растущая разница между временем
// прихода и отправки — очередь копится, пора сбавлять. Охватывает подряд идущие
// сквозные номера (`tw_seq`) от `base_seq`.
// ```text
// байт 0      : 0x25 — v2, тип 5
// байты 1..3  : base_seq (u16) — сквозной номер первого пакета
// байты 3..5  : count (u16, 1..=1024)
// байты 5..9  : ref_time (u32) — приход самого раннего из полученных, единицы 250 мкс,
//               по часам получателя
// байты 9..   : count × u16 — приход позже ref_time в единицах 250 мкс; 0xFFFF — не пришёл
// ```

/// Байт 0 транспортной обратной связи.
pub const TRANSPORT_FEEDBACK_V2: u8 = 0x25;
/// Длина её заголовка.
pub const TRANSPORT_FEEDBACK_HEADER_LEN: usize = 9;
/// Больше пакетов в одной не бывает.
pub const MAX_FEEDBACK_PACKETS: usize = 1024;
/// Единица времени прихода, мкс.
pub const FEEDBACK_TICK_US: u64 = 250;
/// «Пакет не пришёл».
const NOT_RECEIVED: u16 = 0xFFFF;

/// Когда пришли пакеты `base_seq`, `base_seq + 1`, … .
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransportFeedback {
    pub base_seq: u16,
    /// Время прихода самого раннего из полученных, в [`FEEDBACK_TICK_US`].
    pub ref_time: u32,
    /// По пакету: на сколько единиц позже `ref_time` пришёл; `None` — не пришёл.
    pub arrivals: Vec<Option<u16>>,
}

/// Собрать обратную связь. Пакетов сверх [`MAX_FEEDBACK_PACKETS`] не пишет; задержки
/// больше 16 с обрезаются до предельной. `None` — пакетов нет.
pub fn write_transport_feedback(f: &TransportFeedback) -> Option<Vec<u8>> {
    let arrivals = &f.arrivals[..f.arrivals.len().min(MAX_FEEDBACK_PACKETS)];
    if arrivals.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(TRANSPORT_FEEDBACK_HEADER_LEN + 2 * arrivals.len());
    out.push(TRANSPORT_FEEDBACK_V2);
    out.extend_from_slice(&f.base_seq.to_be_bytes());
    out.extend_from_slice(&(arrivals.len() as u16).to_be_bytes());
    out.extend_from_slice(&f.ref_time.to_be_bytes());
    for a in arrivals {
        let d = a.map_or(NOT_RECEIVED, |d| d.min(NOT_RECEIVED - 1));
        out.extend_from_slice(&d.to_be_bytes());
    }
    Some(out)
}

/// Разобрать обратную связь. `None` — не она, длина не сходится, пакетов 0 или больше
/// предела.
pub fn parse_transport_feedback(dg: &[u8]) -> Option<TransportFeedback> {
    if dg.len() < TRANSPORT_FEEDBACK_HEADER_LEN || dg[0] != TRANSPORT_FEEDBACK_V2 {
        return None;
    }
    let count = u16::from_be_bytes([dg[3], dg[4]]) as usize;
    if count == 0
        || count > MAX_FEEDBACK_PACKETS
        || dg.len() != TRANSPORT_FEEDBACK_HEADER_LEN + 2 * count
    {
        return None;
    }
    let arrivals = dg[TRANSPORT_FEEDBACK_HEADER_LEN..]
        .chunks_exact(2)
        .map(|d| Some(u16::from_be_bytes([d[0], d[1]])).filter(|&d| d != NOT_RECEIVED))
        .collect();
    Some(TransportFeedback {
        base_seq: u16::from_be_bytes([dg[1], dg[2]]),
        ref_time: be32(&dg[5..]),
        arrivals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_report() {
        let r = Report {
            ts_ms: 0x0102_0304,
            echo: Some((0x0A0B_0C0D, 20)),
            blocks: vec![ReportBlock {
                track: TRACK_VIDEO,
                fraction_lost: 64,
                stream_id: 0x1122_3344,
                highest_seq: 0x0001_0005,
                cumulative_lost: 7,
                jitter: 900,
            }],
        };
        let bytes = write_report(&r);
        let want: Vec<u8> = [
            &[0x21, 1, 2, 3, 4, 0x0A, 0x0B, 0x0C, 0x0D, 0, 0, 0, 20, 1][..],
            &[
                1, 64, 0x11, 0x22, 0x33, 0x44, 0, 1, 0, 5, 0, 0, 0, 7, 0, 0, 3, 0x84,
            ],
        ]
        .concat();
        assert_eq!(bytes, want);
        assert_eq!(parse_report(&bytes), Some(r));
    }

    #[test]
    fn report_without_echo_and_bad_lengths() {
        let r = Report {
            ts_ms: 5,
            echo: None,
            blocks: vec![],
        };
        let bytes = write_report(&r);
        assert_eq!(parse_report(&bytes), Some(r));
        // Обрезанный, лишний хвост, блоков больше трёх — не отчёт.
        assert!(parse_report(&bytes[..REPORT_HEADER_LEN - 1]).is_none());
        assert!(parse_report(&[&bytes[..], &[0]].concat()).is_none());
        let mut many = bytes.clone();
        many[13] = 4;
        many.extend_from_slice(&[0; 4 * REPORT_BLOCK_LEN]);
        assert!(parse_report(&many).is_none());
        // Медиа-пакет не разбирается как отчёт и наоборот.
        assert!(parse_media(&bytes).is_none());
    }

    const H: MediaHeader = MediaHeader {
        track: TRACK_AUDIO,
        stream_id: 0xA1B2_C3D4,
        seq: 0xBEEF,
        tw_seq: 7,
        ts: 0x0102_0304,
    };

    #[test]
    fn header_roundtrip() {
        let mut buf = [0u8; MEDIA_HEADER_LEN + 4];
        write_media_header(&mut buf, &H);
        buf[MEDIA_HEADER_LEN..].copy_from_slice(&[1, 2, 3, 4]);

        let f = parse_media(&buf).unwrap();
        assert_eq!(f.header, H);
        assert_eq!(f.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn parse_rejects_non_media_and_short() {
        assert!(parse_media(&[0x01; MEDIA_HEADER_LEN]).is_none()); // байт чата, не медиа
        assert!(parse_media(&[MEDIA_V2, 0, 0]).is_none()); // короче заголовка
        assert!(parse_media(&[]).is_none());
    }

    /// Пакет v1 (маркер 0x02, заголовок 8 байт) не принимается за v2.
    #[test]
    fn v1_packet_is_rejected() {
        let mut v1 = vec![0x02, TRACK_AUDIO, 0, 1, 0, 0, 3, 0xC0];
        v1.extend_from_slice(&[0xAA; 20]);
        assert!(parse_media(&v1).is_none());
    }

    #[test]
    fn empty_frame_one_fragment() {
        let frags = fragment_video(&[], 5, VideoInfo::default());
        assert_eq!(frags.len(), 1);
        let f = parse_video_frag(&frags[0]).unwrap();
        assert_eq!(f.frag_count, 1);
        assert_eq!(f.data.len(), 0);
    }

    /// Формат v2 на проводе — байт в байт. Меняется только вместе с версией.
    #[test]
    fn golden_media_header() {
        let mut buf = [0u8; MEDIA_HEADER_LEN];
        let h = MediaHeader {
            track: TRACK_VIDEO,
            stream_id: 0x1122_3344,
            seq: 0x0102,
            tw_seq: 0x0506,
            ts: 0x0A0B_0C0D,
        };
        write_media_header(&mut buf, &h);
        assert_eq!(
            buf,
            [0x20, 0x01, 0x11, 0x22, 0x33, 0x44, 0x01, 0x02, 0x05, 0x06, 0x0A, 0x0B, 0x0C, 0x0D]
        );
    }

    #[test]
    fn golden_video_fragments() {
        let frame: Vec<u8> = (0..MAX_VIDEO_CHUNK + 3).map(|i| i as u8).collect();
        let info = VideoInfo {
            keyframe: true,
            rotation: 3,
            fps: 30,
        };
        let frags = fragment_video(&frame, 0x0102_0304, info);
        assert_eq!(frags.len(), 2);
        assert_eq!(
            frags[0][..VIDEO_HDR],
            [0x01, 0x02, 0x03, 0x04, 0, 0, 0, 2, VIDEO_KEYFRAME, 3, 30]
        );
        assert_eq!(frags[0].len(), VIDEO_HDR + MAX_VIDEO_CHUNK);
        assert_eq!(
            frags[1][..VIDEO_HDR],
            [0x01, 0x02, 0x03, 0x04, 0, 1, 0, 2, VIDEO_KEYFRAME, 3, 30]
        );
        assert_eq!(&frags[1][VIDEO_HDR..], &frame[MAX_VIDEO_CHUNK..]);
        assert_eq!(
            (MAX_VIDEO_CHUNK, MAX_MEDIA_PAYLOAD, VIDEO_HDR),
            (8_000, 60_000, 11)
        );
    }

    #[test]
    fn video_info_roundtrip() {
        let info = VideoInfo {
            keyframe: false,
            rotation: 1,
            fps: 24,
        };
        let frags = fragment_video(&[9; 10], 1, info);
        assert_eq!(parse_video_frag(&frags[0]).unwrap().info, info);
    }

    #[test]
    fn golden_nack() {
        let n = Nack {
            track: TRACK_VIDEO,
            stream_id: 0x1122_3344,
            // 10, 11, 13, 26 — одна пара (26 = 10 + 16); 27 — уже следующая.
            seqs: vec![10, 11, 13, 26, 27],
        };
        let bytes = write_nack(&n).unwrap();
        assert_eq!(
            bytes,
            [0x22, 0x01, 0x11, 0x22, 0x33, 0x44, 2, 0, 10, 0x80, 0x05, 0, 27, 0, 0]
        );
        assert_eq!(parse_nack(&bytes), Some(n));
    }

    #[test]
    fn nack_wraps_and_rejects_garbage() {
        let n = Nack {
            track: TRACK_VIDEO,
            stream_id: 1,
            seqs: vec![65_535, 0, 1],
        };
        let bytes = write_nack(&n).unwrap();
        assert_eq!(bytes.len(), NACK_HEADER_LEN + 4, "обёртка не рвёт пару");
        assert_eq!(parse_nack(&bytes), Some(n));
        assert_eq!(write_nack(&Nack::default()), None);
        assert_eq!(parse_nack(&bytes[..bytes.len() - 1]), None);
        let mut zero = bytes.clone();
        zero[6] = 0;
        zero.truncate(NACK_HEADER_LEN);
        assert_eq!(parse_nack(&zero), None, "пустой NACK");
    }

    #[test]
    fn nack_items_are_capped() {
        // Номера через 17 — каждый в своей паре; лишние пары отброшены.
        let seqs: Vec<u16> = (0..200u16).map(|i| i * 17).collect();
        let bytes = write_nack(&Nack {
            track: TRACK_VIDEO,
            stream_id: 1,
            seqs,
        })
        .unwrap();
        assert_eq!(bytes.len(), NACK_HEADER_LEN + 4 * MAX_NACK_ITEMS);
        assert!(parse_nack(&bytes).is_some());
    }

    #[test]
    fn golden_keyframe_request() {
        let bytes = write_keyframe_request(TRACK_VIDEO, 0x0A0B_0C0D);
        assert_eq!(bytes, [0x23, 0x01, 0x0A, 0x0B, 0x0C, 0x0D]);
        assert_eq!(
            parse_keyframe_request(&bytes),
            Some((TRACK_VIDEO, 0x0A0B_0C0D))
        );
        assert_eq!(parse_keyframe_request(&bytes[..5]), None);
    }

    #[test]
    fn retransmit_type_parses_as_media() {
        let mut pkt = vec![0u8; MEDIA_HEADER_LEN + 2];
        write_media_header(
            &mut pkt,
            &MediaHeader {
                track: TRACK_VIDEO,
                seq: 7,
                ..Default::default()
            },
        );
        assert!(!parse_media(&pkt).unwrap().retransmit);
        pkt[0] = MEDIA_RTX_V2;
        let m = parse_media(&pkt).unwrap();
        assert!(m.retransmit);
        assert_eq!(m.header.seq, 7);
    }

    #[test]
    fn golden_transport_feedback() {
        let f = TransportFeedback {
            base_seq: 0xFFFE,
            ref_time: 0x0102_0304,
            arrivals: vec![Some(0), None, Some(0x0A0B)],
        };
        let bytes = write_transport_feedback(&f).unwrap();
        assert_eq!(
            bytes,
            [0x25, 0xFF, 0xFE, 0, 3, 1, 2, 3, 4, 0, 0, 0xFF, 0xFF, 0x0A, 0x0B]
        );
        assert_eq!(parse_transport_feedback(&bytes), Some(f));
        assert_eq!(parse_transport_feedback(&bytes[..bytes.len() - 1]), None);
        assert_eq!(
            write_transport_feedback(&TransportFeedback::default()),
            None
        );
        let mut empty = bytes[..TRANSPORT_FEEDBACK_HEADER_LEN].to_vec();
        empty[3..5].copy_from_slice(&[0, 0]);
        assert_eq!(parse_transport_feedback(&empty), None);
    }

    /// Пределы включительно: пакет из одного заголовка (пустой payload), отчёт с
    /// предельным числом блоков, обратная связь о предельном числе пакетов.
    #[test]
    fn wire_limits_are_inclusive() {
        let mut pkt = vec![0; MEDIA_HEADER_LEN];
        write_media_header(&mut pkt, &MediaHeader::default());
        assert_eq!(parse_media(&pkt).map(|m| m.payload.len()), Some(0));
        let r = Report {
            blocks: vec![ReportBlock::default(); MAX_REPORT_BLOCKS],
            ..Report::default()
        };
        assert_eq!(parse_report(&write_report(&r)), Some(r));
        let f = TransportFeedback {
            base_seq: 1,
            ref_time: 2,
            arrivals: vec![Some(3); MAX_FEEDBACK_PACKETS],
        };
        let dg = write_transport_feedback(&f).unwrap();
        assert_eq!(parse_transport_feedback(&dg), Some(f));
    }

    /// Задержка, совпавшая с меткой «не пришёл», пишется на единицу меньше — пакет
    /// остаётся пришедшим.
    #[test]
    fn huge_arrival_delay_stays_received() {
        let f = TransportFeedback {
            base_seq: 0,
            ref_time: 0,
            arrivals: vec![Some(u16::MAX), None],
        };
        let dg = write_transport_feedback(&f).unwrap();
        assert_eq!(
            parse_transport_feedback(&dg).unwrap().arrivals,
            [Some(u16::MAX - 1), None]
        );
    }
}
