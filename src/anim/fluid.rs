//! Fluid animations: a lava lamp (metaballs), topographic drift (contours of evolving
//! noise terrain), and ink in water (a stable-fluids simulation rendered as ASCII).

use std::f32::consts::TAU;

use super::contour::{fbm3, Field};
use super::loops::{Pen, HAIR, MID};
use super::rng::hash3;
use super::{Animation, Size};
use crate::grid::{Cell, Grid};
use crate::shape::Shape;

fn rand01(a: u32, b: u32) -> f32 {
    (hash3(a, b, 77) & 0xFFFF) as f32 / 65535.0
}

// ---------------------------------------------------------------------------------------
// Lava lamp

/// Blobs per primary-monitor area (a 1.6:1 screen gets about seven).
const BLOBS_PER_SCREEN: f32 = 7.0;

/// Soft metaball blobs that rise, merge and split on slow, staggered cycles: a glowing
/// halftone core, a crisp contour at the blob edge and a faint halo line around it.
#[derive(Default)]
pub struct LavaLamp {
    t: f32,
    w: f32,
    h: f32,
    /// Feature scale: the primary monitor's smaller side.
    unit: f32,
}

impl LavaLamp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Blob centers and radii (px) at the current time.
    fn blobs(&self) -> Vec<(f32, f32, f32)> {
        let unit = self.unit;
        let count = (self.w * self.h / (unit * unit * 1.6) * BLOBS_PER_SCREEN)
            .round()
            .clamp(3.0, 20.0) as u32;
        (0..count)
            .map(|i| {
                let r = |k: u32| rand01(i, k);
                let speed = 0.05 + 0.07 * r(1); // each blob has its own rise-and-sink cycle
                let x = self.w * (0.15 + 0.7 * r(2))
                    + self.w * 0.06 * (self.t * (0.05 + 0.04 * r(3)) + TAU * r(4)).sin();
                let y = self.h * (0.5 + 0.4 * (self.t * speed + TAU * r(5)).sin());
                let radius =
                    unit * (0.06 + 0.06 * r(6)) * (1.0 + 0.15 * (self.t * 0.3 + TAU * r(7)).sin());
                (x, y, radius)
            })
            .collect()
    }
}

impl Animation for LavaLamp {
    fn name(&self) -> &'static str {
        "lava"
    }
    fn warm(&self) -> bool {
        true
    }
    fn fps(&self) -> u32 {
        30
    }
    fn init(&mut self, size: Size, seed: u64) {
        (self.w, self.h, self.unit) = (size.width, size.height, size.focus.unit().max(1.0));
        self.t = (seed % 1000) as f32;
    }
    fn update(&mut self, dt: f32) {
        self.t += dt;
    }
    fn draw(&self, out: &mut Vec<Shape>) {
        let pen = Pen::new(self.unit);
        let blobs = self.blobs();
        // Metaball field: 1.0 at a lone blob's radius, larger inside, merging smoothly.
        let field = |x: f32, y: f32| {
            blobs
                .iter()
                .map(|&(bx, by, r)| r * r / ((x - bx).powi(2) + (y - by).powi(2) + 1.0))
                .sum::<f32>()
        };
        // Glowing core: halftone dots that grow toward the center of each mass.
        let step = pen.px(10.0);
        let (nx, ny) = (
            (self.w / step) as i32 + 1,
            (self.h / (step * 0.866)) as i32 + 1,
        );
        for j in 0..ny {
            for i in 0..nx {
                let x = (i as f32 + if j % 2 != 0 { 0.5 } else { 0.0 }) * step;
                let y = j as f32 * step * 0.866;
                let f = field(x, y);
                if f < 1.0 {
                    continue;
                }
                let glow = ((f - 1.0) / 2.5).clamp(0.0, 1.0);
                out.push(Shape::Circle {
                    c: (x, y),
                    r: step * (0.16 + 0.24 * glow.sqrt()), // capped below touching, so cores keep texture
                    fill: true,
                    level: (90.0 + 150.0 * glow) as u8,
                });
            }
        }
        // Edge and halo contours.
        let grid = Field::sample(self.w, self.h, pen.px(9.0), field);
        for (level, weight, brightness) in [(1.0, MID, 210), (0.62, HAIR, 70)] {
            grid.contour(level, |a, b| {
                out.push(Shape::Line {
                    a,
                    b,
                    width: pen.px(weight),
                    level: brightness,
                });
            });
        }
    }
}

// ---------------------------------------------------------------------------------------
// Topographic drift

/// Contour lines over slowly evolving noise terrain. Every fifth line is a heavier index
/// contour, as on a survey map; higher ground is brighter. With a clock, the terrain is
/// warped around a box at the primary monitor's center, so the lines flow around it the way
/// streamlines pass an obstacle.
#[derive(Default)]
pub struct Topographic {
    t: f32,
    w: f32,
    h: f32,
    /// Feature scale: the primary monitor's smaller side.
    unit: f32,
    /// Clock box center and half-size (px).
    center: (f32, f32),
    half: (f32, f32),
    /// Time and date, when the clock is on.
    clock: Option<[String; 2]>,
}

impl Topographic {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where terrain point (x, y) is sampled from: with the clock on, points outside the box
    /// are pulled toward its center (fully at the edge, fading out with distance), and
    /// points inside return None. Without the clock, the identity.
    fn warp(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        if self.clock.is_none() {
            return Some((x, y));
        }
        let (qx, qy) = (x - self.center.0, y - self.center.1);
        let (ux, uy) = (qx / self.half.0, qy / self.half.1);
        // Superellipse "radius": 1 on a rounded-rectangle boundary.
        let rho = (ux.powi(4) + uy.powi(4)).sqrt().sqrt();
        if rho <= 1.0 {
            return None;
        }
        let k = (rho - 1.0 / (rho * rho)) / rho; // 0 at the edge, toward 1 far away
        Some((self.center.0 + qx * k, self.center.1 + qy * k))
    }
}

impl Animation for Topographic {
    fn name(&self) -> &'static str {
        "topo"
    }
    fn fps(&self) -> u32 {
        30
    }
    fn init(&mut self, size: Size, seed: u64) {
        let f = size.focus;
        (self.w, self.h, self.unit) = (size.width, size.height, f.unit().max(1.0));
        let obj = f.object_unit().max(1.0);
        self.center = (f.x + f.w * 0.5, f.y + f.h * 0.5);
        self.half = (obj * 0.22, obj * 0.095);
        self.t = (seed % 1000) as f32;
    }
    fn update(&mut self, dt: f32) {
        self.t += dt;
    }
    fn set_clock(&mut self, clock: Option<&[String; 2]>) {
        if self.clock.as_ref() != clock {
            self.clock = clock.cloned();
        }
    }
    fn draw(&self, out: &mut Vec<Shape>) {
        const LEVELS: i32 = 16; // contour interval: 1/LEVELS of the height range per side
        let unit = self.unit;
        let pen = Pen::new(unit);
        let (scale, drift, morph) = (1.0 / (unit * 0.45), self.t * 0.012, self.t * 0.035);
        let height = |x: f32, y: f32| fbm3(x * scale + drift, y * scale, morph);
        let (cx, cy) = self.center;
        let inside = height(cx, cy); // flat inside the box: no lines cross it
        let grid = Field::sample(self.w, self.h, pen.px(12.0), |x, y| match self.warp(x, y) {
            Some((x, y)) => height(x, y),
            None => inside,
        });
        for k in -LEVELS..=LEVELS {
            let level = k as f32 / LEVELS as f32 * 0.8;
            let index = k % 5 == 0; // heavier index contour
            let height = (level / 0.8 + 1.0) / 2.0; // 0 low .. 1 high
            let brightness =
                (40.0 + 150.0 * height + if index { 40.0 } else { 0.0 }).min(255.0) as u8;
            let weight = if index { MID } else { HAIR };
            grid.contour(level, |a, b| {
                out.push(Shape::Line {
                    a,
                    b,
                    width: pen.px(weight),
                    level: brightness,
                });
            });
        }
        if let Some([time, date]) = &self.clock {
            let (hw, hh) = self.half;
            // Corner brackets: a survey-map frame, inset inside the rounded boundary.
            let (bx, by, arm) = (hw * 0.8, hh * 0.78, hh * 0.22);
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = (cx + sx * bx, cy + sy * by);
                for end in [
                    (corner.0 - sx * arm, corner.1),
                    (corner.0, corner.1 - sy * arm),
                ] {
                    out.push(Shape::Line {
                        a: corner,
                        b: end,
                        width: pen.px(MID),
                        level: 150,
                    });
                }
            }
            out.push(Shape::Text {
                c: (cx, cy - hh * 0.12),
                size: hh * 0.82,
                weight: 300,
                text: time.clone(),
                level: 255,
            });
            out.push(Shape::Text {
                c: (cx, cy + hh * 0.5),
                size: hh * 0.17,
                weight: 400,
                text: date.to_uppercase(),
                level: 185,
            });
        }
    }
}

// ---------------------------------------------------------------------------------------
// Ink in water

const STEP: f32 = 1.0 / 30.0; // fixed simulation step
const JACOBI: usize = 20; // pressure solver iterations
const EMITTERS: u32 = 4;
/// Simulation width cap (cells): keeps the solver cheap on large screens; the grid is
/// sampled smoothly onto however many characters the screen has.
const MAX_SIM_W: usize = 200;
const RAMP: &[u8] = b" .,:-=+*#%@";

/// Ink in water: Jos Stam's stable fluids (semi-Lagrangian advection and a pressure
/// projection) on a periodic grid with square cells, twice the character rows. Wandering
/// nozzles inject dye and swirling force; the dye is carried along, curls and slowly fades.
#[derive(Default)]
pub struct Ink {
    /// Simulation grid size (square cells).
    w: usize,
    h: usize,
    /// Character grid size.
    cols: usize,
    rows: usize,
    u: Vec<f32>,
    v: Vec<f32>,
    dye: Vec<f32>,
    scratch: [Vec<f32>; 3],
    t: f32,
    accum: f32,
}

impl Ink {
    pub fn new() -> Self {
        Self::default()
    }

    fn idx(&self, i: isize, j: isize) -> usize {
        let (w, h) = (self.w as isize, self.h as isize);
        (j.rem_euclid(h) * w + i.rem_euclid(w)) as usize
    }

    /// Bilinear sample of `f` at fractional cell (x, y), wrapping around the edges.
    fn sample(&self, f: &[f32], x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (i, j) = (x0 as isize, y0 as isize);
        let a = f[self.idx(i, j)] + (f[self.idx(i + 1, j)] - f[self.idx(i, j)]) * fx;
        let b = f[self.idx(i, j + 1)] + (f[self.idx(i + 1, j + 1)] - f[self.idx(i, j + 1)]) * fx;
        a + (b - a) * fy
    }

    /// Carry field `f` along the velocity for one step (trace back and sample).
    fn advect(&self, f: &[f32], out: &mut [f32]) {
        for j in 0..self.h {
            for i in 0..self.w {
                let k = j * self.w + i;
                let (x, y) = (
                    i as f32 - STEP * 30.0 * self.u[k],
                    j as f32 - STEP * 30.0 * self.v[k],
                );
                out[k] = self.sample(f, x, y);
            }
        }
    }

    /// Make the velocity divergence-free (incompressible), Jacobi-solving for pressure.
    fn project(&mut self) {
        let n = self.w * self.h;
        let [div, p, tmp] = &mut self.scratch;
        div.resize(n, 0.0);
        p.clear();
        p.resize(n, 0.0);
        tmp.resize(n, 0.0);
        let (w, h) = (self.w as isize, self.h as isize);
        let at = |i: isize, j: isize| (j.rem_euclid(h) * w + i.rem_euclid(w)) as usize;
        for j in 0..h {
            for i in 0..w {
                div[at(i, j)] = -0.5
                    * (self.u[at(i + 1, j)] - self.u[at(i - 1, j)] + self.v[at(i, j + 1)]
                        - self.v[at(i, j - 1)]);
            }
        }
        for _ in 0..JACOBI {
            for j in 0..h {
                for i in 0..w {
                    tmp[at(i, j)] = (div[at(i, j)]
                        + p[at(i - 1, j)]
                        + p[at(i + 1, j)]
                        + p[at(i, j - 1)]
                        + p[at(i, j + 1)])
                        / 4.0;
                }
            }
            std::mem::swap(p, tmp);
        }
        for j in 0..h {
            for i in 0..w {
                let k = at(i, j);
                self.u[k] -= 0.5 * (p[at(i + 1, j)] - p[at(i - 1, j)]);
                self.v[k] -= 0.5 * (p[at(i, j + 1)] - p[at(i, j - 1)]);
            }
        }
    }

    fn step(&mut self) {
        self.t += STEP;
        let (w, h) = (self.w as f32, self.h as f32);
        // Nozzles wander on slow paths, each pushing in a slowly turning direction.
        for e in 0..EMITTERS {
            let r = |k: u32| rand01(e, k);
            // Each nozzle wanders around its own share of the width, so ink stays spread out.
            let home = (e as f32 + 0.5) / EMITTERS as f32;
            let cx = w * (home + 0.16 * (self.t * (0.05 + 0.04 * r(1)) + TAU * r(2)).sin());
            let cy = h * (0.5 + 0.36 * (self.t * (0.04 + 0.03 * r(3)) + TAU * r(4)).cos());
            let angle = self.t * (0.3 + 0.3 * r(5)) + TAU * r(6);
            let (fx, fy) = (angle.cos() * 0.35, angle.sin() * 0.35);
            let radius = 3.5f32;
            for dj in -4..=4 {
                for di in -4..=4 {
                    let d2 = (di * di + dj * dj) as f32;
                    if d2 > radius * radius {
                        continue;
                    }
                    let fall = 1.0 - d2 / (radius * radius);
                    let k = self.idx(cx as isize + di, cy as isize + dj);
                    self.dye[k] = (self.dye[k] + 0.16 * fall).min(1.5);
                    self.u[k] += fx * fall * 0.2;
                    self.v[k] += fy * fall * 0.2;
                }
            }
        }
        let n = self.w * self.h;
        let mut next = std::mem::take(&mut self.scratch[0]);
        next.resize(n, 0.0);
        self.advect(&self.u.clone(), &mut next);
        std::mem::swap(&mut self.u, &mut next);
        self.advect(&self.v.clone(), &mut next);
        std::mem::swap(&mut self.v, &mut next);
        self.scratch[0] = next;
        for x in self.u.iter_mut().chain(self.v.iter_mut()) {
            *x *= 0.995; // a little drag keeps it calm
        }
        self.project();
        let mut next = std::mem::take(&mut self.scratch[0]);
        next.resize(n, 0.0);
        self.advect(&self.dye, &mut next);
        for d in next.iter_mut() {
            *d *= 0.996; // ink slowly dissolves (half-life about 6 s)
        }
        std::mem::swap(&mut self.dye, &mut next);
        self.scratch[0] = next;
    }
}

impl Animation for Ink {
    fn name(&self) -> &'static str {
        "ink"
    }

    fn init(&mut self, size: Size, seed: u64) {
        // Character cells are about twice as tall as wide, so square sim cells need twice
        // the rows; then cap the width (and scale the height to match).
        (self.cols, self.rows) = (size.cols.max(4), size.rows.max(2));
        self.w = self.cols.min(MAX_SIM_W);
        self.h = (self.rows * 2 * self.w / self.cols).max(2);
        let n = self.w * self.h;
        self.u = vec![0.0; n];
        self.v = vec![0.0; n];
        self.dye = vec![0.0; n];
        self.t = (seed % 1000) as f32;
        self.accum = 0.0;
        for _ in 0..120 {
            self.step(); // start with ink already swirling
        }
    }

    fn update(&mut self, dt: f32) {
        // At most a few solver steps per frame; after a stall (sleep, a long pause) the
        // backlog is dropped rather than ground through.
        self.accum += dt;
        for _ in 0..4 {
            if self.accum < STEP {
                break;
            }
            self.accum -= STEP;
            self.step();
        }
        self.accum = self.accum.min(STEP);
    }

    fn render(&self, grid: &mut Grid) {
        grid.clear();
        if self.cols == 0 {
            return;
        }
        let (sx, sy) = (
            self.w as f32 / self.cols as f32,
            self.h as f32 / self.rows as f32,
        );
        for row in 0..grid.rows().min(self.rows) {
            for col in 0..grid.cols().min(self.cols) {
                let x = (col as f32 + 0.5) * sx - 0.5;
                let y = (row as f32 + 0.5) * sy - 0.5;
                let d = self.sample(&self.dye, x, y).clamp(0.0, 1.0).powf(0.8);
                if d < 0.03 {
                    continue;
                }
                let ch =
                    RAMP[((d * (RAMP.len() - 1) as f32) as usize).clamp(1, RAMP.len() - 1)] as char;
                grid.set(
                    col,
                    row,
                    Cell {
                        ch,
                        brightness: (40.0 + 215.0 * d) as u8,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::Rect;

    #[test]
    fn topo_lines_flow_around_the_clock_box() {
        let mut t = Topographic::new();
        t.init(
            Size {
                cols: 200,
                rows: 60,
                width: 1600.0,
                height: 1000.0,
                focus: Rect::new(0.0, 0.0, 1600.0, 1000.0),
            },
            5,
        );
        t.set_clock(Some(&["12:34".into(), "Saturday".into()]));
        for _ in 0..4 {
            t.update(9.0);
            let mut out = Vec::new();
            t.draw(&mut out);
            assert!(out.iter().any(|s| matches!(s, Shape::Text { .. })));

            for s in &out {
                if let Shape::Line { a, b, level, .. } = *s {
                    if level == 150 {
                        continue; // the frame's corner brackets
                    }
                    for p in [a, b] {
                        let (ux, uy) = ((p.0 - 800.0) / t.half.0, (p.1 - 500.0) / t.half.1);
                        let inside = ux.powi(4) + uy.powi(4) < 0.5; // one 12 px cell of slack
                        assert!(!inside, "contour point {p:?} inside the clock box");
                    }
                }
            }
        }
    }
}
