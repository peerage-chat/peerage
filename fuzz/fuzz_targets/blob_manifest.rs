//! Блобы: манифест из JSON, части манифеста из сети и сборка чанков. Части
//! генерируются структурно (`arbitrary`): числа в них — то, что пришлёт чужой.
#![no_main]

use libfuzzer_sys::arbitrary::{self, Arbitrary};
use libfuzzer_sys::fuzz_target;
use p2p_blob::{Limits, Manifest, ManifestAssembler, ManifestPart, Reassembler};

#[derive(Arbitrary, Debug)]
struct Part {
    id: u8,
    size: u64,
    part: u32,
    total: u32,
    hashes: u8,
}

#[derive(Arbitrary, Debug)]
struct Input<'a> {
    json: &'a str,
    parts: Vec<Part>,
    chunks: Vec<(u16, &'a [u8])>,
}

fuzz_target!(|inp: Input| {
    let limits = Limits::default();
    if let Ok(m) = Manifest::from_json(inp.json) {
        if m.validate(&limits).is_ok() {
            if let Ok(mut r) = Reassembler::new(m, &limits) {
                for (i, c) in &inp.chunks {
                    let _ = r.add(*i as usize, c);
                }
                let _ = r.finish();
            }
        }
    }
    // Части манифеста: ровно как в движке — размер уже сверен с лимитом.
    let mut asm: Option<ManifestAssembler> = None;
    for p in inp.parts {
        let part = ManifestPart {
            id: [p.id; 32],
            mime: String::new(),
            size: p.size % (limits.max_size + 1),
            part: p.part,
            total: p.total,
            hashes: vec![[0; 32]; p.hashes as usize],
        };
        match asm.as_mut() {
            Some(a) => {
                let _ = a.add(&part);
            }
            None => asm = ManifestAssembler::new(&part).ok(),
        }
    }
    if let Some(m) = asm.and_then(|a| a.finish()) {
        let _ = m.validate(&limits);
    }
});
