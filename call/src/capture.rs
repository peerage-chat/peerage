//! Источники видео для звонка: экран и камера. Оба реализуют [`VideoSource`]
//! (RGB8-кадры заданного размера), поэтому проходят один конвейер энкодинга —
//! шаринг экрана это просто «источник = экран».

use image::imageops::FilterType;
use p2p_media::VideoSource;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("захват экрана: {0}")]
    Screen(#[from] xcap::XCapError),
    #[error("монитор не найден")]
    NoMonitor,
}

/// Захват экрана (первый монитор), масштабирование под заданный размер.
pub struct ScreenSource {
    monitor: xcap::Monitor,
    w: u32,
    h: u32,
}

impl ScreenSource {
    /// Источник по первому монитору. Размер приводится к чётному (требование YUV420).
    pub fn primary(width: u32, height: u32) -> Result<Self, CaptureError> {
        let monitor = xcap::Monitor::all()?
            .into_iter()
            .next()
            .ok_or(CaptureError::NoMonitor)?;
        Ok(Self { monitor, w: width & !1, h: height & !1 })
    }
}

impl VideoSource for ScreenSource {
    fn dimensions(&self) -> (u32, u32) {
        (self.w, self.h)
    }

    fn next_frame(&mut self) -> Option<Vec<u8>> {
        let img = self.monitor.capture_image().ok()?; // RgbaImage
        let resized = image::imageops::resize(&img, self.w, self.h, FilterType::Triangle);
        let mut rgb = Vec::with_capacity((self.w * self.h * 3) as usize);
        for px in resized.pixels() {
            rgb.push(px[0]);
            rgb.push(px[1]);
            rgb.push(px[2]);
        }
        Some(rgb)
    }
}
