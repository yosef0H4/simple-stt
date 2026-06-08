#[cfg(windows)]
#[path = "overlay_windows.rs"]
mod platform;

#[cfg(not(windows))]
mod platform {
    use anyhow::Result;
    use std::sync::{atomic::AtomicU32, Arc};
    use std::time::Duration;
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OverlayPrimary {
        Hidden,
        Recording,
        Transcribing,
        Typing,
    }
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
        pub fn start_recording(&self, _: isize) {}
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
pub use platform::{OverlayHandle, OverlayPrimary};
