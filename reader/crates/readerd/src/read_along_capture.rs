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
#[cfg_attr(windows, derive(serde::Serialize, serde::Deserialize))]
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
    #[cfg_attr(
        all(windows, not(test)),
        expect(
            dead_code,
            reason = "Portable window IDs are retained for backend safety tests"
        )
    )]
    X11(u32),
    #[cfg_attr(
        windows,
        expect(dead_code, reason = "The KWin adapter is only constructed on Linux")
    )]
    KWin(String),
    #[cfg(windows)]
    Windows { hwnd: isize, process_id: u32 },
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
    Sequence(reader_core::recorded_action::RecordedAction),
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
    Ok(Box::new(windows_backend::WindowsBackend::default()))
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
mod windows_graphics;
#[cfg(windows)]
mod windows_picker;

#[cfg(windows)]
pub(crate) fn capture_windows_frame(request: &str, path: &std::path::Path) -> Result<()> {
    let request = serde_json::from_str(request)?;
    let frame = windows_graphics::graphics_frame(&request)?;
    ensure!(frame.has_color(), "The capture returned an empty image");
    Ok(std::fs::write(path, frame.pixels)?)
}

#[cfg(windows)]
pub(crate) fn capture_windows_region(
    hwnd: isize,
    process_id: u32,
    region: Rect,
    path: &std::path::Path,
) -> Result<()> {
    windows_picker::capture_window(&WindowId::Windows { hwnd, process_id }, region, path)
}

#[cfg(windows)]
mod windows_backend {
    use super::*;
    use anyhow::Context;
    use std::fs;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output, Stdio};
    use std::time::Duration;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    const HELPER: &str = include_str!("../../../assets/windows-read-along.ps1");

    #[derive(Default)]
    pub(super) struct WindowsBackend {
        selection_image: Option<PathBuf>,
    }

    impl WindowsBackend {
        fn advance_legacy(args: &[(&str, String)], current: &dyn Fn() -> bool) -> Result<Output> {
            let helper = crate::privacy::TemporaryFile(std::env::temp_dir().join(format!(
                "simple-stt-read-along-{}.ps1",
                uuid::Uuid::new_v4()
            )));
            fs::write(&helper.0, HELPER).context("writing Windows read-along helper")?;
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]);
            command.arg(&helper.0);
            for (key, value) in args {
                command.arg(format!("-{key}")).arg(value);
            }
            let mut child = command
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("running Windows read-along helper")?;
            while child.try_wait()?.is_none() {
                if !current() {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("read-along operation canceled");
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            child
                .wait_with_output()
                .context("reading Windows read-along result")
        }

        fn capture_child(
            window: &WindowSnapshot,
            region: Rect,
            path: &std::path::Path,
            current: &dyn Fn() -> bool,
        ) -> Result<()> {
            let WindowId::Windows { hwnd, process_id } = window.id else {
                bail!("target is not a Windows window")
            };
            let mut command = Command::new(std::env::current_exe()?);
            command
                .arg("capture-window")
                .args([
                    hwnd.to_string(),
                    process_id.to_string(),
                    region.x.to_string(),
                    region.y.to_string(),
                    region.width.to_string(),
                    region.height.to_string(),
                ])
                .arg(path);
            windows_graphics::await_capture(&mut command, current, Duration::from_secs(5))
        }

        fn recorded(
            &self,
            window: &WindowSnapshot,
            sequence: &reader_core::recorded_action::RecordedAction,
            current: &dyn Fn() -> bool,
        ) -> Result<()> {
            use std::io::Write;
            use std::process::Stdio;
            use std::time::{Duration, Instant};
            sequence.validate()?;
            let WindowId::Windows { hwnd, process_id } = window.id else {
                bail!("target is not a Windows window")
            };
            let root = std::env::current_exe()?
                .parent()
                .context("missing executable directory")?
                .to_path_buf();
            let mut runtimes = vec![root.join("AutoHotkey64.exe")];
            for variable in ["ProgramFiles", "LOCALAPPDATA"] {
                if let Some(base) = std::env::var_os(variable) {
                    let base = PathBuf::from(base);
                    runtimes.push(base.join("AutoHotkey/v2/AutoHotkey64.exe"));
                    runtimes.push(base.join("Programs/AutoHotkey/v2/AutoHotkey64.exe"));
                }
            }
            let runtime = runtimes
                .into_iter()
                .find(|path| path.is_file())
                .context("AutoHotkey v2 is required for recorded actions")?;
            let directory =
                std::env::temp_dir().join(format!("simple-stt-action-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&directory)?;
            let result = (|| -> Result<()> {
                let script = directory.join("action-replay.ahk");
                let events = directory.join("events.tsv");
                let cancel = directory.join("cancel");
                fs::write(&script, include_str!("../../../../ahk/action-replay.ahk"))?;
                let mut output = fs::File::create(&events)?;
                for event in &sequence.events {
                    writeln!(
                        output,
                        "{}\t{}\t{}",
                        event.at_ms,
                        reader_core::recorded_action::windows_scan_code(&event.key)?,
                        u8::from(event.down)
                    )?;
                }
                drop(output);
                let mut child = Command::new(&runtime)
                    .arg("/ErrorStdOut=UTF-8")
                    .arg(&script)
                    .arg(hwnd.to_string())
                    .arg(process_id.to_string())
                    .arg(&events)
                    .arg(&cancel)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                let started = Instant::now();
                let mut canceled = None;
                loop {
                    if let Some(status) = child.try_wait()? {
                        ensure!(
                            status.success(),
                            "recorded action canceled or target unavailable"
                        );
                        return Ok(());
                    }
                    if !current() && canceled.is_none() {
                        fs::write(&cancel, "cancel")?;
                        canceled = Some(Instant::now());
                    }
                    if started.elapsed() > Duration::from_secs(35)
                        || canceled.is_some_and(|at| at.elapsed() > Duration::from_secs(2))
                    {
                        let _ = child.kill();
                        let _ = child.wait();
                        // A forced exit cannot run OnExit. A separate AHK pass releases
                        // only keys in the helper's atomically saved held-key journal.
                        let _ = Command::new(&runtime)
                            .arg("/ErrorStdOut=UTF-8")
                            .arg(&script)
                            .arg(hwnd.to_string())
                            .arg(process_id.to_string())
                            .arg(&events)
                            .arg(&cancel)
                            .arg("release")
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                        bail!("recorded action helper did not stop");
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })();
            let _ = fs::remove_dir_all(directory);
            result
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
            let image = private_capture_path()?;
            self.selection_image = Some(image.clone());
            windows_picker::select(&image, current)
        }

        fn inspect(&mut self, id: &WindowId) -> Result<Option<WindowSnapshot>> {
            Ok(windows_picker::inspect(id))
        }

        fn capture(
            &mut self,
            window: &WindowSnapshot,
            region: Rect,
            current: &dyn Fn() -> bool,
        ) -> Result<PathBuf> {
            ensure!(current(), "read-along capture canceled");
            if let Some(path) = self.selection_image.take().filter(|path| path.is_file()) {
                return Ok(path);
            }
            let path = private_capture_path()?;
            Self::capture_child(window, region, &path, current).inspect_err(|_| {
                let _ = fs::remove_file(&path);
            })?;
            if !current() {
                let _ = fs::remove_file(&path);
                bail!("read-along capture canceled");
            }
            Ok(path)
        }

        fn input_ready(&mut self, window: &WindowSnapshot) -> Result<bool> {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetAncestor, GetForegroundWindow, GA_ROOT,
            };
            let WindowId::Windows { hwnd, .. } = window.id else {
                bail!("target is not a Windows window")
            };
            Ok(unsafe { GetForegroundWindow() == GetAncestor(hwnd as _, GA_ROOT) })
        }

        fn advance(
            &mut self,
            window: &WindowSnapshot,
            region: Rect,
            action: &AdvanceAction,
            current: &dyn Fn() -> bool,
        ) -> Result<()> {
            ensure!(current(), "read-along action canceled");
            let mut args = vec![("TargetJson", Self::target_json(window, region)?)];
            match action {
                AdvanceAction::Sequence(sequence) => {
                    return self.recorded(window, sequence, current)
                }
                AdvanceAction::Key(key) if key.eq_ignore_ascii_case("space") => {
                    return windows_picker::advance_space(&window.id, region, current);
                }
                AdvanceAction::Key(key) => {
                    args.extend([("Action", "key".into()), ("Key", key.clone())])
                }
                AdvanceAction::Click { x, y } => args.extend([
                    ("Action", "click".into()),
                    ("X", x.to_string()),
                    ("Y", y.to_string()),
                ]),
            }
            let output = Self::advance_legacy(&args, current)?;
            if output.status.code() == Some(3) {
                return Err(FocusChanged.into());
            }
            ensure!(output.status.success(), "window-bound input failed");
            Ok(())
        }

        fn supports_window_bound_input(&self) -> bool {
            true
        }
    }

    impl Drop for WindowsBackend {
        fn drop(&mut self) {
            if let Some(path) = &self.selection_image {
                let _ = fs::remove_file(path);
            }
        }
    }
}

/// Read-only focus guard for foreground portal input on the supported KDE host.
#[cfg(target_os = "linux")]
pub(crate) struct InputFocus {
    observer: Option<kde_backend::KdeBackend>,
    window: Option<String>,
    x11: Option<(x11rb::rust_connection::RustConnection, u32)>,
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
        let x11 = if observer.is_none()
            && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "x11"
        {
            use x11rb::protocol::xproto::ConnectionExt;
            let (connection, _) = x11rb::connect(None)?;
            let focused = connection.get_input_focus()?.reply()?.focus;
            Some((connection, focused))
        } else {
            None
        };
        Ok(Self {
            observer,
            window,
            x11,
        })
    }
    pub(crate) fn has_observer(&self) -> bool {
        self.observer.is_some() || self.x11.is_some()
    }
    pub(crate) fn check(&mut self) -> Result<()> {
        if let Some((connection, expected)) = &self.x11 {
            use x11rb::protocol::xproto::ConnectionExt;
            ensure!(
                connection.get_input_focus()?.reply()?.focus == *expected,
                "focused window changed; recorded action stopped"
            );
        }
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
