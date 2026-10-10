//! Line-field animations in the topographic style: ridgelines, isobars, wood grain and wave
//! interference. Fine lines, a weight hierarchy, slow organic change.

use std::f32::consts::TAU;

use super::contour::{fbm3, noise3, Field};
use super::loops::{Pen, HAIR, MID};
use super::rng::hash3;
use super::{Animation, Size};
use crate::grid::{Cell, Grid};
use crate::shape::Shape;

fn rand01(a: u32, b: u32) -> f32 {
    (hash3(a, b, 91) & 0xFFFF) as f32 / 65535.0
}

/// Elapsed time and drawing area, shared by every animation here.
#[derive(Default)]
struct Clock {
    t: f32,
    w: f32,
    h: f32,
    cols: usize,
    rows: usize,
}

impl Clock {
    fn init(&mut self, size: Size, seed: u64) {
        (self.w, self.h, self.cols, self.rows) = (size.width, size.height, size.cols, size.rows);
        self.t = (seed % 1000) as f32;
    }
}

macro_rules! line_animation {
    ($ty:ident, $name:literal) => {
        #[derive(Default)]
        pub struct $ty(Clock);

        impl $ty {
            pub fn new() -> Self {
                Self::default()
            }
        }

        impl Animation for $ty {
            fn name(&self) -> &'static str {
                $name
            }
            fn fps(&self) -> u32 {
                30
            }
            fn init(&mut self, size: Size, seed: u64) {
                self.0.init(size, seed);
            }
            fn update(&mut self, dt: f32) {
                self.0.t += dt;
            }
            fn render(&self, grid: &mut Grid) {
                self.text(grid);
            }
            fn draw(&self, out: &mut Vec<Shape>) {
                self.frame(out);
            }
        }
    };
}

line_animation!(Ridgelines, "ridges");
line_animation!(Isobars, "isobars");
line_animation!(WoodGrain, "wood");
line_animation!(Interference, "interference");

/// Draw every iso-line of `field` at `levels`, styling each with `style(index) -> (weight,
/// brightness)`.
fn contours(
    out: &mut Vec<Shape>,
    pen: Pen,
    field: &Field,
    levels: &[f32],
    style: impl Fn(usize) -> (f32, u8),
) {
    for (k, &level) in levels.iter().enumerate() {
        let (weight, brightness) = style(k);
        field.contour(level, |a, b| {
            out.push(Shape::Line {
                a,
                b,
                width: pen.px(weight),
                level: brightness,
            });
        });
    }
}

impl Ridgelines {
    fn text(&self, grid: &mut Grid) {
        grid.clear();
    }

    /// Stacked terrain profiles, peaking toward the middle, flowing toward the viewer.
    /// Drawn front to back with a floating horizon: a line shows only where it rises above
    /// everything already drawn, so near ridges hide the ones behind.
    fn frame(&self, out: &mut Vec<Shape>) {
        const LINES: usize = 46;
        let Clock { t, w, h, .. } = self.0;
        let unit = w.min(h);
        let pen = Pen::new(unit);
        let (x0, x1) = (w * 0.12, w * 0.88);
        let samples = ((x1 - x0) / pen.px(4.0)) as usize;
        let (base, spacing, amp) = (h * 0.84, h * 0.012, h * 0.3);
        let mut horizon = vec![f32::MAX; samples + 1];
        for line in 0..LINES {
            // Noise row coordinate advances with time: the terrain flows toward the viewer.
            let row = line as f32 * 0.16 + t * 0.12;
            let depth = line as f32 / LINES as f32; // 0 front .. 1 back
            let y_base = base - line as f32 * spacing;
            let pts: Vec<(f32, f32)> = (0..=samples)
                .map(|k| {
                    let f = k as f32 / samples as f32;
                    let x = x0 + (x1 - x0) * f;
                    let envelope = (-((f - 0.5) / 0.16).powi(2)).exp(); // peaks in the middle
                    let n = (fbm3(f * 5.0, row, t * 0.02) * 0.5 + 0.5).powf(2.4) * 1.6;
                    (
                        x,
                        y_base
                            - amp * envelope * n
                            - h * 0.004 * noise3(f * 30.0, row * 2.0, t * 0.1),
                    )
                })
                .collect();
            let brightness = (60.0 + 180.0 * (1.0 - depth)) as u8;
            let weight = if line % 5 == 0 {
                MID
            } else {
                HAIR + 0.3 * (1.0 - depth)
            };
            for k in 0..samples {
                let (a, b) = (pts[k], pts[k + 1]);
                if a.1 < horizon[k] && b.1 < horizon[k + 1] {
                    out.push(Shape::Line {
                        a,
                        b,
                        width: pen.px(weight),
                        level: brightness,
                    });
                }
            }
            for (hz, p) in horizon.iter_mut().zip(&pts) {
                *hz = hz.min(p.1);
            }
        }
    }
}

/// A drifting high- or low-pressure system: (x, y, radius) in px, signed strength.
fn systems(c: &Clock) -> Vec<(f32, f32, f32, f32)> {
    let unit = c.w.min(c.h);
    (0..6)
        .map(|i| {
            let r = |k: u32| rand01(i, k);
            // Systems drift eastward and wrap, wandering a little north and south.
            let speed = unit * (0.006 + 0.006 * r(1));
            let x = (c.w * r(2) + c.t * speed).rem_euclid(c.w * 1.4) - c.w * 0.2;
            let y = c.h * (0.2 + 0.6 * r(3)) + c.h * 0.05 * (c.t * 0.03 + TAU * r(4)).sin();
            let strength = if i % 2 == 0 { 1.0 } else { -1.0 } * (0.6 + 0.4 * r(5));
            (x, y, unit * (0.22 + 0.15 * r(6)), strength)
        })
        .collect()
}

impl Isobars {
    fn pressure(&self, x: f32, y: f32) -> f32 {
        let unit = self.0.w.min(self.0.h);
        systems(&self.0)
            .iter()
            .map(|&(sx, sy, r, a)| a * (-((x - sx).powi(2) + (y - sy).powi(2)) / (r * r)).exp())
            .sum::<f32>()
            + 0.12 * fbm3(x / (unit * 0.5), y / (unit * 0.5), self.0.t * 0.01)
    }

    /// H and L markers with their central pressure, in the character grid.
    fn text(&self, grid: &mut Grid) {
        grid.clear();
        let c = &self.0;
        if c.cols == 0 {
            return;
        }
        let (cw, ch) = (c.w / c.cols as f32, c.h / c.rows as f32);
        for (x, y, _, a) in systems(c) {
            if x < 0.0 || x > c.w {
                continue;
            }
            let hpa = 1013.0 + 22.0 * self.pressure(x, y);
            let label = format!("{} {:.0}", if a > 0.0 { 'H' } else { 'L' }, hpa);
            let (col, row) = ((x / cw) as isize - 1, (y / ch) as isize);
            for (i, chr) in label.chars().enumerate() {
                let cx = col + i as isize;
                if cx >= 0 && row >= 0 && chr != ' ' {
                    let level = if i == 0 { 230 } else { 120 };
                    grid.set(
                        cx as usize,
                        row as usize,
                        Cell {
                            ch: chr,
                            brightness: level,
                        },
                    );
                }
            }
        }
    }

    /// Isobars every 2 hPa, heavier every 5th; brighter toward high pressure.
    fn frame(&self, out: &mut Vec<Shape>) {
        let Clock { w, h, .. } = self.0;
        let pen = Pen::new(w.min(h));
        let field = Field::sample(w, h, pen.px(12.0), |x, y| self.pressure(x, y));
        let levels: Vec<f32> = (-12..=12).map(|k| k as f32 * 2.0 / 22.0).collect();
        contours(out, pen, &field, &levels, |k| {
            let high = k as f32 / (levels.len() - 1) as f32;
            if (k as i32 - 12) % 5 == 0 {
                (MID, (110.0 + 130.0 * high) as u8)
            } else {
                (HAIR, (45.0 + 120.0 * high) as u8)
            }
        });
    }
}

impl WoodGrain {
    fn text(&self, grid: &mut Grid) {
        grid.clear();
    }

    /// Tree rings: elongated, noise-warped rings around a slowly drifting knot. Ring widths
    /// vary year to year, and late-wood rings (every other) are heavier and brighter.
    fn frame(&self, out: &mut Vec<Shape>) {
        const RINGS: usize = 64;
        let Clock { t, w, h, .. } = self.0;
        let unit = w.min(h);
        let pen = Pen::new(unit);
        let knot = (
            w * (0.5 + 0.15 * (t * 0.013).sin()),
            h * (0.5 + 0.1 * (t * 0.017).cos()),
        );
        let field = Field::sample(w, h, pen.px(8.0), |x, y| {
            let (dx, dy) = ((x - knot.0) / (unit * 2.6), (y - knot.1) / unit); // stretched along the grain
                                                                               // Warp streaked along the grain: long in x, short in y.
            let warp = 0.11 * fbm3(x / (unit * 0.9), y / (unit * 0.18), t * 0.015);
            (dx * dx + dy * dy).sqrt() + warp
        });
        // Irregular growth: each ring's width varies, as good and bad years do.
        let mut levels = Vec::with_capacity(RINGS);
        let mut r = 0.0;
        for k in 0..RINGS as u32 {
            r += 0.006 + 0.012 * rand01(k, 5);
            levels.push(r);
        }
        contours(out, pen, &field, &levels, |k| {
            let fade = 1.0 - k as f32 / RINGS as f32 * 0.4; // outer rings slightly dimmer
            if k % 2 == 0 {
                (HAIR + 0.55, (200.0 * fade) as u8) // late wood
            } else {
                (HAIR, (115.0 * fade) as u8)
            }
        });
    }
}

impl Interference {
    fn text(&self, grid: &mut Grid) {
        grid.clear();
    }

    /// Circular waves from three slowly wandering sources; contours of their sum make
    /// shifting moiré bands, like a ripple tank seen from above.
    fn frame(&self, out: &mut Vec<Shape>) {
        let Clock { t, w, h, .. } = self.0;
        let unit = w.min(h);
        let pen = Pen::new(unit);
        let sources: Vec<(f32, f32)> = (0..3)
            .map(|i| {
                let r = |k: u32| rand01(i + 10, k);
                (
                    w * (0.5 + 0.3 * (t * (0.02 + 0.02 * r(1)) + TAU * r(2)).sin()),
                    h * (0.5 + 0.3 * (t * (0.02 + 0.02 * r(3)) + TAU * r(4)).cos()),
                )
            })
            .collect();
        let k = TAU / (unit * 0.12); // wavelength: 12% of the screen
        let field = Field::sample(w, h, pen.px(7.0), |x, y| {
            sources
                .iter()
                .map(|&(sx, sy)| (k * ((x - sx).powi(2) + (y - sy).powi(2)).sqrt() - t * 0.8).cos())
                .sum::<f32>()
                / 3.0
        });
        contours(out, pen, &field, &[-0.6, 0.0, 0.6], |i| match i {
            1 => (MID, 190),
            _ => (HAIR, 80),
        });
    }
}
