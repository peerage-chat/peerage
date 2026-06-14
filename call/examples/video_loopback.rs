//! Headless-проверка видео-конвейера (без камеры/экрана): синтетический источник
//! → H.264 энкод → фрагментация → реальный Yggdrasil → реассемблер → H.264 декод
//! → кадры RGB. Аудио выключено (нет устройств в WSL).
//!
//! Запуск (WSL): cargo run -p p2p-call --example video_loopback

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use p2p_call::Call;
use p2p_core::settings::VideoSettings;
use p2p_core::Identity;
use p2p_media::VideoSource;
use p2p_messenger::Messenger;
use p2p_ygg::Node;

/// Синтетический источник: анимированный градиент заданного размера.
struct TestSource {
    w: u32,
    h: u32,
    t: u32,
}
impl VideoSource for TestSource {
    fn dimensions(&self) -> (u32, u32) {
        (self.w, self.h)
    }
    fn next_frame(&mut self) -> Option<Vec<u8>> {
        self.t = self.t.wrapping_add(3);
        let mut v = vec![0u8; (self.w * self.h * 3) as usize];
        for y in 0..self.h {
            for x in 0..self.w {
                let i = ((y * self.w + x) * 3) as usize;
                v[i] = (x.wrapping_add(self.t) % 256) as u8;
                v[i + 1] = (y % 256) as u8;
                v[i + 2] = (self.t % 256) as u8;
            }
        }
        Some(v)
    }
}

fn main() {
    let id_a = Identity::from_seed(&[1u8; 32]).unwrap();
    let id_b = Identity::from_seed(&[2u8; 32]).unwrap();
    let pk_a = id_a.public_key_bytes();
    let pk_b = id_b.public_key_bytes();

    let node_a = Node::start(&id_a.seed()).unwrap();
    let node_b = Node::start(&id_b.seed()).unwrap();
    let uri = node_a.listen("tls://127.0.0.1:0").unwrap();
    node_b.add_peer(&uri).unwrap();
    let t = Instant::now();
    while !(node_a.tree_count() > 1 && node_b.tree_count() > 1) {
        if t.elapsed() > Duration::from_secs(10) {
            eprintln!("✗ нет дерева");
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(3));

    let (ma, _ra) = Messenger::start(id_a, node_a);
    let (mb, _rb) = Messenger::start(id_b, node_b);
    let ma = Arc::new(ma);
    let mb = Arc::new(mb);

    // Поднимаем шифр-сессию (двунаправленный Hello).
    thread::scope(|s| {
        let h = s.spawn(|| ma.connect(&pk_b, Duration::from_secs(20)));
        mb.connect(&pk_a, Duration::from_secs(20)).unwrap();
        h.join().unwrap().unwrap();
    });
    println!("✓ сессия установлена");

    let vcfg = VideoSettings { width: 320, height: 240, fps: 15, bitrate: 1_000_000 };

    // A шлёт видео; B принимает.
    let _call_a = Call::start(ma.clone(), pk_b, None, Some(vcfg), Some(Box::new(TestSource { w: 320, h: 240, t: 0 })))
        .expect("call A");
    let mut call_b = Call::start(mb.clone(), pk_a, None, None, None).expect("call B");
    let frames = call_b.take_frames().expect("канал кадров");

    let mut got = 0;
    let mut dims = (0u32, 0u32);
    let deadline = Instant::now() + Duration::from_secs(15);
    while got < 10 && Instant::now() < deadline {
        if let Ok(f) = frames.recv_timeout(Duration::from_millis(500)) {
            got += 1;
            dims = (f.width, f.height);
        }
    }

    println!("принято видео-кадров: {got}, размер {}x{}", dims.0, dims.1);
    if got >= 3 && dims == (320, 240) {
        println!("\n✓✓ Видео-конвейер работает сквозь Yggdrasil (H.264 + фрагментация)");
    } else {
        eprintln!("\n✗ проверка не пройдена");
        std::process::exit(1);
    }
}
