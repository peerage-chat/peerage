//! Видео-кодек H.264 (openh264) + абстракция источника кадров.
//!
//! Кодек работает с RGB8-кадрами (по 3 байта на пиксель). Источник видео
//! ([`VideoSource`]) абстрагирован: камера и захват экрана дают одинаковые
//! RGB8-кадры и проходят один и тот же конвейер энкодинга.

use openh264::decoder::Decoder;
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::{RgbSliceU8, YUVBuffer, YUVSource};
use openh264::OpenH264API;

pub use p2p_core::settings::VideoSettings;

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("openh264: {0}")]
    Codec(#[from] openh264::Error),
    #[error("неверный размер RGB-кадра: ожидалось {expected}, получено {got}")]
    BadFrameSize { expected: usize, got: usize },
}

/// Источник видео-кадров (камера, экран, …). Кадр — RGB8, ширина×высота×3 байт.
pub trait VideoSource {
    fn dimensions(&self) -> (u32, u32);
    /// Следующий кадр (RGB8) или None, если нового кадра пока нет / поток закончился.
    fn next_frame(&mut self) -> Option<Vec<u8>>;
}

use std::sync::{Arc, Mutex};

/// «Почтовый ящик» последнего кадра — связывает push-сторону (внешний захват)
/// с pull-стороной ([`VideoSource`]). Один слот: новый кадр вытесняет старый
/// (реалтайм — держим только свежий).
type Slot = Arc<Mutex<Option<Vec<u8>>>>;

/// Куда внешний захват кладёт кадры. На Android сюда из Kotlin через JNI
/// поступают кадры MediaProjection/Camera2; на десктопе — любой источник.
#[derive(Clone)]
pub struct FrameFeeder {
    slot: Slot,
    w: u32,
    h: u32,
}

impl FrameFeeder {
    /// Положить свежий RGB8-кадр (вытесняет непрочитанный предыдущий).
    pub fn push(&self, rgb: Vec<u8>) {
        *self.slot.lock().unwrap() = Some(rgb);
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.w, self.h)
    }
}

/// Pull-источник, питаемый извне через [`FrameFeeder`]. Позволяет гонять
/// push-захват (Android MediaProjection и т.п.) через тот же видео-конвейер.
pub struct PushSource {
    slot: Slot,
    w: u32,
    h: u32,
}

impl VideoSource for PushSource {
    fn dimensions(&self) -> (u32, u32) {
        (self.w, self.h)
    }
    fn next_frame(&mut self) -> Option<Vec<u8>> {
        self.slot.lock().unwrap().take()
    }
}

/// Создать пару «кормилец + источник» для push-захвата заданного размера.
pub fn push_source(width: u32, height: u32) -> (FrameFeeder, PushSource) {
    let slot: Slot = Arc::new(Mutex::new(None));
    (
        FrameFeeder { slot: slot.clone(), w: width, h: height },
        PushSource { slot, w: width, h: height },
    )
}

/// Энкодер H.264. Ключевые кадры форсируются периодически (интервал из fps).
pub struct VideoEncoder {
    enc: Encoder,
    width: u32,
    height: u32,
    frame: u64,
    keyframe_interval: u64,
}

impl VideoEncoder {
    pub fn new(settings: VideoSettings) -> Result<Self, VideoError> {
        let config = EncoderConfig::new()
            .set_bitrate_bps(settings.bitrate.max(0) as u32)
            .max_frame_rate(settings.fps as f32);
        let enc = Encoder::with_api_config(OpenH264API::from_source(), config)?;
        Ok(Self {
            enc,
            width: settings.width,
            height: settings.height,
            frame: 0,
            keyframe_interval: (settings.fps as u64).max(1) * 2, // ключевой кадр ~раз в 2с
        })
    }

    /// Закодировать RGB8-кадр. Возвращает (H.264-битстрим, признак ключевого кадра).
    pub fn encode_rgb(&mut self, rgb: &[u8]) -> Result<(Vec<u8>, bool), VideoError> {
        let expected = (self.width * self.height * 3) as usize;
        if rgb.len() != expected {
            return Err(VideoError::BadFrameSize { expected, got: rgb.len() });
        }
        let keyframe = self.frame % self.keyframe_interval == 0;
        if keyframe {
            self.enc.force_intra_frame();
        }
        let yuv = YUVBuffer::from_rgb8_source(RgbSliceU8::new(
            rgb,
            (self.width as usize, self.height as usize),
        ));
        let bitstream = self.enc.encode(&yuv)?;
        self.frame += 1;
        Ok((bitstream.to_vec(), keyframe))
    }
}

/// Декодированный кадр в RGB8.
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Декодер H.264 → RGB8.
pub struct VideoDecoder {
    dec: Decoder,
}

impl VideoDecoder {
    pub fn new() -> Result<Self, VideoError> {
        Ok(Self { dec: Decoder::new()? })
    }

    /// Декодировать H.264-пакет. Может вернуть None, если кадр ещё не готов.
    pub fn decode(&mut self, packet: &[u8]) -> Result<Option<DecodedFrame>, VideoError> {
        match self.dec.decode(packet)? {
            Some(yuv) => {
                let (w, h) = yuv.dimensions();
                let mut rgb = vec![0u8; w * h * 3];
                yuv.write_rgb8(&mut rgb);
                Ok(Some(DecodedFrame { width: w as u32, height: h as u32, rgb }))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                v.push((x % 256) as u8);
                v.push((y % 256) as u8);
                v.push(128);
            }
        }
        v
    }

    #[test]
    fn encode_decode_roundtrip_dimensions() {
        let s = VideoSettings { width: 320, height: 240, fps: 30, bitrate: 1_000_000 };
        let mut enc = VideoEncoder::new(s).unwrap();
        let mut dec = VideoDecoder::new().unwrap();

        let frame = gradient(s.width, s.height);
        let (bitstream, keyframe) = enc.encode_rgb(&frame).unwrap();
        assert!(keyframe, "первый кадр должен быть ключевым");
        assert!(!bitstream.is_empty(), "битстрим не пустой");

        let decoded = dec.decode(&bitstream).unwrap().expect("кадр декодирован");
        assert_eq!((decoded.width, decoded.height), (320, 240));
        assert_eq!(decoded.rgb.len(), 320 * 240 * 3);
    }

    #[test]
    fn push_source_delivers_latest_frame() {
        let (feeder, mut src) = push_source(4, 4);
        assert_eq!(src.dimensions(), (4, 4));
        assert!(src.next_frame().is_none()); // пока пусто
        feeder.push(vec![1u8; 48]);
        feeder.push(vec![2u8; 48]); // вытесняет первый — держим свежий
        assert_eq!(src.next_frame().unwrap()[0], 2);
        assert!(src.next_frame().is_none()); // забрали — снова пусто
    }

    #[test]
    fn wrong_frame_size_rejected() {
        let s = VideoSettings { width: 64, height: 64, fps: 30, bitrate: 500_000 };
        let mut enc = VideoEncoder::new(s).unwrap();
        assert!(matches!(enc.encode_rgb(&[0u8; 10]), Err(VideoError::BadFrameSize { .. })));
    }
}
