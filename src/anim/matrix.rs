use super::{rng::hash3, Animation, Rng};
use crate::grid::{Cell, Grid};

const GLYPHS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz@#$%&*+=<>?";

struct Column {
    head: f32,
    speed: f32,
    len: u32,
}

pub struct Matrix {
    cols: Vec<Column>,
    rows: usize,
    rng: Rng,
    tick: u32,
    tick_accum: f32,
}

impl Matrix {
    pub fn new() -> Self {
        Self {
            cols: Vec::new(),
            rows: 0,
            rng: Rng::new(1),
            tick: 0,
            tick_accum: 0.0,
        }
    }

    fn respawn(rng: &mut Rng, rows: usize, initial: bool) -> Column {
        let len = rng.range(6.0, (rows as f32 * 0.6).max(8.0)) as u32;
        let speed = rng.range(6.0, 22.0); // cells per second
        let head = if initial {
            rng.range(-(rows as f32) * 1.5, rows as f32)
        } else {
            -rng.range(0.0, rows as f32 * 0.5)
        };
        Column { head, speed, len }
    }
}

impl Default for Matrix {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for Matrix {
    fn name(&self) -> &'static str {
        "matrix"
    }

    fn init(&mut self, cols: usize, rows: usize, seed: u64) {
        self.rng = Rng::new(seed);
        self.rows = rows;
        self.tick = 0;
        self.tick_accum = 0.0;
        self.cols = (0..cols)
            .map(|_| Self::respawn(&mut self.rng, rows, true))
            .collect();
    }

    fn update(&mut self, dt: f32) {
        // Glyph flicker advances ~12x/s regardless of frame rate.
        self.tick_accum += dt;
        while self.tick_accum >= 1.0 / 12.0 {
            self.tick_accum -= 1.0 / 12.0;
            self.tick = self.tick.wrapping_add(1);
        }
        let rows = self.rows;
        for c in &mut self.cols {
            c.head += c.speed * dt;
            if c.head - c.len as f32 > rows as f32 {
                *c = Self::respawn(&mut self.rng, rows, false);
            }
        }
    }

    fn render(&self, grid: &mut Grid) {
        grid.clear();
        for (x, c) in self.cols.iter().enumerate() {
            let head = c.head.floor() as i32;
            for i in 0..c.len as i32 {
                let y = head - i;
                if y < 0 || y >= grid.rows() as i32 {
                    continue;
                }
                let fade = 1.0 - i as f32 / c.len as f32;
                let brightness = if i == 0 {
                    255
                } else {
                    (fade * fade * 220.0) as u8
                };
                let h = hash3(x as u32, y as u32, self.tick);
                let ch = GLYPHS[(h % GLYPHS.len() as u64) as usize] as char;
                grid.set(x, y as usize, Cell { ch, brightness });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seed: u64) -> String {
        let mut a = Matrix::new();
        a.init(20, 10, seed);
        for _ in 0..30 {
            a.update(1.0 / 24.0);
        }
        let mut g = Grid::new(20, 10);
        a.render(&mut g);
        g.to_text()
    }

    #[test]
    fn deterministic_for_seed() {
        assert_eq!(frame(42), frame(42));
        assert_ne!(frame(42), frame(43));
    }

    #[test]
    fn draws_something_and_stays_in_bounds() {
        let t = frame(7);
        assert!(t.chars().any(|c| c.is_ascii_alphanumeric()));
        assert_eq!(t.lines().count(), 10);
        assert!(t.lines().all(|l| l.chars().count() == 20));
    }
}
