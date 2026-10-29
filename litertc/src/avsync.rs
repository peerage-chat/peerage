//! Синхронизация звука и видео участника (ADR-014, шаг 8b).
//!
//! Метки звука (48 кГц) и видео (90 кГц) отправитель считает от одного начала
//! отсчёта по моменту захвата, поэтому обе означают время на одних его часах. Для
//! каждого трека меряем отставание `момент, когда услышали/увидели − метка`: разница
//! часов в нём одна и та же и при сравнении сокращается. Разница отставаний — на
//! сколько губы расходятся со звуком.
//!
//! Подтягиваем того, кто спешит: добавляем задержку звуку, если видео отстаёт, и
//! видео — если отстаёт звук. Раз в секунду не больше чем на [`MAX_STEP_MS`] (рывок
//! звука — один кадр маскировки, видео — короткая пауза) и только если расхождение
//! заметно ([`DEADBAND_MS`]). Добавленная задержка сначала снимается с другого
//! трека: оба сразу ждать незачем.

use std::time::{Duration, Instant};

use crate::wire::VIDEO_CLOCK_HZ;

/// Расхождение меньше этого не заметно (порог восприятия — 45 мс звука вперёд,
/// 125 мс назад) — не трогаем.
const DEADBAND_MS: f64 = 30.0;
/// Шаг поправки за раз.
const MAX_STEP_MS: f64 = 40.0;
/// Как часто поправлять.
const UPDATE_EVERY: Duration = Duration::from_secs(1);
/// Больше этого ради губ звук не задерживаем: в разговоре задержка речи хуже.
const MAX_DELAY_MS: f64 = 500.0;
/// Трек считается идущим, если его последний замер не дальше этого от текущего.
/// Замеры — моменты, когда кадр будет слышен или виден, то есть в будущем, и у
/// треков на разное расхождение: сравниваем по модулю, с запасом на него.
const FRESH: Duration = Duration::from_secs(2);
/// Сглаживание отставания (доля нового замера): звук — 50 замеров в секунду,
/// видео — до 30, так за секунду замер почти полностью входит в среднее.
const SMOOTHING: f64 = 0.1;
/// Период оборота метки видео, мс (u32 при 90 кГц — 13 часов): разница
/// отставаний приводится к ближайшему, если трек начали принимать после оборота.
const VIDEO_WRAP_MS: f64 = (1u64 << 32) as f64 * 1000.0 / VIDEO_CLOCK_HZ as f64;

/// Отставание одного трека.
#[derive(Default)]
struct Lag {
    /// Метка последнего замера и её значение без оборота u32.
    last_ts: Option<(u32, i64)>,
    /// Сглаженное отставание, мс, и когда мерили.
    value: Option<(f64, Instant)>,
}

impl Lag {
    fn observe(&mut self, epoch: Instant, at: Instant, raw: u32, clock_hz: u32) {
        let ts = match self.last_ts {
            Some((last, base)) => base + raw.wrapping_sub(last) as i32 as i64,
            None => raw as i64,
        };
        self.last_ts = Some((raw, ts));
        let lag = ms_since(epoch, at) - ts as f64 * 1000.0 / clock_hz as f64;
        let v = match self.value {
            Some((v, _)) => v + (lag - v) * SMOOTHING,
            None => lag,
        };
        self.value = Some((v, at));
    }

    fn fresh(&self, now: Instant) -> Option<f64> {
        self.value
            .filter(|&(_, at)| now.max(at) - now.min(at) < FRESH)
            .map(|(v, _)| v)
    }
}

/// Мс от `epoch` до `t`, со знаком.
fn ms_since(epoch: Instant, t: Instant) -> f64 {
    match t.checked_duration_since(epoch) {
        Some(d) => d.as_secs_f64() * 1000.0,
        None => -(epoch - t).as_secs_f64() * 1000.0,
    }
}

/// Синхронизация одного участника: сколько добавить задержки звуку и видео.
#[derive(Default)]
pub(crate) struct AvSync {
    epoch: Option<Instant>,
    audio: Lag,
    video: Lag,
    audio_delay_ms: f64,
    video_delay_ms: f64,
    next_update: Option<Instant>,
    /// Расхождение при последней поправке, мс; `None` — одного трека нет.
    error_ms: Option<f64>,
}

impl AvSync {
    /// Кадр звука с меткой `ts` будет слышен в `at`.
    pub fn observe_audio(&mut self, at: Instant, ts: u32, clock_hz: u32) {
        let epoch = *self.epoch.get_or_insert(at);
        self.audio.observe(epoch, at, ts, clock_hz);
        self.update(at);
    }

    /// Видео-кадр с меткой `ts` будет на экране в `at`.
    pub fn observe_video(&mut self, at: Instant, ts: u32) {
        let epoch = *self.epoch.get_or_insert(at);
        self.video.observe(epoch, at, ts, VIDEO_CLOCK_HZ);
        self.update(at);
    }

    /// Сколько задержать звук ради губ.
    pub fn audio_delay(&self) -> Duration {
        Duration::from_secs_f64(self.audio_delay_ms / 1000.0)
    }

    /// На сколько видео позже своего звука, мс (< 0 — раньше), по последней
    /// поправке; `None` — одного из треков нет.
    pub fn error_ms(&self) -> Option<f64> {
        self.error_ms
    }

    /// Сколько задержать видео ради губ.
    pub fn video_delay(&self) -> Duration {
        Duration::from_secs_f64(self.video_delay_ms / 1000.0)
    }

    fn update(&mut self, now: Instant) {
        if self.next_update.is_some_and(|t| now < t) {
            return;
        }
        self.next_update = Some(now + UPDATE_EVERY);
        self.error_ms = None;
        let (Some(a), Some(v)) = (self.audio.fresh(now), self.video.fresh(now)) else {
            // Одного трека нет (камера выключена, тайл не на экране) — ждать
            // незачем: добавленное снимаем.
            self.audio_delay_ms = (self.audio_delay_ms - MAX_STEP_MS).max(0.0);
            self.video_delay_ms = (self.video_delay_ms - MAX_STEP_MS).max(0.0);
            return;
        };
        // > 0 — видео показывают позже, чем слышен его звук.
        let mut d = v - a;
        d -= VIDEO_WRAP_MS * (d / VIDEO_WRAP_MS).round();
        self.error_ms = Some(d);
        if d.abs() <= DEADBAND_MS {
            return;
        }
        let step = d.abs().min(MAX_STEP_MS);
        let (slow, fast) = if d > 0.0 {
            (&mut self.video_delay_ms, &mut self.audio_delay_ms)
        } else {
            (&mut self.audio_delay_ms, &mut self.video_delay_ms)
        };
        if *slow > 0.0 {
            *slow = (*slow - step).max(0.0);
        } else {
            *fast = (*fast + step).min(MAX_DELAY_MS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUDIO_HZ: u32 = 48_000;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Звонок, где видео идёт с отставанием `video_ms`, звук — `audio_ms` (без
    /// поправки); поправку применяем как настоящие треки. Возвращает итоговую
    /// разницу показа видео и звука, мс.
    fn run(sync: &mut AvSync, t0: Instant, secs: u64, audio_ms: u64, video_ms: u64) -> f64 {
        let mut gap = 0.0;
        for i in 0..secs * 50 {
            let cap = i * 20; // мс захвата
            let audio_at = t0 + ms(cap + audio_ms) + sync.audio_delay();
            sync.observe_audio(audio_at, (cap * 48) as u32, AUDIO_HZ);
            if i % 2 == 0 {
                let video_at = t0 + ms(cap + video_ms) + sync.video_delay();
                sync.observe_video(video_at, (cap * 90) as u32);
                gap =
                    (video_at - t0).as_secs_f64() * 1000.0 - (audio_at - t0).as_secs_f64() * 1000.0;
            }
        }
        gap
    }

    /// Видео на 200 мс позже звука — звук задерживается, пока губы не сойдутся.
    #[test]
    fn audio_waits_for_late_video() {
        let mut s = AvSync::default();
        let gap = run(&mut s, Instant::now(), 15, 100, 300);
        assert!(gap.abs() <= DEADBAND_MS, "расхождение {gap} мс");
        assert!(
            s.audio_delay() >= ms(170),
            "звук ждёт {:?}",
            s.audio_delay()
        );
        assert_eq!(s.video_delay(), Duration::ZERO);
    }

    /// Звук позже видео — ждёт видео.
    #[test]
    fn video_waits_for_late_audio() {
        let mut s = AvSync::default();
        let gap = run(&mut s, Instant::now(), 10, 250, 120);
        assert!(gap.abs() <= DEADBAND_MS, "расхождение {gap} мс");
        assert_eq!(s.audio_delay(), Duration::ZERO);
    }

    /// Видео ускорилось — добавленная звуку задержка снимается, а не уходит в
    /// задержку видео.
    #[test]
    fn delay_is_taken_back_before_the_other_track_waits() {
        let t0 = Instant::now();
        let mut s = AvSync::default();
        run(&mut s, t0, 15, 100, 300);
        let gap = run(&mut s, t0 + ms(15_000), 15, 100, 120);
        assert!(gap.abs() <= DEADBAND_MS, "расхождение {gap} мс");
        assert!(s.audio_delay() <= ms(40), "звук ждёт {:?}", s.audio_delay());
        assert_eq!(s.video_delay(), Duration::ZERO);
    }

    /// Ради губ звук не задерживается больше полусекунды.
    #[test]
    fn audio_delay_is_capped() {
        let mut s = AvSync::default();
        run(&mut s, Instant::now(), 40, 50, 1500);
        assert_eq!(s.audio_delay(), ms(500));
    }

    /// Видео пропало — задержка звука уходит.
    #[test]
    fn delay_goes_away_without_video() {
        let t0 = Instant::now();
        let mut s = AvSync::default();
        run(&mut s, t0, 15, 100, 300);
        for i in 0..20 * 50u64 {
            let at = t0 + ms(15_000 + i * 20);
            s.observe_audio(at, ((15_000 + i * 20) * 48) as u32, AUDIO_HZ);
        }
        assert_eq!(s.audio_delay(), Duration::ZERO);
    }
}
