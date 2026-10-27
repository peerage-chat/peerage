//! Сквозная проверка аудио-пайплайна на синтетике (без физических устройств):
//! синусоида → Opus энкод → лёгкий медиа-путь Yggdrasil → джиттер-буфер →
//! Opus декод → сверка длины и энергии (RMS). Полное качество (128 кбит/с).
//!
//! Запуск (WSL): cargo run -p peerage-media --example audio_loopback

use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use litertc::{JitterBuffer, JitterOut, TRACK_AUDIO};
use peerage_core::Identity;
use peerage_media::{AudioDecoder, AudioEncoder, AudioSettings};
use peerage_messenger::{MediaPacket, Messenger};
use peerage_ygg::Node;

const FRAMES: u16 = 150; // 150 × 20мс = 3с

fn rms(s: &[i16]) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let sum: f64 = s.iter().map(|&x| (x as f64).powi(2)).sum();
    (sum / s.len() as f64).sqrt()
}

fn main() {
    let cfg = AudioSettings::default();
    println!(
        "конфиг: {} Гц, {} кан., {} кбит/с, кадр {}мс ({} сэмплов/кан.)",
        cfg.sample_rate, cfg.channels, cfg.bitrate / 1000, cfg.frame_ms, cfg.frame_samples()
    );

    // Исходный сигнал: синусоида 440 Гц, кадрами.
    let il = cfg.interleaved_len();
    let mut source: Vec<Vec<i16>> = Vec::with_capacity(FRAMES as usize);
    let mut phase = 0u64;
    for _ in 0..FRAMES {
        let mut frame = Vec::with_capacity(il);
        for _ in 0..cfg.frame_samples() {
            let t = phase as f32 / cfg.sample_rate as f32;
            let v = ((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 9000.0) as i16;
            for _ in 0..cfg.channels {
                frame.push(v);
            }
            phase += 1;
        }
        source.push(frame);
    }
    let src_rms = rms(&source.concat());

    // Два узла по localhost.
    let id_a = Identity::from_seed(&[1u8; 32]).unwrap();
    let id_b = Identity::from_seed(&[2u8; 32]).unwrap();
    let pk_a = id_a.public_key_bytes();
    let pk_b = id_b.public_key_bytes();
    let node_a = Node::start(&id_a.seed()).unwrap();
    let node_b = Node::start(&id_b.seed()).unwrap();
    let uri = node_a.listen("tls://127.0.0.1:0").unwrap();
    node_b.add_peer(&uri).unwrap();
    let t0 = Instant::now();
    while !(node_a.tree_count() > 1 && node_b.tree_count() > 1) {
        if t0.elapsed() > Duration::from_secs(10) {
            eprintln!("✗ нет дерева");
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(3));

    let (a, _rxa) = Messenger::start(id_a, node_a);
    let (b, _rxb) = Messenger::start(id_b, node_b);
    let a = Arc::new(a);
    thread::scope(|s| {
        let ha = s.spawn(|| a.connect(&pk_b, Duration::from_secs(20)));
        b.connect(&pk_a, Duration::from_secs(20)).unwrap();
        ha.join().unwrap().unwrap();
    });

    // Приёмник: jitter + decode в отдельном потоке (декод вне приёмного цикла).
    let media_rx: Receiver<MediaPacket> = b.media_channel(512);
    let reader = thread::spawn(move || {
        let mut dec = AudioDecoder::new(cfg).unwrap();
        let mut jit = JitterBuffer::new(3);
        let mut out: Vec<i16> = Vec::new();
        let mut got = 0usize;
        let deadline = Instant::now() + Duration::from_secs(15);
        while got < FRAMES as usize && Instant::now() < deadline {
            if let Ok(p) = media_rx.recv_timeout(Duration::from_millis(300)) {
                if p.track == TRACK_AUDIO {
                    jit.push(p.seq, p.payload);
                    got += 1;
                }
            }
            while let Some(o) = jit.pop() {
                match o {
                    JitterOut::Frame(pkt) => out.extend(dec.decode(Some(&pkt)).unwrap()),
                    JitterOut::Lost => out.extend(dec.decode(None).unwrap()),
                }
            }
        }
        // дренаж остатка
        while let Some(o) = jit.pop() {
            match o {
                JitterOut::Frame(pkt) => out.extend(dec.decode(Some(&pkt)).unwrap()),
                JitterOut::Lost => out.extend(dec.decode(None).unwrap()),
            }
        }
        (got, out)
    });

    // Энкод + отправка с реалистичным ритмом (20мс).
    let mut enc = AudioEncoder::new(cfg).unwrap();
    for (seq, frame) in source.iter().enumerate() {
        let pkt = enc.encode(frame).unwrap();
        a.send_media(&pk_b, TRACK_AUDIO, seq as u16, seq as u32 * cfg.frame_samples() as u32, &pkt)
            .unwrap();
        thread::sleep(Duration::from_millis(20));
    }

    let (received, decoded) = reader.join().unwrap();
    let dec_rms = rms(&decoded);
    let ratio = if src_rms > 0.0 { dec_rms / src_rms } else { 0.0 };
    println!("получено кадров: {received}/{FRAMES}");
    println!("декодировано сэмплов: {} (исходно ~{})", decoded.len(), il * FRAMES as usize);
    println!("RMS источник={src_rms:.0} декод={dec_rms:.0} (отношение {ratio:.2})");

    let frames_ok = received >= (FRAMES as usize * 95 / 100);
    let energy_ok = ratio > 0.6 && ratio < 1.4;
    if frames_ok && energy_ok {
        println!("\n✓✓ Аудио-пайплайн работает сквозь Yggdrasil (Opus {}к + джиттер)", cfg.bitrate / 1000);
    } else {
        eprintln!("\n✗ проверка не пройдена (frames_ok={frames_ok}, energy_ok={energy_ok})");
        std::process::exit(1);
    }
}
