//! Heat-propagation fire, low (about a third of the screen) and soft.

use super::{Animation, Rng, Size};
use crate::grid::{Cell, Grid};

/// Fixed step: the same look at any frame rate.
const STEP: f32 = 1.0 / 30.0;
/// Flame height as a fraction of the screen.
const HEIGHT: f32 = 0.3;

pub struct Fire {
    ramp: Vec<char>,
    heat: Vec<f32>,
    cols: usize,
    rows: usize,
    rng: Rng,
    accum: f32,
}

impl Fire {
    pub fn new(ramp: &str) -> Self {
        Self {
            ramp: ramp.chars().collect(),
            heat: Vec::new(),
            cols: 0,
            rows: 0,
            rng: Rng::new(1),
            accum: 0.0,
        }
    }

    fn step(&mut self) {
        let (w, h) = (self.cols, self.rows);
        // Fuel row: flickering heat along the bottom.
        for x in 0..w {
            self.heat[(h - 1) * w + x] = self.rng.range(0.55, 1.0);
        }
        // Each cell averages the cells below, minus random cooling that burns out after
        // HEIGHT * rows rows.
        let cool = 2.0 / (h as f32 * HEIGHT).max(4.0);
        for y in 0..h - 1 {
            let below = (y + 1) * w;
            let below2 = (y + 2).min(h - 1) * w;
            for x in 0..w {
                let l = self.heat[below + x.saturating_sub(1)];
                let r = self.heat[below + (x + 1).min(w - 1)];
                let avg = (l + r + self.heat[below + x] + self.heat[below2 + x]) / 4.0;
                self.heat[y * w + x] = (avg - self.rng.range(0.0, cool)).max(0.0);
            }
        }
    }
}

impl Animation for Fire {
    fn warm(&self) -> bool {
        true
    }

    fn init(&mut self, size: Size, seed: u64) {
        let (cols, rows) = (size.cols, size.rows);
        self.cols = cols;
        self.rows = rows.max(2);
        self.heat = vec![0.0; self.cols * self.rows];
        self.rng = Rng::new(seed);
        self.accum = 0.0;
        // Pre-warm so the first frame isn't empty.
        for _ in 0..(self.rows as f32 * HEIGHT) as usize * 2 {
            self.step();
        }
    }

    fn update(&mut self, dt: f32) {
        self.accum += dt;
        while self.accum >= STEP {
            self.accum -= STEP;
            self.step();
        }
    }

    fn render(&self, grid: &mut Grid) {
        let n = self.ramp.len();
        for y in 0..grid.rows().min(self.rows) {
            for x in 0..grid.cols().min(self.cols) {
                let v = self.heat[y * self.cols + x];
                let cell = if v < 0.04 {
                    Cell::EMPTY
                } else {
                    Cell {
                        ch: self.ramp[((v * n as f32) as usize).clamp(1, n - 1)],
                        brightness: (v.min(1.0) * 255.0) as u8,
                    }
                };
                grid.set(x, y, cell);
            }
        }
    }
}
