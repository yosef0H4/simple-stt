//! Shared desktop messages. The same UTF-8 catalogs feed Settings and generated AHK.
use crate::config::UiLanguage;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    LazyLock,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    En,
    Ar,
}
impl Locale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ar => "ar",
        }
    }
    pub fn direction(self) -> &'static str {
        match self {
            Self::En => "ltr",
            Self::Ar => "rtl",
        }
    }
    pub fn from_tag(tag: &str) -> Self {
        if tag
            .split(['-', '_', '.', '@'])
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case("ar"))
        {
            Self::Ar
        } else {
            Self::En
        }
    }
}
type Catalog = BTreeMap<String, String>;
static EN: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../web/settings/src/lib/locales/desktop.en.json"
    ))
    .expect("validated English catalog")
});
static AR: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../web/settings/src/lib/locales/desktop.ar.json"
    ))
    .expect("validated Arabic catalog")
});
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Linux gettext message-locale precedence; LANGUAGE is ignored in C/POSIX locales.
pub fn message_locale(
    language: Option<&str>,
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
) -> Locale {
    let base = [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .unwrap_or("C");
    if base == "C" || base.starts_with("C.") || base == "POSIX" {
        return Locale::En;
    }
    if let Some(preferences) = language.filter(|s| !s.is_empty()) {
        for tag in preferences.split(':') {
            let primary = tag.split(['-', '_', '.', '@']).next().unwrap_or("");
            if primary.eq_ignore_ascii_case("ar") {
                return Locale::Ar;
            }
            if primary.eq_ignore_ascii_case("en") {
                return Locale::En;
            }
        }
    }
    Locale::from_tag(base)
}
pub fn system_locale() -> Locale {
    #[cfg(windows)]
    {
        // Match AutoHotkey's resolver and the user's display language, never keyboard layout.
        let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        if id & 0x3ff == 0x01 {
            Locale::Ar
        } else {
            Locale::En
        }
    }
    #[cfg(not(windows))]
    {
        let env = |key| std::env::var(key).ok();
        message_locale(
            env("LANGUAGE").as_deref(),
            env("LC_ALL").as_deref(),
            env("LC_MESSAGES").as_deref(),
            env("LANG").as_deref(),
        )
    }
}
pub fn resolve(language: UiLanguage) -> Locale {
    match language {
        UiLanguage::Auto => system_locale(),
        UiLanguage::En => Locale::En,
        UiLanguage::Ar => Locale::Ar,
    }
}
pub fn set_language(language: UiLanguage) -> Locale {
    let locale = resolve(language);
    CURRENT.store(u8::from(locale == Locale::Ar), Ordering::Relaxed);
    locale
}
pub fn current() -> Locale {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Locale::Ar
    } else {
        Locale::En
    }
}
pub fn contains(id: &str) -> bool {
    EN.contains_key(id)
}
pub fn translate(locale: Locale, id: &str, args: &[(&str, &str)]) -> String {
    let catalog = if locale == Locale::Ar { &*AR } else { &*EN };
    let template = catalog
        .get(id)
        .or_else(|| EN.get(id))
        .map(String::as_str)
        .unwrap_or(id);
    // Single-pass interpolation: user text containing braces is never interpreted again.
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}').map(|end| start + end) else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &rest[start + 1..end];
        if let Some((_, value)) = args.iter().find(|(key, _)| *key == name) {
            out.push_str(value);
        } else {
            out.push_str(&rest[start..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}
pub fn tr(id: &str) -> String {
    translate(current(), id, &[])
}
pub fn format(id: &str, args: &[(&str, &str)]) -> String {
    translate(current(), id, args)
}
pub fn message_values(id: &str, args: &[(&str, &str)]) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("message_id".into(), id.into()),
        (
            "message_args".into(),
            serde_json::to_string(&args.iter().copied().collect::<BTreeMap<_, _>>())
                .expect("string map serializes"),
        ),
    ])
}
pub fn state(language: UiLanguage) -> serde_json::Value {
    let locale = resolve(language);
    serde_json::json!({"system_locale": system_locale().as_str(), "locale":locale.as_str(), "direction":locale.direction()})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_precedence_and_regions() {
        assert_eq!(Locale::from_tag("ar-SA.UTF-8"), Locale::Ar);
        assert_eq!(Locale::from_tag("AR_eg"), Locale::Ar);
        assert_eq!(
            message_locale(None, Some("en_US"), Some("ar_SA"), Some("ar")),
            Locale::En
        );
        assert_eq!(
            message_locale(Some("fr:ar:en"), None, None, Some("en_US.UTF-8")),
            Locale::Ar
        );
        assert_eq!(
            message_locale(Some("ar"), Some("C.UTF-8"), None, None),
            Locale::En
        );
        assert_eq!(
            message_locale(None, None, Some("ar_EG"), Some("en")),
            Locale::Ar
        );
        assert_eq!(Locale::from_tag("fr_FR"), Locale::En);
    }
    #[test]
    fn catalogs_and_interpolation() {
        assert_eq!(EN.keys().collect::<Vec<_>>(), AR.keys().collect::<Vec<_>>());
        assert_eq!(
            translate(Locale::Ar, "notice.heard", &[("text", "abc {seconds}")]),
            "المسموع: abc {seconds}"
        );
        assert_eq!(translate(Locale::Ar, "unknown", &[]), "unknown");
        assert_eq!(translate(Locale::En, "notice.cancelled", &[]), "Cancelled");
    }
    #[test]
    fn additive_language_config() {
        for value in [
            serde_json::json!({"general":{}}),
            serde_json::json!({"general":{"ui_language":"invalid"}}),
        ] {
            assert_eq!(
                crate::config::AppConfig::normalize_json(&value)
                    .general
                    .ui_language,
                UiLanguage::Auto
            );
        }
        let config = crate::config::AppConfig::normalize_json(
            &serde_json::json!({"schema_version":9,"general":{"ui_language":"ar"}}),
        );
        assert_eq!(config.general.ui_language, UiLanguage::Ar);
        assert_eq!(
            crate::config::AppConfig::normalize_json(&serde_json::to_value(&config).unwrap()),
            config
        );
    }
}
