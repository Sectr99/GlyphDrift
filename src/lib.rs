//! ASCII live wallpaper.
//!
//! Portable core (`grid`, `anim`, `config`) builds and tests on any platform.
//! Windows-only host code (`desktop`, `render`, `tray`, `power`, `app`) is cfg-gated.

pub mod anim;
pub mod config;
pub mod grid;

#[cfg(windows)]
pub mod app;
#[cfg(windows)]
pub mod desktop;
#[cfg(windows)]
pub mod power;
#[cfg(windows)]
pub mod render;
#[cfg(windows)]
pub mod tray;
