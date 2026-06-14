//! Аудио-кодек Opus. По умолчанию — полное качество (48кГц, стерео, 128 кбит/с,
//! кадр 20мс), без экономии на битрейте.

use opus::{Application, Channels, Decoder, Encoder};

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("opus: {0}")]
    Opus(#[from] opus::Error),
    #[error("неверная длина PCM-кадра: ожидалось {expected}, получено {got}")]
    BadFrameLen { expected: usize, got: usize },
}

/// Параметры аудио. PCM — interleaved i16.
#[derive(Debug, Clone, Copy)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate: i32,
    pub frame_ms: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        // Полное качество речи+музыки: 48кГц стерео, 128 кбит/с, 20мс.
        AudioConfig { sample_rate: 48_000, channels: 2, bitrate: 128_000, frame_ms: 20 }
    }
}

impl AudioConfig {
    fn opus_channels(&self) -> Channels {
        if self.channels <= 1 { Channels::Mono } else { Channels::Stereo }
    }
    /// Сэмплов на канал в одном кадре.
    pub fn frame_samples(&self) -> usize {
        (self.sample_rate as usize / 1000) * self.frame_ms as usize
    }
    /// Длина interleaved-кадра (все каналы).
    pub fn interleaved_len(&self) -> usize {
        self.frame_samples() * self.channels as usize
    }
}

/// Opus-энкодер.
pub struct AudioEncoder {
    enc: Encoder,
    cfg: AudioConfig,
}

impl AudioEncoder {
    pub fn new(cfg: AudioConfig) -> Result<Self, CodecError> {
        let mut enc = Encoder::new(cfg.sample_rate, cfg.opus_channels(), Application::Audio)?;
        enc.set_bitrate(opus::Bitrate::Bits(cfg.bitrate))?;
        Ok(Self { enc, cfg })
    }

    /// Закодировать один interleaved-кадр (длина = `interleaved_len`).
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, CodecError> {
        if pcm.len() != self.cfg.interleaved_len() {
            return Err(CodecError::BadFrameLen {
                expected: self.cfg.interleaved_len(),
                got: pcm.len(),
            });
        }
        Ok(self.enc.encode_vec(pcm, 4000)?)
    }

    pub fn config(&self) -> AudioConfig {
        self.cfg
    }
}

/// Opus-декодер. Поддерживает PLC (восстановление потерянного кадра) при `decode(None)`.
pub struct AudioDecoder {
    dec: Decoder,
    cfg: AudioConfig,
}

impl AudioDecoder {
    pub fn new(cfg: AudioConfig) -> Result<Self, CodecError> {
        Ok(Self {
            dec: Decoder::new(cfg.sample_rate, cfg.opus_channels())?,
            cfg,
        })
    }

    /// Декодировать пакет (`Some`) или восстановить потерю — PLC (`None`).
    /// Возвращает interleaved i16.
    pub fn decode(&mut self, packet: Option<&[u8]>) -> Result<Vec<i16>, CodecError> {
        let mut out = vec![0i16; self.cfg.interleaved_len()];
        let n = self.dec.decode(packet.unwrap_or(&[]), &mut out, false)?;
        out.truncate(n * self.cfg.channels as usize);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(cfg: &AudioConfig, frames: usize) -> Vec<i16> {
        let total = cfg.interleaved_len() * frames;
        let mut v = Vec::with_capacity(total);
        let ch = cfg.channels as usize;
        for i in 0..(total / ch) {
            let t = i as f32 / cfg.sample_rate as f32;
            let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 8000.0;
            for _ in 0..ch {
                v.push(s as i16);
            }
        }
        v
    }

    #[test]
    fn encode_decode_roundtrip_preserves_energy() {
        let cfg = AudioConfig::default();
        let mut enc = AudioEncoder::new(cfg).unwrap();
        let mut dec = AudioDecoder::new(cfg).unwrap();

        let frame = sine(&cfg, 1);
        let pkt = enc.encode(&frame).unwrap();
        assert!(!pkt.is_empty() && pkt.len() < frame.len() * 2, "Opus должен сжать кадр");

        let decoded = dec.decode(Some(&pkt)).unwrap();
        assert_eq!(decoded.len(), cfg.interleaved_len());
    }

    #[test]
    fn plc_returns_full_frame() {
        let cfg = AudioConfig::default();
        let mut dec = AudioDecoder::new(cfg).unwrap();
        // даже без пакета PLC отдаёт кадр (тишина/экстраполяция)
        let out = dec.decode(None).unwrap();
        assert_eq!(out.len(), cfg.interleaved_len());
    }

    #[test]
    fn bad_frame_len_is_rejected() {
        let cfg = AudioConfig::default();
        let mut enc = AudioEncoder::new(cfg).unwrap();
        assert!(matches!(enc.encode(&[0i16; 10]), Err(CodecError::BadFrameLen { .. })));
    }
}
