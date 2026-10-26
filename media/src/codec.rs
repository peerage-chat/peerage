//! Аудио-кодек Opus. Все параметры приходят из пользовательских настроек
//! ([`AudioSettings`]) — частота, каналы, битрейт, размер кадра, режим, FEC.

use opus::{Application, Channels, Decoder, Encoder};

pub use peerage_core::settings::{AudioSettings, OpusMode};

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("opus: {0}")]
    Opus(#[from] opus::Error),
    #[error("неверная длина PCM-кадра: ожидалось {expected}, получено {got}")]
    BadFrameLen { expected: usize, got: usize },
}

fn opus_channels(channels: u16) -> Channels {
    if channels <= 1 { Channels::Mono } else { Channels::Stereo }
}
fn opus_application(mode: OpusMode) -> Application {
    match mode {
        OpusMode::Voip => Application::Voip,
        OpusMode::Audio => Application::Audio,
        OpusMode::LowDelay => Application::LowDelay,
    }
}

/// Opus-энкодер, сконфигурированный из [`AudioSettings`].
pub struct AudioEncoder {
    enc: Encoder,
    settings: AudioSettings,
}

impl AudioEncoder {
    pub fn new(settings: AudioSettings) -> Result<Self, CodecError> {
        let mut enc = Encoder::new(
            settings.sample_rate,
            opus_channels(settings.channels),
            opus_application(settings.mode),
        )?;
        enc.set_bitrate(opus::Bitrate::Bits(settings.bitrate))?;
        enc.set_inband_fec(settings.fec)?;
        if settings.fec {
            // Подсказка кодеку об ожидаемых потерях, чтобы FEC реально включился.
            enc.set_packet_loss_perc(10)?;
        }
        Ok(Self { enc, settings })
    }

    /// Закодировать один interleaved-кадр (длина = `interleaved_len`).
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, CodecError> {
        let expected = self.settings.interleaved_len();
        if pcm.len() != expected {
            return Err(CodecError::BadFrameLen { expected, got: pcm.len() });
        }
        Ok(self.enc.encode_vec(pcm, 4000)?)
    }

    /// Сменить битрейт на лету (без пересоздания энкодера) — настройки звука в звонке.
    pub fn set_bitrate(&mut self, bitrate: i32) {
        let _ = self.enc.set_bitrate(opus::Bitrate::Bits(bitrate));
        self.settings.bitrate = bitrate;
    }

    pub fn settings(&self) -> AudioSettings {
        self.settings
    }
}

/// Opus-декодер. Поддерживает PLC (восстановление потери) при `decode(None)`.
pub struct AudioDecoder {
    dec: Decoder,
    settings: AudioSettings,
}

impl AudioDecoder {
    pub fn new(settings: AudioSettings) -> Result<Self, CodecError> {
        Ok(Self {
            dec: Decoder::new(settings.sample_rate, opus_channels(settings.channels))?,
            settings,
        })
    }

    /// Декодировать пакет (`Some`) или восстановить потерю — PLC (`None`).
    pub fn decode(&mut self, packet: Option<&[u8]>) -> Result<Vec<i16>, CodecError> {
        let mut out = vec![0i16; self.settings.interleaved_len()];
        let fec = false;
        let n = self.dec.decode(packet.unwrap_or(&[]), &mut out, fec)?;
        out.truncate(n * self.settings.channels as usize);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(s: &AudioSettings, frames: usize) -> Vec<i16> {
        let total = s.interleaved_len() * frames;
        let ch = s.channels as usize;
        let mut v = Vec::with_capacity(total);
        for i in 0..(total / ch) {
            let t = i as f32 / s.sample_rate as f32;
            let val = ((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 8000.0) as i16;
            for _ in 0..ch {
                v.push(val);
            }
        }
        v
    }

    #[test]
    fn encode_decode_roundtrip() {
        let s = AudioSettings::default();
        let mut enc = AudioEncoder::new(s).unwrap();
        let mut dec = AudioDecoder::new(s).unwrap();
        let frame = sine(&s, 1);
        let pkt = enc.encode(&frame).unwrap();
        assert!(!pkt.is_empty());
        assert_eq!(dec.decode(Some(&pkt)).unwrap().len(), s.interleaved_len());
    }

    #[test]
    fn stereo_config_roundtrip() {
        let s = AudioSettings { channels: 2, ..AudioSettings::default() };
        let mut enc = AudioEncoder::new(s).unwrap();
        let mut dec = AudioDecoder::new(s).unwrap();
        let frame = sine(&s, 1);
        let pkt = enc.encode(&frame).unwrap();
        assert_eq!(dec.decode(Some(&pkt)).unwrap().len(), s.interleaved_len());
    }

    #[test]
    fn plc_returns_full_frame() {
        let s = AudioSettings::default();
        let mut dec = AudioDecoder::new(s).unwrap();
        assert_eq!(dec.decode(None).unwrap().len(), s.interleaved_len());
    }

    #[test]
    fn bad_frame_len_rejected() {
        let s = AudioSettings::default();
        let mut enc = AudioEncoder::new(s).unwrap();
        assert!(matches!(enc.encode(&[0i16; 7]), Err(CodecError::BadFrameLen { .. })));
    }
}
