//! Wayland-free rendering for the Linux overlay.
//!
//! Renders the overlay text in a single Pango (Sans) layout — the original
//! look — and sizes the surface to fit the text so the panel grows/shrinks with
//! its content (dynamic, tooltip-style UI).

use super::overlay_model::{render_overlay_text, OverlayPrimary, VisualizerLevels};

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
) -> Option<LayoutPlan> {
    let text = render_overlay_text(primary, notice, levels).replace("\r\n", "\n");
    if text.trim().is_empty() {
        return None;
    }

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    let layout = build_layout(&cr, &text);
    let (w, h) = layout.pixel_size();

    let width = ((TEXT_X + f64::from(w) + PAD_RIGHT).ceil() as u32).clamp(MIN_WIDTH, MAX_WIDTH);
    let height = ((TEXT_Y + f64::from(h) + PAD_BOTTOM).ceil() as u32).clamp(MIN_HEIGHT, MAX_HEIGHT);
    Some(LayoutPlan {
        width,
        height,
        signature: text.clone(),
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
    rounded_rect(&cr, ACCENT_X, 13.0, ACCENT_W, f64::from(height) - 26.0, 2.0);
    let _ = cr.fill();

    // Text.
    let layout = build_layout(&cr, &plan.text);
    cr.move_to(TEXT_X, TEXT_Y);
    cr.set_source_rgba(0.94, 0.96, 0.98, 1.0);
    pangocairo::functions::show_layout(&cr, &layout);

    drop(layout);
    drop(cr);
    Some(surface)
}

fn build_layout(cr: &cairo::Context, text: &str) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_text(text);
    // Allow long notices to wrap rather than overflow; the surface is sized to
    // the resulting (possibly smaller) extent.
    layout.set_width((MAX_WIDTH as i32 - 44) * pango::SCALE);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_spacing(LINE_SPACING_PX * pango::SCALE);
    layout.set_font_description(Some(&pango::FontDescription::from_string(FONT)));
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
