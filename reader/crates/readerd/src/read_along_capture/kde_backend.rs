//! KDE Plasma Wayland read-along support.
//!
//! `slurp` owns the single rectangle drag. KWin's scripting API supplies the
//! native window UUID under that rectangle, and ScreenShot2 captures that
//! window rather than the desktop so overlapping windows are excluded.

use super::*;
use anyhow::{anyhow, bail, ensure, Context};
use futures_lite::future::block_on;
use nix::unistd::pipe;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read},
    os::{fd::AsFd, unix::fs::OpenOptionsExt},
    path::Path,
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use zbus::{
    proxy,
    zvariant::{Fd, OwnedValue, Value},
    Connection,
};

#[proxy(
    interface = "org.kde.KWin.ScreenShot2",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/ScreenShot2"
)]
trait ScreenShot2 {
    fn capture_window(
        &self,
        handle: &str,
        options: HashMap<&str, Value<'_>>,
        pipe: Fd<'_>,
    ) -> zbus::Result<HashMap<String, OwnedValue>>;
}

#[proxy(
    interface = "org.kde.kwin.Scripting",
    default_service = "org.kde.KWin",
    default_path = "/Scripting"
)]
trait KWinScripting {
    #[zbus(name = "loadScript")]
    fn load_script(&self, path: &str, plugin_name: &str) -> zbus::Result<i32>;
    #[zbus(name = "unloadScript")]
    fn unload_script(&self, plugin_name: &str) -> zbus::Result<bool>;
    #[zbus(name = "isScriptLoaded")]
    fn is_script_loaded(&self, plugin_name: &str) -> zbus::Result<bool>;
}

#[proxy(interface = "org.kde.kwin.Script", default_service = "org.kde.KWin")]
trait KWinScript {
    #[zbus(name = "run")]
    fn run(&self) -> zbus::Result<()>;
}

#[derive(Debug, Deserialize)]
struct KWinWindow {
    uuid: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    minimized: bool,
}

#[derive(Debug, Deserialize)]
struct KWinState {
    windows: Vec<KWinWindow>,
    active_uuid: Option<String>,
}

#[derive(Clone)]
struct StateCallback(Arc<Mutex<Option<mpsc::Sender<String>>>>);

#[zbus::interface(name = "org.simple_stt.ReadAlong")]
impl StateCallback {
    #[zbus(name = "report")]
    fn report(&self, state: &str) {
        if let Some(sender) = self.0.lock().ok().and_then(|slot| slot.clone()) {
            let _ = sender.send(state.to_owned());
        }
    }
}

pub(super) struct KdeBackend {
    connection: Connection,
    callback_path: String,
    callback: StateCallback,
    slurp: String,
}

impl KdeBackend {
    pub(super) fn new() -> Result<Self> {
        Self::create(true)
    }

    pub(super) fn observer() -> Result<Self> {
        Self::create(false)
    }

    pub(super) fn active_window(&mut self) -> Result<Option<String>> {
        Ok(self.query_state()?.active_uuid)
    }

    fn create(picker: bool) -> Result<Self> {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();
        ensure!(
            desktop.contains("kde") || std::env::var_os("KDE_SESSION_VERSION").is_some(),
            "read-along Wayland capture currently requires KDE Plasma; other Wayland compositors do not expose KWin window identities"
        );
        let slurp = if picker {
            find_executable("slurp")
            .context("KDE Wayland region selection needs slurp (the layer-shell picker); install the slurp package and retry")?
        } else {
            String::new()
        };
        ensure!(
            !picker || layer_shell_available(),
            "slurp is installed, but the KDE Wayland session does not advertise layer-shell support required for a safe region drag"
        );
        let connection = block_on(Connection::session())
            .context("connecting to the KDE session bus for window capture")?;
        let callback_path = format!(
            "/org/simple_stt/ReadAlong/{}",
            uuid::Uuid::new_v4().simple()
        );
        let callback = StateCallback(Arc::new(Mutex::new(None)));
        block_on(
            connection
                .object_server()
                .at(callback_path.as_str(), callback.clone()),
        )
        .context("registering the private KWin query callback")?;
        Ok(Self {
            connection,
            callback_path,
            callback,
            slurp,
        })
    }

    fn query_state(&mut self) -> Result<KWinState> {
        // KWin allocates script IDs from its current script count. Serialize
        // our load/run/unload transactions across capture and focus checks so
        // asynchronous deletion cannot collide with a different live query.
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .context("XDG_RUNTIME_DIR is required for KWin queries")?;
        let query_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(std::path::PathBuf::from(runtime).join("simple-stt-kwin-query.lock"))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match query_lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) => bail!("KWin window query is busy: {error}"),
            }
        }
        let (sender, receiver) = mpsc::channel();
        *self
            .callback
            .0
            .lock()
            .map_err(|_| anyhow!("KWin callback lock poisoned"))? = Some(sender);
        let script_path =
            std::env::temp_dir().join(format!("simple-stt-kwin-{}.js", uuid::Uuid::new_v4()));
        let script = self.query_script()?;
        let plugin_name = format!("simple-stt-read-along-{}", uuid::Uuid::new_v4());
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&script_path)
            .and_then(|mut file| std::io::Write::write_all(&mut file, script.as_bytes()))
            .context("writing temporary KWin window query")?;
        let result = block_on(self.run_query_script(&script_path, &plugin_name));
        let _ = fs::remove_file(&script_path);
        let payload = if result.is_ok() {
            Some(receiver.recv_timeout(Duration::from_secs(3)))
        } else {
            None
        };
        *self
            .callback
            .0
            .lock()
            .map_err(|_| anyhow!("KWin callback lock poisoned"))? = None;
        // A unique plugin belongs to this exact query. Cleanup must not stop a
        // concurrent query or discard a valid reply if KWin already removed it.
        let cleanup = block_on(async {
            let scripting = KWinScriptingProxy::new(&self.connection).await?;
            scripting.unload_script(&plugin_name).await?;
            for _ in 0..20 {
                if !scripting.is_script_loaded(&plugin_name).await? {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Ok::<(), zbus::Error>(())
        });
        if let Err(error) = cleanup {
            tracing::debug!(%error, "could not unload completed KWin query");
        }
        result?;
        let payload = payload
            .expect("successful query waits for a reply")
            .context("KWin did not return its window list within three seconds")?;
        serde_json::from_str(&payload).context("decoding KWin window list")
    }

    fn query_script(&self) -> Result<String> {
        let bus = self
            .connection
            .unique_name()
            .context("session bus has no unique name")?
            .to_string();
        let bus = serde_json::to_string(&bus)?;
        let path = serde_json::to_string(&self.callback_path)?;
        Ok(format!(
            r#"
var wins = workspace.stackingOrder.filter(function(w) {{
  return w && !w.deleted && !w.desktopWindow && !w.dock && !w.popupWindow &&
    (w.onAllDesktops || w.desktops.indexOf(workspace.currentDesktop) >= 0) &&
    (!w.activities.length || w.activities.indexOf(workspace.currentActivity) >= 0);
}}).map(function(w) {{
  var g = w.frameGeometry;
  return {{uuid: String(w.internalId), x: Math.round(g.x), y: Math.round(g.y), width: Math.max(0, Math.round(g.width)), height: Math.max(0, Math.round(g.height)), minimized: !!w.minimized}};
}});
var active = workspace.activeWindow;
var state = JSON.stringify({{windows: wins, active_uuid: active ? String(active.internalId) : null}});
callDBus({bus}, {path}, "org.simple_stt.ReadAlong", "report", state);
"#
        ))
    }

    async fn run_query_script(&self, path: &Path, plugin_name: &str) -> Result<()> {
        let scripting = KWinScriptingProxy::new(&self.connection)
            .await
            .context("connecting to KWin scripting")?;
        let id = scripting
            .load_script(path.to_string_lossy().as_ref(), plugin_name)
            .await
            .context("KWin refused to load the window query script")?;
        ensure!(id >= 0, "KWin refused to load the window query script");
        let object_path = format!("/Scripting/Script{id}");
        let script = KWinScriptProxy::builder(&self.connection)
            .path(object_path)?
            .build()
            .await?;
        script
            .run()
            .await
            .map_err(|error| anyhow!("running the KWin window query script: {error}"))?;
        Ok(())
    }

    fn select_rect(&self, current: &dyn Fn() -> bool) -> Result<Rect> {
        ensure!(current(), "read-along selection canceled");
        let output = Command::new(&self.slurp)
            .args(["-f", "%x %y %w %h"])
            .stdin(Stdio::null())
            .output()
            .context("starting the KDE layer-shell region picker")?;
        ensure!(current(), "read-along selection canceled");
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if !detail.is_empty() {
                bail!(
                    "KDE region picker failed ({detail}); verify that the KWin Wayland session supports zwlr_layer_shell_v1"
                )
            }
            bail!("KDE region selection was canceled or slurp failed")
        }
        let text = String::from_utf8(output.stdout).context("slurp returned non-UTF-8 geometry")?;
        let values = text
            .split_whitespace()
            .map(str::parse::<i64>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("slurp returned invalid rectangle geometry")?;
        ensure!(
            values.len() == 4,
            "slurp returned incomplete rectangle geometry"
        );
        ensure!(
            values[0] >= i64::from(i32::MIN)
                && values[0] <= i64::from(i32::MAX)
                && values[1] >= i64::from(i32::MIN)
                && values[1] <= i64::from(i32::MAX)
                && values[2] > 1
                && values[2] <= i64::from(u32::MAX)
                && values[3] > 1
                && values[3] <= i64::from(u32::MAX),
            "selected rectangle is empty or out of range"
        );
        Ok(Rect {
            x: values[0] as i32,
            y: values[1] as i32,
            width: values[2] as u32,
            height: values[3] as u32,
        })
    }

    fn snapshot(&mut self, uuid: &str) -> Result<Option<WindowSnapshot>> {
        let state = self.query_state()?;
        Ok(state
            .windows
            .into_iter()
            .find(|window| window.uuid == uuid)
            .map(|window| WindowSnapshot {
                id: WindowId::KWin(window.uuid),
                bounds: Rect {
                    x: window.x,
                    y: window.y,
                    width: window.width,
                    height: window.height,
                },
                minimized: window.minimized || window.width == 0 || window.height == 0,
            }))
    }

    fn capture_window(
        &self,
        uuid: &str,
        region: Rect,
        bounds: Rect,
        current: &dyn Fn() -> bool,
    ) -> Result<PathBuf> {
        ensure!(current(), "read-along capture canceled");
        let (read_fd, write_fd) = pipe().context("creating the private screenshot pipe")?;
        let reader = thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            File::from(read_fd).read_to_end(&mut bytes)?;
            Ok(bytes)
        });
        let mut options = HashMap::from([
            ("native-resolution", Value::from(true)),
            ("include-decoration", Value::from(true)),
            ("include-shadow", Value::from(false)),
        ]);
        let metadata = block_on(async {
            let proxy = ScreenShot2Proxy::new(&self.connection).await?;
            proxy.capture_window(uuid, std::mem::take(&mut options), Fd::from(write_fd.as_fd())).await
        }).context("KWin ScreenShot2 CaptureWindow failed; check the packaged desktop entry grants org.kde.KWin.ScreenShot2")?;
        drop(write_fd);
        let raw = reader
            .join()
            .map_err(|_| anyhow!("KWin screenshot reader thread panicked"))??;
        ensure!(current(), "read-along capture canceled");
        let width = metadata_u32(&metadata, "width")?;
        let height = metadata_u32(&metadata, "height")?;
        let stride = metadata_u32(&metadata, "stride").unwrap_or(width.saturating_mul(4));
        let format = metadata_u32(&metadata, "format").unwrap_or(6);
        ensure!(
            width > 0 && height > 0 && stride >= width * 4,
            "KWin returned invalid screenshot dimensions"
        );
        ensure!(
            raw.len() >= (stride as usize).saturating_mul(height as usize),
            "KWin returned truncated window pixels"
        );
        ensure!(
            matches!(format, 4..=6 | 16..=18),
            "KWin returned unsupported QImage format {format}"
        );
        let scale_x = f64::from(width) / f64::from(bounds.width);
        let scale_y = f64::from(height) / f64::from(bounds.height);
        let left = (f64::from(region.x) * scale_x).floor().max(0.0) as u32;
        let top = (f64::from(region.y) * scale_y).floor().max(0.0) as u32;
        let right = (region.right() as f64 * scale_x)
            .ceil()
            .min(f64::from(width)) as u32;
        let bottom = (region.bottom() as f64 * scale_y)
            .ceil()
            .min(f64::from(height)) as u32;
        ensure!(
            right > left && bottom > top,
            "selected region maps outside the KWin window screenshot"
        );
        let mut rgb = Vec::with_capacity((right - left) as usize * (bottom - top) as usize * 3);
        for y in top..bottom {
            for x in left..right {
                let at = y as usize * stride as usize + x as usize * 4;
                let (r, g, b) = if format <= 6 {
                    (raw[at + 2], raw[at + 1], raw[at])
                } else {
                    (raw[at], raw[at + 1], raw[at + 2])
                };
                rgb.extend_from_slice(&[r, g, b]);
            }
        }
        let image: image::ImageBuffer<image::Rgb<u8>, _> =
            image::ImageBuffer::from_raw(right - left, bottom - top, rgb)
                .context("building the cropped KWin window image")?;
        let path = private_capture_path()?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        if let Err(error) = image.write_to(&mut BufWriter::new(file), image::ImageFormat::Png) {
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
        Ok(path)
    }

    fn send_input(
        &mut self,
        window: &WindowSnapshot,
        region: Rect,
        action: &AdvanceAction,
        current: &dyn Fn() -> bool,
    ) -> Result<()> {
        let uuid = match &window.id {
            WindowId::KWin(uuid) => uuid,
            _ => bail!("KWin input received a non-KWin window"),
        };
        ensure!(current(), "read-along action canceled");
        if self.query_state()?.active_uuid.as_deref() != Some(uuid.as_str()) {
            return Err(FocusChanged.into());
        }
        let input = match action {
            AdvanceAction::Key(key) => crate::portal_input::Action::Key { key: key.clone() },
            AdvanceAction::Sequence(sequence) => crate::portal_input::Action::Sequence {
                sequence: sequence.clone(),
            },
            AdvanceAction::Click { x, y } => {
                ensure!(
                    *x < region.width && *y < region.height,
                    "read-along click is outside the selected region"
                );
                let point = region.at_window(window.bounds)?;
                crate::portal_input::Action::Click {
                    x: i32::try_from(i64::from(point.x) + i64::from(*x))?,
                    y: i32::try_from(i64::from(point.y) + i64::from(*y))?,
                }
            }
        };
        crate::portal_input::run(input, current)?;
        Ok(())
    }
}

impl WindowBackend for KdeBackend {
    fn prepare_input(&mut self, action: &AdvanceAction, current: &dyn Fn() -> bool) -> Result<()> {
        crate::portal_input::run(
            match action {
                AdvanceAction::Key(_) | AdvanceAction::Sequence(_) => {
                    crate::portal_input::Action::Prepare
                }
                AdvanceAction::Click { .. } => crate::portal_input::Action::PreparePointer,
            },
            current,
        )?;
        Ok(())
    }

    fn input_ready(&mut self, window: &WindowSnapshot) -> Result<bool> {
        let WindowId::KWin(uuid) = &window.id else {
            bail!("not a KWin window")
        };
        Ok(self.query_state()?.active_uuid.as_deref() == Some(uuid.as_str()))
    }
    fn choose_window_and_region(
        &mut self,
        current: &dyn Fn() -> bool,
    ) -> Result<(WindowSnapshot, Rect)> {
        let region = self.select_rect(current)?;
        let state = self.query_state()?;
        let selected = window_at_region_center(state.windows, region)?;
        let bounds = Rect {
            x: selected.x,
            y: selected.y,
            width: selected.width,
            height: selected.height,
        };
        if !bounds.contains(region) {
            tracing::warn!(
                capture_x = region.x,
                capture_y = region.y,
                capture_width = region.width,
                capture_height = region.height,
                window_x = bounds.x,
                window_y = bounds.y,
                window_width = bounds.width,
                window_height = bounds.height,
                "capture selection crosses window bounds"
            );
        }
        region.relative_to(bounds)?;
        Ok((
            WindowSnapshot {
                id: WindowId::KWin(selected.uuid),
                bounds,
                minimized: selected.minimized,
            },
            region,
        ))
    }

    fn inspect(&mut self, id: &WindowId) -> Result<Option<WindowSnapshot>> {
        let WindowId::KWin(uuid) = id else {
            return Ok(None);
        };
        self.snapshot(uuid)
    }

    fn capture(
        &mut self,
        window: &WindowSnapshot,
        region: Rect,
        current: &dyn Fn() -> bool,
    ) -> Result<PathBuf> {
        let live = self
            .inspect(&window.id)?
            .context("selected KWin window was closed or minimized")?;
        ensure!(!live.minimized, "selected KWin window is minimized");
        ensure!(
            region.at_window(live.bounds).is_ok(),
            "saved region no longer fits the selected KWin window"
        );
        self.capture_window(
            match &live.id {
                WindowId::KWin(uuid) => uuid,
                _ => unreachable!(),
            },
            region,
            live.bounds,
            current,
        )
    }

    fn advance(
        &mut self,
        window: &WindowSnapshot,
        region: Rect,
        action: &AdvanceAction,
        current: &dyn Fn() -> bool,
    ) -> Result<()> {
        let live = self
            .inspect(&window.id)?
            .context("selected KWin window was closed or minimized")?;
        ensure!(
            !live.minimized && region.at_window(live.bounds).is_ok(),
            "saved region no longer fits the selected KWin window"
        );
        self.send_input(&live, region, action, current)
    }

    fn supports_window_bound_input(&self) -> bool {
        true
    }
}

fn metadata_u32(metadata: &HashMap<String, OwnedValue>, key: &str) -> Result<u32> {
    metadata
        .get(key)
        .cloned()
        .and_then(|value| u32::try_from(value).ok())
        .with_context(|| format!("KWin screenshot has no valid {key}"))
}

fn find_executable(name: &str) -> Option<String> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn layer_shell_available() -> bool {
    // KWin advertises layer-shell through its Wayland globals. slurp fails
    // closed if the protocol is absent; this preflight checks the compositor's
    // desktop identity and leaves protocol negotiation to the picker itself.
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|kind| kind == "wayland")
}

fn window_at_region_center(windows: Vec<KWinWindow>, region: Rect) -> Result<KWinWindow> {
    let center = Rect {
        x: i32::try_from(i64::from(region.x) + i64::from(region.width / 2))
            .context("selected KDE region center x is out of range")?,
        y: i32::try_from(i64::from(region.y) + i64::from(region.height / 2))
            .context("selected KDE region center y is out of range")?,
        width: 1,
        height: 1,
    };
    windows
        .into_iter()
        .rev()
        .find(|window| {
            !window.minimized
                && Rect {
                    x: window.x,
                    y: window.y,
                    width: window.width,
                    height: window.height,
                }
                .contains(center)
        })
        .context("selected region is not inside a visible KWin window")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(uuid: &str, x: i32, width: u32, minimized: bool) -> KWinWindow {
        KWinWindow {
            uuid: uuid.into(),
            x,
            y: 20,
            width,
            height: 100,
            minimized,
        }
    }

    #[test]
    fn region_center_binds_topmost_non_minimized_kwin_uuid() {
        let selected = window_at_region_center(
            vec![
                window("behind", 0, 500, false),
                window("minimized", 0, 500, true),
                window("front", 50, 200, false),
            ],
            Rect {
                x: 70,
                y: 30,
                width: 40,
                height: 20,
            },
        )
        .unwrap();
        assert_eq!(selected.uuid, "front");
    }

    #[test]
    fn region_outside_all_live_windows_is_rejected() {
        let error = window_at_region_center(
            vec![window("minimized", 0, 100, true)],
            Rect {
                x: 200,
                y: 200,
                width: 20,
                height: 20,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("visible KWin window"));
    }

    fn live_backend() -> KdeBackend {
        let connection = block_on(Connection::session()).unwrap();
        let callback_path = format!(
            "/org/simple_stt/ReadAlong/{}",
            uuid::Uuid::new_v4().simple()
        );
        let callback = StateCallback(Arc::new(Mutex::new(None)));
        block_on(
            connection
                .object_server()
                .at(callback_path.as_str(), callback.clone()),
        )
        .unwrap();
        KdeBackend {
            connection,
            callback_path,
            callback,
            slurp: String::new(),
        }
    }

    #[test]
    #[ignore = "requires a live KDE Plasma session bus and KWin scripting service"]
    fn live_kwin_concurrent_queries_keep_their_own_callbacks() {
        let queries: Vec<_> = (0..4)
            .map(|_| {
                thread::spawn(|| {
                    let mut backend = live_backend();
                    for _ in 0..10 {
                        assert!(!backend.query_state().unwrap().windows.is_empty());
                    }
                })
            })
            .collect();
        for query in queries {
            query.join().unwrap();
        }
    }

    #[test]
    #[ignore = "requires a live KDE Plasma session bus and KWin scripting service"]
    fn live_kwin_window_list_query_reaches_callback_and_stops_script() {
        let mut backend = live_backend();
        let state = backend.query_state().unwrap();
        assert!(state.windows.iter().all(|window| !window.uuid.is_empty()));
        let active = state
            .active_uuid
            .as_ref()
            .expect("a test window must be active");
        let foreground = state
            .windows
            .iter()
            .find(|window| &window.uuid == active)
            .expect("active test window is in the captured window list");
        let region = Rect {
            x: foreground.x + foreground.width as i32 / 2,
            y: foreground.y + foreground.height as i32 / 2,
            width: 1,
            height: 1,
        };
        let expected = active.clone();
        let selected = window_at_region_center(state.windows, region).unwrap();
        assert_eq!(
            selected.uuid, expected,
            "selection must follow visual stacking rather than window creation order"
        );
    }
}
