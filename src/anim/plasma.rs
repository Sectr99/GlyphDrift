use super::{Animation, Size};
use crate::grid::{Cell, Grid};

pub struct Plasma {
    ramp: Vec<char>,
    t: f32,
    cols: usize,
    rows: usize,
}

impl Plasma {
    pub fn new(ramp: &str) -> Self {
        let mut ramp: Vec<char> = ramp.chars().collect();
        if ramp.len() < 2 {
            ramp = " .:-=+*#%@".chars().collect();
        }
        Self {
            ramp,
            t: 0.0,
            cols: 0,
            rows: 0,
        }
    }

    /// Field value in [0, 1] at cell (x, y).
    fn field(&self, x: f32, y: f32) -> f32 {
        let t = self.t;
        let v = (x * 0.15 + t).sin()
            + (y * 0.22 + t * 1.3).sin()
            + ((x + y) * 0.1 + t * 0.7).sin()
            + ((x * x + y * y).sqrt() * 0.12 - t).sin();
        (v + 4.0) / 8.0
    }
}

impl Animation for Plasma {
    fn init(&mut self, size: Size, _seed: u64) {
        let (cols, rows) = (size.cols, size.rows);
        self.cols = cols;
        self.rows = rows;
        self.t = 0.0;
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
    }

    fn render(&self, grid: &mut Grid) {
        let n = self.ramp.len();
        for y in 0..grid.rows() {
            for x in 0..grid.cols() {
                // Cells are ~2x taller than wide; stretch y to match.
                let v = self.field(x as f32, y as f32 * 2.0).clamp(0.0, 1.0);
                let idx = ((v * n as f32) as usize).min(n - 1);
                grid.set(
                    x,
                    y,
                    Cell {
                        ch: self.ramp[idx],
                        brightness: (v * 255.0) as u8,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_whole_ramp_range() {
        let mut p = Plasma::new(" .:-=+*#%@");
        p.init(Size::cells(80, 40), 0);
        let mut seen = std::collections::HashSet::new();
        let mut g = Grid::new(80, 40);
        for _ in 0..40 {
            p.update(0.1);
            p.render(&mut g);
            for y in 0..40 {
                for c in g.row(y) {
                    seen.insert(c.ch);
                }
            }
        }
        assert!(seen.len() >= 5, "only saw {} distinct glyphs", seen.len());
    }

    #[test]
    fn bad_ramp_falls_back() {
        let p = Plasma::new("x");
        assert!(p.ramp.len() >= 2);
    }
}
