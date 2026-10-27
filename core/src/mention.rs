//! Упоминания в групповых сообщениях.
//!
//! Упоминание — это ССЫЛКА на человека, а не строка в тексте: участок текста
//! `[at, at+len)` (байты UTF-8) плюс ключ личности. Получатель рисует участок как
//! «@» + имя, которое ОН САМ знает для этого ключа (контакт, профиль), а не текст
//! отправителя. Поэтому подделать «@Аня» нельзя, и ников для этого не нужно —
//! набранное «@Аня» без ссылки остаётся просто текстом.

use serde::{Deserialize, Serialize};

/// Сколько упоминаний в одном сообщении принимаем. Больше — это уже не разговор, а
/// рассылка: режем, а не отвергаем сообщение целиком.
pub const MAX_MENTIONS: usize = 32;

/// Одно упоминание: участок текста и чей он.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mention {
    /// Начало участка в байтах UTF-8.
    pub at: u32,
    /// Длина участка в байтах UTF-8.
    pub len: u32,
    /// hex ключа личности упомянутого (64 символа).
    pub key: String,
}

/// Оставить только упоминания, которые можно честно нарисовать поверх `text`.
///
/// Вызывается и при отправке, и при приёме: отправитель может быть пропатчен, а
/// кривой участок у получателя — это паника на срезе строки или подсветка не того
/// куска. Отбрасываются: пустые и вылезающие за текст участки, границы посреди
/// символа, перекрытия (остаётся тот, что раньше), ключи не из 64 hex-символов и
/// всё сверх [`MAX_MENTIONS`]. Результат отсортирован по началу участка.
pub fn clean(text: &str, mut mentions: Vec<Mention>) -> Vec<Mention> {
    mentions.sort_by_key(|m| (m.at, m.len));
    let mut out: Vec<Mention> = Vec::new();
    let mut end_prev = 0usize;
    for mut m in mentions {
        if out.len() == MAX_MENTIONS {
            break;
        }
        let key_ok = m.key.len() == 64 && m.key.bytes().all(|b| b.is_ascii_hexdigit());
        let Some(end) = span(text, m.at, m.len, end_prev).filter(|_| key_ok) else { continue };
        m.key.make_ascii_lowercase();
        end_prev = end;
        out.push(m);
    }
    out
}

/// Можно ли взять участок `[at, at+len)` после предыдущего, кончившегося на
/// `end_prev`: непустой, внутри текста, на границах символов, без перекрытия.
/// Возвращает конец участка. Вынесено отдельно ради доказательства Kani — это ровно
/// то условие, от которого зависит, не упадёт ли срез строки у получателя.
fn span(text: &str, at: u32, len: u32, end_prev: usize) -> Option<usize> {
    let (at, len) = (at as usize, len as usize);
    let end = at.checked_add(len)?;
    let ok = len > 0
        && at >= end_prev
        && end <= text.len()
        && text.is_char_boundary(at)
        && text.is_char_boundary(end);
    ok.then_some(end)
}

/// Текст, где каждый участок упоминания заменён на «@» + имя от `name_of(key)`.
///
/// Нужен там, где ссылка дальше не едет и остаётся только строка: пересылка в
/// другой чат, копирование, превью. Иначе наружу уходит текст ОТПРАВИТЕЛЯ — метка,
/// которой он сам записал человека, — а не то имя, что видел пересылающий.
/// Участки проходят [`clean`], поэтому кривой список не роняет срез строки.
pub fn plain(text: &str, mentions: &[Mention], name_of: impl Fn(&str) -> String) -> String {
    let spans = clean(text, mentions.to_vec());
    if spans.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    for m in &spans {
        let (at, end) = (m.at as usize, (m.at + m.len) as usize);
        out.push_str(&text[pos..at]);
        out.push('@');
        out.push_str(&name_of(&m.key));
        pos = end;
    }
    out.push_str(&text[pos..]);
    out
}

/// Упомянут ли `key_hex` в списке (регистр hex не важен).
pub fn mentions_key(mentions: &[Mention], key_hex: &str) -> bool {
    mentions.iter().any(|m| m.key.eq_ignore_ascii_case(key_hex))
}

/// Доказательство Kani: какие бы числа ни прислал участник группы, участок,
/// который пропускает [`span`] (а значит и [`clean`]), режет текст без паники и не
/// заходит на предыдущий. Ключи и повторная чистка — на тестах ниже и фаззере
/// `mention`: строки на 64 символа верификатору не по силам за разумное время.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(12)]
    fn accepted_span_is_safe_to_slice() {
        // Кириллица (2 байта) и эмодзи (4): границы символов не совпадают с байтами.
        const TEXT: &str = "@Я 👋";
        let (at, len, end_prev): (u32, u32, usize) = (kani::any(), kani::any(), kani::any());
        if let Some(end) = span(TEXT, at, len, end_prev) {
            let at = at as usize;
            assert!(end_prev <= at && at < end && end <= TEXT.len());
            let _ = &TEXT[at..end]; // паника здесь = кривой участок прошёл проверку
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(at: u32, len: u32) -> Mention {
        Mention { at, len, key: "ab".repeat(32) }
    }

    #[test]
    fn keeps_valid_spans_sorted() {
        let text = "@Аня и @Боб, привет";
        let a = "@Аня".len() as u32;
        let b_at = "@Аня и ".len() as u32;
        let b = "@Боб".len() as u32;
        let got = clean(text, vec![m(b_at, b), m(0, a)]);
        assert_eq!(got, vec![m(0, a), m(b_at, b)]);
        assert_eq!(&text[got[0].at as usize..(got[0].at + got[0].len) as usize], "@Аня");
    }

    #[test]
    fn drops_broken_spans() {
        let text = "@Аня";
        // посреди буквы «А» (2 байта), за концом, пустой, переполнение
        assert!(clean(text, vec![m(0, 2)]).is_empty());
        assert!(clean(text, vec![m(0, 99)]).is_empty());
        assert!(clean(text, vec![m(1, 0)]).is_empty());
        assert!(clean(text, vec![m(u32::MAX, 2)]).is_empty());
    }

    #[test]
    fn drops_bad_keys_and_overlaps() {
        let text = "@Аня @Боб";
        let mut bad = m(0, 1);
        bad.key = "zz".repeat(32);
        let mut short = m(0, 1);
        short.key = "ab".into();
        assert!(clean(text, vec![bad, short]).is_empty());
        // перекрытие: второй участок начинается внутри первого — выкидывается
        let got = clean(text, vec![m(0, 7), m(1, 3)]);
        assert_eq!(got, vec![m(0, 7)]);
    }

    #[test]
    fn caps_count_and_normalises_key() {
        let text = "x".repeat(100);
        let many: Vec<_> = (0..50).map(|i| m(i, 1)).collect();
        assert_eq!(clean(&text, many).len(), MAX_MENTIONS);
        let mut upper = m(0, 1);
        upper.key = "AB".repeat(32);
        let got = clean(&text, vec![upper]);
        assert_eq!(got[0].key, "ab".repeat(32));
        assert!(mentions_key(&got, &"AB".repeat(32)));
    }

    #[test]
    fn plain_swaps_spans_for_local_names() {
        // Отправитель записал человека как «Мама», у пересылающего он «Ольга».
        let text = "@Мама, глянь 👋 @Мама";
        let at2 = "@Мама, глянь 👋 ".len() as u32;
        let len = "@Мама".len() as u32;
        let got = plain(text, &[m(at2, len), m(0, len)], |_| "Ольга".into());
        assert_eq!(got, "@Ольга, глянь 👋 @Ольга");
        // Без упоминаний и с кривым участком текст не трогается.
        assert_eq!(plain(text, &[], |_| "x".into()), text);
        assert_eq!(plain(text, &[m(0, 2)], |_| "x".into()), text);
    }
}
