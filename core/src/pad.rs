//! Паддинг сообщений до размерных бакетов (веха M5, анти-трафик-анализ).
//!
//! Наблюдатель меша видит РАЗМЕР зашифрованной датаграммы. Без паддинга длина текста
//! («да» vs абзац) утекает по размеру шифртекста. Добивка plaintext до кратного
//! `bucket` ДО шифрования прячет точную длину в пределах бакета.
//!
//! Формат (самоописывающий): `[len: u32 BE][content][нули до бакета]`. `unpad` читает
//! длину и отбрасывает добивку. Применяется в [`crate::protocol`] к тексту (DM/группа);
//! мелкий control/keepalive не паддится (иначе амплификация). Честный предел: скрыт
//! РАЗМЕР, не тайминг; бакетизация, не полная униформа — см. threat-model / v1.0-scope.

/// Дописать паддинг до кратного `bucket` (0 = только префикс длины, без добивки).
pub fn pad(content: &[u8], bucket: usize) -> Vec<u8> {
    let base = 4 + content.len();
    let total = if bucket == 0 { base } else { base.div_ceil(bucket) * bucket };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&(content.len() as u32).to_be_bytes());
    out.extend_from_slice(content);
    out.resize(total, 0);
    out
}

/// Снять паддинг: вернуть исходное содержимое или None при повреждении.
pub fn unpad(padded: &[u8]) -> Option<Vec<u8>> {
    if padded.len() < 4 {
        return None;
    }
    let len = u32::from_be_bytes([padded[0], padded[1], padded[2], padded[3]]) as usize;
    let end = 4usize.checked_add(len)?;
    if end > padded.len() {
        return None;
    }
    Some(padded[4..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_unpad_roundtrip() {
        for len in [0usize, 1, 100, 4095, 4096, 4097, 10_000] {
            let content: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let padded = pad(&content, 4096);
            assert_eq!(unpad(&padded).unwrap(), content);
        }
    }

    #[test]
    fn padded_size_is_bucket_multiple_and_hides_length() {
        let short = pad(b"da", 4096);
        let longer = pad(&vec![7u8; 500], 4096);
        // Разные по длине тексты → одинаковый размер на проводе (в пределах бакета).
        assert_eq!(short.len(), 4096);
        assert_eq!(longer.len(), 4096);
        // Крупнее бакета → следующий кратный.
        assert_eq!(pad(&vec![0u8; 5000], 4096).len(), 8192);
    }

    #[test]
    fn bucket_zero_is_length_prefix_only() {
        let p = pad(b"ping", 0);
        assert_eq!(p.len(), 4 + 4); // префикс + содержимое, без добивки
        assert_eq!(unpad(&p).unwrap(), b"ping");
    }

    #[test]
    fn unpad_rejects_corrupt() {
        assert!(unpad(&[0, 0]).is_none()); // короче префикса
        assert!(unpad(&[0, 0, 0, 200, 1, 2]).is_none()); // длина больше буфера
    }
}
