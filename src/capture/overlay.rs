#[path = "overlay_model.rs"]
pub mod overlay_model;

#[cfg(target_os = "linux")]
#[path = "overlay_font.rs"]
pub mod overlay_font;

#[cfg(target_os = "linux")]
#[path = "overlay_render.rs"]
pub mod overlay_render;

#[cfg(windows)]
#[path = "overlay_windows.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "overlay_linux.rs"]
mod platform;

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::overlay_model::{OverlayPrimary, RecordingIndicators};
    use anyhow::Result;
    use std::sync::{atomic::AtomicU32, Arc};
    use std::time::Duration;
    #[derive(Debug, Clone)]
    pub struct OverlayHandle {
        level: Arc<AtomicU32>,
    }
    impl OverlayHandle {
        pub fn spawn() -> Result<Self> {
            Ok(Self {
                level: Arc::new(AtomicU32::new(0)),
            })
        }
        pub fn start_recording(&self, _: isize, _: RecordingIndicators) {}
        pub fn set_primary(&self, _: OverlayPrimary) {}
        pub fn notify_info(&self, _: impl Into<String>, _: Option<Duration>) {}
        pub fn notify_warning(&self, _: impl Into<String>, _: Duration) {}
        pub fn notify_error(&self, _: impl Into<String>, _: Duration) {}
        pub fn clear_notice(&self) {}
        pub fn level_cell(&self) -> Arc<AtomicU32> {
            Arc::clone(&self.level)
        }
        pub fn hide(&self) {}
    }
}
pub use overlay_model::{
    ascii_visualizer, empty_visualizer_levels, linux_overlay_lines, render_overlay_text,
    set_visualizer_level, NoticeLevel, OverlayPrimary, RecordingIndicators,
};
pub use platform::OverlayHandle;
