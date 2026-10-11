//! Topographic-style line fields. Shaded ones draw ASCII relief (cheap) under vector lines.

use std::f32::consts::TAU;

use super::contour::{fbm3, noise3, Field, MIN_CELL};
use super::loops::{Pen, HAIR, MID};
use super::rng::hash3;
use super::{Animation, Rect, Size};
use crate::grid::{Cell, Grid};
use crate::shape::Shape;

fn rand01(a: u32, b: u32) -> f32 {
    (hash3(a, b, 91) & 0xFFFF) as f32 / 65535.0
}

/// Elapsed time and drawing area.
#[derive(Default)]
struct Clock {
    t: f32,
    w: f32,
    h: f32,
    cols: usize,
    rows: usize,
    /// The primary monitor within the window, and its smaller side (the feature scale).
    focus: Rect,
    unit: f32,
}

impl Clock {
    fn init(&mut self, size: Size, seed: u64) {
        (self.w, self.h, self.cols, self.rows) = (size.width, size.height, size.cols, size.rows);
        (self.focus, self.unit) = (size.focus, size.focus.unit().max(1.0));
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
                20 // slow drift: fewer frames save the most
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
line_animation!(Hillshade, "hillshade");
line_animation!(Dunes, "dunes");

/// Draw iso-lines of `field` at ascending `levels`; `style(k)` gives (weight, brightness).
fn contours(
    out: &mut Vec<Shape>,
    pen: Pen,
    field: &Field,
    levels: &[f32],
    style: impl Fn(usize) -> (f32, u8),
) {
    let styles: Vec<(f32, u8)> = (0..levels.len())
        .map(|k| {
            let (weight, brightness) = style(k);
            (pen.px(weight), brightness)
        })
        .collect();
    for (segments, &(width, level)) in field.segments_by_level(levels).iter().zip(&styles) {
        out.extend(
            segments
                .iter()
                .map(|&(a, b)| Shape::Line { a, b, width, level }),
        );
    }
}

impl Ridgelines {
    fn text(&self, grid: &mut Grid) {
        grid.clear();
    }

    /// Stacked terrain profiles flowing toward the viewer, peaking mid-monitor. Drawn front
    /// to back with a floating horizon, so near ridges hide the ones behind.
    fn frame(&self, out: &mut Vec<Shape>) {
        const LINES: usize = 46;
        let Clock { t, w, h, .. } = self.0;
        let unit = self.0.unit;
        let pen = Pen::new(unit);
        let (x0, x1) = (0.0, w);
        let (peak_x, peak_w) = (self.0.focus.x + self.0.focus.w / 2.0, unit * 0.33);
        let samples = ((x1 - x0) / pen.px(4.0)) as usize;
        let (base, spacing, amp) = (h * 0.84, h * 0.012, h * 0.3);
        let mut horizon = vec![f32::MAX; samples + 1];
        for line in 0..LINES {
            // The noise row advances with time: the terrain flows forward.
            let row = line as f32 * 0.16 + t * 0.12;
            let depth = line as f32 / LINES as f32; // 0 front .. 1 back
            let y_base = base - line as f32 * spacing;
            let pts: Vec<(f32, f32)> = (0..=samples)
                .map(|k| {
                    let f = k as f32 / samples as f32;
                    let x = x0 + (x1 - x0) * f;
                    let envelope = (-((x - peak_x) / peak_w).powi(2)).exp();
                    let u = x / unit;
                    let n = (fbm3(u * 3.0, row, t * 0.02) * 0.5 + 0.5).powf(2.4) * 1.6;
                    (
                        x,
                        y_base
                            - amp * envelope * n
                            - h * 0.004 * noise3(u * 18.0, row * 2.0, t * 0.1),
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

/// Drifting pressure systems: (x, y, radius) in px and signed strength.
fn systems(c: &Clock) -> Vec<(f32, f32, f32, f32)> {
    let unit = c.unit;
    // About six per 1.6:1 screen area, so wide desktops aren't sparse.
    let screens = c.w * c.h / (c.unit * c.unit * 1.6);
    let count = (6.0 * screens).round().clamp(4.0, 18.0) as u32;
    (0..count)
        .map(|i| {
            let r = |k: u32| rand01(i, k);
            // Drift east and wrap, wandering north and south.
            let speed = unit * (0.006 + 0.006 * r(1));
            let x = (c.w * r(2) + c.t * speed).rem_euclid(c.w * 1.4) - c.w * 0.2;
            let y = c.h * (0.2 + 0.6 * r(3)) + c.h * 0.05 * (c.t * 0.03 + TAU * r(4)).sin();
            let strength = if i % 2 == 0 { 1.0 } else { -1.0 } * (0.6 + 0.4 * r(5));
            (x, y, unit * (0.22 + 0.15 * r(6)), strength)
        })
        .collect()
}

impl Isobars {
    /// Pressure at (x, y) from this frame's `systems`.
    fn pressure(&self, systems: &[(f32, f32, f32, f32)], x: f32, y: f32) -> f32 {
        let unit = self.0.unit;
        systems
            .iter()
            .map(|&(sx, sy, r, a)| {
                let d2 = ((x - sx).powi(2) + (y - sy).powi(2)) / (r * r);
                // Negligible beyond 3 radii.
                if d2 > 9.0 {
                    0.0
                } else {
                    a * (-d2).exp()
                }
            })
            .sum::<f32>()
            + 0.12 * fbm3(x / (unit * 0.5), y / (unit * 0.5), self.0.t * 0.01)
    }

    /// H and L labels with central pressure.
    fn text(&self, grid: &mut Grid) {
        grid.clear();
        let c = &self.0;
        if c.cols == 0 {
            return;
        }
        let (cw, ch) = (c.w / c.cols as f32, c.h / c.rows as f32);
        let all = systems(c);
        for &(x, y, _, a) in &all {
            if x < 0.0 || x > c.w {
                continue;
            }
            let hpa = 1013.0 + 22.0 * self.pressure(&all, x, y);
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

    /// Isobars every 2 hPa, heavier every 5th.
    fn frame(&self, out: &mut Vec<Shape>) {
        let Clock { w, h, .. } = self.0;
        let pen = Pen::new(self.0.unit);
        let all = systems(&self.0);
        let field = Field::sample(w, h, pen.px(12.0).max(MIN_CELL), |x, y| {
            self.pressure(&all, x, y)
        });
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

    /// Tree rings around a drifting knot; ring widths vary, late wood is heavier.
    fn frame(&self, out: &mut Vec<Shape>) {
        const RINGS: usize = 64;
        let Clock { t, w, h, .. } = self.0;
        let unit = self.0.unit;
        let pen = Pen::new(unit);
        let f = self.0.focus; // the knot sits near the primary monitor's center
        let knot = (
            f.x + f.w * (0.5 + 0.15 * (t * 0.013).sin()),
            f.y + f.h * (0.5 + 0.1 * (t * 0.017).cos()),
        );
        let field = Field::sample(w, h, pen.px(8.0).max(MIN_CELL), |x, y| {
            let (dx, dy) = ((x - knot.0) / (unit * 2.6), (y - knot.1) / unit); // along the grain
            let warp = 0.11 * fbm3(x / (unit * 0.9), y / (unit * 0.18), t * 0.015);
            (dx * dx + dy * dy).sqrt() + warp
        });
        // Irregular growth: good and bad years.
        let mut levels = Vec::with_capacity(RINGS);
        let mut r = 0.0;
        for k in 0..RINGS as u32 {
            r += 0.006 + 0.012 * rand01(k, 5);
            levels.push(r);
        }
        contours(out, pen, &field, &levels, |k| {
            let fade = 1.0 - k as f32 / RINGS as f32 * 0.4;
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

    /// Waves from three wandering sources; contours of their sum make moiré bands.
    fn frame(&self, out: &mut Vec<Shape>) {
        let Clock { t, w, h, .. } = self.0;
        let unit = self.0.unit;
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
        let field = Field::sample(w, h, pen.px(7.0).max(MIN_CELL), |x, y| {
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

/// Heights sampled once per character cell, with slopes from neighboring cells: 5x
/// cheaper than sampling around each cell, for surfaces smooth at that scale.
struct CellHeights {
    cols: usize,
    rows: usize,
    cw: f32,
    ch: f32,
    v: Vec<f32>,
}

impl CellHeights {
    fn sample(c: &Clock, h: impl Fn(f32, f32) -> f32) -> Self {
        let (cols, rows) = (c.cols.max(1), c.rows.max(1));
        let (cw, ch) = (c.w / cols as f32, c.h / rows as f32);
        let mut v = Vec::with_capacity(cols * rows);
        for row in 0..rows {
            for col in 0..cols {
                v.push(h((col as f32 + 0.5) * cw, (row as f32 + 0.5) * ch));
            }
        }
        Self {
            cols,
            rows,
            cw,
            ch,
            v,
        }
    }

    /// (height, d/dx, d/dy) at a cell, slopes per px.
    fn slope(&self, col: usize, row: usize) -> (f32, f32, f32) {
        let at = |c: usize, r: usize| self.v[r * self.cols + c];
        let (c0, c1) = (col.saturating_sub(1), (col + 1).min(self.cols - 1));
        let (r0, r1) = (row.saturating_sub(1), (row + 1).min(self.rows - 1));
        let dx = (at(c1, row) - at(c0, row)) / ((c1 - c0).max(1) as f32 * self.cw);
        let dy = (at(col, r1) - at(col, r0)) / ((r1 - r0).max(1) as f32 * self.ch);
        (at(col, row), dx, dy)
    }
}

/// Shade the grid from `surface(col, row, x, y) -> (height, d/dx, d/dy)` lit by `light`.
/// `relief` scales slopes, `ambient` lifts shadowed faces, `tint(height)` brightens.
fn shade_cells(
    c: &Clock,
    grid: &mut Grid,
    relief: f32,
    light: (f32, f32, f32),
    ambient: f32,
    surface: impl Fn(usize, usize, f32, f32) -> (f32, f32, f32),
    tint: impl Fn(f32) -> f32,
) {
    const RAMP: &[u8] = b" .,:;=+*#%";
    grid.clear();
    if c.cols == 0 || c.rows == 0 {
        return;
    }
    let (cw, ch) = (c.w / c.cols as f32, c.h / c.rows as f32);
    for row in 0..grid.rows().min(c.rows) {
        for col in 0..grid.cols().min(c.cols) {
            let (x, y) = ((col as f32 + 0.5) * cw, (row as f32 + 0.5) * ch);
            let (height, hx, hy) = surface(col, row, x, y);
            let (hx, hy) = (hx * relief, hy * relief);
            let len = (hx * hx + hy * hy + 1.0).sqrt();
            let lit = ((-hx * light.0 - hy * light.1 + light.2) / len).max(0.0);
            let s = ((ambient + (1.0 - ambient) * lit) * tint(height)).clamp(0.0, 1.0);
            if s < 0.12 {
                continue;
            }
            let glyph =
                RAMP[((s * (RAMP.len() - 1) as f32) as usize).clamp(1, RAMP.len() - 1)] as char;
            grid.set(
                col,
                row,
                Cell {
                    ch: glyph,
                    brightness: (20.0 + 130.0 * s) as u8,
                },
            );
        }
    }
}

/// Unit vector.
fn light(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let l = (x * x + y * y + z * z).sqrt();
    (x / l, y / l, z / l)
}

impl Hillshade {
    /// The same terrain as Topographic drift.
    fn height(&self, x: f32, y: f32) -> f32 {
        let unit = self.0.unit;
        let scale = 1.0 / (unit * 0.45);
        fbm3(x * scale + self.0.t * 0.012, y * scale, self.0.t * 0.035)
    }

    /// Shaded relief lit from the upper left; high ground a little brighter.
    fn text(&self, grid: &mut Grid) {
        let unit = self.0.unit;
        let heights = CellHeights::sample(&self.0, |x, y| self.height(x, y));
        shade_cells(
            &self.0,
            grid,
            unit * 0.6,
            light(-0.55, -0.55, 0.62),
            0.0,
            |col, row, _, _| heights.slope(col, row),
            |h| 0.7 + 0.3 * (h + 1.0) / 2.0,
        );
    }

    /// Contours every 0.1 of height, heavier every 4th.
    fn frame(&self, out: &mut Vec<Shape>) {
        let Clock { w, h, .. } = self.0;
        let pen = Pen::new(self.0.unit);
        let field = Field::sample(w, h, pen.px(12.0).max(MIN_CELL), |x, y| self.height(x, y));
        let levels: Vec<f32> = (-8..=8).map(|k| k as f32 * 0.1).collect();
        contours(out, pen, &field, &levels, |k| {
            let high = k as f32 / (levels.len() - 1) as f32;
            if (k as i32 - 8) % 4 == 0 {
                (MID, (150.0 + 100.0 * high) as u8)
            } else {
                (HAIR, (90.0 + 110.0 * high) as u8)
            }
        });
    }
}

/// atan2 to within 0.004 rad, several times faster than `f32::atan2`, and continuous.
fn fast_atan2(y: f32, x: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let (ax, ay) = (x.abs(), y.abs());
    if ax == 0.0 && ay == 0.0 {
        return 0.0;
    }
    // atan(z), z in [0, 1].
    let atan01 = |z: f32| FRAC_PI_4 * z + 0.273 * z * (1.0 - z);
    let a = if ay <= ax {
        atan01(ay / ax)
    } else {
        FRAC_PI_2 - atan01(ax / ay)
    };
    let a = if x < 0.0 { PI - a } else { a };
    if y < 0.0 {
        -a
    } else {
        a
    }
}

/// Dune-ripple values that change per frame, not per point.
struct DuneFrame {
    /// Wavenumber (rad per px) and the across-wind direction.
    k: f32,
    dir: (f32, f32),
    /// Dislocations: position (px) and charge (+1 or -1).
    defects: Vec<(f32, f32, f32)>,
}

impl Dunes {
    fn frame_state(&self) -> DuneFrame {
        let c = &self.0;
        let wind = 0.5 + 0.1 * (c.t * 0.01).sin(); // crests run across the wind
        let defects = (0..4)
            .map(|i| {
                let r = |n: u32| rand01(i + 40, n);
                let x = c.w * (0.15 + 0.7 * r(1) + 0.05 * (c.t * 0.02 + TAU * r(2)).sin());
                let y = c.h * (0.15 + 0.7 * r(3) + 0.05 * (c.t * 0.017 + TAU * r(4)).cos());
                let charge = if i % 2 == 0 { 1.0 } else { -1.0 }; // paired, so they balance
                (x, y, charge)
            })
            .collect();
        DuneFrame {
            // Wavelength 7% of the screen, at least 70 px (small scales would multiply crests).
            k: TAU / (c.unit * 0.07).max(70.0),
            dir: (wind.cos(), wind.sin()),
            defects,
        }
    }

    /// Slow warp that bends the crests.
    fn warp(&self, x: f32, y: f32) -> f32 {
        let unit = self.0.unit;
        4.5 * noise3(x / (unit * 0.55), y / (unit * 0.55), self.0.t * 0.01)
    }

    /// Big, slow swells under the ripples.
    fn swell(&self, x: f32, y: f32) -> f32 {
        let unit = self.0.unit;
        15.0 * fbm3(x / (unit * 0.8), y / (unit * 0.8), self.0.t * 0.005)
    }

    /// Ripple phase and its gradient (per px), given the warp and its gradient. Dislocations
    /// make crests split and end.
    fn phase(&self, d: &DuneFrame, x: f32, y: f32, warp: (f32, f32, f32)) -> (f32, f32, f32) {
        let (mut f, mut fx, mut fy) = (
            d.k * (x * d.dir.0 + y * d.dir.1) + warp.0 - self.0.t * 0.4,
            d.k * d.dir.0 + warp.1,
            d.k * d.dir.1 + warp.2,
        );
        for &(px, py, charge) in &d.defects {
            let (dx, dy) = (x - px, y - py);
            let r2 = (dx * dx + dy * dy).max(1.0);
            f += charge * fast_atan2(dy, dx);
            fx -= charge * dy / r2;
            fy += charge * dx / r2;
        }
        (f, fx, fy)
    }

    /// Gentle windward slope, steep lee face; continuous across the dislocations' 2 pi jumps.
    fn ripple(f: f32) -> f32 {
        (f + 0.45 * f.sin()).sin()
    }

    /// Sand shaded under a low raking light. Swell and warp come from per-cell samples, the
    /// sharp ripples from their exact slope.
    fn text(&self, grid: &mut Grid) {
        let d = self.frame_state();
        let swell = CellHeights::sample(&self.0, |x, y| self.swell(x, y));
        let warp = CellHeights::sample(&self.0, |x, y| self.warp(x, y));
        shade_cells(
            &self.0,
            grid,
            90.0,
            light(-0.85, -0.25, 0.45),
            0.2,
            |col, row, x, y| {
                let (f, fx, fy) = self.phase(&d, x, y, warp.slope(col, row));
                // d/df sin(f + 0.45 sin f) = cos(f + 0.45 sin f) (1 + 0.45 cos f)
                let dr = (f + 0.45 * f.sin()).cos() * (1.0 + 0.45 * f.cos());
                let (s, sx, sy) = swell.slope(col, row);
                (
                    s + 0.18 * Self::ripple(f),
                    sx + 0.18 * dr * fx,
                    sy + 0.18 * dr * fy,
                )
            },
            |_| 1.0,
        );
    }

    /// A fine line along each crest.
    fn frame(&self, out: &mut Vec<Shape>) {
        let Clock { w, h, .. } = self.0;
        let pen = Pen::new(self.0.unit);
        let d = self.frame_state();
        // The warp is smooth, so a coarse grid interpolates it.
        let warp = Field::sample(w, h, pen.px(48.0), |x, y| self.warp(x, y));
        // Traced just below the crest, where the band is wide enough to follow. 5 px still
        // resolves it at the smallest scale.
        let field = Field::sample(w, h, pen.px(6.0).max(5.0), |x, y| {
            let (f, _, _) = self.phase(&d, x, y, (warp.value_at(x, y), 0.0, 0.0));
            Self::ripple(f)
        });
        contours(out, pen, &field, &[0.75], |_| (HAIR, 150));
    }
}

#[cfg(test)]
mod tests {
    use super::fast_atan2;

    #[test]
    fn fast_atan2_is_close_everywhere() {
        let mut worst = 0.0f32;
        for i in 0..3600 {
            let t = i as f32 * std::f32::consts::TAU / 3600.0;
            for r in [0.01f32, 1.0, 250.0] {
                let (y, x) = (r * t.sin(), r * t.cos());
                worst = worst.max((fast_atan2(y, x) - y.atan2(x)).abs());
            }
        }
        assert!(worst < 0.005, "worst error {worst}");
    }
}
