//! Capture before taking focus and present each repaint as one completed frame.
use super::{Rect, WindowId, WindowSnapshot};
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
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
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
    pixels: *const u8,
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
    frame: Option<Bitmap>,
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

fn cursor_in_window(hwnd: HWND) -> POINT {
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point);
        ScreenToClient(hwnd, &mut point);
    }
    point
}

fn paint(hwnd: HWND, picker: &Picker) {
    let mut paint = PAINTSTRUCT::default();
    let dest = unsafe { BeginPaint(hwnd, &mut paint) };
    if let (Some(screenshot), Some(frame)) = (&picker.screenshot, &picker.frame) {
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
                (*picker).anchor = Some(cursor_in_window(hwnd));
                SetCapture(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some(anchor) = unsafe { (*picker).anchor } {
                let area = rectangle(anchor, cursor_in_window(hwnd));
                let old = unsafe { (*picker).selection.replace(area) };
                redraw(hwnd, old);
                redraw(hwnd, Some(area));
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some(anchor) = unsafe { (*picker).anchor.take() } {
                unsafe {
                    (*picker).selection = Some(rectangle(anchor, cursor_in_window(hwnd)));
                    (*picker).finished = true;
                    ReleaseCapture();
                }
            }
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

pub(super) fn select(path: &Path, current: &dyn Fn() -> bool) -> Result<(WindowSnapshot, Rect)> {
    let _dpi = DpiContext::physical_pixels();
    let (x, y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    ensure!(width > 0 && height > 0, "The desktop is unavailable");
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
    let state = Box::new(UnsafeCell::new(Picker::default()));
    let picker = state.get();
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            WINDOWS_PICKER_CLASS,
            null(),
            WS_POPUP,
            x,
            y,
            width,
            height,
            null_mut(),
            null_mut(),
            instance,
            picker.cast(),
        )
    };
    ensure!(!hwnd.is_null(), "Could not open the screen-region picker");
    let _window = PickerWindow(hwnd);
    hide_tooltips(current)?;
    let screen = ScreenDc(unsafe { GetDC(null_mut()) });
    ensure!(!screen.0.is_null(), "Could not access the desktop");
    let screenshot = Bitmap::new(screen.0, width, height)?;
    ensure!(
        unsafe {
            BitBlt(
                screenshot.dc,
                0,
                0,
                width,
                height,
                screen.0,
                x,
                y,
                SRCCOPY | CAPTUREBLT,
            ) != 0
        },
        "Windows could not capture this desktop"
    );
    let frame = Bitmap::new(screen.0, width, height)?;
    unsafe {
        (*picker).screenshot = Some(screenshot);
        (*picker).frame = Some(frame);
    }
    drop(screen);
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
    let area = RECT {
        left: area.left.clamp(0, width),
        right: area.right.clamp(0, width),
        top: area.top.clamp(0, height),
        bottom: area.bottom.clamp(0, height),
    };
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
    let target = unsafe {
        WindowFromPoint(POINT {
            x: region.x + crop_width / 2,
            y: region.y + crop_height / 2,
        })
    };
    let root = unsafe { GetAncestor(target, GA_ROOT) };
    let window = [target, root]
        .into_iter()
        .find_map(|hwnd| {
            let mut process_id = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
            inspect(&WindowId::Windows {
                hwnd: hwnd as isize,
                process_id,
            })
            .filter(|window| !window.minimized && window.bounds.contains(region))
        })
        .context("The selected region must fit within one window")?;
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

/// Runs in a short-lived child because PrintWindow can wait for the target app.
pub(super) fn capture_window(id: &WindowId, region: Rect, path: &Path) -> Result<()> {
    let WindowId::Windows { hwnd, .. } = *id else {
        anyhow::bail!("target is not a Windows window");
    };
    let _dpi = DpiContext::physical_pixels();
    let window = inspect(id).context("The selected window is unavailable")?;
    ensure!(!window.minimized, "The selected window is minimized");
    region.at_window(window.bounds)?;
    let screen = ScreenDc(unsafe { GetDC(null_mut()) });
    ensure!(!screen.0.is_null(), "Screen capture is unavailable");
    let bitmap = Bitmap::new(
        screen.0,
        window.bounds.width as i32,
        window.bounds.height as i32,
    )?;
    const RENDER_FULL_CONTENT: u32 = 2;
    ensure!(
        unsafe { PrintWindow(hwnd as HWND, bitmap.dc, RENDER_FULL_CONTENT) } != 0,
        "The selected window could not be captured"
    );
    ensure!(
        inspect(id).is_some_and(|current| !current.minimized && current.bounds == window.bounds),
        "The selected window changed during capture"
    );
    bitmap.save_region(region, path)
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
