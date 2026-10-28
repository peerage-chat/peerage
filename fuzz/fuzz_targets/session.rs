//! Пакеты сессий 1:1 (ADR-012) на каждом шаге рукопожатия, в живой сессии и
//! медиа-канал сессии (SEC-4).
//!
//! Случайные байты почти никогда не попадают в наш индекс, поэтому цель заранее
//! проводит настоящее рукопожатие и запоминает состояние обеих сторон на каждом
//! шаге (дорого: ML-KEM). Вход портит пакет нужного шага: первый байт выбирает шаг
//! и способ порчи, остальное — сами байты или маска поверх настоящего пакета. Всё,
//! что получатель ответит, отдаётся второй стороне — так проверяется и путь «после
//! подделки».
#![no_main]

use std::sync::OnceLock;
use std::time::Instant;

use libfuzzer_sys::fuzz_target;
use peerage_core::session::{Received, Sessions};
use peerage_core::Identity;
use rand_core::{CryptoRng, RngCore};

/// Детерминированная «случайность» (splitmix64): находку можно воспроизвести.
struct Det(u64);
impl RngCore for Det {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for c in dest.chunks_mut(8) {
            let v = self.next_u64().to_le_bytes();
            c.copy_from_slice(&v[..c.len()]);
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
impl CryptoRng for Det {}

fn text() -> Vec<u8> {
    peerage_core::pad::pad(b"fuzz", peerage_core::protocol::APP_BUCKET)
}

/// Шаг рукопожатия: состояния сторон перед приёмом `template`, кто его принимает.
struct Stage {
    a: Sessions,
    b: Sessions,
    template: Vec<u8>,
    to_b: bool,
}

struct Fixture {
    pa: [u8; 32],
    pb: [u8; 32],
    t: Instant,
    stages: Vec<Stage>,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let (ia, ib) = (Identity::from_seed(&[1; 32]).unwrap(), Identity::from_seed(&[2; 32]).unwrap());
        let (pa, pb) = (ia.public_key_bytes(), ib.public_key_bytes());
        let (mut a, mut b) = (Sessions::new(&ia), Sessions::new(&ib));
        let mut rng = Det(7);
        let t = Instant::now();
        let mut stages = Vec::new();

        let m1 = a.send(&pb, &text(), true, t, &mut rng).remove(0);
        stages.push(Stage { a: a.clone(), b: b.clone(), template: m1.clone(), to_b: true });
        let Received::Reply(m2) = b.receive(&m1, t, &mut rng) else { unreachable!() };
        stages.push(Stage { a: a.clone(), b: b.clone(), template: m2.clone(), to_b: false });
        let Received::Established { out, .. } = a.receive(&m2, t, &mut rng) else { unreachable!() };
        stages.push(Stage { a: a.clone(), b: b.clone(), template: out[0].clone(), to_b: true });
        for p in &out {
            let _ = b.receive(p, t, &mut rng);
        }
        let data = a.send(&pb, &text(), true, t, &mut rng).remove(0);
        stages.push(Stage { a: a.clone(), b: b.clone(), template: data, to_b: true });
        let mut frame = vec![0u8; litertc::MEDIA_HEADER_LEN + 5];
        litertc::write_media_header(&mut frame, &litertc::MediaHeader { track: litertc::TRACK_AUDIO, seq: 1, ..Default::default() });
        let media = a.seal_media(&pb, &frame, t).unwrap();
        stages.push(Stage { a, b, template: media, to_b: true });
        Fixture { pa, pb, t, stages }
    })
}

fuzz_target!(|data: &[u8]| {
    let Some((&sel, rest)) = data.split_first() else { return };
    let f = fixture();
    let st = &f.stages[usize::from(sel % 5)];
    let (mut a, mut b) = (st.a.clone(), st.b.clone());
    let mut rng = Det(u64::from(sel));

    // Порча: свой индекс + чужие байты, либо маска поверх настоящего пакета.
    let pkt = if sel & 4 == 0 {
        [&st.template[..4], rest].concat()
    } else {
        let mut p = st.template.clone();
        for (x, m) in p.iter_mut().zip(rest) {
            *x ^= m;
        }
        if sel & 8 != 0 {
            p.truncate(rest.len());
        }
        p
    };

    let (rx, tx) = if st.to_b { (&mut b, &mut a) } else { (&mut a, &mut b) };
    // Любой пакет пробуем и как медиа: путь разбора у медиа свой.
    if let Some((_, frame)) = rx.open_media(&pkt, f.t) {
        let _ = litertc::parse_media(&frame);
    }
    let back = match rx.receive(&pkt, f.t, &mut rng) {
        Received::Reply(m) => vec![m],
        Received::Established { out, .. } => out,
        _ => Vec::new(),
    };
    for p in back {
        let _ = tx.receive(&p, f.t, &mut rng);
    }
    // Состояние после подделки обязано оставаться рабочим.
    let _ = a.send(&f.pb, &text(), true, f.t, &mut rng);
    let _ = b.send(&f.pa, &text(), true, f.t, &mut rng);
    let _ = a.tick(f.t, &mut rng);
});
