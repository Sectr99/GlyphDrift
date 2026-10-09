//! ASCII live wallpaper.
//!
//! Portable core (`grid`, `anim`, `config`, `theme`, `icon`, `shape`) builds and tests on any platform.
//! Windows-only host code (`desktop`, `render`, `tray`, `power`, `autostart`, `app`) is cfg-gated.

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
pub mod power;
#[cfg(windows)]
pub mod render;
#[cfg(windows)]
pub mod tray;
