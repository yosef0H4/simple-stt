//! Generation-time preparation after routing; input remains unchanged for detection.
use crate::routing::Binding;

fn digit(c: char) -> Option<u32> {
    match c {
        '0'..='9' => Some(c as u32 - '0' as u32),
        '٠'..='٩' => Some(c as u32 - '٠' as u32),
        '۰'..='۹' => Some(c as u32 - '۰' as u32),
        _ => None,
    }
}

fn dollar_token(token: &str, language: &str) -> Option<String> {
    let end = token.trim_end_matches(['.', ',', '!', '?', ':', ';', '؟', '،', '؛']);
    let punctuation = &token[end.len()..];
    let digits = end.strip_prefix('$').or_else(|| end.strip_suffix('$'))?;
    let count = digits.chars().count();
    if !(1..=6).contains(&count) {
        return None;
    }
    let mut value = 0u32;
    for (i, c) in digits.chars().enumerate() {
        let n = digit(c)?;
        if i == 0 && n == 0 && count > 1 {
            return None;
        }
        value = value * 10 + n;
    }
    let words = match language {
        "en" => format!("{digits} {}", if value == 1 { "dollar" } else { "dollars" }),
        "ar" if value == 1 => "دولار واحد".to_owned(),
        "ar" if value == 2 => "دولاران".to_owned(),
        "ar" => format!(
            "{} {}",
            arabic_small_integer(value).unwrap_or_else(|| digits.to_owned()),
            if (3..=10).contains(&(value % 100)) {
                "دولارات"
            } else {
                "دولار"
            }
        ),
        _ => return None,
    };
    Some(format!("{words}{punctuation}"))
}

// Small standalone amounts have unambiguous Arabic wording. Larger numbers
// remain digits for the model; this is not a general date/number normalizer.
fn arabic_small_integer(value: u32) -> Option<String> {
    const ONES: [&str; 11] = [
        "صفر",
        "واحد",
        "اثنان",
        "ثلاثة",
        "أربعة",
        "خمسة",
        "ستة",
        "سبعة",
        "ثمانية",
        "تسعة",
        "عشرة",
    ];
    const TENS: [&str; 10] = [
        "",
        "",
        "عشرون",
        "ثلاثون",
        "أربعون",
        "خمسون",
        "ستون",
        "سبعون",
        "ثمانون",
        "تسعون",
    ];
    match value {
        0..=10 => Some(ONES[value as usize].into()),
        11 => Some("أحد عشر".into()),
        12 => Some("اثنا عشر".into()),
        13..=19 => Some(format!("{} عشر", ONES[(value % 10) as usize])),
        20..=99 if value.is_multiple_of(10) => Some(TENS[(value / 10) as usize].into()),
        20..=99 => Some(format!(
            "{} و{}",
            ONES[(value % 10) as usize],
            TENS[(value / 10) as usize]
        )),
        _ => None,
    }
}

fn slash_words(token: &str) -> Option<String> {
    let end = token.trim_end_matches(['.', ',', '!', '?', ':', ';', '؟', '،', '؛']);
    let punctuation = &token[end.len()..];
    let (left, right) = end.split_once('/')?;
    if left.is_empty()
        || right.is_empty()
        || !left.chars().all(char::is_alphabetic)
        || !right.chars().all(char::is_alphabetic)
    {
        return None;
    }
    Some(format!("{left}, {right}{punctuation}"))
}

/// Track runs of backticks consistently across independently planned units.
pub fn code_after(text: &str, mut in_code: bool) -> bool {
    let mut in_ticks = false;
    for c in text.chars() {
        if c == '`' {
            if !in_ticks {
                in_code = !in_code;
            }
            in_ticks = true;
        } else {
            in_ticks = false;
        }
    }
    in_code
}

fn clean_words(
    text: &str,
    language: &str,
    model: &str,
    binding: &Binding,
    starts_in_code: bool,
) -> String {
    let mut result = String::with_capacity(text.len());
    let mut token = String::new();
    let mut code = starts_in_code;
    let flush = |token: &mut String, result: &mut String, code: &mut bool| {
        if token.is_empty() {
            return;
        }
        let next_code = code_after(token, *code);
        if *code || token.contains('`') {
            result.push_str(token);
        } else {
            // Markdown emphasis and list markers are visual formatting, not speech.
            let plain = token.trim_matches('*');
            let prepared = if model == "kokoro" && binding.lowercase {
                plain.to_lowercase()
            } else {
                plain.to_owned()
            };
            let prepared = if model == "supertonic3" {
                prepared
                    .chars()
                    .map(|c| match c {
                        '۰'..='۹' => {
                            char::from_u32('0' as u32 + c as u32 - '۰' as u32).unwrap_or(c)
                        }
                        _ => c,
                    })
                    .collect::<String>()
            } else {
                prepared
            };
            let converted = (if binding.currency_words {
                dollar_token(&prepared, language)
            } else {
                None
            })
            .or_else(|| slash_words(&prepared));
            result.push_str(converted.as_deref().unwrap_or(&prepared));
        }
        *code = next_code;
        token.clear();
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut token, &mut result, &mut code);
            result.push(c);
        } else {
            token.push(c);
        }
    }
    flush(&mut token, &mut result, &mut code);
    result
}

pub fn normalize(text: &str, model: &str, language: &str, binding: &Binding) -> String {
    normalize_with_context(text, model, language, binding, false)
}

pub fn normalize_with_context(
    text: &str,
    model: &str,
    language: &str,
    binding: &Binding,
    starts_in_code: bool,
) -> String {
    if !binding.clean_text || !matches!(model, "kokoro" | "supertonic3") {
        return text.to_owned();
    }
    let primary = language.split(['-', '_']).next().unwrap_or("");
    clean_words(
        text,
        &primary.to_ascii_lowercase(),
        model,
        binding,
        starts_in_code,
    )
}

/// Literal, case-insensitive whole-word/phrase removal; original OCR text is retained by callers.
pub fn exclude_words(text: &str, words: &[String]) -> String {
    let mut entries: Vec<_> = words
        .iter()
        .map(|word| word.trim())
        .filter(|word| !word.is_empty())
        .collect();
    entries.sort_by_key(|word| std::cmp::Reverse(word.len()));
    if entries.is_empty() {
        return text.to_owned();
    }
    let pattern = entries
        .iter()
        .map(|word| regex::escape(word))
        .collect::<Vec<_>>()
        .join("|");
    let Ok(expression) = regex::RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
    else {
        return text.to_owned();
    };
    static WORD: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\w").unwrap());
    let word_char = |c: char| {
        let mut bytes = [0; 4];
        WORD.is_match(c.encode_utf8(&mut bytes))
    };
    let mut result = String::with_capacity(text.len());
    let mut end = 0;
    for found in expression.find_iter(text) {
        let first = found.as_str().chars().next().unwrap();
        let last = found.as_str().chars().next_back().unwrap();
        if (word_char(first)
            && text[..found.start()]
                .chars()
                .next_back()
                .is_some_and(word_char))
            || (word_char(last) && text[found.end()..].chars().next().is_some_and(word_char))
        {
            continue;
        }
        result.push_str(&text[end..found.start()]);
        end = found.end();
    }
    result.push_str(&text[end..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excluded_words_are_literal_case_insensitive_and_preserve_word_boundaries() {
        let words = vec![
            "Ann".into(),
            "مرحبا".into(),
            "do not read".into(),
            "a+b".into(),
        ];
        assert_eq!(
            exclude_words("ANN, annual. مرحبا بالعالم. Do Not Read this a+b.", &words),
            ", annual.  بالعالم.  this ."
        );
        assert_eq!(exclude_words("unchanged", &[]), "unchanged");
        assert_eq!(exclude_words("everything", &["everything".into()]), "");
    }

    #[test]
    fn emphasis_markers_are_silent_only_when_cleanup_is_enabled() {
        let source = "* I wrote **backend code** for your three tasks. **T22:**";
        let binding = Binding::default();
        for model in ["kokoro", "supertonic3"] {
            assert_eq!(
                normalize(source, model, "en", &binding),
                " I wrote backend code for your three tasks. T22:"
            );
            assert_eq!(
                normalize("`*literal*` 2*3", model, "en", &binding),
                "`*literal*` 2*3"
            );
            assert_eq!(
                normalize(
                    source,
                    model,
                    "en",
                    &Binding {
                        clean_text: false,
                        ..binding.clone()
                    }
                ),
                source
            );
        }
    }
    #[test]
    fn lowercase_is_opt_in_and_kokoro_only() {
        let mut binding = Binding::default();
        let input = "NASA TEST 123! مرحبا";
        assert_eq!(normalize(input, "kokoro", "en", &binding), input);
        binding.lowercase = true;
        assert_eq!(
            normalize(input, "kokoro", "en", &binding),
            "nasa test 123! مرحبا"
        );
        assert_eq!(normalize(input, "supertonic3", "ar", &binding), input);
    }
    #[test]
    fn dollar_amounts_respect_language_and_boundaries() {
        let binding = Binding {
            currency_words: true,
            ..Default::default()
        };
        assert_eq!(
            normalize("$1 $2 5$.", "kokoro", "en-US", &binding),
            "1 dollar 2 dollars 5 dollars."
        );
        assert_eq!(
            normalize("١$ ٢$ ٥$ ١١$ ١٠٣$", "supertonic3", "ar", &binding),
            "دولار واحد دولاران خمسة دولارات أحد عشر دولار ١٠٣ دولارات"
        );
        let unchanged =
            "$PATH $5.20 5.20$ 01$ -5$ 1234567$ 03/04/2026 `5$` ``` 5$ ``` https://x/5$ abc5$";
        assert_eq!(normalize(unchanged, "kokoro", "en", &binding), unchanged);
        assert_eq!(normalize("5$", "kokoro", "fr", &binding), "5$");
        assert_eq!(normalize("5$", "remote", "en", &binding), "5$");
        assert_eq!(
            normalize("5$", "kokoro", "en", &Binding::default()),
            "5 dollars"
        );
        assert_eq!(
            normalize("0$ 10$ 21$ 99$ 100$", "supertonic3", "ar", &binding),
            "صفر دولار عشرة دولارات واحد وعشرون دولار تسعة وتسعون دولار 100 دولار"
        );
    }
    #[test]
    fn cleanup_slashes_conservatively_and_can_be_disabled() {
        let binding = Binding::default();
        assert_eq!(
            normalize(
                "heap/data word/word. ذاكرة/بيانات",
                "kokoro",
                "en",
                &binding
            ),
            "heap, data word, word. ذاكرة, بيانات"
        );
        let literal = "https://x/y /home/user x/y/z 3/4 03/04/2026 `heap/data` ``` heap/data ``` -heap/data heap/data$";
        assert_eq!(normalize(literal, "kokoro", "en", &binding), literal);
        let disabled = Binding {
            clean_text: false,
            lowercase: true,
            ..Default::default()
        };
        let source = "NASA heap/data 5$ ۱۲۳";
        assert_eq!(normalize(source, "kokoro", "en", &disabled), source);
        assert_eq!(normalize(source, "supertonic3", "ar", &disabled), source);
    }
    #[test]
    fn fenced_code_stays_literal_across_planned_chunks() {
        let source = format!(
            "before/code ``` {} heap/data 5$ NASA ۱۲۳ ``` after/code",
            "word ".repeat(100)
        );
        let units = crate::text::plan_units(&source, 240).unwrap();
        assert!(units.len() > 2);
        let binding = Binding {
            lowercase: true,
            ..Default::default()
        };
        let output = units
            .iter()
            .map(|u| normalize_with_context(&u.text, "kokoro", "en", &binding, u.starts_in_code))
            .collect::<Vec<_>>()
            .join(" ");
        let expected = source
            .replace("before/code", "before, code")
            .replace("after/code", "after, code");
        assert_eq!(output, crate::text::normalize_source(&expected).unwrap());
        assert!(units
            .iter()
            .any(|u| u.starts_in_code && u.text.contains("heap/data")));
    }
    #[test]
    fn supertonic_maps_extended_arabic_indic_digits() {
        let binding = Binding::default();
        assert_eq!(
            normalize("۱۲۳ ١٢٣", "supertonic3", "ar", &binding),
            "123 ١٢٣"
        );
        assert_eq!(normalize("۱۲۳", "kokoro", "ar", &binding), "۱۲۳");
    }
}
