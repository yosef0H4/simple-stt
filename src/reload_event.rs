//! Cross-process config reload signaling via a sentinel file.
//!
//! The `run` process starts a background thread that polls for a sentinel
//! file.  When the settings subprocess saves a new config it calls
//! `signal_reload()`, which writes that sentinel file.  The background thread
//! detects it, removes it, and forwards a unit value over a channel into the
//! main `select!` loop so the hotkey is updated without restarting.

use anyhow::Result;
use crossbeam_channel::{bounded, Receiver};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn sentinel_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("uvox")
        .join("reload-requested")
}

/// Start the background thread that watches for the sentinel file.
///
/// Returns a receiver that yields `()` each time the settings subprocess
/// signals a reload.  The thread exits when the receiver is dropped.
pub fn create_reload_channel() -> Result<Receiver<()>> {
    let (tx, rx) = bounded::<()>(4);
    thread::spawn(move || {
        let path = sentinel_path();
        tracing::debug!(path = %path.display(), "config reload sentinel watcher started");
        loop {
            thread::sleep(Duration::from_millis(50));
            if path.exists() {
                // Remove the sentinel before notifying so a rapid double-save
                // doesn't deliver two reloads from a single file.
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::warn!(error = %e, "could not remove config reload sentinel");
                }
                tracing::debug!("config reload sentinel detected");
                if tx.send(()).is_err() {
                    // Receiver dropped — run process is shutting down.
                    break;
                }
            }
        }
        tracing::debug!("config reload sentinel watcher exiting");
    });
    Ok(rx)
}

/// Write the sentinel file (called by the settings subprocess after saving).
///
/// If the run process is not running this is a no-op — the file will simply
/// be cleaned up on the next launch.
pub fn signal_reload() {
    let path = sentinel_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&path, b"1") {
        Ok(()) => tracing::debug!(path = %path.display(), "config reload sentinel written"),
        Err(e) => tracing::warn!(error = %e, "could not write config reload sentinel"),
    }
}
