//! A single Windows Graphics Capture frame from a window or monitor.
use super::{private_capture_path, Rect};
use anyhow::{ensure, Context, Result};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows::core::{factory, Interface};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::{Common::*, IDXGIAdapter, IDXGIDevice};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

const FRAME_WAIT: Duration = Duration::from_millis(200);
const MAX_PIXELS: u64 = 40_000_000;

pub(super) struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Frame {
    pub fn has_color(&self) -> bool {
        self.pixels
            .chunks_exact(4)
            .any(|pixel| pixel[..3] != [0, 0, 0])
    }
}

struct GraphicsDevice {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

impl GraphicsDevice {
    fn new() -> Result<Self> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.context("D3D11 device unavailable")?;
        let context = context.context("D3D11 context unavailable")?;
        // The frame-pool thread and this thread share the immediate context.
        let multithread: ID3D11Multithread = context.cast()?;
        let _previous = unsafe { multithread.SetMultithreadProtected(true) };
        Ok(Self { device, context })
    }

    fn read(&self, texture: &ID3D11Texture2D, area: Rect) -> Result<Frame> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let Rect {
            x,
            y,
            width,
            height,
        } = area;
        ensure!(
            (Rect {
                x: 0,
                y: 0,
                width: desc.Width,
                height: desc.Height
            })
            .contains(area)
                && u64::from(width) * u64::from(height) <= MAX_PIXELS
                && desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM,
            "Unexpected capture texture size"
        );
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut staging = None;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let area = D3D11_BOX {
            left: x as u32,
            top: y as u32,
            right: x as u32 + width,
            bottom: y as u32 + height,
            back: 1,
            ..Default::default()
        };
        unsafe {
            self.device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))?
        };
        let staging = staging.context("Capture staging texture unavailable")?;
        let stride = width as usize * 4;
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        unsafe {
            self.context
                .CopySubresourceRegion(&staging, 0, 0, 0, 0, texture, 0, Some(&area));
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            if mapped.pData.is_null() || (mapped.RowPitch as usize) < stride {
                self.context.Unmap(&staging, 0);
                anyhow::bail!("The captured surface could not be read");
            }
            for row in 0..height as usize {
                let from = mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize);
                pixels.extend_from_slice(std::slice::from_raw_parts(from, stride));
            }
            self.context.Unmap(&staging, 0);
        }
        Ok(Frame {
            width,
            height,
            pixels,
        })
    }
}

struct Runtime;

impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(super) enum CaptureTarget {
    Window(isize),
    Monitor(isize),
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct FrameRequest {
    pub target: CaptureTarget,
    pub bounds: [Rect; 2],
    pub region: Rect,
}

pub(super) fn isolated_frame(request: &FrameRequest, current: &dyn Fn() -> bool) -> Result<Frame> {
    let path = private_capture_path()?.with_extension("bgra");
    let _remove = crate::privacy::TemporaryFile(path.clone());
    let args = serde_json::to_string(request)?;
    await_capture(
        Command::new(std::env::current_exe()?)
            .args(["capture-frame", &args])
            .arg(&path),
        current,
        Duration::from_secs(2),
    )?;
    let pixels = std::fs::read(path)?;
    let Rect { width, height, .. } = request.region;
    ensure!(
        pixels.len() as u64 == u64::from(width) * u64::from(height) * 4,
        "The capture frame was incomplete"
    );
    Ok(Frame {
        width,
        height,
        pixels,
    })
}

pub(super) fn await_capture(
    command: &mut Command,
    current: &dyn Fn() -> bool,
    timeout: Duration,
) -> Result<()> {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    ensure!(current(), "screen capture canceled");
    let mut child = command
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if !current() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            ensure!(current(), "screen capture canceled");
            anyhow::bail!("the selected window took too long to capture");
        }
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "the selected window could not be captured"
            );
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn graphics_frame(request: &FrameRequest) -> Result<Frame> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let _runtime = Runtime;
    ensure!(
        GraphicsCaptureSession::IsSupported()?,
        "Window graphics capture is unavailable"
    );
    let item: GraphicsCaptureItem = unsafe {
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        match request.target {
            CaptureTarget::Window(hwnd) => interop.CreateForWindow(HWND(hwnd as _))?,
            CaptureTarget::Monitor(monitor) => interop.CreateForMonitor(HMONITOR(monitor as _))?,
        }
    };
    let size = item.Size()?;
    ensure!(
        size.Width > 0
            && size.Height > 0
            && u64::from(size.Width as u32) * u64::from(size.Height as u32) <= MAX_PIXELS,
        "The window capture is too large or empty"
    );
    let graphics = GraphicsDevice::new()?;
    let dxgi: IDXGIDevice = graphics.device.cast()?;
    let device: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )?;
    let result = (|| {
        let session = pool.CreateCaptureSession(&item)?;
        // Older Windows builds may not expose the cursor setting.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let result = (|| {
            session.StartCapture()?;
            let frame = next_window_frame(&pool)?;
            let size = frame.ContentSize()?;
            let crop = request
                .bounds
                .into_iter()
                .find(|bounds| {
                    bounds.width as i32 == size.Width && bounds.height as i32 == size.Height
                })
                .context("Window capture geometry changed")?;
            let relative = request.region.relative_to(crop)?;
            let dxgi: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = unsafe { dxgi.GetInterface()? };
            graphics.read(&texture, relative)
        })();
        let _ = session.Close();
        result
    })();
    let _ = pool.Close();
    result
}

fn next_window_frame(pool: &Direct3D11CaptureFramePool) -> Result<Direct3D11CaptureFrame> {
    let deadline = Instant::now() + FRAME_WAIT;
    loop {
        if let Ok(frame) = pool.TryGetNextFrame() {
            return Ok(frame);
        }
        ensure!(
            Instant::now() < deadline,
            "Timed out waiting for a window capture"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
