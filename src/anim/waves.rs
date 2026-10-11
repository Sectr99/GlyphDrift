//! Ocean swell in perspective: thin crests far away, broad ones closer.

use std::f32::consts::TAU;

use super::{rng::hash3, Animation, Size};
use crate::grid::{Cell, Grid};

/// Horizon as a fraction of screen height.
const HORIZON: f32 = 0.35;

pub struct Waves {
    t: f32,
}

impl Waves {
    pub fn new() -> Self {
        Self { t: 0.0 }
    }
}

impl Default for Waves {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for Waves {
    fn name(&self) -> &'static str {
        "waves"
    }

    fn init(&mut self, _size: Size, seed: u64) {
        // The seed only offsets the phase.
        self.t = (seed % 1000) as f32;
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
    }

    fn render(&self, grid: &mut Grid) {
        grid.clear();
        let rows = grid.rows() as f32;
        let horizon = rows * HORIZON;
        let t = self.t;
        for y in (horizon.ceil() as usize)..grid.rows() {
            // d: 0 at the horizon, 1 at the bottom. z: distance.
            let d = (y as f32 - horizon + 1.0) / (rows - horizon);
            let z = 4.0 / (d + 0.12);
            // A band of brightness rolling toward the viewer.
            let swell = 0.5 + 0.5 * (z * 1.4 + t * 0.9).sin();
            // Each row drifts on its own.
            let h = hash3(y as u32, 0, 0);
            let phase = (h & 0xFFFF) as f32 / 65536.0 * TAU;
            let drift = (0.3 + ((h >> 16) & 0xFF) as f32 / 255.0 * 0.5)
                * if h >> 63 == 0 { 1.0 } else { -1.0 };
            let freq = 0.9 / (1.0 + 6.0 * d); // short dashes far, long ones near
            for x in 0..grid.cols() {
                let xf = x as f32 * freq;
                // The nested sine varies dash spacing.
                let w = (xf + phase + t * drift + 1.5 * (xf * 0.29 + phase * 2.0).sin()).sin();
                if w < 0.75 {
                    continue;
                }
                let s = (w - 0.75) / 0.25; // dash strength 0..1
                let ch = if s > 0.6 {
                    '~'
                } else if s > 0.25 {
                    '-'
                } else {
                    '.'
                };
                let b = (0.25 + 0.75 * swell) * (0.3 + 0.7 * d) * (0.4 + 0.6 * s);
                grid.set(
                    x,
                    y,
                    Cell {
                        ch,
                        brightness: (b * 255.0) as u8,
                    },
                );
            }
        }
    }
}
