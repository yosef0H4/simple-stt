use super::overlay_model::{
    empty_visualizer_levels, set_visualizer_level, NoticeLevel, OverlayPrimary, VisualizerLevels,
};
use super::overlay_render::{plan_for, render_surface, LayoutPlan, MAX_HEIGHT, MAX_WIDTH};
use anyhow::{Context, Result};
use smithay_client_toolkit::reexports::calloop::{
    channel::{self, Event},
    timer::{TimeoutAction, Timer},
    EventLoop,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

// Starting size; the overlay resizes itself to fit its content on every draw.
const INITIAL_WIDTH: u32 = 220;
const INITIAL_HEIGHT: u32 = 44;
const MARGIN_TOP: i32 = 36;
const MARGIN_RIGHT: i32 = 36;

#[derive(Debug, Clone)]
pub struct OverlayHandle {
    tx: channel::Sender<OverlayCommand>,
    level: Arc<AtomicU32>,
}

#[derive(Debug, Clone)]
enum OverlayCommand {
    StartRecording,
    SetPrimary(OverlayPrimary),
    Notify {
        level: NoticeLevel,
        text: String,
        duration: Option<Duration>,
    },
    ClearNotice,
    Hide,
}

#[derive(Debug, Clone)]
struct Notice {
    level: NoticeLevel,
    text: String,
    expires_at: Option<Instant>,
}

impl Notice {
    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|expires_at| Instant::now() >= expires_at)
            .unwrap_or(false)
    }
}

impl OverlayHandle {
    pub fn spawn() -> Result<Self> {
        let level = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
        let (tx, rx) = channel::channel();
        let thread_level = Arc::clone(&level);
        std::thread::Builder::new()
            .name("simple-stt-linux-overlay".to_owned())
            .spawn(move || {
                if let Err(error) = overlay_thread(rx, thread_level) {
                    tracing::warn!(%error, "linux layer-shell overlay disabled");
                }
            })
            .context("spawning linux overlay thread")?;
        Ok(Self { tx, level })
    }

    pub fn start_recording(&self, _: isize) {
        let _ = self.tx.send(OverlayCommand::StartRecording);
    }

    pub fn set_primary(&self, primary: OverlayPrimary) {
        let _ = self.tx.send(OverlayCommand::SetPrimary(primary));
    }

    pub fn notify_info(&self, text: impl Into<String>, duration: Option<Duration>) {
        self.notify(NoticeLevel::Info, text, duration);
    }

    pub fn notify_warning(&self, text: impl Into<String>, duration: Duration) {
        self.notify(NoticeLevel::Warning, text, Some(duration));
    }

    pub fn notify_error(&self, text: impl Into<String>, duration: Duration) {
        self.notify(NoticeLevel::Error, text, Some(duration));
    }

    pub fn clear_notice(&self) {
        let _ = self.tx.send(OverlayCommand::ClearNotice);
    }

    pub fn level_cell(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.level)
    }

    pub fn hide(&self) {
        let _ = self.tx.send(OverlayCommand::Hide);
    }

    fn notify(&self, level: NoticeLevel, text: impl Into<String>, duration: Option<Duration>) {
        let _ = self.tx.send(OverlayCommand::Notify {
            level,
            text: text.into(),
            duration,
        });
    }
}

fn overlay_thread(rx: channel::Channel<OverlayCommand>, level: Arc<AtomicU32>) -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        anyhow::bail!("WAYLAND_DISPLAY is not set");
    }

    // Register the bundled font before any Pango layout is created.
    super::overlay_font::ensure_registered();

    let conn = Connection::connect_to_env().context("connecting to Wayland compositor")?;
    let (globals, event_queue) = registry_queue_init(&conn).context("initializing registry")?;
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<LayerOverlay> =
        EventLoop::try_new().context("creating overlay event loop")?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .context("registering Wayland source")?;

    let compositor = CompositorState::bind(&globals, &qh).context("wl_compositor missing")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("wlr layer-shell missing")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm missing")?;

    // The layer surface is created lazily (and destroyed on hide), so don't
    // create it up front.
    let pool = SlotPool::new((MAX_WIDTH * MAX_HEIGHT * 4) as usize, &shm)
        .context("creating Wayland SHM pool")?;
    let mut app = LayerOverlay {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        layer_shell,
        qh: qh.clone(),
        layer: None,
        _input_region: None,
        pool,
        width: INITIAL_WIDTH,
        height: INITIAL_HEIGHT,
        configured: false,
        mapped: false,
        primary: OverlayPrimary::Hidden,
        notice: None,
        target_level: 0.0,
        display_level: 0.0,
        visualizer_levels: empty_visualizer_levels(),
        level_sample_tick: 0,
        last_signature: String::new(),
        needs_redraw: true,
        exit: false,
        level,
    };

    event_loop
        .handle()
        .insert_source(rx, |event, _, app| match event {
            Event::Msg(command) => app.handle_command(command),
            Event::Closed => app.exit = true,
        })
        .map_err(|error| anyhow::anyhow!("registering overlay command channel: {error:?}"))?;

    event_loop
        .handle()
        .insert_source(
            Timer::from_duration(Duration::from_millis(16)),
            |_, _, app| {
                app.tick();
                TimeoutAction::ToDuration(Duration::from_millis(16))
            },
        )
        .map_err(|error| anyhow::anyhow!("registering overlay timer: {error:?}"))?;

    while !app.exit {
        event_loop
            .dispatch(Duration::from_millis(50), &mut app)
            .context("dispatching overlay events")?;
    }

    Ok(())
}

struct LayerOverlay {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    qh: QueueHandle<LayerOverlay>,
    layer: Option<LayerSurface>,
    _input_region: Option<Region>,
    pool: SlotPool,
    width: u32,
    height: u32,
    configured: bool,
    mapped: bool,
    primary: OverlayPrimary,
    notice: Option<Notice>,
    target_level: f32,
    display_level: f32,
    visualizer_levels: VisualizerLevels,
    level_sample_tick: usize,
    last_signature: String,
    needs_redraw: bool,
    exit: bool,
    level: Arc<AtomicU32>,
}

impl LayerOverlay {
    fn handle_command(&mut self, command: OverlayCommand) {
        match command {
            OverlayCommand::StartRecording => {
                self.primary = OverlayPrimary::Recording;
                self.notice = None;
                self.target_level = 0.0;
                self.display_level = 0.0;
                self.visualizer_levels = empty_visualizer_levels();
                self.level_sample_tick = 0;
            }
            OverlayCommand::SetPrimary(primary) => {
                self.primary = primary;
                if primary != OverlayPrimary::Recording {
                    self.target_level = 0.0;
                    self.display_level = 0.0;
                    self.visualizer_levels = empty_visualizer_levels();
                    self.level_sample_tick = 0;
                }
            }
            OverlayCommand::Notify {
                level,
                text,
                duration,
            } => self.notify(level, text, duration),
            OverlayCommand::ClearNotice => self.notice = None,
            OverlayCommand::Hide => self.hide(),
        }
        self.needs_redraw = true;
        self.draw_if_ready();
    }

    fn notify(&mut self, level: NoticeLevel, text: String, duration: Option<Duration>) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        if let Some(current) = self.notice.as_ref().filter(|notice| !notice.is_expired()) {
            if current.level == level && current.text == text {
                return;
            }
            if current.level > level {
                return;
            }
        }
        self.notice = Some(Notice {
            level,
            text,
            expires_at: duration.map(|duration| Instant::now() + duration),
        });
    }

    fn hide(&mut self) {
        self.primary = OverlayPrimary::Hidden;
        self.notice = None;
        self.target_level = 0.0;
        self.display_level = 0.0;
        self.visualizer_levels = empty_visualizer_levels();
        self.level_sample_tick = 0;
    }

    fn tick(&mut self) {
        if self.notice.as_ref().is_some_and(Notice::is_expired) {
            self.notice = None;
            self.needs_redraw = true;
        }
        if self.primary == OverlayPrimary::Recording {
            self.target_level = f32::from_bits(self.level.load(Ordering::Relaxed)).clamp(0.0, 1.0);
            // The meter already applies attack/release ballistics, so follow it
            // closely here to avoid adding visible lag.
            self.display_level = self.target_level;
            self.level_sample_tick = self.level_sample_tick.wrapping_add(1);
            if self.level_sample_tick >= 2 {
                self.level_sample_tick = 0;
                set_visualizer_level(&mut self.visualizer_levels, self.display_level);
                self.needs_redraw = true;
            }
        }
        self.draw_if_ready();
    }

    fn draw_if_ready(&mut self) {
        // Note: not gated on `configured` — present() may need to *create* the
        // layer surface (which is what triggers the first configure).
        if self.needs_redraw {
            self.present();
        }
    }

    /// Decide what to show. If the surface needs to be (re)mapped or resized,
    /// request a new size and wait for the compositor's configure before
    /// attaching a buffer (required by wlr-layer-shell to map the surface);
    /// otherwise draw the buffer immediately.
    fn present(&mut self) {
        let Some(plan) = self.build_plan() else {
            // Nothing to show: destroy the surface entirely. KWin (and others)
            // do not reliably re-display a layer surface that was unmapped with
            // a null buffer, so we recreate a fresh one on the next show.
            self.destroy_layer();
            self.needs_redraw = false;
            return;
        };

        // No surface yet (first show, or after a hide): create one sized to the
        // content and wait for the compositor's configure before drawing.
        if self.layer.is_none() {
            self.create_layer(plan.width, plan.height);
            self.needs_redraw = true;
            return;
        }

        // Surface created but not yet acked: wait for the configure event, which
        // will attach the buffer itself.
        if !self.configured {
            self.needs_redraw = true;
            return;
        }

        if self.mapped
            && plan.signature == self.last_signature
            && plan.width == self.width
            && plan.height == self.height
        {
            self.needs_redraw = false;
            return;
        }

        // A resize must go through a configure round.
        if plan.width != self.width || plan.height != self.height {
            if let Some(layer) = &self.layer {
                layer.set_size(plan.width, plan.height);
                layer.commit();
            }
            self.needs_redraw = true;
            return;
        }

        self.draw_buffer(&plan);
    }

    fn create_layer(&mut self, width: u32, height: u32) {
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("simple-stt"),
            None,
        );
        let input_region = match Region::new(&self.compositor) {
            Ok(region) => region,
            Err(error) => {
                tracing::warn!(%error, "overlay: failed to create input region");
                return;
            }
        };
        layer.set_input_region(Some(input_region.wl_region()));
        layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_margin(MARGIN_TOP, MARGIN_RIGHT, 0, 0);
        layer.set_size(width, height);
        layer.commit();
        self.layer = Some(layer);
        self._input_region = Some(input_region);
        self.width = width;
        self.height = height;
        self.configured = false;
        self.mapped = false;
        self.last_signature.clear();
        tracing::debug!(width, height, "overlay: created layer surface");
    }

    /// Destroy the layer surface so it is fully removed from the compositor (no
    /// leftover region that could still be blurred). A fresh one is created on
    /// the next show.
    fn destroy_layer(&mut self) {
        if self.layer.is_some() {
            tracing::debug!("overlay: destroying layer surface (hide)");
        }
        self.layer = None;
        self._input_region = None;
        self.mapped = false;
        self.configured = false;
        self.last_signature.clear();
    }

    fn draw_buffer(&mut self, plan: &LayoutPlan) {
        let stride = self.width as i32 * 4;
        let Ok((buffer, canvas)) = self.pool.create_buffer(
            self.width as i32,
            self.height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) else {
            return;
        };

        if let Some(mut surface) = render_surface(plan) {
            surface.flush();
            if let Ok(data) = surface.data() {
                let len = canvas.len().min(data.len());
                canvas[..len].copy_from_slice(&data[..len]);
            }
        }
        let Some(layer) = self.layer.as_ref() else {
            return;
        };
        layer
            .wl_surface()
            .damage_buffer(0, 0, self.width as i32, self.height as i32);
        let _ = buffer.attach_to(layer.wl_surface());
        layer.commit();
        self.mapped = true;
        self.last_signature = plan.signature.clone();
        self.needs_redraw = false;
    }

    /// Build the lines to render plus the surface size needed to fit them.
    fn build_plan(&self) -> Option<LayoutPlan> {
        plan_for(
            self.primary,
            self.notice.as_ref().map(|notice| notice.text.as_str()),
            &self.visualizer_levels,
        )
    }
}

impl CompositorHandler for LayerOverlay {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.needs_redraw = true;
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for LayerOverlay {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for LayerOverlay {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 > 0 {
            self.width = configure.new_size.0;
        }
        if configure.new_size.1 > 0 {
            self.height = configure.new_size.1;
        }
        self.configured = true;
        // The compositor has acked our size; attach a buffer now to map at the
        // confirmed size, or destroy the surface if there is nothing to show.
        match self.build_plan() {
            Some(plan) => self.draw_buffer(&plan),
            None => self.destroy_layer(),
        }
        self.needs_redraw = false;
    }
}

impl ShmHandler for LayerOverlay {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(LayerOverlay);
delegate_output!(LayerOverlay);
delegate_shm!(LayerOverlay);
delegate_layer!(LayerOverlay);
delegate_registry!(LayerOverlay);

impl ProvidesRegistryState for LayerOverlay {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
