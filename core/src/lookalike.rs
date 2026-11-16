//! Похожие имена (шаг беты 7e). Личность — это ключ, поэтому «ключ контакта сменился» не
//! бывает: новый ключ — новый человек, и он приходит запросом в контакты. Подмена выглядит
//! так: запрос с тем же именем, что у контакта. Сравнение — по скелету имени: регистр,
//! пробелы и знаки не различаются, а латинские буквы, неотличимые на глаз от кириллических,
//! сводятся к ним (`Аlice` с кириллической «А» и `Alice` — одно имя).

/// Скелет имени: двойники сведены к одной букве, всё строчное, только буквы и цифры.
pub fn skeleton(name: &str) -> String {
    name.chars()
        .map(fold)
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Выглядят ли имена одинаково. Пустое ни на что не похоже: безымянных не сравниваем.
pub fn same_name(a: &str, b: &str) -> bool {
    let a = skeleton(a);
    !a.is_empty() && a == skeleton(b)
}

/// Буква → её кириллический двойник (до перевода в строчные: «B» похожа на «В», «b» — нет).
fn fold(c: char) -> char {
    match c {
        'A' => 'А',
        'B' => 'В',
        'C' => 'С',
        'E' => 'Е',
        'H' => 'Н',
        'K' => 'К',
        'M' => 'М',
        'O' | '0' => 'О',
        'P' => 'Р',
        'T' => 'Т',
        'X' => 'Х',
        'Y' => 'У',
        'a' => 'а',
        'c' => 'с',
        'e' | 'ё' => 'е',
        'Ё' => 'Е',
        'o' => 'о',
        'p' => 'р',
        'x' => 'х',
        'y' => 'у',
        // «I», «l» и «1» в рубленых шрифтах — одна палочка.
        'I' | '1' => 'l',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookalike_letters_and_case_do_not_matter() {
        assert!(same_name("Алиса", "АЛИСА"));
        assert!(same_name("Алиса", "Aлиса"), "латинская A");
        assert!(same_name("Сергей", "Cepгeй"), "три латинских двойника");
        assert!(same_name("Alice", "Аlice"), "кириллическая А в латинском имени");
        assert!(same_name("Bob", "Воb"));
        assert!(same_name("Алёна", "Алена"));
        assert!(same_name("Paul", "PauI"), "заглавная I вместо l");
        assert!(same_name("Ольга", "0льга"), "ноль вместо О");
    }

    #[test]
    fn spaces_and_punctuation_do_not_matter() {
        assert!(same_name("Анна К.", "анна к"));
        assert!(same_name(" Борис ", "Борис"));
        assert!(same_name("Анна-Мария", "Анна Мария"));
    }

    #[test]
    fn different_names_differ() {
        assert!(!same_name("Анна", "Алиса"));
        assert!(!same_name("Bob", "Bobby"));
        assert!(!same_name("bob", "вов"), "строчная b на «в» не похожа");
    }

    #[test]
    fn empty_names_match_nothing() {
        assert!(!same_name("", ""));
        assert!(!same_name("...", "!!!"), "одни знаки — пустой скелет");
        assert!(!same_name("", "Анна"));
    }
}
