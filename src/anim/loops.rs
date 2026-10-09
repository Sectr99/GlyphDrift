//! Minimal geometric loops: one centered line-art object, shaded by depth.
//!
//! All four share the same shape: a clock `t`, the window size, and a `draw` that builds the
//! frame from scratch. They hold no other state, so any frame is a pure function of `t`.

use std::f32::consts::{PI, TAU};

use super::geom::{center, shade, V3};
use super::{Animation, Size};
use crate::shape::Shape;

/// Shared state for every loop: elapsed time and drawing area.
#[derive(Default)]
struct Clock {
    t: f32,
    w: f32,
    h: f32,
}

impl Clock {
    fn init(&mut self, size: Size, seed: u64) {
        (self.w, self.h) = (size.width, size.height);
        self.t = (seed % 1000) as f32; // restarts don't all begin at the same pose
    }
}

fn line(out: &mut Vec<Shape>, a: (f32, f32), b: (f32, f32), level: u8) {
    out.push(Shape::Line {
        a,
        b,
        width: 1.0,
        level,
    });
}

/// Polyline through `points` (with depth), each segment shaded by its mean depth.
fn shaded_path(out: &mut Vec<Shape>, points: &[((f32, f32), f32)], far: f32, near: f32) {
    for seg in points.windows(2) {
        let ((a, za), (b, zb)) = (seg[0], seg[1]);
        line(out, a, b, shade((za + zb) / 2.0, far, near));
    }
}

macro_rules! loop_animation {
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
            fn draw(&self, out: &mut Vec<Shape>) {
                self.frame(out);
            }
        }
    };
}

loop_animation!(Helix, "helix");
loop_animation!(Globe, "globe");
loop_animation!(DotWave, "dotwave");
loop_animation!(Cubes, "cubes");

impl Helix {
    /// A double helix lying across the screen, turning about its long axis.
    fn frame(&self, out: &mut Vec<Shape>) {
        const N: usize = 40;
        let Clock { t, w, h } = self.0;
        let (c, unit) = center(w, h);
        let (half_len, r) = ((w * 0.3).min(unit * 0.6), unit * 0.09);
        let mut a_pts = Vec::with_capacity(N);
        let mut b_pts = Vec::with_capacity(N);
        for i in 0..N {
            let f = i as f32 / (N - 1) as f32;
            let x = c.0 + (f * 2.0 - 1.0) * half_len;
            let ang = f * TAU * 2.5 + t * 0.6;
            let (y, z) = (ang.cos(), ang.sin());
            a_pts.push(((x, c.1 + y * r), z));
            b_pts.push(((x, c.1 - y * r), -z));
            if i % 2 == 0 {
                line(out, (x, c.1 + y * r), (x, c.1 - y * r), 45); // rung
            }
        }
        shaded_path(out, &a_pts, 30.0, 170.0);
        shaded_path(out, &b_pts, 30.0, 170.0);
        for &(p, z) in a_pts.iter().chain(&b_pts).step_by(2) {
            out.push(Shape::Circle {
                c: p,
                r: 2.0 + 1.2 * z,
                fill: true,
                level: shade(z, 50.0, 235.0),
            });
        }
    }
}

impl Globe {
    /// A tilted wireframe sphere rotating slowly.
    fn frame(&self, out: &mut Vec<Shape>) {
        const SEG: usize = 48;
        let Clock { t, w, h } = self.0;
        let (c, unit) = center(w, h);
        let r = unit * 0.3;
        let (spin, tilt) = (t * 0.25, 0.4);
        let project = |p: V3| {
            let q = p.rot_y(spin).rot_x(tilt);
            (q.screen(c, r), q.z)
        };
        let ring = |f: &dyn Fn(f32) -> V3| -> Vec<((f32, f32), f32)> {
            (0..=SEG)
                .map(|k| project(f(k as f32 / SEG as f32 * TAU)))
                .collect()
        };
        for m in 0..6 {
            // Great circles through the poles: 6 of them draw 12 meridians.
            let lon = m as f32 * PI / 6.0;
            let pts = ring(&|a| V3::new(a.cos() * lon.cos(), a.sin(), a.cos() * lon.sin()));
            shaded_path(out, &pts, 25.0, 190.0);
        }
        for j in 1..8 {
            let lat = -PI / 2.0 + j as f32 * PI / 8.0;
            let (y, rr) = (lat.sin(), lat.cos());
            let pts = ring(&|a| V3::new(a.cos() * rr, y, a.sin() * rr));
            shaded_path(out, &pts, 25.0, 190.0);
        }
    }
}

impl DotWave {
    /// A field of dots seen in perspective, with ripples spreading from its center.
    fn frame(&self, out: &mut Vec<Shape>) {
        const COLS: usize = 44;
        const ROWS: usize = 26;
        let Clock { t, w, h } = self.0;
        let (horizon, focal, cam_h) = (h * 0.42, h * 0.55, 0.7);
        for j in 0..ROWS {
            let depth = 1.2 + j as f32 / (ROWS - 1) as f32 * 5.0; // near .. far
            for i in 0..COLS {
                let x = (i as f32 / (COLS - 1) as f32 * 2.0 - 1.0) * 3.2;
                let d = (x * x + (depth - 3.5).powi(2)).sqrt();
                let y = 0.18 * (d * 2.2 - t * 1.4).sin();
                let p = (
                    w / 2.0 + x / depth * focal,
                    horizon + (cam_h - y) / depth * focal,
                );
                if p.0 < 0.0 || p.0 > w || p.1 > h {
                    continue;
                }
                let near = 1.0 - (depth - 1.2) / 5.0; // 1 near, 0 far
                let lift = (y / 0.18 + 1.0) / 2.0; // crests brighter
                out.push(Shape::Circle {
                    c: p,
                    r: 0.8 + 2.2 * near,
                    fill: true,
                    level: (40.0 + 190.0 * near * (0.45 + 0.55 * lift)) as u8,
                });
            }
        }
    }
}

impl Cubes {
    /// A 3x3x3 block of wireframe cubes turning slowly, the gaps between them breathing.
    fn frame(&self, out: &mut Vec<Shape>) {
        let Clock { t, w, h } = self.0;
        let (c, unit) = center(w, h);
        let s = unit * 0.1;
        let gap = 1.12 + 0.1 * (t * 0.8).sin();
        let (spin, tilt) = (t * 0.3, 0.55 + 0.15 * (t * 0.2).sin());
        let project = |p: V3| {
            let q = p.rot_y(spin).rot_x(tilt);
            (q.screen(c, s), q.z / 2.2) // z spans about +-2.2 units; normalize to +-1
        };
        // Edges of a unit cube as index pairs into its 8 corners (bit 0 = x, 1 = y, 2 = z).
        const EDGES: [(usize, usize); 12] = [
            (0, 1),
            (2, 3),
            (4, 5),
            (6, 7), // along x
            (0, 2),
            (1, 3),
            (4, 6),
            (5, 7), // along y
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7), // along z
        ];
        for cx in -1..=1 {
            for cy in -1..=1 {
                for cz in -1..=1 {
                    let o = V3::new(cx as f32 * gap, cy as f32 * gap, cz as f32 * gap);
                    let corner = |k: usize| {
                        let d = |bit: usize| if k >> bit & 1 == 1 { 0.5 } else { -0.5 };
                        project(V3::new(o.x + d(0), o.y + d(1), o.z + d(2)))
                    };
                    for &(a, b) in &EDGES {
                        let ((pa, za), (pb, zb)) = (corner(a), corner(b));
                        line(out, pa, pb, shade((za + zb) / 2.0, 25.0, 200.0));
                    }
                }
            }
        }
    }
}
