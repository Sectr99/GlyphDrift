//! WallMotion: a live wallpaper for Windows.
//!
//! Portable core (`grid`, `anim`, `config`, `theme`, `icon`, `shape`) builds and tests on any platform.
//! Windows-only host code (`desktop`, `render`, `tray`, `settings`, `power`, `autostart`, `log`, `app`) is cfg-gated.

/// `log!(...)`: like `eprintln!`, but also appended to the log file (see `log`).
#[cfg(windows)]
macro_rules! log {
    ($($t:tt)*) => { $crate::log::write(&format!($($t)*)) };
}

pub mod anim;
pub mod config;
pub mod grid;
pub mod icon;
pub mod shape;
pub mod theme;

#[cfg(windows)]
pub mod app;
#[cfg(windows)]
pub mod autostart;
#[cfg(windows)]
pub mod desktop;
#[cfg(windows)]
pub mod log;
#[cfg(windows)]
pub mod power;
#[cfg(windows)]
pub mod render;
#[cfg(windows)]
pub mod settings;
#[cfg(windows)]
pub mod tray;
