//! Glyphdrift: a live wallpaper for Windows. The portable core builds and tests anywhere;
//! Windows host code is cfg-gated.

/// `log!(...)`: `eprintln!` that also appends to the log file.
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
