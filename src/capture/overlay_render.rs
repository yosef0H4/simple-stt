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
pub const MAX_HEIGHT: u32 = 220;

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
    let text = render_overlay_text_with_locale(primary, notice, levels, indicators, locale)
        .replace("\r\n", "\n");
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

    // Text.
    let layout = build_layout(&cr, &plan.text, plan.locale, width - 44);
    cr.move_to(
        if plan.locale == Locale::Ar {
            PAD_RIGHT
        } else {
            TEXT_X
        },
        TEXT_Y,
    );
    cr.set_source_rgba(0.94, 0.96, 0.98, 1.0);
    pangocairo::functions::show_layout(&cr, &layout);

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
    if locale == Locale::Ar {
        let attrs = pango::AttrList::new();
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
        layout.set_attributes(Some(&attrs));
    }
    layout
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
