//! Lazy, offline language identification using the bundled fastText model.
use std::sync::{Mutex, OnceLock};

static CLASSIFIER: OnceLock<Option<Mutex<fasttext::FastText>>> = OnceLock::new();

fn load_classifier() -> Option<Mutex<fasttext::FastText>> {
    let executable = std::env::current_exe().ok()?;
    let installed = executable.parent()?.join("lid.176.ftz");
    #[cfg(debug_assertions)]
    let path = if installed.is_file() {
        installed
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../assets/language/lid.176.ftz")
    };
    #[cfg(not(debug_assertions))]
    let path = installed;
    let mut model = fasttext::FastText::new();
    model.load_model(path.to_str()?).ok()?;
    Some(Mutex::new(model))
}

/// Return the top three language labels and scores, including unsupported languages.
/// Input is bounded and excludes obvious URLs, email addresses and absolute paths.
pub fn candidates(text: &str) -> Vec<(String, f32)> {
    let sample: String = text
        .split_whitespace()
        .filter(|word| !word.contains("://") && !word.contains('@') && !word.starts_with('/'))
        .take(500)
        .flat_map(|word| word.chars().chain(std::iter::once(' ')))
        .take(16_384)
        .collect();
    if !sample.chars().any(char::is_alphabetic) {
        return Vec::new();
    }
    let Some(model) = CLASSIFIER.get_or_init(load_classifier).as_ref() else {
        return Vec::new();
    };
    let Ok(model) = model.lock() else {
        return Vec::new();
    };
    model
        .predict(&sample, 3, 0.0)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|prediction| {
            prediction
                .label
                .strip_prefix("__label__")
                .map(|label| (label.to_owned(), prediction.prob))
        })
        .collect()
}

/// Return a language only when the leading candidate is sufficiently distinct.
/// Calibrated on the fixed multilingual fixtures below. Shared short words remain uncertain.
pub fn detect(text: &str) -> Option<String> {
    let results = candidates(text);
    let (language, confidence) = results.first()?;
    let runner_up = results.get(1).map_or(0.0, |(_, score)| *score);
    (*confidence >= 0.8 && *confidence - runner_up >= 0.2).then(|| language.clone())
}

/// Use script hints for short text after statistical detection declines. This
/// identifies a language; assignment lookup still requires that exact language.
pub fn resolve(text: &str) -> String {
    detect(text).unwrap_or_else(|| {
        if text
            .chars()
            .any(|c| matches!(c as u32, 0x0600..=0x06ff | 0x0750..=0x077f | 0x08a0..=0x08ff))
        {
            "ar".into()
        } else {
            "en".into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_has_no_language() {
        assert_eq!(detect("123 🙂 https://example.com/path"), None);
    }

    #[test]
    fn common_text_and_short_script_hints() {
        assert_eq!(detect("Hello world.").as_deref(), Some("en"));
        assert_eq!(resolve("مرحبا"), "ar");
        assert_eq!(resolve("شكرا"), "ar");
        assert_eq!(
            detect("این یک جمله فارسی برای آزمایش تشخیص زبان است.").as_deref(),
            Some("fa")
        );
        assert_eq!(
            detect("یہ زبان کی شناخت کے لیے ایک اردو جملہ ہے۔").as_deref(),
            Some("ur")
        );
    }

    // Public, fixed fixtures only. Scores are intended for threshold calibration.
    #[test]
    fn calibration_samples() {
        let fixtures = [
            ("ar-short-1", "مرحبا"),
            ("ar-short-2", "شكرا"),
            ("ambiguous", "سلام"),
            ("fa", "این یک جمله فارسی برای آزمایش تشخیص زبان است."),
            ("ur", "یہ زبان کی شناخت کے لیے ایک اردو جملہ ہے۔"),
            ("en-short", "Hello world."),
            (
                "en",
                "This is an English sentence that should be read aloud using the English voice.",
            ),
            (
                "ar",
                "هذه جملة عربية واضحة أريد قراءتها بصوت عال باستخدام النموذج العربي.",
            ),
            ("ko", "이것은 언어를 식별하기 위한 한국어 문장입니다."),
            ("ja", "これは言語を識別するための日本語の文章です。"),
            (
                "es",
                "Esta es una oración en español para identificar el idioma.",
            ),
            (
                "pt",
                "Esta é uma frase em português para identificar o idioma.",
            ),
            (
                "fr",
                "Ceci est une phrase française pour identifier la langue.",
            ),
            (
                "de",
                "Dies ist ein deutscher Satz zur Erkennung der Sprache.",
            ),
            (
                "ru",
                "Это русское предложение для определения языка текста.",
            ),
            (
                "it",
                "Questa è una frase italiana per identificare la lingua.",
            ),
            ("nl", "Dit is een Nederlandse zin om de taal te herkennen."),
            ("pl", "To jest polskie zdanie do rozpoznawania języka."),
            (
                "sv",
                "Det här är en svensk mening för att identifiera språket.",
            ),
            (
                "tr",
                "Bu, dili tanımlamak için kullanılan Türkçe bir cümledir.",
            ),
            (
                "id",
                "Ini adalah kalimat bahasa Indonesia untuk mengenali bahasa.",
            ),
            ("hi", "यह भाषा की पहचान करने के लिए एक हिंदी वाक्य है।"),
            (
                "fi",
                "Tämä on suomenkielinen lause kielen tunnistamista varten.",
            ),
            ("vi", "Đây là một câu tiếng Việt để xác định ngôn ngữ."),
            ("he", "זהו משפט בעברית שנועד לזהות את השפה של הטקסט."),
            ("uk", "Це українське речення для визначення мови тексту."),
            (
                "el",
                "Αυτή είναι μια ελληνική πρόταση για την αναγνώριση της γλώσσας.",
            ),
            (
                "da",
                "Dette er en dansk sætning til at identificere sproget.",
            ),
            (
                "ms",
                "Ini ialah ayat bahasa Melayu untuk mengenal pasti bahasa.",
            ),
            (
                "ro",
                "Aceasta este o propoziție în limba română pentru identificarea limbii.",
            ),
            ("hu", "Ez egy magyar mondat a nyelv azonosítására szolgál."),
            ("ta", "இது மொழியை அடையாளம் காண உதவும் தமிழ் வாக்கியம்."),
            ("th", "นี่คือประโยคภาษาไทยสำหรับใช้ในการระบุภาษา"),
            ("cs", "Toto je česká věta pro rozpoznání jazyka textu."),
            ("sk", "Toto je slovenská veta na rozpoznanie jazyka textu."),
            ("hr", "Ovo je hrvatska rečenica za prepoznavanje jezika."),
            ("bg", "Това е българско изречение за разпознаване на езика."),
            ("et", "See on eestikeelne lause keele tuvastamiseks."),
            ("lt", "Tai lietuviškas sakinys kalbai nustatyti."),
            ("lv", "Šis ir teikums latviešu valodā valodas noteikšanai."),
            ("sl", "To je slovenski stavek za prepoznavanje jezika."),
            ("noise", "123 🙂 https://example.com/path"),
        ];
        for (name, text) in fixtures {
            eprintln!(
                "{name}: {:?}, selected={:?}",
                candidates(text),
                detect(text)
            );
        }
    }
}
