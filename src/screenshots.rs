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
    let original_cursor = set_cursor_for_overlay_screenshot();
    let overlay = crate::overlay::OverlayHandle::spawn()?;
    overlay.show(crate::input::foreground_window_id());
    for level in [0.18, 0.44, 0.82, 0.64, 0.92, 0.30, 0.86] {
        overlay.set_level(level);
        std::thread::sleep(Duration::from_millis(140));
    }
    let result = save_desktop_capture(output);
    overlay.hide();
    restore_cursor_after_overlay_screenshot(original_cursor);
    result
}

#[cfg(not(windows))]
fn save_runtime_overlay_desktop(_output: &Path) -> Result<()> {
    anyhow::bail!("desktop overlay capture is only implemented on Windows")
}

#[cfg(windows)]
fn save_desktop_capture(output: &Path) -> Result<()> {
    use anyhow::bail;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HBITMAP, HDC, HGDIOBJ, CAPTUREBLT, SRCCOPY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        let screen_dc: HDC = GetDC(0 as HWND);
        if screen_dc == null_mut() {
            bail!("GetDC failed for desktop capture");
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap: HBITMAP = CreateCompatibleBitmap(screen_dc, width, height);
        let old = SelectObject(mem_dc, bitmap as HGDIOBJ);
        let _ = BitBlt(
            mem_dc,
            0,
            0,
            width,
            height,
            screen_dc,
            0,
            0,
            SRCCOPY | CAPTUREBLT,
        );

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [zeroed_rgbquad(); 1],
        };
        let mut bytes = vec![0_u8; (width as usize) * (height as usize) * 4];
        let lines = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as u32,
            bytes.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old);
        DeleteObject(bitmap as _);
        DeleteDC(mem_dc);
        ReleaseDC(0 as HWND, screen_dc);

        if lines == 0 {
            bail!("GetDIBits failed for desktop capture");
        }

        for px in bytes.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(width as u32, height as u32, bytes)
                .context("building desktop screenshot image")?;
        img.save(output)
            .with_context(|| format!("writing {}", output.display()))
    }
}

#[cfg(windows)]
fn set_cursor_for_overlay_screenshot() -> Option<windows_sys::Win32::Foundation::POINT> {
    use windows_sys::Win32::Foundation::{HWND, POINT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN,
    };

    let mut original = POINT { x: 0, y: 0 };
    let captured = unsafe { GetCursorPos(&mut original) != 0 };
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let _ = HWND::default();
    unsafe {
        SetCursorPos(width / 2, (height as f32 * 0.62) as i32);
    }
    captured.then_some(original)
}

#[cfg(windows)]
fn restore_cursor_after_overlay_screenshot(point: Option<windows_sys::Win32::Foundation::POINT>) {
    if let Some(point) = point {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(point.x, point.y);
        }
    }
}

#[cfg(not(windows))]
fn save_desktop_capture(_output: &Path) -> Result<()> {
    anyhow::bail!("desktop capture is only implemented on Windows")
}

#[cfg(windows)]
const fn zeroed_rgbquad() -> windows_sys::Win32::Graphics::Gdi::RGBQUAD {
    windows_sys::Win32::Graphics::Gdi::RGBQUAD {
        rgbBlue: 0,
        rgbGreen: 0,
        rgbRed: 0,
        rgbReserved: 0,
    }
}
