use anyhow::{Context, Result};
use crossbeam_channel::bounded;
use image::{ImageBuffer, Rgba};
use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::slint_ui::{RecordingOverlay, SettingsWindow};

#[derive(Debug, Clone, Copy)]
pub enum UiSurface {
    Settings,
    Overlay,
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

fn save_window_snapshot(window: &slint::Window, output: &Path) -> Result<()> {
    let buffer = window.take_snapshot().context("taking Slint snapshot")?;
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(buffer.width(), buffer.height(), buffer.as_bytes().to_vec())
            .context("building screenshot image")?;
    img.save(output)
        .with_context(|| format!("writing {}", output.display()))
}
