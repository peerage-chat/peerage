//! Проба захвата экрана: берёт один кадр и печатает размер.
//! Запуск (WSL): cargo run -p p2p-call --example screen_probe

use p2p_call::capture::ScreenSource;
use p2p_media::VideoSource;

fn main() {
    let mut src = match ScreenSource::primary(640, 360) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("✗ не удалось открыть экран: {e}");
            std::process::exit(1);
        }
    };
    let (w, h) = src.dimensions();
    match src.next_frame() {
        Some(rgb) if rgb.len() == (w * h * 3) as usize => {
            println!("✓ кадр экрана захвачен: {w}x{h}, {} байт RGB", rgb.len());
        }
        Some(rgb) => {
            eprintln!("✗ неожиданный размер кадра: {} (ждали {})", rgb.len(), w * h * 3);
            std::process::exit(1);
        }
        None => {
            eprintln!("✗ кадр не получен");
            std::process::exit(1);
        }
    }
}
