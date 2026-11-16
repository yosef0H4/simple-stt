//! Compact, localized descriptions of registered shortcuts, shared by overlays.
use crate::{config::AppConfig, localization::Locale};
use std::collections::BTreeMap;

pub const ACTIONS: [(&str, &str); 11] = [
    ("record", "shortcut.record"),
    ("cancel", "shortcut.cancel"),
    ("delivery", "shortcut.delivery"),
    ("cleanup", "shortcut.cleanup"),
    ("retry", "shortcut.retry"),
    ("shortcuts", "shortcut.show"),
    ("read-selection", "menu.readSelection"),
    ("read-region", "menu.readRegion"),
    ("copy-region", "menu.copyRegion"),
    ("listen-clipboard", "shortcut.listenClipboard"),
    ("read-along", "reading.read_along_start"),
];

pub fn assigned(config: &AppConfig) -> BTreeMap<String, String> {
    if cfg!(target_os = "linux") {
        // Config contains suggested chords; the desktop owns the actual bindings.
        return std::fs::read(AppConfig::local_data_dir().join("linux-shortcuts.json"))
            .ok()
            .and_then(|body| serde_json::from_slice(&body).ok())
            .unwrap_or_default();
    }
    let g = &config.general;
    let mut bindings: BTreeMap<String, String> = ACTIONS
        .iter()
        .map(|(id, _)| *id)
        .zip([
            if g.enabled { &g.record_hotkey } else { "None" },
            &g.cancel_hotkey,
            &g.toggle_delivery_hotkey,
            &g.toggle_cleanup_hotkey,
            &g.retry_delivery_hotkey,
            &g.show_shortcuts_hotkey,
        ])
        .map(|(id, chord)| (id.to_owned(), chord.to_owned()))
        .collect();
    if let Ok(reader) = crate::reader::load() {
        for (id, chord) in [
            ("read-selection", reader.shortcuts.read_selection),
            ("read-region", reader.shortcuts.read_region),
            ("copy-region", reader.shortcuts.copy_region),
            ("listen-clipboard", reader.shortcuts.listen_clipboard),
            ("read-along", reader.shortcuts.read_along),
        ] {
            bindings.insert(id.to_owned(), chord);
        }
    }
    bindings
}

pub fn action_icon(id: &str) -> &'static str {
    match id {
        "record" => "🔴",
        "cancel" => "❌",
        "delivery" => "🔄",
        "cleanup" => "✨",
        "retry" => "↪️",
        "shortcuts" => "⌨️",
        "read-selection" => "🔊",
        "read-region" => "📷",
        "copy-region" => "📋",
        "listen-clipboard" => "📋",
        "read-along" => "📖",
        _ => "",
    }
}

fn compact_chord(chord: &str) -> String {
    let chord = chord.replace(['\r', '\n'], " ");
    let (main, plus_key) = if chord == "+" {
        ("", true)
    } else if let Some(main) = chord.strip_suffix("++") {
        (main, true)
    } else {
        (chord.as_str(), false)
    };
    let mut caps = main
        .split('+')
        .filter(|key| !key.trim().is_empty())
        .map(|key| {
            let label = match key.trim().to_ascii_lowercase().as_str() {
                "meta" | "super" => "Meta",
                "win" => "Win",
                "control" | "ctrl" => "Ctrl",
                "shift" => "Shift",
                "alt" => "Alt",
                "capslock" => "CapsLock",
                "[" => "BracketLeft",
                "]" => "BracketRight",
                _ => key.trim(),
            };
            format!("[{label}]")
        })
        .collect::<String>();
    if plus_key {
        caps.push_str("[+]");
    }
    caps
}

pub fn summary(bindings: &BTreeMap<String, String>, locale: Locale) -> String {
    let items: Vec<_> = ACTIONS
        .iter()
        .filter_map(|(id, _)| {
            let chord = bindings.get(*id)?.trim();
            if chord.is_empty()
                || ["none", "off", "disabled"]
                    .iter()
                    .any(|v| chord.eq_ignore_ascii_case(v))
            {
                return None;
            }
            // Isolate the chord so + and Latin keys retain their order in Arabic.
            let chord = compact_chord(chord);
            Some(format!("{} \u{2066}{chord}\u{2069}", action_icon(id)))
        })
        .collect();
    let columns = if items.len() <= 4 { 2 } else { 3 };
    items
        .chunks(columns)
        .map(|row| {
            let line = row.join("\t");
            if locale == Locale::Ar {
                format!("\u{2066}{line}\u{2069}")
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn current(config: &AppConfig, locale: Locale) -> String {
    summary(&assigned(config), locale)
}

/// Reminders never create an idle tooltip or duplicate an explicit shortcut list.
pub fn append(text: String, reminder: &str, enabled: bool) -> String {
    if !enabled || text.is_empty() || reminder.is_empty() || text.contains(reminder) {
        text
    } else {
        format!("{text}\n{reminder}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_assigned_known_shortcuts_are_listed_in_action_order() {
        let map = BTreeMap::from([
            ("retry".into(), "Meta+V".into()),
            ("record".into(), "Meta+Z".into()),
            ("cancel".into(), "None".into()),
            ("cleanup".into(), "".into()),
            ("unknown".into(), "Meta+X".into()),
        ]);
        for locale in [Locale::En, Locale::Ar] {
            let text = summary(&map, locale);
            assert!(text.find("[Meta][Z]") < text.find("[Meta][V]"));
            assert!(!text.contains("None") && !text.contains("Meta+X"));
            assert_eq!(text.lines().count(), 1);
            assert!(text.contains("\u{2066}[Meta][Z]\u{2069}"));
        }
    }
    #[test]
    fn compact_modifiers_preserve_literal_keys_and_side_specific_names() {
        assert_eq!(compact_chord("Meta+Ctrl+,"), "[Meta][Ctrl][,]");
        assert_eq!(compact_chord("Meta+Ctrl+."), "[Meta][Ctrl][.]");
        assert_eq!(compact_chord("Ctrl++"), "[Ctrl][+]");
        assert_eq!(compact_chord("CapsLock+Shift+H"), "[CapsLock][Shift][H]");
        assert_eq!(compact_chord("LCtrl+Backspace"), "[LCtrl][Backspace]");
        assert_eq!(compact_chord("Meta+]"), "[Meta][BracketRight]");
        assert_eq!(compact_chord("+"), "[+]");
        let bindings = ACTIONS
            .iter()
            .map(|(id, _)| (id.to_string(), "Meta+Z".to_owned()))
            .collect();
        for locale in [Locale::En, Locale::Ar] {
            let text = summary(&bindings, locale);
            assert_eq!(text.lines().count(), 4);
            assert!(!text.contains("Record") && !text.contains("Cancel"));
            assert!(text.contains("↪️") && text.contains("⌨️"));
        }
    }

    #[test]
    fn single_abort_binding_replaces_reader_stop_shortcut() {
        assert_eq!(ACTIONS.iter().filter(|(id, _)| *id == "cancel").count(), 1);
        assert!(!ACTIONS.iter().any(|(id, _)| *id == "stop-reading"));
        assert_eq!(
            ACTIONS
                .iter()
                .filter(|(id, _)| *id == "copy-region")
                .count(),
            1
        );
    }

    #[test]
    fn reminders_do_not_keep_overlay_alive_or_duplicate_manual_list() {
        assert_eq!(append(String::new(), "keys", true), "");
        assert_eq!(append("Pasting".into(), "keys", false), "Pasting");
        assert_eq!(
            append("Shortcuts\nkeys".into(), "keys", true),
            "Shortcuts\nkeys"
        );
        assert_eq!(append("Pasting".into(), "keys", true), "Pasting\nkeys");
    }
    #[test]
    fn defaults_and_explicit_off_round_trip() {
        let config = AppConfig::normalize_json(&serde_json::json!({}));
        assert!(config.general.shortcut_reminders);
        let off = AppConfig::normalize_json(
            &serde_json::json!({"general":{"shortcut_reminders":false,"show_shortcuts_hotkey":"None"}}),
        );
        assert!(!off.general.shortcut_reminders);
        assert_eq!(off.general.show_shortcuts_hotkey, "None");
        assert_eq!(
            serde_json::from_value::<AppConfig>(serde_json::to_value(&off).unwrap()).unwrap(),
            off
        );
    }
}
