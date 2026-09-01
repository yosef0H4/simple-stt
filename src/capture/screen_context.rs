use crate::cleanup::CleanupImage;
use crate::config::{ScreenshotConfig, ScreenshotScope};
use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::DynamicImage;
#[cfg(target_os = "linux")]
use image::ImageReader;
#[cfg(target_os = "linux")]
use std::io::Cursor;

pub fn capture(
    config: &ScreenshotConfig,
    target_window: Option<i64>,
) -> Result<Option<CleanupImage>> {
    if !config.enabled {
        return Ok(None);
    }
    #[cfg(windows)]
    let image = capture_windows(config, target_window)?;
    #[cfg(target_os = "linux")]
    let image = capture_linux(config)?;
    #[cfg(not(any(windows, target_os = "linux")))]
    let image: Option<DynamicImage> = None;
    image
        .map(|image| encode(image, config.max_edge_pixels, config.jpeg_quality))
        .transpose()
}

fn encode(image: DynamicImage, max_edge: u32, quality: u8) -> Result<CleanupImage> {
    let image = if image.width().max(image.height()) > max_edge {
        image.resize(max_edge, max_edge, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(&image)
        .context("encoding screenshot")?;
    Ok(CleanupImage {
        mime_type: "image/jpeg".into(),
        base64_data: STANDARD.encode(bytes),
    })
}

#[cfg(windows)]
fn capture_windows(
    config: &ScreenshotConfig,
    target_window: Option<i64>,
) -> Result<Option<DynamicImage>> {
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
        DIB_RGB_COLORS, SRCCOPY,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    let hwnd = target_window.unwrap_or_default() as HWND;
    if config.scope == ScreenshotScope::ActiveWindow {
        anyhow::ensure!(!hwnd.is_null(), "no active target window was supplied");
    }
    if !hwnd.is_null() {
        let mut identity = String::new();
        let title_len = unsafe { GetWindowTextLengthW(hwnd) };
        if title_len > 0 {
            let mut title = vec![0u16; title_len as usize + 1];
            let copied = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
            if copied > 0 {
                identity.push_str(&String::from_utf16_lossy(&title[..copied as usize]));
            }
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid != 0 {
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if !process.is_null() {
                let mut path = vec![0u16; 32_768];
                let mut length = path.len() as u32;
                if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) }
                    != 0
                {
                    identity.push(' ');
                    identity.push_str(&String::from_utf16_lossy(&path[..length as usize]));
                }
                unsafe { CloseHandle(process) };
            }
        }
        let identity = identity.to_ascii_lowercase();
        if config
            .excluded_apps
            .iter()
            .any(|name| identity.contains(&name.to_ascii_lowercase()))
        {
            return Ok(None);
        }
    }

    let (x, y, width, height) = if config.scope == ScreenshotScope::FullScreen {
        unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        }
    } else {
        let mut rect = RECT::default();
        anyhow::ensure!(
            unsafe { GetWindowRect(hwnd, &mut rect) } != 0,
            "could not locate target window"
        );
        (
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        )
    };
    anyhow::ensure!(width > 0 && height > 0, "screenshot area is empty");
    let screen_dc = unsafe { GetDC(std::ptr::null_mut()) };
    anyhow::ensure!(!screen_dc.is_null(), "GetDC failed");
    let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
    let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width, height) };
    if memory_dc.is_null() || bitmap.is_null() {
        unsafe {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !memory_dc.is_null() {
                DeleteDC(memory_dc);
            }
            ReleaseDC(std::ptr::null_mut(), screen_dc);
        }
        anyhow::bail!("creating screenshot buffer failed");
    }
    let previous = unsafe { SelectObject(memory_dc, bitmap) };
    let copied = unsafe {
        BitBlt(
            memory_dc,
            0,
            0,
            width,
            height,
            screen_dc,
            x,
            y,
            SRCCOPY | CAPTUREBLT,
        )
    };
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let rows = unsafe {
        GetDIBits(
            memory_dc,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        SelectObject(memory_dc, previous);
        DeleteObject(bitmap);
        DeleteDC(memory_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);
    }
    anyhow::ensure!(
        copied != 0 && rows == height,
        "copying screenshot pixels failed"
    );
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let image = image::RgbaImage::from_raw(width as u32, height as u32, pixels)
        .context("screenshot pixel dimensions were invalid")?;
    Ok(Some(DynamicImage::ImageRgba8(image)))
}

#[cfg(target_os = "linux")]
fn capture_linux(config: &ScreenshotConfig) -> Result<Option<DynamicImage>> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return capture_wayland_portal(config);
    }
    capture_x11(config)
}

#[cfg(target_os = "linux")]
fn capture_wayland_portal(config: &ScreenshotConfig) -> Result<Option<DynamicImage>> {
    use ashpd::desktop::screenshot::{AvailableTargets, Screenshot};
    let target = match config.scope {
        ScreenshotScope::ActiveWindow => AvailableTargets::ActiveWindow,
        ScreenshotScope::FullScreen => AvailableTargets::Screen,
    };
    let response = async_io::block_on(async {
        Screenshot::request()
            .interactive(true)
            .modal(true)
            .target(target)
            .send()
            .await?
            .response()
    });
    let response = match response {
        Ok(value) => value,
        Err(error) => {
            tracing::info!(%error, "Wayland screenshot request denied or cancelled; continuing text-only");
            return Ok(None);
        }
    };
    let uri = url::Url::parse(response.uri().as_str())
        .context("portal returned an invalid screenshot URI")?;
    let path = uri
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("portal screenshot was not a local file"))?;
    let bytes = std::fs::read(&path)
        .with_context(|| format!("reading portal screenshot {}", path.display()))?;
    let _ = std::fs::remove_file(&path);
    Ok(Some(
        ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()?
            .decode()?,
    ))
}

#[cfg(target_os = "linux")]
fn capture_x11(config: &ScreenshotConfig) -> Result<Option<DynamicImage>> {
    use std::process::Command;
    let active_window = Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .context("xdotool is required for X11 screen context")?;
    anyhow::ensure!(
        active_window.status.success(),
        "could not find the active X11 window"
    );
    let active_window = String::from_utf8(active_window.stdout)?.trim().to_owned();
    let window = if config.scope == ScreenshotScope::FullScreen {
        "root".to_owned()
    } else {
        active_window.clone()
    };
    let title = Command::new("xdotool")
        .args(["getwindowname", &active_window])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if config
        .excluded_apps
        .iter()
        .any(|name| title.contains(&name.to_ascii_lowercase()))
    {
        return Ok(None);
    }
    let output = Command::new("import")
        .args(["-window", &window, "png:-"])
        .output()
        .context("ImageMagick import is required for X11 screen context")?;
    anyhow::ensure!(output.status.success(), "X11 screenshot capture failed");
    Ok(Some(
        ImageReader::new(Cursor::new(output.stdout))
            .with_guessed_format()?
            .decode()?,
    ))
}
