//! Raindrops on still water: rings that spread out and fade.

use super::{Animation, Rng};
use crate::grid::{Cell, Grid};

const SPEED: f32 = 5.0; // ring growth, cells per second
const LIFE: f32 = 3.5; // seconds until a ring has faded out
const CELLS_PER_DROP: f32 = 3000.0; // spawn rate: one drop per second per this many cells

struct Drop {
    x: f32,
    y: f32,
    age: f32,
}

pub struct Ripples {
    drops: Vec<Drop>,
    cols: usize,
    rows: usize,
    rng: Rng,
    spawn: f32,
}

impl Ripples {
    pub fn new() -> Self {
        Self {
            drops: Vec::new(),
            cols: 0,
            rows: 0,
            rng: Rng::new(1),
            spawn: 0.0,
        }
    }

    fn add_drop(&mut self, age: f32) {
        self.drops.push(Drop {
            x: self.rng.range(0.0, self.cols as f32),
            y: self.rng.range(0.0, self.rows as f32),
            age,
        });
    }
}

impl Default for Ripples {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for Ripples {
    fn name(&self) -> &'static str {
        "ripples"
    }

    fn init(&mut self, cols: usize, rows: usize, seed: u64) {
        self.cols = cols;
        self.rows = rows;
        self.rng = Rng::new(seed);
        self.drops.clear();
        self.spawn = 0.0;
        // Start mid-shower: a few rings already at different ages.
        let n = ((cols * rows) as f32 / CELLS_PER_DROP * LIFE).ceil() as usize;
        for _ in 0..n {
            let age = self.rng.range(0.0, LIFE);
            self.add_drop(age);
        }
    }

    fn update(&mut self, dt: f32) {
        for d in &mut self.drops {
            d.age += dt;
        }
        self.drops.retain(|d| d.age < LIFE);
        self.spawn += dt * (self.cols * self.rows) as f32 / CELLS_PER_DROP;
        while self.spawn >= 1.0 {
            self.spawn -= 1.0;
            self.add_drop(0.0);
        }
    }

    fn render(&self, grid: &mut Grid) {
        grid.clear();
        for d in &self.drops {
            let fade = 1.0 - d.age / LIFE;
            // An outer ring plus a weaker inner one that trails behind it.
            for (r, k) in [(d.age * SPEED, 1.0), (d.age * SPEED * 0.55, 0.5)] {
                // Cells are about twice as tall as wide, so the ring spans r/2 rows.
                let (x0, x1) = ((d.x - r - 1.0).max(0.0), d.x + r + 1.0);
                let (y0, y1) = ((d.y - r / 2.0 - 1.0).max(0.0), d.y + r / 2.0 + 1.0);
                for y in y0 as usize..=(y1 as usize).min(grid.rows().saturating_sub(1)) {
                    for x in x0 as usize..=(x1 as usize).min(grid.cols().saturating_sub(1)) {
                        let (dx, dy) = (x as f32 + 0.5 - d.x, (y as f32 + 0.5 - d.y) * 2.0);
                        let e = ((dx * dx + dy * dy).sqrt() - r).abs();
                        if e >= 0.7 {
                            continue;
                        }
                        let b = (fade * k * (1.0 - e / 0.7) * 255.0) as u8;
                        if grid.get(x, y).is_some_and(|c| c.brightness >= b) {
                            continue;
                        }
                        let ch = if d.age < 0.2 {
                            '*'
                        } else if b > 110 {
                            'o'
                        } else {
                            '.'
                        };
                        grid.set(x, y, Cell { ch, brightness: b });
                    }
                }
            }
        }
    }
}
