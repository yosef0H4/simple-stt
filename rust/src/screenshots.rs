use anyhow::{Context, Result};
use crossbeam_channel::bounded;
use image::{ImageBuffer, Rgba};
use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::slint_ui::{RecordingOverlay, SettingsWindow};

#[cfg(windows)]
type DesktopCaptureError = Box<dyn std::error::Error + Send + Sync>;

#[cfg(windows)]
struct OneFrameCapture {
    output: PathBuf,
    started_at: std::time::Instant,
}

#[cfg(windows)]
impl windows_capture::capture::GraphicsCaptureApiHandler for OneFrameCapture {
    type Flags = PathBuf;
    type Error = DesktopCaptureError;

    fn new(
        ctx: windows_capture::capture::Context<Self::Flags>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            output: ctx.flags,
            started_at: std::time::Instant::now(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut windows_capture::frame::Frame,
        capture_control: windows_capture::graphics_capture_api::InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        if self.started_at.elapsed() >= Duration::from_millis(700) {
            frame.save_as_image(&self.output, windows_capture::encoder::ImageFormat::Png)?;
            capture_control.stop();
        }
        Ok(())
    }
}

#[cfg(windows)]
struct DesktopCaptureControl {
    inner: windows_capture::capture::CaptureControl<OneFrameCapture, DesktopCaptureError>,
}

#[derive(Debug, Clone, Copy)]
pub enum UiSurface {
    Settings,
    Overlay,
    OverlayDesktop,
}

pub fn save(surface: UiSurface, output: &Path, section: Option<&str>) -> Result<()> {
    if std::env::var_os("SLINT_BACKEND").is_none() {
        std::env::set_var("SLINT_BACKEND", "winit-software");
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    match surface {
        UiSurface::Settings => save_settings(output, section),
        UiSurface::Overlay => save_overlay(output),
        UiSurface::OverlayDesktop => save_overlay_desktop(output),
    }
}

fn save_settings(output: &Path, section: Option<&str>) -> Result<()> {
    let app = SettingsWindow::new().context("creating settings screenshot surface")?;
    crate::gui::configure_settings_for_screenshot(&app, section);
    app.show().context("showing settings screenshot surface")?;
    capture_after_event_loop_tick(&app, output.to_path_buf())
}

fn save_overlay(output: &Path) -> Result<()> {
    let app = RecordingOverlay::new().context("creating overlay screenshot surface")?;
    app.set_level(0.78);
    app.show().context("showing overlay screenshot surface")?;
    capture_after_event_loop_tick(&app, output.to_path_buf())
}

fn save_overlay_desktop(output: &Path) -> Result<()> {
    save_runtime_overlay_desktop(output)
}

fn save_window_snapshot(window: &slint::Window, output: &Path) -> Result<()> {
    let buffer = window.take_snapshot().context("taking Slint snapshot")?;
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(buffer.width(), buffer.height(), buffer.as_bytes().to_vec())
            .context("building screenshot image")?;
    img.save(output)
        .with_context(|| format!("writing {}", output.display()))
}

fn capture_after_event_loop_tick<T: ComponentHandle + 'static>(
    app: &T,
    output: PathBuf,
) -> Result<()> {
    let (tx, rx) = bounded(1);
    let weak = app.as_weak();
    slint::invoke_from_event_loop(move || {
        slint::Timer::single_shot(Duration::from_millis(150), move || {
            let result = weak
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("screenshot surface closed"))
                .and_then(|app| save_window_snapshot(app.window(), &output));
            let _ = tx.send(result);
            let _ = slint::quit_event_loop();
        });
    })
    .context("scheduling Slint screenshot")?;
    slint::run_event_loop().context("running Slint screenshot event loop")?;
    rx.recv().context("receiving screenshot result")?
}

#[cfg(windows)]
fn save_runtime_overlay_desktop(output: &Path) -> Result<()> {
    let overlay = crate::overlay::OverlayHandle::spawn()?;
    overlay.show(crate::input::foreground_window_id());
    let capture = start_desktop_capture(output)?;
    for level in [0.18, 0.44, 0.82, 0.64, 0.92, 0.30, 0.86] {
        overlay.set_level(level);
        std::thread::sleep(Duration::from_millis(140));
    }
    let result = finish_desktop_capture(capture, output);
    overlay.hide();
    result
}

#[cfg(not(windows))]
fn save_runtime_overlay_desktop(_output: &Path) -> Result<()> {
    anyhow::bail!("desktop overlay capture is only implemented on Windows")
}

#[cfg(windows)]
fn start_desktop_capture(output: &Path) -> Result<DesktopCaptureControl> {
    use windows_capture::capture::{CaptureControl, GraphicsCaptureApiError, GraphicsCaptureApiHandler};
    use windows_capture::monitor::Monitor;
    use windows_capture::settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    };

    let monitor = Monitor::primary().context("selecting primary monitor for desktop capture")?;
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Include,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        output.to_path_buf(),
    );
    let control: CaptureControl<OneFrameCapture, DesktopCaptureError> =
        OneFrameCapture::start_free_threaded(settings)
            .map_err(|error: GraphicsCaptureApiError<DesktopCaptureError>| {
                anyhow::anyhow!("starting Windows Graphics Capture failed: {error}")
            })?;
    Ok(DesktopCaptureControl { inner: control })
}

#[cfg(windows)]
fn finish_desktop_capture(capture: DesktopCaptureControl, output: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if output.exists() && output.metadata().map(|m| m.len()).unwrap_or(0) > 0 {
            return capture
                .inner
                .stop()
                .map_err(|error| anyhow::anyhow!("stopping Windows Graphics Capture failed: {error}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = capture.inner.stop();
    anyhow::bail!("Windows Graphics Capture did not produce a screenshot within timeout")
}

#[cfg(not(windows))]
fn save_desktop_capture(_output: &Path) -> Result<()> {
    anyhow::bail!("desktop capture is only implemented on Windows")
}
