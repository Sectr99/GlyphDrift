//! Animation trait and built-in animations.

mod matrix;
mod plasma;
mod rng;

pub use matrix::Matrix;
pub use plasma::Plasma;
pub use rng::Rng;

use crate::grid::Grid;

pub trait Animation {
    fn name(&self) -> &'static str;
    /// Called once before the first `update`, and again on resize.
    fn init(&mut self, cols: usize, rows: usize, seed: u64);
    /// Advance simulation by `dt` seconds.
    fn update(&mut self, dt: f32);
    /// Write the current frame into `grid` (which is `cols` x `rows` from `init`).
    fn render(&self, grid: &mut Grid);
}

pub const NAMES: &[&str] = &["matrix", "plasma"];

/// Build an animation by config name. Unknown names fall back to matrix.
pub fn by_name(name: &str, ramp: &str) -> Box<dyn Animation> {
    match name {
        "plasma" => Box::new(Plasma::new(ramp)),
        _ => Box::new(Matrix::new()),
    }
}
