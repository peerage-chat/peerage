//! Пользовательские настройки медиа (аудио/видео). Всё настраивается явно и
//! сохраняется на диск — это осознанный приоритет: юзер сам выбирает качество,
//! битрейт, кодек и т.д. Значения по умолчанию — высокое качество, без экономии.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::store::StoreError;

/// Режим Opus (применяется при создании энкодера).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpusMode {
    /// Оптимизировано под речь.
    Voip,
    /// Универсальное (речь+музыка), высшее качество.
    Audio,
    /// Минимальная задержка.
    LowDelay,
}

/// Настройки аудио. PCM — interleaved i16.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSettings {
    /// Частота дискретизации (Гц): 8000/12000/16000/24000/48000.
    pub sample_rate: u32,
    /// Каналы: 1 (моно) или 2 (стерео).
    pub channels: u16,
    /// Целевой битрейт Opus (бит/с).
    pub bitrate: i32,
    /// Длительность кадра (мс): 10/20/40/60.
    pub frame_ms: u32,
    /// Режим Opus.
    pub mode: OpusMode,
    /// Inband FEC (помехоустойчивость к потерям ценой битрейта).
    pub fec: bool,
    /// Глубина джиттер-буфера в кадрах (задержка/устойчивость).
    pub jitter_depth: usize,
}

impl Default for AudioSettings {
    fn default() -> Self {
        // Voip-режим: оптимизирован под речь, DTX (тишина не шлётся), FEC.
        // 64кбит/с достаточно для чёткой речи; экономим канал для видео.
        AudioSettings {
            sample_rate: 48_000,
            channels: 1,
            bitrate: 64_000,
            frame_ms: 20,
            mode: OpusMode::Voip,
            fec: true,
            jitter_depth: 4,
        }
    }
}

impl AudioSettings {
    /// Сэмплов на канал в одном кадре.
    pub fn frame_samples(&self) -> usize {
        (self.sample_rate as usize / 1000) * self.frame_ms as usize
    }
    /// Длина interleaved-кадра (все каналы).
    pub fn interleaved_len(&self) -> usize {
        self.frame_samples() * self.channels as usize
    }
}

/// Настройки видео (используются в Фазе B; задаются пользователем явно — ТЗ).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: i32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        // Цель проекта — 1080p60, высокий битрейт.
        VideoSettings { width: 1920, height: 1080, fps: 60, bitrate: 8_000_000 }
    }
}

/// Все пользовательские настройки (сохраняются в settings.json).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub video: VideoSettings,
}

impl Settings {
    /// Загрузить из файла, либо вернуть значения по умолчанию (и создать файл).
    pub fn load_or_create(path: &Path) -> Result<Settings, StoreError> {
        if path.exists() {
            let data = std::fs::read(path)?;
            Ok(serde_json::from_slice(&data)?)
        } else {
            let s = Settings::default();
            s.save(path)?;
            Ok(s)
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        crate::fsutil::write_atomic(path, &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp() -> std::path::PathBuf {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("p2p-settings-{n}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_are_high_quality() {
        let s = AudioSettings::default();
        assert_eq!(s.sample_rate, 48_000);
        assert_eq!(s.frame_samples(), 960);
        assert_eq!(VideoSettings::default().height, 1080);
    }

    #[test]
    fn load_or_create_then_persist_changes() {
        let dir = tmp();
        let path = dir.join("settings.json");
        let mut s = Settings::load_or_create(&path).unwrap();
        assert_eq!(s.audio.bitrate, 64_000);

        s.audio.bitrate = 160_000;
        s.video.fps = 30;
        s.save(&path).unwrap();

        let reloaded = Settings::load_or_create(&path).unwrap();
        assert_eq!(reloaded.audio.bitrate, 160_000);
        assert_eq!(reloaded.video.fps, 30);
        std::fs::remove_dir_all(&dir).ok();
    }
}
