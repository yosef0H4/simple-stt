pub mod config;
pub mod logging;
pub mod models;
pub mod resample;
pub mod screenshots;
pub mod slint_ui;
pub mod transcript;
pub mod winutil;

#[cfg(windows)]
pub mod audio;
#[cfg(windows)]
pub mod gui;
#[cfg(windows)]
pub mod hotkey;
#[cfg(windows)]
pub mod input;
#[cfg(windows)]
pub mod overlay;
#[cfg(windows)]
pub mod parakeet_native;
#[cfg(windows)]
pub mod reload_event;
#[cfg(windows)]
pub mod startup;
#[cfg(windows)]
pub mod tray;
