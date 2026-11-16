//! Window-bound capture primitives for read-along.
//!
//! A selected region is stored relative to a native window. Every capture and
//! action revalidates that window before using its current screen position.
//! Platform adapters are intentionally kept behind `WindowBackend` so the
//! geometry and safety checks can be exercised without touching a desktop.

use anyhow::{bail, ensure, Result};
use std::path::PathBuf;

fn private_capture_path() -> Result<PathBuf> {
    let directory = reader_core::config::data_dir().join("capture");
    std::fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(directory.join(format!("read-along-{}.png", uuid::Uuid::new_v4())))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    pub fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    pub fn contains(self, other: Self) -> bool {
        self.width > 0
            && self.height > 0
            && other.width > 0
            && other.height > 0
            && i64::from(other.x) >= i64::from(self.x)
            && i64::from(other.y) >= i64::from(self.y)
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    pub fn relative_to(self, window: Self) -> Result<Self> {
        ensure!(
            window.contains(self),
            "selected region is outside its window"
        );
        let x = i64::from(self.x) - i64::from(window.x);
        let y = i64::from(self.y) - i64::from(window.y);
        ensure!(
            x <= i64::from(i32::MAX) && y <= i64::from(i32::MAX),
            "region offset is out of range"
        );
        Ok(Self {
            x: x as i32,
            y: y as i32,
            width: self.width,
            height: self.height,
        })
    }

    pub fn at_window(self, window: Self) -> Result<Self> {
        ensure!(
            window.width > 0 && window.height > 0,
            "window is minimized or empty"
        );
        let x = i64::from(window.x) + i64::from(self.x);
        let y = i64::from(window.y) + i64::from(self.y);
        ensure!(
            x >= i64::from(i32::MIN) && x <= i64::from(i32::MAX),
            "region x is out of range"
        );
        ensure!(
            y >= i64::from(i32::MIN) && y <= i64::from(i32::MAX),
            "region y is out of range"
        );
        let translated = Self {
            x: x as i32,
            y: y as i32,
            ..self
        };
        ensure!(
            window.contains(translated),
            "saved region no longer fits its window"
        );
        Ok(translated)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowId {
    X11(u32),
    KWin(String),
    #[cfg(windows)]
    Windows {
        hwnd: isize,
        process_id: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub id: WindowId,
    pub bounds: Rect,
    pub minimized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvanceAction {
    Key(String),
    Click { x: u32, y: u32 },
}

/// Platform boundary used by the native adapters and fake-backend tests.
pub trait WindowBackend {
    fn prepare_input(
        &mut self,
        _action: &AdvanceAction,
        _current: &dyn Fn() -> bool,
    ) -> Result<()> {
        Ok(())
    }
    fn input_ready(&mut self, _window: &WindowSnapshot) -> Result<bool> {
        Ok(true)
    }
    fn choose_window_and_region(
        &mut self,
        current: &dyn Fn() -> bool,
    ) -> Result<(WindowSnapshot, Rect)>;
    fn inspect(&mut self, id: &WindowId) -> Result<Option<WindowSnapshot>>;
    fn capture(
        &mut self,
        window: &WindowSnapshot,
        region: Rect,
        current: &dyn Fn() -> bool,
    ) -> Result<PathBuf>;
    fn advance(
        &mut self,
        window: &WindowSnapshot,
        region: Rect,
        action: &AdvanceAction,
        current: &dyn Fn() -> bool,
    ) -> Result<()>;
    fn supports_window_bound_input(&self) -> bool;
}

#[derive(Debug, Clone)]
pub struct Target {
    pub window: WindowId,
    /// Region coordinates in the selected window's client coordinate space.
    pub region: Rect,
}

impl Target {
    /// Whether this host exposes one of the window adapters used by Target.
    /// This is only an environment capability check; selection can still fail
    /// if the compositor or display becomes unavailable.
    pub fn supported_on_current_desktop() -> bool {
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                let desktop = std::env::var("XDG_CURRENT_DESKTOP")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                return desktop.contains("kde")
                    || std::env::var_os("KDE_SESSION_VERSION").is_some();
            }
            std::env::var_os("DISPLAY").is_some()
        }
        #[cfg(windows)]
        {
            true
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            false
        }
    }

    /// Select a window and drag a single region. The returned image is the
    /// first screenshot, ready to pass to OCR.
    pub fn select(current: impl Fn() -> bool) -> Result<(Self, PathBuf)> {
        let mut backend = native_backend()?;
        let (window, screen_region) = backend.choose_window_and_region(&current)?;
        ensure!(!window.minimized, "selected window is minimized");
        ensure!(current(), "read-along selection canceled");
        let relative = screen_region.relative_to(window.bounds)?;
        let image = backend.capture(&window, relative, &current)?;
        Ok((
            Self {
                window: window.id,
                region: relative,
            },
            image,
        ))
    }

    /// Capture the saved window-relative region after checking that the exact
    /// window still exists and the region remains within its current bounds.
    pub fn capture(&self, current: &dyn Fn() -> bool) -> Result<PathBuf> {
        let mut backend = native_backend()?;
        let window = self.live_window(backend.as_mut())?;
        ensure!(current(), "read-along capture canceled");
        backend.capture(&window, self.region, &current)
    }

    /// Apply a key or region-relative click only through an adapter that can
    /// address the selected native window directly.
    pub fn prepare_input(&self, action: &AdvanceAction, current: &dyn Fn() -> bool) -> Result<()> {
        let mut backend = native_backend()?;
        self.live_window(backend.as_mut())?;
        backend.prepare_input(action, current)
    }

    pub fn advance(
        &self,
        action: AdvanceAction,
        current: &dyn Fn() -> bool,
        waiting: &dyn Fn(bool),
    ) -> Result<()> {
        let mut backend = native_backend()?;
        ensure!(
            backend.supports_window_bound_input(),
            "window-bound input is unavailable on this desktop"
        );
        backend.prepare_input(&action, current)?;
        loop {
            ensure!(current(), "read-along action canceled");
            let window = backend
                .inspect(&self.window)?
                .ok_or_else(|| anyhow::anyhow!("selected window was closed"))?;
            ensure!(window.id == self.window, "selected window identity changed");
            if window.minimized || !backend.input_ready(&window)? {
                waiting(true);
                std::thread::sleep(std::time::Duration::from_millis(250));
                continue;
            }
            self.region.at_window(window.bounds)?;
            waiting(false);
            match backend.advance(&window, self.region, &action, current) {
                Err(error) if error.is::<FocusChanged>() => continue,
                result => return result,
            }
        }
    }

    fn live_window(&self, backend: &mut dyn WindowBackend) -> Result<WindowSnapshot> {
        let window = backend
            .inspect(&self.window)?
            .ok_or_else(|| anyhow::anyhow!("selected window was closed"))?;
        ensure!(!window.minimized, "selected window is minimized");
        ensure!(
            self.region.at_window(window.bounds).is_ok(),
            "saved region no longer fits the selected window"
        );
        ensure!(window.id == self.window, "selected window identity changed");
        Ok(window)
    }
}

#[derive(Debug)]
struct FocusChanged;
impl std::fmt::Display for FocusChanged {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("waiting for the selected reading window")
    }
}
impl std::error::Error for FocusChanged {}

#[cfg(windows)]
fn native_backend() -> Result<Box<dyn WindowBackend>> {
    Ok(Box::new(windows_backend::WindowsBackend::new()?))
}

#[cfg(target_os = "linux")]
fn native_backend() -> Result<Box<dyn WindowBackend>> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Ok(Box::new(kde_backend::KdeBackend::new()?));
    }
    Ok(Box::new(linux_backend::LinuxBackend::new()?))
}

#[cfg(target_os = "linux")]
#[path = "read_along_capture/linux_backend.rs"]
mod linux_backend;

#[cfg(target_os = "linux")]
#[path = "read_along_capture/kde_backend.rs"]
mod kde_backend;

#[cfg(not(any(windows, target_os = "linux")))]
fn native_backend() -> Result<Box<dyn WindowBackend>> {
    bail!("read-along capture is unsupported on this platform")
}

#[cfg(windows)]
mod windows_backend {
    use super::*;
    use anyhow::Context;
    use std::fs;
    use std::process::{Command, Output};

    const HELPER: &str = include_str!("../../../assets/windows-read-along.ps1");

    pub(super) struct WindowsBackend(PathBuf);

    impl WindowsBackend {
        pub(super) fn new() -> Result<Self> {
            let path = std::env::temp_dir().join(format!(
                "simple-stt-read-along-{}.ps1",
                uuid::Uuid::new_v4()
            ));
            fs::write(&path, HELPER).context("writing Windows read-along helper")?;
            Ok(Self(path))
        }

        fn run(&self, args: &[(&str, String)]) -> Result<Output> {
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]);
            command.arg(&self.0);
            for (key, value) in args {
                command.arg(format!("-{key}")).arg(value);
            }
            command
                .output()
                .context("running Windows read-along helper")
        }

        fn target_json(window: &WindowSnapshot, region: Rect) -> Result<String> {
            let WindowId::Windows { hwnd, process_id } = window.id else {
                bail!("target is not a Windows window")
            };
            Ok(serde_json::json!({
                "hwnd": hwnd, "pid": process_id, "x": region.x, "y": region.y,
                "width": region.width, "height": region.height
            })
            .to_string())
        }
    }

    impl WindowBackend for WindowsBackend {
        fn choose_window_and_region(
            &mut self,
            current: &dyn Fn() -> bool,
        ) -> Result<(WindowSnapshot, Rect)> {
            ensure!(current(), "read-along selection canceled");
            let output = self.run(&[("Mode", "Select".into())])?;
            ensure!(
                output.status.success(),
                "window-region selection canceled or failed"
            );
            let selected: serde_json::Value = serde_json::from_slice(&output.stdout)
                .context("parsing selected window and region")?;
            let number = |key: &str| {
                selected
                    .get(key)
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| anyhow::anyhow!("selector returned invalid {key}"))
            };
            let id = WindowId::Windows {
                hwnd: number("hwnd")? as isize,
                process_id: number("pid")? as u32,
            };
            let region = Rect {
                x: number("x")? as i32,
                y: number("y")? as i32,
                width: number("width")? as u32,
                height: number("height")? as u32,
            };
            let window = self.inspect(&id)?.context("selected window was closed")?;
            Ok((window, region))
        }

        fn inspect(&mut self, id: &WindowId) -> Result<Option<WindowSnapshot>> {
            let WindowId::Windows { hwnd, process_id } = id else {
                return Ok(None);
            };
            let data = serde_json::json!({ "hwnd": hwnd, "pid": process_id, "x": 0, "y": 0, "width": 1, "height": 1 })
                .to_string();
            let output = self.run(&[("Mode", "Inspect".into()), ("TargetJson", data)])?;
            if !output.status.success() {
                return Ok(None);
            }
            let value: serde_json::Value =
                serde_json::from_slice(&output.stdout).context("parsing window geometry")?;
            let field = |key: &str| {
                value
                    .get(key)
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| anyhow::anyhow!("helper returned invalid window {key}"))
            };
            let width = field("width")?;
            let height = field("height")?;
            Ok(Some(WindowSnapshot {
                id: id.clone(),
                bounds: Rect {
                    x: field("left")? as i32,
                    y: field("top")? as i32,
                    width: width as u32,
                    height: height as u32,
                },
                minimized: width <= 0 || height <= 0,
            }))
        }

        fn capture(
            &mut self,
            window: &WindowSnapshot,
            region: Rect,
            current: &dyn Fn() -> bool,
        ) -> Result<PathBuf> {
            ensure!(current(), "read-along capture canceled");
            let path = private_capture_path()?;
            let result = self.run(&[
                ("Mode", "Capture".into()),
                ("TargetJson", Self::target_json(window, region)?),
                ("OutputPath", path.to_string_lossy().into_owned()),
            ])?;
            if !result.status.success() {
                let _ = fs::remove_file(&path);
                bail!("window screenshot failed");
            }
            if !current() {
                let _ = fs::remove_file(&path);
                bail!("read-along capture canceled");
            }
            Ok(path)
        }

        fn advance(
            &mut self,
            window: &WindowSnapshot,
            region: Rect,
            action: &AdvanceAction,
            current: &dyn Fn() -> bool,
        ) -> Result<()> {
            ensure!(current(), "read-along action canceled");
            let (kind, key, x, y) = match action {
                AdvanceAction::Key(key) => ("key", key.clone(), String::new(), String::new()),
                AdvanceAction::Click { x, y } => {
                    ("click", String::new(), x.to_string(), y.to_string())
                }
            };
            let output = self.run(&[
                ("Mode", "Advance".into()),
                ("TargetJson", Self::target_json(window, region)?),
                ("Action", kind.into()),
                ("Key", key),
                ("X", x),
                ("Y", y),
            ])?;
            ensure!(output.status.success(), "window-bound input failed");
            Ok(())
        }

        fn supports_window_bound_input(&self) -> bool {
            true
        }
    }

    impl Drop for WindowsBackend {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
}

/// Read-only focus guard for foreground portal input on the supported KDE host.
#[cfg(target_os = "linux")]
pub(crate) struct InputFocus {
    observer: Option<kde_backend::KdeBackend>,
    window: Option<String>,
}
#[cfg(target_os = "linux")]
impl InputFocus {
    pub(crate) fn capture() -> Result<Self> {
        let kde = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase()
            .contains("kde")
            || std::env::var_os("KDE_SESSION_VERSION").is_some();
        let mut observer = if kde {
            Some(kde_backend::KdeBackend::observer()?)
        } else {
            None
        };
        let window = observer
            .as_mut()
            .map(|o| o.active_window())
            .transpose()?
            .flatten();
        if kde {
            ensure!(
                window.is_some(),
                "no focused target window for desktop input"
            );
        }
        Ok(Self { observer, window })
    }
    pub(crate) fn check(&mut self) -> Result<()> {
        if let Some(observer) = &mut self.observer {
            ensure!(
                observer.active_window()? == self.window,
                "focused window changed; input stopped before sending more text"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeBackend(Option<WindowSnapshot>);

    impl WindowBackend for FakeBackend {
        fn choose_window_and_region(
            &mut self,
            _current: &dyn Fn() -> bool,
        ) -> Result<(WindowSnapshot, Rect)> {
            bail!("selection is not used by geometry tests")
        }

        fn inspect(&mut self, id: &WindowId) -> Result<Option<WindowSnapshot>> {
            Ok(self.0.clone().filter(|snapshot| &snapshot.id == id))
        }

        fn capture(
            &mut self,
            _window: &WindowSnapshot,
            _region: Rect,
            _current: &dyn Fn() -> bool,
        ) -> Result<PathBuf> {
            bail!("capture is not used by geometry tests")
        }

        fn advance(
            &mut self,
            _window: &WindowSnapshot,
            _region: Rect,
            _action: &AdvanceAction,
            _current: &dyn Fn() -> bool,
        ) -> Result<()> {
            bail!("input is not used by geometry tests")
        }

        fn supports_window_bound_input(&self) -> bool {
            false
        }
    }

    #[test]
    fn region_tracks_window_movement() {
        let window = Rect {
            x: -400,
            y: 80,
            width: 800,
            height: 600,
        };
        let screen = Rect {
            x: -300,
            y: 180,
            width: 200,
            height: 100,
        };
        let relative = screen.relative_to(window).unwrap();
        assert_eq!(
            relative,
            Rect {
                x: 100,
                y: 100,
                width: 200,
                height: 100
            }
        );
        assert_eq!(
            relative
                .at_window(Rect {
                    x: 20,
                    y: 40,
                    ..window
                })
                .unwrap(),
            Rect {
                x: 120,
                y: 140,
                width: 200,
                height: 100
            }
        );
    }

    #[test]
    fn rejects_closed_out_of_bounds_and_minimized_geometry() {
        let relative = Rect {
            x: 300,
            y: 200,
            width: 100,
            height: 80,
        };
        let resized = Rect {
            x: 0,
            y: 0,
            width: 350,
            height: 250,
        };
        assert!(relative.at_window(resized).is_err());
        assert!(relative
            .at_window(Rect {
                width: 0,
                height: 0,
                ..resized
            })
            .is_err());
        assert!(!Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100
        }
        .contains(Rect {
            x: 99,
            y: 99,
            width: 2,
            height: 2
        }));
    }

    #[test]
    fn fake_backend_revalidates_window_identity_and_bounds() {
        let id = WindowId::X11(42);
        let target = Target {
            window: id.clone(),
            region: Rect {
                x: 20,
                y: 30,
                width: 80,
                height: 50,
            },
        };
        let mut moved = FakeBackend(Some(WindowSnapshot {
            id: id.clone(),
            bounds: Rect {
                x: 500,
                y: -80,
                width: 200,
                height: 150,
            },
            minimized: false,
        }));
        let live = target.live_window(&mut moved).unwrap();
        assert_eq!(target.region.at_window(live.bounds).unwrap().x, 520);

        let mut destroyed = FakeBackend(None);
        assert!(target.live_window(&mut destroyed).is_err());

        let mut resized = FakeBackend(Some(WindowSnapshot {
            id: id.clone(),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 60,
                height: 60,
            },
            minimized: false,
        }));
        assert!(target.live_window(&mut resized).is_err());

        let mut replaced = FakeBackend(Some(WindowSnapshot {
            id: WindowId::X11(43),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 150,
            },
            minimized: false,
        }));
        assert!(target.live_window(&mut replaced).is_err());
    }
}
