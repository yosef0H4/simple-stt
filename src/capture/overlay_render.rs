//! Wayland-free rendering for the Linux overlay.
//!
//! Renders the overlay text in a single Pango (Sans) layout — the original
//! look — and sizes the surface to fit the text so the panel grows/shrinks with
//! its content (dynamic, tooltip-style UI).

use crate::localization::Locale;

use super::overlay_model::{
    render_overlay_text_with_locale, OverlayPrimary, RecordingIndicators, VisualizerLevels,
};

// Monospace font that renders the block glyphs (▁▂▃…▇) full-cell and crisp,
// matching how they look in a terminal. JetBrainsMono is SIL OFL licensed.
// The fallback chain keeps it working if that family is not installed.
pub const FONT: &str = "JetBrainsMono Nerd Font, JetBrains Mono, monospace 12";

// Box / content layout (logical pixels).
const ACCENT_X: f64 = 12.0;
const ACCENT_W: f64 = 4.0;
const TEXT_X: f64 = 26.0;
const TEXT_Y: f64 = 13.0;
const PAD_RIGHT: f64 = 18.0;
const PAD_BOTTOM: f64 = 13.0;
const LINE_SPACING_PX: i32 = 2;

pub const MIN_WIDTH: u32 = 90;
pub const MIN_HEIGHT: u32 = 40;
pub const MAX_WIDTH: u32 = 560;
pub const MAX_HEIGHT: u32 = 420;

pub struct LayoutPlan {
    pub locale: Locale,
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub signature: String,
}

/// Build the overlay text and the surface size needed to fit it.
pub fn plan_for(
    primary: OverlayPrimary,
    notice: Option<&str>,
    levels: &VisualizerLevels,
    indicators: RecordingIndicators,
) -> Option<LayoutPlan> {
    plan_for_locale(primary, notice, levels, indicators, Locale::En)
}

pub fn plan_for_locale(
    primary: OverlayPrimary,
    notice: Option<&str>,
    levels: &VisualizerLevels,
    indicators: RecordingIndicators,
    locale: Locale,
) -> Option<LayoutPlan> {
    plan_with_shortcuts(primary, notice, levels, indicators, locale, "", false)
}

pub fn plan_with_shortcuts(
    primary: OverlayPrimary,
    notice: Option<&str>,
    levels: &VisualizerLevels,
    indicators: RecordingIndicators,
    locale: Locale,
    shortcuts: &str,
    enabled: bool,
) -> Option<LayoutPlan> {
    let text = crate::shortcuts::append(
        render_overlay_text_with_locale(primary, notice, levels, indicators, locale)
            .replace("\r\n", "\n"),
        shortcuts,
        enabled,
    );
    if text.trim().is_empty() {
        return None;
    }

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    let layout = build_layout(&cr, &text, locale, MAX_WIDTH as i32 - 44);
    let (w, h) = layout.pixel_size();

    let width = ((TEXT_X + f64::from(w) + PAD_RIGHT).ceil() as u32).clamp(MIN_WIDTH, MAX_WIDTH);
    let height = ((TEXT_Y + f64::from(h) + PAD_BOTTOM).ceil() as u32).clamp(MIN_HEIGHT, MAX_HEIGHT);
    Some(LayoutPlan {
        width,
        height,
        signature: format!("{}:{text}", locale.as_str()),
        locale,
        text,
    })
}

/// Render a plan to an ARGB32 Cairo image surface of `plan.width × plan.height`.
pub fn render_surface(plan: &LayoutPlan) -> Option<cairo::ImageSurface> {
    let width = plan.width as i32;
    let height = plan.height as i32;
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;

    // Background panel.
    cr.set_source_rgba(0.045, 0.050, 0.055, 0.92);
    rounded_rect(
        &cr,
        0.5,
        0.5,
        f64::from(width - 1),
        f64::from(height - 1),
        7.0,
    );
    let _ = cr.fill();

    // Left accent bar.
    cr.set_source_rgba(0.24, 0.70, 1.0, 1.0);
    let accent_x = if plan.locale == Locale::Ar {
        f64::from(width) - ACCENT_X - ACCENT_W
    } else {
        ACCENT_X
    };
    rounded_rect(&cr, accent_x, 13.0, ACCENT_W, f64::from(height) - 26.0, 2.0);
    let _ = cr.fill();

    // Share the exact same shaped layout for measurement, keycaps and text.
    let layout = build_layout(&cr, &plan.text, plan.locale, width - 44);
    let _ = cr.save();
    cr.translate(
        if plan.locale == Locale::Ar {
            PAD_RIGHT
        } else {
            TEXT_X
        },
        TEXT_Y,
    );
    paint_keycaps(&cr, &layout, &plan.text);
    cr.move_to(0.0, 0.0);
    cr.set_source_rgba(0.94, 0.96, 0.98, 1.0);
    pangocairo::functions::show_layout(&cr, &layout);
    let _ = cr.restore();

    drop(layout);
    drop(cr);
    Some(surface)
}

fn build_layout(cr: &cairo::Context, text: &str, locale: Locale, width: i32) -> pango::Layout {
    super::overlay_font::ensure_registered();
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_text(text);
    // Allow long notices to wrap rather than overflow; the surface is sized to
    // the resulting (possibly smaller) extent.
    layout.set_width(width * pango::SCALE);
    layout.context().set_base_dir(if locale == Locale::Ar {
        pango::Direction::Rtl
    } else {
        pango::Direction::Ltr
    });
    layout
        .context()
        .set_language(Some(&pango::Language::from_string(locale.as_str())));
    layout.set_auto_dir(false);
    layout.set_alignment(if locale == Locale::Ar {
        pango::Alignment::Right
    } else {
        pango::Alignment::Left
    });
    layout.set_wrap(pango::WrapMode::WordChar);
    // Do not ellipsize: notification text must remain readable. Pango wraps it
    // within MAX_WIDTH and plan_for grows the surface vertically. The compact
    // one-line visualizer never reaches this limit, so its layout is unchanged.
    layout.set_ellipsize(pango::EllipsizeMode::None);
    layout.set_spacing(LINE_SPACING_PX * pango::SCALE);
    layout.set_font_description(Some(&pango::FontDescription::from_string(
        if locale == Locale::Ar {
            "Noto Sans Arabic, JetBrains Mono, sans-serif 12"
        } else {
            FONT
        },
    )));
    let attrs = pango::AttrList::new();
    if locale == Locale::Ar {
        let mut start = 0;
        for word in text.split_inclusive('\n') {
            if word.starts_with('\u{2066}') {
                let mut attr = pango::AttrFontDesc::new(&pango::FontDescription::from_string(FONT));
                attr.set_start_index(start);
                attr.set_end_index(start + word.len() as u32);
                attrs.insert(attr);
            }
            start += word.len() as u32;
        }
    }
    decorate_shortcuts(&layout, text, &attrs);
    if text.contains('\t') && !shortcut_runs(text).is_empty() {
        let mut widths = [0; 2];
        for line in text.lines().filter(|line| line.contains('\t')) {
            for (column, entry) in line.split('\t').take(2).enumerate() {
                let entry_layout = build_layout(cr, entry, locale, width);
                widths[column] = widths[column].max(entry_layout.pixel_size().0);
            }
        }
        let mut tabs = pango::TabArray::new(2, true);
        tabs.set_tab(0, pango::TabAlign::Left, widths[0] + 20);
        tabs.set_tab(1, pango::TabAlign::Left, widths[0] + widths[1] + 40);
        layout.set_tabs(Some(&tabs));
    }
    layout
}

#[derive(Debug)]
struct ShortcutRun {
    icon: std::ops::Range<usize>,
    keys: Vec<std::ops::Range<usize>>,
    end: usize,
}

// Only app-generated icon + isolated keycap runs receive shortcut styling.
// Notices, transcripts, filenames and ordinary bracketed text stay plain.
fn shortcut_runs(text: &str) -> Vec<ShortcutRun> {
    let mut runs = Vec::new();
    for action in crate::shortcuts::ACTIONS {
        let icon = action.icon;
        let prefix = format!("{icon} \u{2066}[");
        for (start, _) in text.match_indices(&prefix) {
            let keys_start = start + icon.len() + " \u{2066}".len();
            let Some(length) = text[keys_start..].find('\u{2069}') else {
                continue;
            };
            let end = keys_start + length;
            let mut keys = Vec::new();
            let mut offset = keys_start;
            while offset < end && text.as_bytes()[offset] == b'[' {
                let Some(close) = text[offset + 1..end].find(']') else {
                    break;
                };
                let next = offset + close + 2;
                keys.push(offset..next);
                offset = next;
            }
            if !keys.is_empty() && offset == end {
                runs.push(ShortcutRun {
                    icon: start..start + icon.len(),
                    keys,
                    end,
                });
            }
        }
    }
    runs
}

fn span(
    attrs: &pango::AttrList,
    attribute: impl Into<pango::Attribute>,
    range: std::ops::Range<usize>,
) {
    let mut attribute = attribute.into();
    attribute.set_start_index(range.start as u32);
    attribute.set_end_index(range.end as u32);
    attrs.insert(attribute);
}

fn reserve(attrs: &pango::AttrList, range: std::ops::Range<usize>, width: i32) {
    let ink = pango::Rectangle::new(0, 0, 0, 0);
    let logical = pango::Rectangle::new(
        0,
        -15 * pango::SCALE,
        width * pango::SCALE,
        20 * pango::SCALE,
    );
    span(attrs, pango::AttrShape::new(&ink, &logical), range.clone());
    span(attrs, pango::AttrInt::new_foreground_alpha(0), range);
}

fn decorate_shortcuts(layout: &pango::Layout, text: &str, attrs: &pango::AttrList) {
    let emoji_font =
        pango::FontDescription::from_string("Noto Color Emoji, Segoe UI Emoji, emoji 12");
    for glyph in ["🎙", "🤖", "📷"] {
        for (start, _) in text.match_indices(glyph) {
            span(
                attrs,
                pango::AttrFontDesc::new(&emoji_font),
                start..start + glyph.len(),
            );
        }
    }
    for run in shortcut_runs(text) {
        span(
            attrs,
            pango::AttrFontDesc::new(&pango::FontDescription::from_string("JetBrains Mono 10")),
            run.icon.start..run.end,
        );
        // Whole shortcut groups wrap together; individual keycaps never split.
        span(
            attrs,
            pango::AttrInt::new_allow_breaks(false),
            run.icon.start..run.end,
        );
        // Emoji stay ordinary shaped text, shared with the Windows tooltip.
        span(attrs, pango::AttrFontDesc::new(&emoji_font), run.icon);
        for key in run.keys {
            reserve(attrs, key.start..key.start + 1, 6);
            reserve(attrs, key.end - 1..key.end, 6);
        }
    }
    layout.set_attributes(Some(attrs));
}

fn paint_keycaps(cr: &cairo::Context, layout: &pango::Layout, text: &str) {
    let scale = f64::from(pango::SCALE);
    for run in shortcut_runs(text) {
        for key in run.keys {
            let start = layout.index_to_pos(key.start as i32);
            let end = layout.index_to_pos((key.end - 1) as i32);
            let x = f64::from(start.x()) / scale + 1.0;
            let y = f64::from(start.y()) / scale;
            let width = f64::from(end.x() + end.width() - start.x()) / scale - 2.0;
            let height = f64::from(start.height()) / scale;
            cr.set_source_rgb(0.11, 0.125, 0.145);
            rounded_rect(cr, x, y + 0.5, width, height - 1.0, 3.5);
            let _ = cr.fill_preserve();
            cr.set_source_rgb(0.28, 0.31, 0.35);
            cr.set_line_width(0.8);
            let _ = cr.stroke();
        }
    }
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width * 0.5).min(height * 0.5).max(0.0);
    cr.new_sub_path();
    cr.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    #[test]
    fn only_shortcut_keycaps_receive_decorations() {
        assert!(shortcut_runs("File [Meta][Ctrl][.] — обычный текст").is_empty());
        let text = crate::shortcuts::summary(
            &std::collections::BTreeMap::from([("delivery".to_owned(), "Meta+Ctrl+.".to_owned())]),
            Locale::En,
        );
        let runs = shortcut_runs(&text);
        assert_eq!(runs.len(), 1);
        assert_eq!(
            &text[runs[0].icon.clone()],
            crate::shortcuts::ACTIONS
                .iter()
                .find(|action| action.id == "delivery")
                .unwrap()
                .icon
        );
        let labels: Vec<_> = runs[0].keys.iter().map(|key| &text[key.clone()]).collect();
        assert_eq!(labels, ["[Meta]", "[Ctrl]", "[.]"]);
    }

    #[test]
    fn shortcut_icons_render_without_missing_glyphs_or_clipping() {
        let bindings = crate::shortcuts::ACTIONS
            .iter()
            .map(|action| (action.id.to_owned(), "Meta+Ctrl+Shift+Z".to_owned()))
            .collect();
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        for locale in [Locale::En, Locale::Ar] {
            let shortcuts = crate::shortcuts::summary(&bindings, locale);
            let plan = plan_with_shortcuts(
                OverlayPrimary::Recording,
                None,
                &[0.5; 10],
                RecordingIndicators::default(),
                locale,
                &shortcuts,
                true,
            )
            .unwrap();
            let layout = build_layout(&cr, &plan.text, locale, plan.width as i32 - 44);
            assert_eq!(layout.unknown_glyphs_count(), 0);
            for run in shortcut_runs(&plan.text) {
                for key in run.keys {
                    let start = layout.index_to_pos(key.start as i32);
                    let end = layout.index_to_pos((key.end - 1) as i32);
                    assert_eq!(start.y(), end.y(), "keycap split across lines");
                    assert!(
                        start.x() >= 0 && end.x() + end.width() <= layout.width(),
                        "keycap outside tooltip"
                    );
                }
            }
            assert!(
                layout.pixel_size().1 + 26 <= plan.height as i32,
                "shortcut rows clipped"
            );
        }
    }

    #[test]
    fn arabic_font_and_bidi_render_without_missing_glyphs() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let layout = build_layout(&cr, "جارٍ تحميل النموذج — model-q8.gguf", Locale::Ar, 516);
        assert_eq!(layout.unknown_glyphs_count(), 0);
        assert_eq!(layout.context().base_dir(), pango::Direction::Rtl);
        let plan = plan_for_locale(
            OverlayPrimary::Transcribing,
            Some("النموذج جاهز"),
            &[0.0; 10],
            RecordingIndicators::default(),
            Locale::Ar,
        )
        .unwrap();
        assert!(plan.text.contains("جارٍ التفريغ"));
        assert!(plan.width <= MAX_WIDTH && plan.height <= MAX_HEIGHT);
        assert!(render_surface(&plan).is_some());
    }
}
