//! Зачистка метаданных видео и HEIC (шаг беты 7m) — на образцах с геометкой.
//!
//! `fixtures/geotag-mdta.mp4` — ключи `mdta` в `moov/udta/meta` (так пишет Android `MediaMuxer`),
//! `fixtures/geotag-xyz.mov` — QuickTime `©xyz` в `moov/udta`. Оба сделаны ffmpeg с
//! `-metadata location=+55.7558+037.6173/ creation_time=2026-11-22T10:00:00Z`.
//! HEIC собирается в тесте: `ftyp` + `meta` (`hdlr pict`, `iinf`, `iloc`) + `mdat`.

use peerage_blob::scrub::{scrub, scrub_file};

const GEO: &[u8] = b"+55.7558";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Время создания `mvhd` (версия 0): 4 байта сразу за версией и флагами.
fn mvhd_creation(d: &[u8]) -> u32 {
    let at = find(d, b"mvhd").unwrap() + 4 + 4;
    u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}

/// Кадры не сдвинулись: всё, что за пределами зачищенных коробок, побайтно прежнее —
/// в частности `mdat` и таблицы смещений `stco`.
fn assert_frames_intact(before: &[u8], after: &[u8]) {
    assert_eq!(before.len(), after.len(), "длина файла прежняя");
    let mdat = find(before, b"mdat").unwrap();
    assert_eq!(find(after, b"mdat"), Some(mdat), "mdat на том же месте");
    let stco = find(before, b"stco").unwrap() - 4; // начало коробки — её размер
    let size = u32::from_be_bytes([
        before[stco],
        before[stco + 1],
        before[stco + 2],
        before[stco + 3],
    ]) as usize;
    assert_eq!(
        &before[stco..stco + size],
        &after[stco..stco + size],
        "таблица смещений кадров цела"
    );
    let m = mdat - 4;
    let msize =
        u32::from_be_bytes([before[m], before[m + 1], before[m + 2], before[m + 3]]) as usize;
    assert_eq!(
        &before[m..m + msize],
        &after[m..m + msize],
        "кадры не тронуты"
    );
}

#[test]
fn android_style_mdta_location_is_gone() {
    let src = fixture("geotag-mdta.mp4");
    assert!(find(&src, GEO).is_some(), "в образце геометка есть");
    assert_ne!(mvhd_creation(&src), 0, "и время создания");

    let out = scrub("video/mp4", &src);
    assert!(find(&out, GEO).is_none(), "геометка ушла");
    assert!(
        find(&out, b"com.apple.quicktime.location").is_none() && find(&out, b"location").is_none()
    );
    assert_eq!(mvhd_creation(&out), 0, "время создания обнулено");
    assert_frames_intact(&src, &out);
}

#[test]
fn quicktime_xyz_location_is_gone() {
    let src = fixture("geotag-xyz.mov");
    assert!(find(&src, "\u{a9}xyz".as_bytes()).is_some() || find(&src, b"\xa9xyz").is_some());
    let out = scrub("video/quicktime", &src);
    assert!(find(&out, GEO).is_none(), "геометка ушла");
    assert!(
        find(&out, b"\xa9xyz").is_none(),
        "коробка ©xyz ушла вместе с udta"
    );
    assert_frames_intact(&src, &out);
}

/// Видео идёт файлом: зачистка на месте даёт те же байты, что и в памяти.
#[test]
fn file_scrub_matches_memory_scrub() {
    let src = fixture("geotag-mdta.mp4");
    let dir = std::env::temp_dir().join(format!("scrub-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("v.mp4");
    std::fs::write(&path, &src).unwrap();
    let n = scrub_file(&path).unwrap();
    assert!(n >= 1, "что-то зачищено");
    assert_eq!(std::fs::read(&path).unwrap(), scrub("video/mp4", &src));
    std::fs::remove_dir_all(&dir).ok();
}

/// Не ISO BMFF файлом не трогается.
#[test]
fn file_scrub_leaves_other_files() {
    let dir = std::env::temp_dir().join(format!("scrub-o-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.bin");
    std::fs::write(&path, b"plain bytes, not a container").unwrap();
    assert_eq!(scrub_file(&path).unwrap(), 0);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"plain bytes, not a container"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── HEIC ────────────────────────────────────────────────────────────────────────

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    b.extend_from_slice(kind);
    b.extend_from_slice(body);
    b
}

fn full(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![version, 0, 0, 0];
    v.extend_from_slice(body);
    bx(kind, &v)
}

fn infe(id: u16, kind: &[u8; 4], extra: &[u8]) -> Vec<u8> {
    let mut b = id.to_be_bytes().to_vec();
    b.extend_from_slice(&[0, 0]); // item_protection_index
    b.extend_from_slice(kind);
    b.push(0); // пустое имя
    b.extend_from_slice(extra);
    full(b"infe", 2, &b)
}

/// HEIC: картинка (`hvc1`), EXIF (`Exif`) и XMP (`mime`) — все три в `mdat`.
fn heic(image: &[u8], exif: &[u8], xmp: &[u8]) -> (Vec<u8>, [usize; 3]) {
    let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
    let hdlr = full(
        b"hdlr",
        0,
        &[
            0, 0, 0, 0, b'p', b'i', b'c', b't', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
    let mut iinf_body = 3u16.to_be_bytes().to_vec();
    iinf_body.extend(infe(1, b"hvc1", &[]));
    iinf_body.extend(infe(2, b"Exif", &[]));
    iinf_body.extend(infe(3, b"mime", b"application/rdf+xml\0"));
    let iinf = full(b"iinf", 0, &iinf_body);
    // Смещения известны только после сборки: iloc фиксированного размера, считаем дважды.
    let build = |offs: [u32; 3]| {
        let mut b = vec![0x44, 0x00]; // offset_size 4, length_size 4; base 0, index 0
        b.extend_from_slice(&3u16.to_be_bytes());
        for (i, (off, len)) in offs
            .iter()
            .zip([image.len(), exif.len(), xmp.len()])
            .enumerate()
        {
            b.extend_from_slice(&((i + 1) as u16).to_be_bytes());
            b.extend_from_slice(&[0, 0]); // data_reference_index
            b.extend_from_slice(&1u16.to_be_bytes()); // один экстент
            b.extend_from_slice(&off.to_be_bytes());
            b.extend_from_slice(&(len as u32).to_be_bytes());
        }
        full(b"iloc", 0, &b)
    };
    let assemble = |offs: [u32; 3]| {
        let mut meta_body = hdlr.clone();
        meta_body.extend(iinf.clone());
        meta_body.extend(build(offs));
        let meta = full(b"meta", 0, &meta_body);
        let mut mdat_body = image.to_vec();
        mdat_body.extend_from_slice(exif);
        mdat_body.extend_from_slice(xmp);
        let mut f = ftyp.clone();
        f.extend(meta);
        let data_at = f.len() + 8;
        f.extend(bx(b"mdat", &mdat_body));
        (f, data_at)
    };
    let (_, at) = assemble([0; 3]);
    let offs = [at, at + image.len(), at + image.len() + exif.len()];
    let (f, _) = assemble(offs.map(|o| o as u32));
    (f, offs)
}

#[test]
fn heic_exif_and_xmp_items_are_zeroed_image_kept() {
    let image = b"HEVC-IMAGE-BYTES".as_slice();
    let exif = b"\0\0\0\0Exif\0\0GPS 55.7558N 37.6173E".as_slice();
    let xmp = b"<x:xmpmeta>geo</x:xmpmeta>".as_slice();
    let (src, offs) = heic(image, exif, xmp);

    let out = scrub("image/heic", &src);
    assert_eq!(out.len(), src.len());
    assert_eq!(&out[offs[0]..offs[0] + image.len()], image, "картинка цела");
    assert!(
        out[offs[1]..offs[1] + exif.len()].iter().all(|&b| b == 0),
        "EXIF занулён"
    );
    assert!(
        out[offs[2]..offs[2] + xmp.len()].iter().all(|&b| b == 0),
        "XMP занулён"
    );
    assert!(
        find(&out, b"meta").is_some() && find(&out, b"iloc").is_some(),
        "структура HEIF на месте"
    );
}
