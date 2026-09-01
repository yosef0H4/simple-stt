use std::borrow::Cow;

pub const BAR_COUNT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayPrimary {
    Hidden,
    Recording,
    Transcribing,
    Typing,
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
    let primary = match primary {
        OverlayPrimary::Hidden => None,
        OverlayPrimary::Recording => Some(format!(
            "\u{1f399}{} {}",
            indicators.text(),
            ascii_visualizer(visualizer)
        )),
        OverlayPrimary::Transcribing => Some("\u{1f399} Transcribing...".to_owned()),
        OverlayPrimary::Typing => Some("\u{1f399} Typing...".to_owned()),
    };
    match (primary, notice_text.filter(|text| !text.trim().is_empty())) {
        (Some(primary), Some(notice)) => format!("{primary}\r\n{}", notice.trim()),
        (Some(primary), None) => primary,
        (None, Some(notice)) => notice.trim().to_owned(),
        (None, None) => String::new(),
    }
}

pub fn linux_overlay_lines(
    primary: OverlayPrimary,
    notice_text: Option<&str>,
    visualizer: &VisualizerLevels,
    indicators: RecordingIndicators,
) -> Vec<Cow<'static, str>> {
    let mut lines = Vec::new();
    match primary {
        OverlayPrimary::Hidden => {}
        OverlayPrimary::Recording => lines.push(Cow::Owned(format!(
            "\u{1f399}{} {}",
            indicators.text(),
            ascii_visualizer(visualizer)
        ))),
        OverlayPrimary::Transcribing => {
            lines.push(Cow::Borrowed("\u{1f399} Transcribing..."));
        }
        OverlayPrimary::Typing => {
            lines.push(Cow::Borrowed("\u{1f399} Typing..."));
        }
    }
    if let Some(text) = notice_text.filter(|text| !text.trim().is_empty()) {
        lines.push(Cow::Owned(text.trim().to_owned()));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

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
