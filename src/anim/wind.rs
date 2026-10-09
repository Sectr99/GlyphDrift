//! Wind: soft streaks drifting across the screen on gently curving paths.

use super::{Animation, Rng, Size};
use crate::grid::{Cell, Grid};

const CELLS_PER_GUST: f32 = 200.0;

struct Gust {
    x: f32,
    y: f32,
    speed: f32,
    len: usize,
    phase: f32,
    amp: f32,
    /// Peak brightness 0..1; dimmer streaks read as farther away.
    depth: f32,
}

pub struct Wind {
    gusts: Vec<Gust>,
    cols: usize,
    rows: usize,
    rng: Rng,
}

impl Wind {
    pub fn new() -> Self {
        Self {
            gusts: Vec::new(),
            cols: 0,
            rows: 0,
            rng: Rng::new(1),
        }
    }

    fn spawn(rng: &mut Rng, cols: usize, rows: usize, anywhere: bool) -> Gust {
        let depth = rng.range(0.25, 1.0);
        Gust {
            x: if anywhere {
                rng.range(0.0, cols as f32)
            } else {
                -rng.range(0.0, cols as f32 * 0.5)
            },
            y: rng.range(0.0, rows as f32),
            speed: 6.0 + 16.0 * depth, // nearer streaks move faster
            len: (4.0 + 12.0 * depth) as usize,
            phase: rng.range(0.0, std::f32::consts::TAU),
            amp: rng.range(1.0, 4.0),
            depth,
        }
    }
}

impl Default for Wind {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for Wind {
    fn name(&self) -> &'static str {
        "wind"
    }

    fn init(&mut self, size: Size, seed: u64) {
        let (cols, rows) = (size.cols, size.rows);
        self.cols = cols;
        self.rows = rows;
        self.rng = Rng::new(seed);
        let n = ((cols * rows) as f32 / CELLS_PER_GUST).ceil() as usize;
        self.gusts = (0..n)
            .map(|_| Self::spawn(&mut self.rng, cols, rows, true))
            .collect();
    }

    fn update(&mut self, dt: f32) {
        let (cols, rows) = (self.cols, self.rows);
        for g in &mut self.gusts {
            g.x += g.speed * dt;
            if g.x - g.len as f32 > cols as f32 {
                *g = Self::spawn(&mut self.rng, cols, rows, false);
            }
        }
    }

    fn render(&self, grid: &mut Grid) {
        grid.clear();
        for g in &self.gusts {
            for i in 0..g.len {
                let px = g.x - i as f32;
                if px < 0.0 {
                    break;
                }
                let py = g.y + g.amp * (px * 0.06 + g.phase).sin();
                if py < 0.0 {
                    continue;
                }
                let fade = 1.0 - i as f32 / g.len as f32;
                let ch = if i == 0 {
                    '~'
                } else if fade > 0.5 {
                    '-'
                } else {
                    '.'
                };
                let brightness = (g.depth * fade * fade * 230.0) as u8;
                grid.set(px as usize, py as usize, Cell { ch, brightness });
            }
        }
    }
}
