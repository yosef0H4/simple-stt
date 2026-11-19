use crate::localization::Locale;
use std::borrow::Cow;

pub const BAR_COUNT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayPrimary {
    Hidden,
    Recording,
    Transcribing,
    Typing,
    Cleaning,
    PreparingClipboard,
    Pasting,
    WaitingForPaste,
    WaitingForKeys,
}

impl OverlayPrimary {
    pub fn for_delivery(phase: crate::common::shell_protocol::DeliveryPhase) -> Self {
        use crate::common::shell_protocol::DeliveryPhase;
        match phase {
            DeliveryPhase::Typing => Self::Typing,
            DeliveryPhase::WaitingForKeys => Self::WaitingForKeys,
            DeliveryPhase::PreparingClipboard => Self::PreparingClipboard,
            DeliveryPhase::Pasting => Self::Pasting,
            DeliveryPhase::WaitingForPaste => Self::WaitingForPaste,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordingIndicators {
    pub ai_cleanup: bool,
    pub screen_context: bool,
}

impl RecordingIndicators {
    fn text(self) -> &'static str {
        match (self.ai_cleanup, self.screen_context) {
            (true, true) => " \u{1f916} \u{1f4f7}",
            (true, false) => " \u{1f916}",
            _ => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum NoticeLevel {
    #[default]
    Info,
    Warning,
    Error,
}

pub type VisualizerLevels = [f32; BAR_COUNT];

pub fn empty_visualizer_levels() -> VisualizerLevels {
    [0.0; BAR_COUNT]
}

pub fn set_visualizer_level(levels: &mut VisualizerLevels, level: f32) {
    let level = level.clamp(0.0, 1.0);
    let center = (BAR_COUNT as f32 - 1.0) * 0.5;
    for (idx, slot) in levels.iter_mut().enumerate() {
        let distance = (idx as f32 - center).abs() / center.max(1.0);
        let envelope = (1.0 - distance * 0.72).max(0.18);
        *slot = level * envelope;
    }
}

pub fn ascii_visualizer(levels: &VisualizerLevels) -> String {
    const GLYPHS: &[char] = &[
        '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
    ];
    let mut line = String::with_capacity(BAR_COUNT * 3);
    for level in levels {
        let strength = (*level + 0.04).clamp(0.0, 1.0);
        let glyph = GLYPHS[(strength * (GLYPHS.len() - 1) as f32).round() as usize];
        line.push(glyph);
    }
    line
}

pub fn render_overlay_text(
    primary: OverlayPrimary,
    notice_text: Option<&str>,
    visualizer: &VisualizerLevels,
    indicators: RecordingIndicators,
) -> String {
    render_overlay_text_with_locale(
        primary,
        notice_text,
        visualizer,
        indicators,
        crate::localization::Locale::En,
    )
}

pub fn render_overlay_text_with_locale(
    primary: OverlayPrimary,
    notice_text: Option<&str>,
    visualizer: &VisualizerLevels,
    indicators: RecordingIndicators,
    locale: crate::localization::Locale,
) -> String {
    let primary = match primary {
        OverlayPrimary::Hidden => None,
        OverlayPrimary::Recording => {
            let line = format!(
                "\u{1f399}{} {}",
                indicators.text(),
                ascii_visualizer(visualizer)
            );
            Some(if locale == crate::localization::Locale::Ar {
                format!("\u{2066}{line}\u{2069}")
            } else {
                line
            })
        }
        OverlayPrimary::Transcribing => Some(crate::localization::translate(
            locale,
            "overlay.transcribing",
            &[],
        )),
        OverlayPrimary::Cleaning => Some(crate::localization::translate(
            locale,
            "overlay.cleaning",
            &[],
        )),
        OverlayPrimary::PreparingClipboard => Some(crate::localization::translate(
            locale,
            "overlay.preparingClipboard",
            &[],
        )),
        OverlayPrimary::Pasting => Some(crate::localization::translate(
            locale,
            "overlay.pasting",
            &[],
        )),
        OverlayPrimary::WaitingForPaste => Some(crate::localization::translate(
            locale,
            "overlay.waitingForPaste",
            &[],
        )),
        OverlayPrimary::WaitingForKeys => Some(crate::localization::translate(
            locale,
            "overlay.waitingForKeys",
            &[],
        )),
        OverlayPrimary::Typing => Some(crate::localization::translate(
            locale,
            "overlay.typing",
            &[],
        )),
    };
    match (primary, notice_text.filter(|text| !text.trim().is_empty())) {
        (Some(primary), Some(notice)) => format!("{primary}\r\n{}", notice.trim()),
        (Some(primary), None) => primary,
        (None, Some(notice)) => notice.trim().to_owned(),
        (None, None) => String::new(),
    }
}

// Common Controls draws plain text. Strip decorative prefixes at the Windows
// boundary so status labels and the user's passage stay readable.
pub fn native_tooltip_text(text: &str, locale: Locale, primary: OverlayPrimary) -> String {
    use crate::localization::translate;
    fn unsupported_control(ch: char) -> bool {
        matches!(ch, '\u{2066}'..='\u{2069}' | '\u{fe0f}')
    }

    let mut result = String::with_capacity(text.len());
    for (index, line) in text.split(['\n', '\t']).enumerate() {
        let line = line.trim().trim_start_matches(unsupported_control);
        let body = if let Some((prefix, rest)) = line.split_once(' ') {
            match prefix.trim_end_matches('\u{fe0f}') {
                "🎙" if index == 0 && primary == OverlayPrimary::Recording => {
                    result.push_str(&translate(locale, "shortcut.record", &[]));
                    result.push(' ');
                    result.push_str(
                        &rest
                            .replace('🤖', "AI")
                            .replace('📷', &translate(locale, "overlay.screenContext", &[]))
                            .replace(unsupported_control, ""),
                    );
                    result.push('\n');
                    continue;
                }
                "⏸" if rest.starts_with('\u{2068}') => {
                    result.push_str(&translate(locale, "reader.phase.paused", &[]));
                    result.push_str(" · ");
                    rest
                }
                "🎙" | "🔊" | "📋" | "📄" | "🌐" | "🔍" | "⏸" | "⏳" | "⟳" | "⚠" => {
                    rest
                }
                _ => line,
            }
        } else {
            line
        };
        result.extend(body.chars().filter(|ch| !unsupported_control(*ch)));
        result.push('\n');
    }
    result.trim_end().to_owned()
}

pub fn linux_overlay_lines(
    primary: OverlayPrimary,
    notice_text: Option<&str>,
    visualizer: &VisualizerLevels,
    indicators: RecordingIndicators,
) -> Vec<Cow<'static, str>> {
    let primary_text = render_overlay_text_with_locale(
        primary,
        None,
        visualizer,
        indicators,
        crate::localization::Locale::En,
    );
    let mut lines = Vec::new();
    if !primary_text.is_empty() {
        lines.push(Cow::Owned(primary_text));
    }
    if let Some(notice) = notice_text.filter(|text| !text.trim().is_empty()) {
        lines.push(Cow::Owned(notice.trim().to_owned()));
    }
    lines
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_presentations_share_every_delivery_phase() {
        use crate::common::shell_protocol::DeliveryPhase;
        for (phase, expected, key) in [
            (
                DeliveryPhase::PreparingClipboard,
                OverlayPrimary::PreparingClipboard,
                "overlay.preparingClipboard",
            ),
            (
                DeliveryPhase::Pasting,
                OverlayPrimary::Pasting,
                "overlay.pasting",
            ),
            (
                DeliveryPhase::WaitingForPaste,
                OverlayPrimary::WaitingForPaste,
                "overlay.waitingForPaste",
            ),
            (
                DeliveryPhase::WaitingForKeys,
                OverlayPrimary::WaitingForKeys,
                "overlay.waitingForKeys",
            ),
            (
                DeliveryPhase::Typing,
                OverlayPrimary::Typing,
                "overlay.typing",
            ),
        ] {
            let primary = OverlayPrimary::for_delivery(phase);
            assert_eq!(primary, expected);
            for locale in [Locale::En, Locale::Ar] {
                let text = render_overlay_text_with_locale(
                    primary,
                    None,
                    &empty_visualizer_levels(),
                    RecordingIndicators::default(),
                    locale,
                );
                assert_eq!(text, crate::localization::translate(locale, key, &[]));
                assert!(!native_tooltip_text(&text, locale, primary).is_empty());
            }
            assert_eq!(
                linux_overlay_lines(
                    primary,
                    None,
                    &empty_visualizer_levels(),
                    RecordingIndicators::default()
                )[0],
                render_overlay_text(
                    primary,
                    None,
                    &empty_visualizer_levels(),
                    RecordingIndicators::default()
                )
            );
        }
    }

    #[test]
    fn native_tooltip_uses_readable_labels() {
        let cases = [
            (
                "🎙 🤖 📷 ▁▂▃",
                OverlayPrimary::Recording,
                "Record AI Screen ▁▂▃",
            ),
            (
                "🎙 Transcribing...",
                OverlayPrimary::Transcribing,
                "Transcribing...",
            ),
            (
                "🔊 \u{2068}Reading 📖 aloud.\u{2069} · 2/3\n📄 Next page · ready",
                OverlayPrimary::Hidden,
                "Reading 📖 aloud. · 2/3\nNext page · ready",
            ),
            (
                "⏸️ \u{2068}A passage.\u{2069}\n🔍 Recognizing text…\n⚠️ Reading failed",
                OverlayPrimary::Hidden,
                "Paused · A passage.\nRecognizing text…\nReading failed",
            ),
        ];
        for (input, primary, expected) in cases {
            assert_eq!(native_tooltip_text(input, Locale::En, primary), expected);
        }
        assert_eq!(
            native_tooltip_text(
                "🔊 \u{2068}نص \u{2066}Simple STT\u{2069}\u{2069}",
                Locale::Ar,
                OverlayPrimary::Hidden
            ),
            "نص Simple STT"
        );
    }

    #[test]
    fn waveform_uses_audio_level_without_phase_motion() {
        let mut levels = empty_visualizer_levels();
        set_visualizer_level(&mut levels, 0.4);
        let first = ascii_visualizer(&levels);
        let same = ascii_visualizer(&levels);
        set_visualizer_level(&mut levels, 0.8);
        let louder = ascii_visualizer(&levels);
        assert_eq!(first.chars().count(), BAR_COUNT);
        assert_eq!(first, same);
        assert_ne!(first, louder);
        assert!(first.chars().all(|glyph| {
            "\u{2581}\u{2582}\u{2583}\u{2584}\u{2585}\u{2586}\u{2587}".contains(glyph)
        }));
    }

    #[test]
    fn render_combines_primary_and_notice() {
        let text = render_overlay_text(
            OverlayPrimary::Transcribing,
            Some("Loading speech model..."),
            &empty_visualizer_levels(),
            RecordingIndicators::default(),
        );
        assert!(text.contains("Transcribing"));
        assert!(text.contains("Loading speech model..."));
    }

    #[test]
    fn recording_shows_ai_and_screen_privacy_indicators() {
        let text = render_overlay_text(
            OverlayPrimary::Recording,
            None,
            &empty_visualizer_levels(),
            RecordingIndicators {
                ai_cleanup: true,
                screen_context: true,
            },
        );
        assert!(text.contains("\u{1f916}"));
        assert!(text.contains("\u{1f4f7}"));
    }

    #[test]
    fn screen_indicator_never_appears_without_ai() {
        let text = render_overlay_text(
            OverlayPrimary::Recording,
            None,
            &empty_visualizer_levels(),
            RecordingIndicators {
                ai_cleanup: false,
                screen_context: true,
            },
        );
        assert!(!text.contains("\u{1f4f7}"));
    }
}
