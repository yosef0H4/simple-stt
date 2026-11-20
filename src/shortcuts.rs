//! Compact, localized descriptions of registered shortcuts, shared by overlays.
use crate::{
    config::{AppConfig, GeneralConfig},
    localization::Locale,
};
use reader_core::config::ShortcutSettings;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Binding {
    General(fn(&GeneralConfig) -> &str),
    Reader(fn(&ShortcutSettings) -> &str),
}

#[derive(Clone, Copy)]
pub struct Shortcut {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    binding: Binding,
}

use Binding::{General, Reader};

pub const ACTIONS: [Shortcut; 11] = [
    Shortcut {
        id: "record",
        label: "shortcut.record_action",
        icon: "🔴",
        binding: General(|g| &g.record_hotkey),
    },
    Shortcut {
        id: "cancel",
        label: "shortcut.cancel_action",
        icon: "❌",
        binding: General(|g| &g.cancel_hotkey),
    },
    Shortcut {
        id: "delivery",
        label: "shortcut.delivery_action",
        icon: "🔄",
        binding: General(|g| &g.toggle_delivery_hotkey),
    },
    Shortcut {
        id: "cleanup",
        label: "shortcut.cleanup_action",
        icon: "✨",
        binding: General(|g| &g.toggle_cleanup_hotkey),
    },
    Shortcut {
        id: "retry",
        label: "shortcut.retry_action",
        icon: "↪️",
        binding: General(|g| &g.retry_delivery_hotkey),
    },
    Shortcut {
        id: "shortcuts",
        label: "shortcut.show_action",
        icon: "⌨️",
        binding: General(|g| &g.show_shortcuts_hotkey),
    },
    Shortcut {
        id: "read-selection",
        label: "menu.readSelection",
        icon: "🔊",
        binding: Reader(|s| &s.read_selection),
    },
    Shortcut {
        id: "read-region",
        label: "menu.readRegion",
        icon: "📷",
        binding: Reader(|s| &s.read_region),
    },
    Shortcut {
        id: "copy-region",
        label: "menu.copyRegion",
        icon: "📋",
        binding: Reader(|s| &s.copy_region),
    },
    Shortcut {
        id: "listen-clipboard",
        label: "shortcut.listenClipboard",
        icon: "🎧",
        binding: Reader(|s| &s.listen_clipboard),
    },
    Shortcut {
        id: "read-along",
        label: "reading.read_along_start",
        icon: "📖",
        binding: Reader(|s| &s.read_along),
    },
];

pub fn reader_bindings(shortcuts: &ShortcutSettings) -> impl Iterator<Item = (&'static str, &str)> {
    ACTIONS
        .iter()
        .filter_map(move |action| match action.binding {
            Reader(chord) => Some((action.id, chord(shortcuts))),
            General(_) => None,
        })
}

pub fn configured<'a>(
    config: &'a AppConfig,
    shortcuts: Option<&'a ShortcutSettings>,
) -> impl Iterator<Item = (&'static str, &'a str)> {
    ACTIONS.iter().filter_map(move |action| {
        let chord = match action.binding {
            General(chord) => chord(&config.general),
            Reader(chord) => chord(shortcuts?),
        };
        Some((action.id, chord))
    })
}

pub fn assigned(config: &AppConfig) -> BTreeMap<String, String> {
    if cfg!(target_os = "linux") {
        // Config contains suggested chords; the desktop owns the actual bindings.
        return std::fs::read(AppConfig::local_data_dir().join("linux-shortcuts.json"))
            .ok()
            .and_then(|body| serde_json::from_slice(&body).ok())
            .unwrap_or_default();
    }
    let reader = crate::reader::load().ok();
    configured(config, reader.as_ref().map(|reader| &reader.shortcuts))
        .filter(|(id, _)| config.general.enabled || *id != "record")
        .map(|(id, chord)| (id.to_owned(), chord.to_owned()))
        .collect()
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
        .filter_map(|action| {
            if action.id == "shortcuts" && cfg!(windows) {
                return None;
            }
            let chord = bindings.get(action.id)?.trim();
            if chord.is_empty()
                || ["none", "off", "disabled"]
                    .iter()
                    .any(|v| chord.eq_ignore_ascii_case(v))
            {
                return None;
            }
            // Isolate the chord so + and Latin keys retain their order in Arabic.
            let chord = compact_chord(chord);
            Some(format!("{} \u{2066}{chord}\u{2069}", action.icon))
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
    fn copy_and_listen_have_distinct_emoji_in_one_row() {
        let bindings = BTreeMap::from([
            ("copy-region".into(), "Ctrl+C".into()),
            ("listen-clipboard".into(), "Ctrl+L".into()),
        ]);
        let text = summary(&bindings, Locale::En);
        assert_eq!(
            text,
            "📋 \u{2066}[Ctrl][C]\u{2069}\t🎧 \u{2066}[Ctrl][L]\u{2069}"
        );
    }

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
            .map(|action| (action.id.to_owned(), "Meta+Z".to_owned()))
            .collect();
        for locale in [Locale::En, Locale::Ar] {
            let text = summary(&bindings, locale);
            assert_eq!(text.lines().count(), 4);
            assert!(!text.contains("Record") && !text.contains("Cancel"));
            assert!(text.contains("↪️"));
            assert_eq!(text.contains("⌨️"), !cfg!(windows));
        }
    }

    #[test]
    fn single_abort_binding_replaces_reader_stop_shortcut() {
        assert_eq!(
            ACTIONS
                .iter()
                .filter(|action| action.id == "cancel")
                .count(),
            1
        );
        assert!(!ACTIONS.iter().any(|action| action.id == "stop-reading"));
        assert_eq!(
            ACTIONS
                .iter()
                .filter(|action| action.id == "copy-region")
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
