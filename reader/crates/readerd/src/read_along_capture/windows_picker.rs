//! Capture before taking focus and present each repaint as one completed frame.
use super::{windows_graphics, Rect, WindowId, WindowSnapshot};
use anyhow::{ensure, Context, Result};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use reader_core::local_ipc::{WINDOWS_PICKER_CLASS, WINDOWS_TOOLTIP_PROPERTY};
use std::cell::UnsafeCell;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmFlush, DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, VK_ESCAPE, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

struct DpiContext(DPI_AWARENESS_CONTEXT);

impl DpiContext {
    fn physical_pixels() -> Self {
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for DpiContext {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { SetThreadDpiAwarenessContext(self.0) };
        }
    }
}

struct ScreenDc(HDC);

impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe { ReleaseDC(null_mut(), self.0) };
    }
}

struct Bitmap {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    pixels: *mut u8,
    width: u32,
    height: u32,
}

impl Bitmap {
    fn new(screen: HDC, width: i32, height: i32) -> Result<Self> {
        ensure!(width > 0 && height > 0, "Empty screenshot bitmap");
        let dc = unsafe { CreateCompatibleDC(screen) };
        ensure!(
            !dc.is_null(),
            "Could not allocate a screenshot device context"
        );
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut pixels = null_mut();
        let bitmap =
            unsafe { CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0) };
        if bitmap.is_null() || pixels.is_null() {
            unsafe { DeleteDC(dc) };
            anyhow::bail!("Could not allocate the screenshot bitmap");
        }
        let previous = unsafe { SelectObject(dc, bitmap) };
        Ok(Self {
            dc,
            bitmap,
            previous,
            pixels: pixels.cast(),
            width: width as u32,
            height: height as u32,
        })
    }

    fn save_region(&self, region: Rect, path: &Path) -> Result<()> {
        region.at_window(Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        })?;
        unsafe { GdiFlush() };
        let mut rgb = Vec::with_capacity(region.width as usize * region.height as usize * 3);
        for row in region.y as usize..region.bottom() as usize {
            let offset = (row * self.width as usize + region.x as usize) * 4;
            let pixels = unsafe {
                std::slice::from_raw_parts(self.pixels.add(offset), region.width as usize * 4)
            };
            for pixel in pixels.chunks_exact(4) {
                rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }
        }
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        PngEncoder::new(file).write_image(
            &rgb,
            region.width,
            region.height,
            ExtendedColorType::Rgb8,
        )?;
        Ok(())
    }

    fn from_frame(screen: HDC, frame: windows_graphics::Frame) -> Result<Self> {
        let bitmap = Self::new(screen, frame.width as i32, frame.height as i32)?;
        unsafe {
            std::ptr::copy_nonoverlapping(frame.pixels.as_ptr(), bitmap.pixels, frame.pixels.len())
        };
        Ok(bitmap)
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

struct PickerWindow(HWND);

impl Drop for PickerWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

#[derive(Default)]
struct Picker {
    screenshot: Option<Bitmap>,
    resized_preview: Option<Bitmap>,
    frame: Option<Bitmap>,
    fullscreen_monitor: HMONITOR,
    anchor: Option<POINT>,
    selection: Option<RECT>,
    finished: bool,
}

fn rectangle(a: POINT, b: POINT) -> RECT {
    RECT {
        left: a.x.min(b.x),
        top: a.y.min(b.y),
        right: a.x.max(b.x),
        bottom: a.y.max(b.y),
    }
}

fn redraw(hwnd: HWND, rect: Option<RECT>) {
    if let Some(mut area) = rect {
        // Include the outline and the pixel at the moving edge.
        area.right += 1;
        area.bottom += 1;
        unsafe { InvalidateRect(hwnd, &area, 0) };
    }
}

fn mouse_position(lparam: LPARAM) -> POINT {
    POINT {
        x: i32::from(lparam as i16),
        y: i32::from((lparam >> 16) as i16),
    }
}

fn paint(hwnd: HWND, picker: &Picker) {
    let mut paint = PAINTSTRUCT::default();
    let dest = unsafe { BeginPaint(hwnd, &mut paint) };
    let preview = picker
        .resized_preview
        .as_ref()
        .or(picker.screenshot.as_ref());
    if let (Some(screenshot), Some(frame)) = (preview, &picker.frame) {
        let area = paint.rcPaint;
        let (x, y) = (area.left, area.top);
        let (width, height) = (area.right - x, area.bottom - y);
        if width > 0 && height > 0 {
            unsafe {
                PatBlt(frame.dc, x, y, width, height, BLACKNESS);
                AlphaBlend(
                    frame.dc,
                    x,
                    y,
                    width,
                    height,
                    screenshot.dc,
                    x,
                    y,
                    width,
                    height,
                    BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 155,
                        AlphaFormat: 0,
                    },
                );
                if let Some(r) = picker.selection {
                    let left = r.left.max(area.left);
                    let top = r.top.max(area.top);
                    let right = r.right.min(area.right);
                    let bottom = r.bottom.min(area.bottom);
                    if right > left && bottom > top {
                        BitBlt(
                            frame.dc,
                            left,
                            top,
                            right - left,
                            bottom - top,
                            screenshot.dc,
                            left,
                            top,
                            SRCCOPY,
                        );
                        FrameRect(frame.dc, &r, GetStockObject(WHITE_BRUSH).cast());
                    }
                }
                BitBlt(dest, x, y, width, height, frame.dc, x, y, SRCCOPY);
            }
        }
    }
    unsafe { EndPaint(hwnd, &paint) };
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    let picker = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Picker;
    if picker.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd, unsafe { &*picker });
            0
        }
        WM_LBUTTONDOWN => {
            unsafe {
                (*picker).anchor = Some(mouse_position(lparam));
                SetCapture(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some(anchor) = unsafe { (*picker).anchor } {
                let area = rectangle(anchor, mouse_position(lparam));
                let old = unsafe { (*picker).selection.replace(area) };
                redraw(hwnd, old);
                redraw(hwnd, Some(area));
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some(anchor) = unsafe { (*picker).anchor.take() } {
                unsafe {
                    (*picker).selection = Some(rectangle(anchor, mouse_position(lparam)));
                    (*picker).finished = true;
                    ReleaseCapture();
                }
            }
            0
        }
        WM_DISPLAYCHANGE
            if !unsafe { (*picker).fullscreen_monitor }.is_null()
                && unsafe { (*picker).anchor }.is_none()
                && !unsafe { (*picker).finished } =>
        {
            let resized = unsafe { (*picker).resize_preview() };
            if let Ok(area) = resized {
                unsafe {
                    SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        area.x,
                        area.y,
                        area.width as i32,
                        area.height as i32,
                        SWP_NOACTIVATE,
                    );
                    InvalidateRect(hwnd, null(), 0);
                }
            } else {
                unsafe { (*picker).finished = true };
            }
            0
        }
        WM_KILLFOCUS
            if !unsafe { (*picker).fullscreen_monitor }.is_null()
                && unsafe { (*picker).anchor }.is_none() =>
        {
            // Windows can change focus while leaving exclusive presentation.
            // The saved frame is safe to show until selection starts.
            0
        }
        WM_CLOSE | WM_CANCELMODE | WM_DISPLAYCHANGE | WM_KILLFOCUS | WM_CAPTURECHANGED => {
            unsafe {
                if !(*picker).finished {
                    (*picker).selection = None;
                    (*picker).finished = true;
                }
            }
            0
        }
        WM_KEYDOWN if wparam == VK_ESCAPE as usize => {
            unsafe {
                (*picker).selection = None;
                (*picker).finished = true;
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

impl Picker {
    fn resize_preview(&mut self) -> Result<Rect> {
        let mut area = monitor_rect(self.fullscreen_monitor).context("Display is unavailable")?;
        let capture = self
            .screenshot
            .as_ref()
            .context("Screenshot is unavailable")?;
        let scale = (area.width as f64 / capture.width as f64)
            .min(area.height as f64 / capture.height as f64)
            .min(1.0);
        area.width = (capture.width as f64 * scale).round().max(1.0) as u32;
        area.height = (capture.height as f64 * scale).round().max(1.0) as u32;
        self.resized_preview = if area.width == capture.width && area.height == capture.height {
            None
        } else {
            let resized = Bitmap::new(capture.dc, area.width as i32, area.height as i32)?;
            ensure!(
                unsafe {
                    StretchBlt(
                        resized.dc,
                        0,
                        0,
                        area.width as i32,
                        area.height as i32,
                        capture.dc,
                        0,
                        0,
                        capture.width as i32,
                        capture.height as i32,
                        SRCCOPY,
                    ) != 0
                },
                "Could not resize the screenshot preview"
            );
            Some(resized)
        };
        Ok(area)
    }

    fn capture_pixels(&self, selection: RECT, width: u32, height: u32) -> RECT {
        let (preview_width, preview_height) = self
            .resized_preview
            .as_ref()
            .map_or((width, height), |preview| (preview.width, preview.height));
        let scale = |coord: i32, from: u32, to: u32, end: bool| {
            let value = u64::from(coord.clamp(0, from as i32) as u32) * u64::from(to);
            let from = u64::from(from);
            (if end {
                value.div_ceil(from)
            } else {
                value / from
            }) as i32
        };
        RECT {
            left: scale(selection.left, preview_width, width, false),
            right: scale(selection.right, preview_width, width, true),
            top: scale(selection.top, preview_height, height, false),
            bottom: scale(selection.bottom, preview_height, height, true),
        }
    }
}

fn hide_tooltips(current: &dyn Fn() -> bool) -> Result<()> {
    unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> BOOL {
        if unsafe {
            !GetPropW(hwnd, WINDOWS_TOOLTIP_PROPERTY).is_null() && IsWindowVisible(hwnd) != 0
        } {
            unsafe { (&mut *(data as *mut Vec<HWND>)).push(hwnd) };
        }
        1
    }
    let mut tooltips = Vec::<HWND>::new();
    unsafe { EnumWindows(Some(find), &mut tooltips as *mut Vec<HWND> as LPARAM) };
    for &hwnd in &tooltips {
        unsafe { ShowWindowAsync(hwnd, SW_HIDE) };
    }
    let started = Instant::now();
    while tooltips
        .iter()
        .any(|&hwnd| unsafe { IsWindowVisible(hwnd) != 0 })
    {
        ensure!(current(), "screen-region selection canceled");
        ensure!(
            started.elapsed() < Duration::from_millis(150),
            "The Simple STT tooltip did not hide"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    if !tooltips.is_empty() {
        unsafe { DwmFlush() };
    }
    Ok(())
}

fn win_rect(bounds: RECT) -> Rect {
    Rect {
        x: bounds.left,
        y: bounds.top,
        width: (bounds.right - bounds.left).max(0) as u32,
        height: (bounds.bottom - bounds.top).max(0) as u32,
    }
}

fn monitor_rect(monitor: HMONITOR) -> Option<Rect> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    (unsafe { GetMonitorInfoW(monitor, &mut info) } != 0).then(|| win_rect(info.rcMonitor))
}

fn visible_bounds(hwnd: HWND, fallback: Rect) -> Rect {
    let mut bounds = RECT::default();
    let status = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut bounds as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if status >= 0 {
        win_rect(bounds)
    } else {
        fallback
    }
}

fn snapshot(hwnd: HWND) -> Option<WindowSnapshot> {
    let mut process_id = 0;
    if unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) } == 0 {
        return None;
    }
    inspect(&WindowId::Windows {
        hwnd: hwnd as isize,
        process_id,
    })
}

fn fullscreen_output(hwnd: HWND, window: &WindowSnapshot) -> Option<(HMONITOR, Rect)> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) };
    let area = monitor_rect(monitor)?;
    (!window.minimized && visible_bounds(hwnd, window.bounds).contains(area))
        .then_some((monitor, area))
}

fn capture_desktop(screen: HDC, area: Rect) -> Result<Bitmap> {
    let bitmap = Bitmap::new(screen, area.width as i32, area.height as i32)?;
    ensure!(
        unsafe {
            BitBlt(
                bitmap.dc,
                0,
                0,
                area.width as i32,
                area.height as i32,
                screen,
                area.x,
                area.y,
                SRCCOPY | CAPTUREBLT,
            ) != 0
        },
        "Windows could not capture this desktop"
    );
    Ok(bitmap)
}

fn capture_monitor(
    screen: HDC,
    monitor: HMONITOR,
    area: Rect,
    current: &dyn Fn() -> bool,
    window: Option<(HWND, &WindowSnapshot)>,
) -> Result<Bitmap> {
    if let Some((hwnd, window)) = window {
        ensure!(
            inspect(&window.id).is_some(),
            "The fullscreen window is unavailable"
        );
        if let Ok((bitmap, _)) = capture_window_graphics(screen, hwnd, window, area, |request| {
            windows_graphics::isolated_frame(request, current)
        }) {
            ensure!(
                inspect(&window.id).is_some(),
                "The fullscreen window is unavailable"
            );
            tracing::info!(
                stage = "wgc_picker_capture",
                "Captured the fullscreen window"
            );
            return Ok(bitmap);
        }
    }
    let request = windows_graphics::FrameRequest {
        target: windows_graphics::CaptureTarget::Monitor(monitor as isize),
        bounds: [area; 2],
        region: area,
    };
    if let Ok(frame) = windows_graphics::isolated_frame(&request, current) {
        ensure!(current(), "The capture target changed");
        tracing::info!(stage = "wgc_picker_capture", "Captured the monitor");
        return Bitmap::from_frame(screen, frame);
    }
    ensure!(current(), "The capture target changed");
    let bitmap = capture_desktop(screen, area)?;
    ensure!(current(), "The capture target changed");
    Ok(bitmap)
}

pub(super) fn select(path: &Path, current: &dyn Fn() -> bool) -> Result<(WindowSnapshot, Rect)> {
    let _dpi = DpiContext::physical_pixels();
    hide_tooltips(current)?;
    ensure!(current(), "screen-region selection canceled");
    let screen = ScreenDc(unsafe { GetDC(null_mut()) });
    ensure!(!screen.0.is_null(), "Could not access the desktop");
    let foreground = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) };
    let fullscreen = snapshot(foreground).and_then(|window| {
        fullscreen_output(foreground, &window).map(|(monitor, area)| (window, monitor, area))
    });
    let (area, screenshot) = if let Some((window, monitor, area)) = &fullscreen {
        let stable = || {
            (unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) }) == foreground
                && inspect(&window.id).as_ref() == Some(window)
        };
        let screenshot = capture_monitor(
            screen.0,
            *monitor,
            *area,
            &stable,
            Some((foreground, window)),
        )?;
        (*area, screenshot)
    } else {
        let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
        ensure!(width > 0 && height > 0, "The desktop is unavailable");
        let area = Rect {
            x: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
            y: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
            width: width as u32,
            height: height as u32,
        };
        let monitor = unsafe {
            MonitorFromPoint(
                POINT {
                    x: area.x,
                    y: area.y,
                },
                MONITOR_DEFAULTTONULL,
            )
        };
        let screenshot = if monitor_rect(monitor) == Some(area) {
            capture_monitor(screen.0, monitor, area, current, None)?
        } else {
            // A Windows Graphics Capture item represents one monitor.
            capture_desktop(screen.0, area)?
        };
        (area, screenshot)
    };
    let Rect {
        x,
        y,
        width,
        height,
    } = area;
    let frame = Bitmap::new(screen.0, width as i32, height as i32)?;
    drop(screen);
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let instance = unsafe { GetModuleHandleW(null()) };
    ensure!(
        *REGISTERED.get_or_init(|| {
            let class = WNDCLASSW {
                hInstance: instance,
                lpszClassName: WINDOWS_PICKER_CLASS,
                lpfnWndProc: Some(window_proc),
                hCursor: unsafe { LoadCursorW(null_mut(), IDC_CROSS) },
                ..Default::default()
            };
            unsafe { RegisterClassW(&class) != 0 }
        }),
        "Could not register the screen-region picker"
    );
    // Window messages use this state on the calling thread. The window is
    // destroyed before the allocation is freed.
    let state = Box::new(UnsafeCell::new(Picker {
        screenshot: Some(screenshot),
        frame: Some(frame),
        fullscreen_monitor: fullscreen
            .as_ref()
            .map_or(null_mut(), |(_, monitor, _)| *monitor),
        ..Default::default()
    }));
    let picker = state.get();
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            WINDOWS_PICKER_CLASS,
            null(),
            WS_POPUP,
            x,
            y,
            width as i32,
            height as i32,
            null_mut(),
            null_mut(),
            instance,
            picker.cast(),
        )
    };
    ensure!(!hwnd.is_null(), "Could not open the screen-region picker");
    let _window = PickerWindow(hwnd);
    ensure!(current(), "screen-region selection canceled");
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        UpdateWindow(hwnd);
    }
    let mut message = MSG::default();
    let mut escape_was_down = unsafe { GetAsyncKeyState(VK_ESCAPE as i32) } < 0;
    while !unsafe { (*picker).finished } && current() {
        // A background daemon cannot always claim the foreground. Escape
        // still cancels the visible picker before its first mouse click.
        let escape_down = unsafe { GetAsyncKeyState(VK_ESCAPE as i32) } < 0;
        if escape_down && !escape_was_down {
            anyhow::bail!("screen-region selection canceled");
        }
        escape_was_down = escape_down;
        while !unsafe { (*picker).finished }
            && unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0
        {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if !unsafe { (*picker).finished } {
            unsafe { MsgWaitForMultipleObjects(0, null(), 0, 10, QS_ALLINPUT) };
        }
    }
    unsafe { ShowWindow(hwnd, SW_HIDE) };
    ensure!(current(), "screen-region selection canceled");
    let picker = unsafe { &*picker };
    let area = picker
        .selection
        .context("screen-region selection canceled")?;
    let area = picker.capture_pixels(area, width, height);
    let crop_width = area.right - area.left;
    let crop_height = area.bottom - area.top;
    ensure!(
        crop_width >= 2 && crop_height >= 2,
        "Select a larger region"
    );
    ensure!(
        i64::from(crop_width) * i64::from(crop_height) <= 20_000_000,
        "The selected region is too large"
    );
    let region = Rect {
        x: x + area.left,
        y: y + area.top,
        width: crop_width as u32,
        height: crop_height as u32,
    };
    let window = if let Some((window, _, _)) = fullscreen {
        // The game may minimize after the picker took focus. The selected
        // pixels and their location still belong to the pre-picker window.
        window
    } else {
        let target = unsafe {
            WindowFromPoint(POINT {
                x: region.x + crop_width / 2,
                y: region.y + crop_height / 2,
            })
        };
        let root = unsafe { GetAncestor(target, GA_ROOT) };
        [target, root]
            .into_iter()
            .filter_map(snapshot)
            .find(|window| !window.minimized && window.bounds.contains(region))
            .context("The selected region must fit within one window")?
    };
    let screenshot = picker
        .screenshot
        .as_ref()
        .context("The screenshot is unavailable")?;
    screenshot.save_region(
        Rect {
            x: area.left,
            y: area.top,
            ..region
        },
        path,
    )?;
    Ok((window, region))
}

/// Runs in a short-lived child so a stalled capture cannot hold the controller.
pub(super) fn capture_window(id: &WindowId, region: Rect, path: &Path) -> Result<()> {
    let WindowId::Windows { hwnd, .. } = *id else {
        anyhow::bail!("target is not a Windows window");
    };
    let _dpi = DpiContext::physical_pixels();
    let window = inspect(id).context("The selected window is unavailable")?;
    ensure!(!window.minimized, "The selected window is minimized");
    let absolute = region.at_window(window.bounds)?;
    let screen = ScreenDc(unsafe { GetDC(null_mut()) });
    ensure!(!screen.0.is_null(), "Screen capture is unavailable");
    let hwnd = hwnd as HWND;
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let root_window = snapshot(root).context("The selected window is unavailable")?;
    let (bitmap, crop) = match capture_window_graphics(
        screen.0,
        root,
        &root_window,
        absolute,
        windows_graphics::graphics_frame,
    ) {
        Ok(capture) => capture,
        Err(_) => {
            if let Some((_, display)) = fullscreen_output(root, &root_window) {
                ensure!(
                    visible_foreground_region(root, absolute),
                    "Focus the fullscreen window and uncover the selected region"
                );
                let crop = absolute.relative_to(display)?;
                let bitmap = capture_desktop(screen.0, display)?;
                ensure!(
                    visible_foreground_region(root, absolute),
                    "The fullscreen window changed during capture"
                );
                (bitmap, crop)
            } else {
                (capture_print_window(screen.0, hwnd, window.bounds)?, region)
            }
        }
    };
    ensure!(
        inspect(id).as_ref() == Some(&window)
            && inspect(&root_window.id).as_ref() == Some(&root_window),
        "The selected window changed during capture"
    );
    bitmap.save_region(crop, path)
}

fn capture_window_graphics(
    screen: HDC,
    hwnd: HWND,
    window: &WindowSnapshot,
    region: Rect,
    capture: impl FnOnce(&windows_graphics::FrameRequest) -> Result<windows_graphics::Frame>,
) -> Result<(Bitmap, Rect)> {
    let request = windows_graphics::FrameRequest {
        target: windows_graphics::CaptureTarget::Window(hwnd as isize),
        bounds: [visible_bounds(hwnd, window.bounds), window.bounds],
        region,
    };
    let frame = capture(&request)?;
    ensure!(frame.has_color(), "The window returned an empty image");
    let crop = Rect {
        x: 0,
        y: 0,
        width: frame.width,
        height: frame.height,
    };
    Ok((Bitmap::from_frame(screen, frame)?, crop))
}

fn capture_print_window(screen: HDC, hwnd: HWND, bounds: Rect) -> Result<Bitmap> {
    let bitmap = Bitmap::new(screen, bounds.width as i32, bounds.height as i32)?;
    const RENDER_FULL_CONTENT: u32 = 2;
    ensure!(
        unsafe { PrintWindow(hwnd, bitmap.dc, RENDER_FULL_CONTENT) } != 0,
        "The selected window could not be captured"
    );
    Ok(bitmap)
}

fn visible_foreground_region(hwnd: HWND, region: Rect) -> bool {
    if unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) } != hwnd {
        return false;
    }
    let mut above = unsafe { GetWindow(hwnd, GW_HWNDPREV) };
    while !above.is_null() {
        if unsafe { IsWindowVisible(above) != 0 && IsIconic(above) == 0 } && !cloaked(above) {
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(above, &mut rect) } != 0
                && i64::from(rect.left) < region.right()
                && i64::from(rect.top) < region.bottom()
                && rect.right > region.x
                && rect.bottom > region.y
            {
                return false;
            }
        }
        above = unsafe { GetWindow(above, GW_HWNDPREV) };
    }
    true
}

fn cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let status = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    status >= 0 && cloaked != 0
}

pub(super) fn advance_space(id: &WindowId, region: Rect, current: &dyn Fn() -> bool) -> Result<()> {
    let WindowId::Windows { hwnd, .. } = *id else {
        anyhow::bail!("target is not a Windows window");
    };
    let hwnd = hwnd as HWND;
    let window = inspect(id).context("The selected window is unavailable")?;
    if window.minimized || unsafe { GetForegroundWindow() != GetAncestor(hwnd, GA_ROOT) } {
        return Err(super::FocusChanged.into());
    }
    region.at_window(window.bounds)?;
    ensure!(current(), "Read-along action canceled");
    ensure!(
        unsafe { PostMessageW(hwnd, WM_KEYDOWN, VK_SPACE as WPARAM, 0) } != 0,
        "Could not send Space to the selected window"
    );
    ensure!(
        unsafe { PostMessageW(hwnd, WM_KEYUP, VK_SPACE as WPARAM, 0) } != 0,
        "Could not release Space in the selected window"
    );
    Ok(())
}

pub(super) fn inspect(id: &WindowId) -> Option<WindowSnapshot> {
    let WindowId::Windows { hwnd, process_id } = *id else {
        return None;
    };
    let _dpi = DpiContext::physical_pixels();
    let hwnd = hwnd as HWND;
    let mut pid = 0;
    let mut bounds = RECT::default();
    if unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid) == 0
            || pid != process_id
            || GetWindowRect(hwnd, &mut bounds) == 0
    } {
        return None;
    }
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    Some(WindowSnapshot {
        id: id.clone(),
        bounds: Rect {
            x: bounds.left,
            y: bounds.top,
            width: width.max(0) as u32,
            height: height.max(0) as u32,
        },
        minimized: width <= 0 || height <= 0 || unsafe { IsIconic(hwnd) != 0 },
    })
}
