pub mod config;
pub mod resample;
pub mod transcript;

#[cfg(windows)]
pub mod audio;
#[cfg(windows)]
pub mod gui;
#[cfg(windows)]
pub mod hotkey;
#[cfg(windows)]
pub mod input;
#[cfg(windows)]
pub mod parakeet_native;
