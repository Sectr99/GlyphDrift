//! Glyphdrift: a live wallpaper for Windows and macOS. The portable core builds and tests
//! anywhere; host code is cfg-gated (Windows modules below, macOS in `macos`).

/// `log!(...)`: `eprintln!` that also appends to the log file.
#[cfg(any(windows, target_os = "macos"))]
macro_rules! log {
    ($($t:tt)*) => { $crate::log::write(&format!($($t)*)) };
}

pub mod anim;
pub mod config;
pub mod engine;
pub mod grid;
pub mod icon;
pub mod shape;
pub mod theme;
pub mod ui;
pub mod update;

#[cfg(windows)]
pub mod app;
#[cfg(windows)]
pub mod autostart;
#[cfg(windows)]
pub mod desktop;
#[cfg(windows)]
pub mod lockscreen;
#[cfg(any(windows, target_os = "macos"))]
pub mod log;
#[cfg(windows)]
pub mod power;
#[cfg(windows)]
pub mod render;
#[cfg(windows)]
pub mod saver;
#[cfg(windows)]
pub mod settings;
#[cfg(windows)]
pub mod tray;

#[cfg(target_os = "macos")]
pub mod macos;
