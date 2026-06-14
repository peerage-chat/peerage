//! Headless-проверка группового звонка (full mesh, видео): B и C шлют видео узлу
//! A; A принимает кадры ПО ИСТОЧНИКАМ (per-source декод). Аудио выключено (нет
//! устройств в WSL).
//!
//! Запуск (WSL): cargo run -p p2p-call --example group_call_loopback

use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use p2p_call::GroupCall;
use p2p_core::settings::VideoSettings;
use p2p_core::Identity;
use p2p_media::VideoSource;
use p2p_messenger::Messenger;
use p2p_ygg::Node;

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
        self.t = self.t.wrapping_add(5);
        let mut v = vec![0u8; (self.w * self.h * 3) as usize];
        for (i, px) in v.chunks_mut(3).enumerate() {
            px[0] = (i as u32).wrapping_add(self.t) as u8;
            px[1] = self.t as u8;
            px[2] = 100;
        }
        Some(v)
    }
}

fn main() {
    let ids: Vec<Identity> = (1u8..=3).map(|i| Identity::from_seed(&[i; 32]).unwrap()).collect();
    let pks: Vec<[u8; 32]> = ids.iter().map(|i| i.public_key_bytes()).collect();
    let nodes: Vec<Node> = ids.iter().map(|i| Node::start(&i.seed()).unwrap()).collect();
    let uri = nodes[0].listen("tls://127.0.0.1:0").unwrap();
    nodes[1].add_peer(&uri).unwrap();
    nodes[2].add_peer(&uri).unwrap();
    let t0 = Instant::now();
    while nodes.iter().any(|n| n.tree_count() < 2) {
        if t0.elapsed() > Duration::from_secs(15) {
            eprintln!("✗ нет дерева");
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_secs(3));

    let mut it = nodes.into_iter();
    let (ma, _ra) = Messenger::start(ids[0].clone(), it.next().unwrap());
    let (mb, _rb) = Messenger::start(ids[1].clone(), it.next().unwrap());
    let (mc, _rc) = Messenger::start(ids[2].clone(), it.next().unwrap());
    let (ma, mb, mc) = (Arc::new(ma), Arc::new(mb), Arc::new(mc));

    let (ka, kb, kc) = (pks[0], pks[1], pks[2]);
    thread::scope(|s| {
        let to = Duration::from_secs(25);
        let (ma1, ma2) = (ma.clone(), ma.clone());
        let (mb1, mc1) = (mb.clone(), mc.clone());
        s.spawn(move || ma1.connect(&kb, to));
        s.spawn(move || ma2.connect(&kc, to));
        s.spawn(move || mb1.connect(&ka, to));
        s.spawn(move || mc1.connect(&ka, to));
    });
    println!("✓ A соединён с B и C");

    let vcfg = VideoSettings { width: 240, height: 160, fps: 12, bitrate: 800_000 };
    // B и C шлют видео A.
    let _b = GroupCall::start(mb, vec![ka], None, Some(vcfg), Some(Box::new(TestSource { w: 240, h: 160, t: 0 }))).unwrap();
    let _c = GroupCall::start(mc, vec![ka], None, Some(vcfg), Some(Box::new(TestSource { w: 240, h: 160, t: 99 }))).unwrap();
    // A принимает от обоих.
    let mut a = GroupCall::start(ma, vec![kb, kc], None, None, None).unwrap();
    let frames = a.take_frames().unwrap();

    let mut srcs: HashSet<[u8; 32]> = HashSet::new();
    let mut count = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    while srcs.len() < 2 && Instant::now() < deadline {
        if let Ok((src, _frame)) = frames.recv_timeout(Duration::from_millis(500)) {
            srcs.insert(src);
            count += 1;
        }
    }

    println!("кадров принято: {count}, уникальных источников: {}", srcs.len());
    if srcs.contains(&kb) && srcs.contains(&kc) {
        println!("\n✓✓ Групповой звонок: видео от B и C принято по источникам (full mesh)");
    } else {
        eprintln!("\n✗ не от всех источников");
        std::process::exit(1);
    }
}
