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

/// Нижняя граница ожидаемых потерь: при нуле Opus не кладёт FEC вовсе, и первая же
/// потеря на чистом канале не восстанавливается.
const MIN_EXPECTED_LOSS: i32 = 5;
/// Верхняя: дальше FEC съедает битрейт основной речи, а не спасает её.
const MAX_EXPECTED_LOSS: i32 = 30;

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
        // Постоянный битрейт — постоянный размер пакета: при переменном размер
        // зашифрованного пакета повторяет звук, и по одним размерам узнаются фразы
        // (ADR-014 §8). Не настройка, а правило безопасности.
        enc.set_vbr(false)?;
        enc.set_inband_fec(settings.fec)?;
        if settings.fec {
            // Подсказка кодеку об ожидаемых потерях, чтобы FEC реально включился;
            // в звонке её уточняют отчёты собеседников ([`Self::set_expected_loss`]).
            enc.set_packet_loss_perc(MIN_EXPECTED_LOSS)?;
        }
        Ok(Self { enc, settings })
    }

    /// Ожидаемые потери в процентах — по ним Opus решает, сколько битов отдать
    /// избыточной копии прошлого кадра (FEC). Без FEC в настройках — ничего.
    pub fn set_expected_loss(&mut self, percent: i32) {
        if self.settings.fec {
            let _ = self
                .enc
                .set_packet_loss_perc(percent.clamp(MIN_EXPECTED_LOSS, MAX_EXPECTED_LOSS));
        }
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
        self.decode_inner(packet.unwrap_or(&[]), false)
    }

    /// Восстановить потерянный кадр из избыточной копии в СЛЕДУЮЩЕМ пакете (Opus
    /// in-band FEC). Нет в нём копии — Opus сам делает PLC. Сам следующий пакет
    /// потом декодируется обычным [`Self::decode`].
    pub fn decode_fec(&mut self, next: &[u8]) -> Result<Vec<i16>, CodecError> {
        self.decode_inner(next, true)
    }

    fn decode_inner(&mut self, packet: &[u8], fec: bool) -> Result<Vec<i16>, CodecError> {
        let mut out = vec![0i16; self.settings.interleaved_len()];
        let n = self.dec.decode(packet, &mut out, fec)?;
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

    /// Размер пакета не зависит от звука: тишина, тон и шум — одной длины.
    #[test]
    fn packet_size_does_not_follow_the_sound() {
        let s = AudioSettings::default();
        let mut enc = AudioEncoder::new(s).unwrap();
        let tone = sine(&s, 1);
        let silence = vec![0i16; s.interleaved_len()];
        let mut seed = 1u32;
        let noise: Vec<i16> = (0..s.interleaved_len())
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (seed >> 16) as i16
            })
            .collect();
        let mut sizes = std::collections::BTreeSet::new();
        for _ in 0..5 {
            for pcm in [&silence, &tone, &noise] {
                sizes.insert(enc.encode(pcm).unwrap().len());
            }
        }
        assert_eq!(sizes.len(), 1, "размеры пакетов: {sizes:?}");
    }

    /// Потерянный кадр восстанавливается из следующего пакета, и это ближе к тому,
    /// что дал бы сам кадр, чем маскировка потери. Сравниваем с декодом без потери,
    /// а не со входом: у Opus задержка 6,5 мс, выход сдвинут.
    #[test]
    fn fec_recovers_lost_frame() {
        let s = AudioSettings::default();
        let mut enc = AudioEncoder::new(s).unwrap();
        enc.set_expected_loss(20);
        let pcm = sine(&s, 12);
        let packets: Vec<Vec<u8>> =
            pcm.chunks(s.interleaved_len()).map(|f| enc.encode(f).unwrap()).collect();
        let decode = |lost: Option<bool>| {
            let mut dec = AudioDecoder::new(s).unwrap();
            for p in &packets[..10] {
                dec.decode(Some(p)).unwrap();
            }
            match lost {
                None => dec.decode(Some(&packets[10])).unwrap(),
                Some(true) => dec.decode_fec(&packets[11]).unwrap(),
                Some(false) => dec.decode(None).unwrap(),
            }
        };
        let reference = decode(None);
        let err = |got: Vec<i16>| {
            assert_eq!(got.len(), s.interleaved_len());
            got.iter().zip(&reference).map(|(a, b)| (*a as i64 - *b as i64).pow(2)).sum::<i64>()
        };
        let (with_fec, with_plc) = (err(decode(Some(true))), err(decode(Some(false))));
        assert!(with_fec < with_plc / 4, "FEC {with_fec} не лучше PLC {with_plc}");
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
