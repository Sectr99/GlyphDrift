//! Minimal geometric loops: one line-art object per animation, in a technical-illustration
//! style.
//!
//! Every frame is a pure function of the clock `t`, so loops are seamless and testable.
//! Design rules shared by all of them:
//! - line hierarchy: one HERO line per composition, MID lines for structure, HAIR lines for
//!   detail and construction; solid, dashed and dotted line types for variety;
//! - value and form: light comes from the upper left; rims are heavy where lit and hairline
//!   in shadow; depth drives weight and brightness together;
//! - texture and depth cues: halftone shadows, overlap, back-to-front drawing;
//! - framing elements (axes, rulers, guide arcs) are quiet, so the object stays the focus;
//! - eased motion with holds, and asymmetry where the form allows.

use std::f32::consts::{PI, TAU};

use super::geom::{center, shade, V3};
use super::rng::hash3;
use super::{Animation, Rect, Size};
use crate::shape::Shape;

/// Shared state for every loop: elapsed time and drawing area.
#[derive(Default)]
struct Clock {
    t: f32,
    w: f32,
    h: f32,
    focus: Rect,
}

impl Clock {
    fn init(&mut self, size: Size, seed: u64) {
        (self.w, self.h, self.focus) = (size.width, size.height, size.focus);
        self.t = (seed % 1000) as f32; // restarts don't all begin at the same pose
    }
}

/// Line-weight tiers in px at a 1000 px screen; `Pen` scales them to the real size.
pub(super) const HERO: f32 = 2.4;
pub(super) const MID: f32 = 1.3;
pub(super) const HAIR: f32 = 0.55;

/// Light direction on screen (upper left), unit length.
pub(super) const LIGHT_2D: (f32, f32) = (-0.6, -0.8);

/// Smoothstep ease, 0..1 -> 0..1 with zero velocity at both ends.
pub(super) fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// 0 at `a`, 1 at `b`, eased in between (works for a > b too).
fn ramp(a: f32, b: f32, x: f32) -> f32 {
    ease((x - a) / (b - a))
}

/// Anchor for single-object loops: a little right of center, clear of the desktop icons on
/// the left, and slightly high (the optical center). Only for compositions with something
/// asymmetric to balance the shift (the globe's guide arc and orbit); balanced ones use
/// `optical_center`, since an unjustified offset reads as a mistake.
fn anchor(f: Rect) -> ((f32, f32), f32) {
    ((f.x + f.w * 0.56, f.y + f.h * 0.47), f.object_unit())
}

/// Horizontally centered, slightly high so the object doesn't appear to sag.
pub(super) fn optical_center(f: Rect) -> ((f32, f32), f32) {
    ((f.x + f.w * 0.5, f.y + f.h * 0.47), f.object_unit())
}

/// Rotate screen offset (dx, dy) by `angle` about `c`.
fn turn(c: (f32, f32), angle: f32, dx: f32, dy: f32) -> (f32, f32) {
    let (s, k) = angle.sin_cos();
    (c.0 + dx * k - dy * s, c.1 + dx * s + dy * k)
}

/// Points along an ellipse (radii `rx`, `ry`, rotated by `rot`) from angle `a0` to `a1`.
fn arc(c: (f32, f32), rx: f32, ry: f32, rot: f32, a0: f32, a1: f32, n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            turn(c, rot, a.cos() * rx, a.sin() * ry)
        })
        .collect()
}

fn lerp(a: (f32, f32), b: (f32, f32), f: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

/// Resolution-aware drawing: weights and spacings are given at a 1000 px screen.
#[derive(Clone, Copy)]
pub(super) struct Pen(f32);

impl Pen {
    pub(super) fn new(unit: f32) -> Self {
        Pen((unit / 1000.0).max(0.75))
    }

    pub(super) fn px(self, v: f32) -> f32 {
        v * self.0
    }

    pub(super) fn line(
        self,
        out: &mut Vec<Shape>,
        a: (f32, f32),
        b: (f32, f32),
        weight: f32,
        level: u8,
    ) {
        out.push(Shape::Line {
            a,
            b,
            width: self.px(weight),
            level,
        });
    }

    pub(super) fn path(self, out: &mut Vec<Shape>, pts: &[(f32, f32)], weight: f32, level: u8) {
        for s in pts.windows(2) {
            self.line(out, s[0], s[1], weight, level);
        }
    }

    /// Dashed polyline; the dash pattern runs continuously across vertices.
    pub(super) fn dashed(
        self,
        out: &mut Vec<Shape>,
        pts: &[(f32, f32)],
        dash: f32,
        gap: f32,
        weight: f32,
        level: u8,
    ) {
        // A dash/gap state machine. Each step consumes at least MIN_STEP of path, so it
        // always terminates; a `%`-based phase can stall on f32 rounding along long paths
        // and loop forever (it once ate 25 GB).
        const MIN_STEP: f32 = 1e-3;
        let (dash, gap) = (self.px(dash).max(0.5), self.px(gap).max(0.5));
        let (mut on, mut left) = (true, dash);
        for s in pts.windows(2) {
            let len = dist(s[0], s[1]);
            let mut at = 0.0;
            while len - at > MIN_STEP {
                let take = left.min(len - at);
                if on {
                    let (a, b) = (
                        lerp(s[0], s[1], at / len),
                        lerp(s[0], s[1], (at + take) / len),
                    );
                    self.line(out, a, b, weight, level);
                }
                at += take;
                left -= take;
                if left <= MIN_STEP {
                    on = !on;
                    left = if on { dash } else { gap };
                }
            }
        }
    }

    /// Dots spaced evenly along a polyline.
    pub(super) fn dotted(
        self,
        out: &mut Vec<Shape>,
        pts: &[(f32, f32)],
        spacing: f32,
        radius: f32,
        level: u8,
    ) {
        let spacing = self.px(spacing);
        let mut next = 0.0;
        let mut walked = 0.0;
        for s in pts.windows(2) {
            let len = dist(s[0], s[1]);
            while next <= walked + len {
                self.dot(
                    out,
                    lerp(s[0], s[1], (next - walked) / len.max(1e-6)),
                    radius,
                    level,
                );
                next += spacing;
            }
            walked += len;
        }
    }

    pub(super) fn dot(self, out: &mut Vec<Shape>, c: (f32, f32), radius: f32, level: u8) {
        out.push(Shape::Circle {
            c,
            r: self.px(radius),
            fill: true,
            level,
        });
    }

    /// Circle outline whose weight follows the light: HERO where lit, HAIR in shadow.
    /// `strength` (0..1) scales the lit side, e.g. for a thin crescent.
    pub(super) fn rim(
        self,
        out: &mut Vec<Shape>,
        c: (f32, f32),
        r: f32,
        light: (f32, f32),
        strength: f32,
    ) {
        const SEG: usize = 128;
        for i in 0..SEG {
            let (a0, a1) = (
                i as f32 / SEG as f32 * TAU,
                (i + 1) as f32 / SEG as f32 * TAU,
            );
            let mid = (a0 + a1) / 2.0;
            let lit = (mid.cos() * light.0 + mid.sin() * light.1)
                .max(0.0)
                .powf(0.8)
                * strength;
            self.line(
                out,
                (c.0 + a0.cos() * r, c.1 + a0.sin() * r),
                (c.0 + a1.cos() * r, c.1 + a1.sin() * r),
                HAIR + (HERO - HAIR) * lit,
                (55.0 + 175.0 * lit) as u8,
            );
        }
    }

    /// Soft shadow as halftone texture: staggered dots in an ellipse, shrinking to the rim.
    pub(super) fn halftone(self, out: &mut Vec<Shape>, c: (f32, f32), rx: f32, ry: f32, level: u8) {
        let step = self.px(7.0);
        let (nx, ny) = ((rx / step) as i32 + 1, (ry / step) as i32 + 1);
        for j in -ny..=ny {
            for i in -nx..=nx {
                let x = i as f32 * step + if j % 2 != 0 { step / 2.0 } else { 0.0 };
                let y = j as f32 * step;
                let d = ((x / rx).powi(2) + (y / ry).powi(2)).sqrt();
                let r = step * 0.42 * (1.0 - d).max(0.0).sqrt();
                if r > 0.35 {
                    out.push(Shape::Circle {
                        c: (c.0 + x, c.1 + y),
                        r,
                        fill: true,
                        level,
                    });
                }
            }
        }
    }
}

/// Shapes tagged with depth, emitted far to near (painter's algorithm).
struct Layers {
    pen: Pen,
    items: Vec<(f32, Shape)>,
}

impl Layers {
    fn new(pen: Pen) -> Self {
        Self {
            pen,
            items: Vec::new(),
        }
    }

    fn push(&mut self, z: f32, s: Shape) {
        self.items.push((z, s));
    }

    /// A line whose weight (HAIR..HERO) and brightness follow depth `z` (-1 far ..= 1 near),
    /// brightness times `k`.
    fn depth_line(&mut self, a: (f32, f32), b: (f32, f32), z: f32, k: f32, far: f32, near: f32) {
        let n = ((z + 1.0) / 2.0).clamp(0.0, 1.0);
        let level = (shade(z, far, near) as f32 * k) as u8;
        let width = self.pen.px(HAIR + (HERO - HAIR) * n.powf(1.6));
        self.push(z, Shape::Line { a, b, width, level });
    }

    fn flush(mut self, out: &mut Vec<Shape>) {
        self.items.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.extend(self.items.into_iter().map(|(_, s)| s));
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
loop_animation!(Sunburst, "sunburst");
loop_animation!(Moon, "moon");

impl Helix {
    /// A double helix receding to the right, turning about a dashed axis, with dotted base
    /// pairs and a ruler beneath. Strands run from hairline (far) to hero weight (near).
    fn frame(&self, out: &mut Vec<Shape>) {
        const SAMPLES: usize = 160;
        const RUNGS: usize = 26;
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = center(focus);
        let pen = Pen::new(unit);
        let (half_len, r) = ((focus.w * 0.32).min(unit * 0.62), unit * 0.085);
        // Perspective shrinks the far (right) end, re-centered so it doesn't pull left.
        let persp = |f: f32| 1.25 / (1.0 + 0.55 * f);
        let shift = (1.25 - 1.25 / 1.55) / 2.0 * half_len;
        let along =
            |f: f32, off: f32| turn(c, -0.2, (f * 2.0 - 1.0) * half_len * persp(f) + shift, off);
        let point = |f: f32, phase: f32| {
            let (z, y) = (f * TAU * 2.4 + t * 0.55 + phase).sin_cos();
            let p = along(f, y * r * persp(f) * (1.0 + 0.1 * z));
            (p, z)
        };
        let taper = |f: f32| ramp(0.0, 0.18, f) * ramp(1.0, 0.82, f) * (1.1 - 0.35 * f);

        // Construction: a dashed axis running past both ends, a ruler with a tick per rung.
        let axis: Vec<_> = (0..=48)
            .map(|i| along(-0.08 + 1.16 * i as f32 / 48.0, 0.0))
            .collect();
        pen.dashed(out, &axis, 6.0, 5.0, HAIR, 45);
        let ruler = |f: f32, d: f32| along(f, r * 1.8 * persp(f) + d);
        pen.line(out, ruler(0.0, 0.0), ruler(1.0, 0.0), HAIR, 50);
        for k in 0..=RUNGS {
            let f = k as f32 / RUNGS as f32;
            let tick = pen.px(if k % 5 == 0 { 9.0 } else { 4.0 });
            pen.line(
                out,
                ruler(f, 0.0),
                ruler(f, tick),
                if k % 5 == 0 { MID } else { HAIR },
                55,
            );
        }

        let mut layers = Layers::new(pen);
        for phase in [0.0, PI] {
            for i in 0..SAMPLES - 1 {
                let (f0, f1) = (i as f32 / SAMPLES as f32, (i + 1) as f32 / SAMPLES as f32);
                let ((a, za), (b, zb)) = (point(f0, phase), point(f1, phase));
                layers.depth_line(a, b, (za + zb) / 2.0, taper(f0), 25.0, 220.0);
            }
        }
        for k in 0..RUNGS {
            let f = (k as f32 + 0.5) / RUNGS as f32;
            let ((a, za), (b, zb)) = (point(f, 0.0), point(f, PI));
            // Base pair: a dotted rung.
            for d in 1..6 {
                let fd = d as f32 / 6.0;
                let z = za + (zb - za) * fd;
                layers.push(
                    z - 0.01,
                    Shape::Circle {
                        c: lerp(a, b, fd),
                        r: pen.px(0.9),
                        fill: true,
                        level: (shade(z, 25.0, 140.0) as f32 * taper(f)) as u8,
                    },
                );
            }
            for (p, z) in [(a, za), (b, zb)] {
                let n = (z + 1.0) / 2.0;
                layers.push(
                    z + 0.01, // nodes sit on top of the strand at the same depth
                    Shape::Circle {
                        c: p,
                        r: pen.px(1.0 + 2.4 * n),
                        fill: true,
                        level: (shade(z, 40.0, 245.0) as f32 * taper(f)) as u8,
                    },
                );
            }
        }
        layers.flush(out);
    }
}

impl Globe {
    /// A wireframe globe: rim-lit limb (the hero line), heavier equator, dotted back grid,
    /// dashed pole axis, a satellite with a fading trail on a dotted orbit, and a partial
    /// dashed guide arc with ticks framing it off-center.
    fn frame(&self, out: &mut Vec<Shape>) {
        const SEG: usize = 64;
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = anchor(focus);
        let pen = Pen::new(unit);
        let r = unit * 0.25;
        let (spin, tilt) = (t * 0.22, 0.38 + 0.08 * (t * 0.3).sin());
        let view = |p: V3| p.rot_y(spin).rot_x(tilt);
        let at = |k: usize| k as f32 / SEG as f32 * TAU;
        const GUIDE: f32 = 1.55; // guide arc radius, in globe radii: clear of the screen edge

        // Frame: a partial guide arc with ticks, opening toward the upper right.
        let (g0, g1) = (PI * 0.7, PI * 1.9);
        pen.dashed(
            out,
            &arc(c, r * GUIDE, r * GUIDE, 0.0, g0, g1, 120),
            5.0,
            6.0,
            HAIR,
            34,
        );
        for i in 0..=((g1 - g0) / (PI / 12.0)) as usize {
            let a = g0 + i as f32 * PI / 12.0;
            let len = if i % 3 == 0 { 0.07 } else { 0.03 };
            let (p, q) = (r * GUIDE, r * (GUIDE + len));
            pen.line(
                out,
                (c.0 + a.cos() * p, c.1 + a.sin() * p),
                (c.0 + a.cos() * q, c.1 + a.sin() * q),
                HAIR,
                44,
            );
        }
        // Pole axis, dashed, extending past the sphere.
        let (n_pole, s_pole) = (
            view(V3::new(0.0, -1.35, 0.0)),
            view(V3::new(0.0, 1.35, 0.0)),
        );
        pen.dashed(
            out,
            &[n_pole.screen(c, r), s_pole.screen(c, r)],
            7.0,
            5.0,
            HAIR,
            85,
        );

        let mut layers = Layers::new(pen);
        // kind: 0 grid, 1 equator, 2 accent meridian
        let mut curve = |f: &dyn Fn(f32) -> V3, kind: u8| {
            for k in 0..SEG {
                let (p, q) = (view(f(at(k))), view(f(at(k + 1))));
                let z = (p.z + q.z) / 2.0;
                let (a, b) = (p.screen(c, r), q.screen(c, r));
                if z < 0.0 {
                    if k % 2 == 0 {
                        // The back of the globe is only suggested: a dotted line.
                        layers.push(
                            z,
                            Shape::Circle {
                                c: lerp(a, b, 0.5),
                                r: pen.px(0.7),
                                fill: true,
                                level: 34,
                            },
                        );
                    }
                    continue;
                }
                let (weight, far, near) = match kind {
                    2 => (MID + 0.4, 150.0, 250.0),
                    1 => (MID, 100.0, 195.0),
                    _ => (HAIR + 0.35 * z, 45.0, 135.0),
                };
                let level = shade(z, far, near);
                layers.push(
                    z + kind as f32 * 0.01,
                    Shape::Line {
                        a,
                        b,
                        width: pen.px(weight),
                        level,
                    },
                );
            }
        };
        for m in 0..6 {
            let lon = m as f32 * PI / 6.0;
            curve(
                &|a| V3::new(a.cos() * lon.cos(), a.sin(), a.cos() * lon.sin()),
                if m == 0 { 2 } else { 0 },
            );
        }
        for lat in [-60f32, -30.0, 0.0, 30.0, 60.0] {
            let (y, rr) = lat.to_radians().sin_cos();
            curve(
                &|a| V3::new(a.cos() * rr, y, a.sin() * rr),
                u8::from(lat == 0.0),
            );
        }

        // Satellite on a dotted orbit, hidden behind the globe, trailing a fading arc.
        let orbit = |a: f32| {
            V3::new(a.cos() * 1.5, 0.0, a.sin() * 1.5)
                .rot_x(0.45)
                .rot_z(0.3)
        };
        let hidden = |p: V3| p.z < 0.0 && (p.x * p.x + p.y * p.y) < 1.0;
        let front: Vec<_> = (0..=96)
            .map(|k| orbit(k as f32 / 96.0 * TAU))
            .collect::<Vec<_>>()
            .split(|p| hidden(*p))
            .flat_map(|run| {
                let pts: Vec<_> = run.iter().map(|p| p.screen(c, r)).collect();
                let mut dots = Vec::new();
                pen.dotted(&mut dots, &pts, 9.0, 0.75, 70);
                dots
            })
            .collect();
        for d in front {
            layers.push(0.9, d);
        }
        let head = t * 0.5;
        for i in 0..16 {
            let (p, q) = (
                orbit(head - i as f32 * 0.035),
                orbit(head - (i + 1) as f32 * 0.035),
            );
            if hidden(p) || hidden(q) {
                continue;
            }
            let fade = 1.0 - i as f32 / 16.0;
            layers.push(
                1.0,
                Shape::Line {
                    a: p.screen(c, r),
                    b: q.screen(c, r),
                    width: pen.px(HAIR + (MID - HAIR) * fade),
                    level: (210.0 * fade) as u8,
                },
            );
        }
        let s = orbit(head);
        if !hidden(s) {
            layers.push(
                1.1,
                Shape::Circle {
                    c: s.screen(c, r),
                    r: pen.px(3.0),
                    fill: true,
                    level: 245,
                },
            );
        }
        layers.flush(out);
        pen.rim(out, c, r, LIGHT_2D, 1.0); // the hero line
    }
}

impl DotWave {
    /// A circular field of dots in perspective; damped ripples spread from its center and
    /// the rim fades out, so it reads as an object rather than a floor.
    fn frame(&self, out: &mut Vec<Shape>) {
        const RADIUS: f32 = 3.0; // field radius, world units
        const SPACING: f32 = 0.22;
        const DOT: f32 = 0.025; // dot radius, world units: shrinks with distance like spacing
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = center(focus);
        // Camera high and back enough to see the whole disc; then center its near-to-far
        // extent on screen.
        let (focal, cam_h, dist) = (unit * 0.6, 3.0, 6.0);
        let y0 = (cam_h / (dist - RADIUS) + cam_h / (dist + RADIUS)) / 2.0 * focal;
        let n = (RADIUS / SPACING) as i32;
        for j in (-n..=n).rev() {
            for i in -n..=n {
                let (x, z) = (i as f32 * SPACING, j as f32 * SPACING);
                let d = (x * x + z * z).sqrt();
                if d > RADIUS {
                    continue;
                }
                let crest = (d * 3.2 - t * 1.6).sin() * (-d * 0.35).exp();
                let y = 0.32 * crest;
                let depth = dist + z; // z > 0 is farther away
                let p = (
                    c.0 + x / depth * focal,
                    c.1 + (cam_h - y) / depth * focal - y0,
                );
                let near = (RADIUS - z) / (2.0 * RADIUS);
                let rim = ramp(RADIUS, RADIUS * 0.65, d);
                let lift = (crest + 1.0) / 2.0; // crests brighter
                out.push(Shape::Circle {
                    c: p,
                    r: (DOT * (0.75 + 0.5 * lift) / depth * focal).max(0.7),
                    fill: true,
                    level: ((35.0 + 200.0 * near) * (0.5 + 0.5 * lift) * rim) as u8,
                });
            }
        }
    }
}

impl Cubes {
    /// A 2x2x2 block of solid, lit cubes making eased quarter-turns about alternating axes
    /// in a staggered cascade, parting mid-turn and settling (follow-through), over a
    /// halftone shadow. The block looks the same after every quarter-turn: seamless loop.
    fn frame(&self, out: &mut Vec<Shape>) {
        const PERIOD: f32 = 4.0; // seconds per quarter-turn, including the hold
        const MOVE: f32 = 0.55; // fraction of the period one cube spends turning
        const LAG: f32 = 0.07; // per step from the leading corner, as a fraction of PERIOD
                               // Faces as corner indices (bit 0 = x, 1 = y, 2 = z) in winding order, with normals.
        const FACES: [([usize; 4], (f32, f32, f32)); 6] = [
            ([0, 2, 6, 4], (-1.0, 0.0, 0.0)),
            ([1, 3, 7, 5], (1.0, 0.0, 0.0)),
            ([0, 1, 5, 4], (0.0, -1.0, 0.0)),
            ([2, 3, 7, 6], (0.0, 1.0, 0.0)),
            ([0, 1, 3, 2], (0.0, 0.0, -1.0)),
            ([4, 5, 7, 6], (0.0, 0.0, 1.0)),
        ];
        let light = V3::new(-0.45, -0.7, 0.55);
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = optical_center(focus);
        let pen = Pen::new(unit);
        let s = unit * 0.12;
        let step = (t / PERIOD).floor();
        let phase = t / PERIOD - step;
        let axis = (step as i64).rem_euclid(3);

        pen.halftone(out, (c.0 + s * 0.5, c.1 + s * 2.5), s * 2.1, s * 0.42, 34);
        let mut layers = Layers::new(pen);
        for cx in [-0.5f32, 0.5] {
            for cy in [-0.5f32, 0.5] {
                for cz in [-0.5f32, 0.5] {
                    // Cascade: cubes farther from the leading corner start later. All finish
                    // within the period (MOVE + 3 * LAG < 1), so the loop stays seamless.
                    let lag = [cx, cy, cz].iter().filter(|&&v| v > 0.0).count() as f32;
                    let progress = ((phase - lag * LAG) / MOVE).clamp(0.0, 1.0);
                    let angle = ease(progress) * PI / 2.0;
                    let gap = 1.08 + 0.3 * (PI * progress).sin();
                    let rot = |p: V3| {
                        let p = match axis {
                            0 => p.rot_y(angle),
                            1 => p.rot_x(angle),
                            _ => p.rot_z(angle),
                        };
                        p.rot_y(0.62).rot_x(0.5)
                    };
                    let o = V3::new(cx * gap, cy * gap, cz * gap);
                    let corners: Vec<V3> = (0..8)
                        .map(|k| {
                            let d = |bit: usize| if k >> bit & 1 == 1 { 0.5 } else { -0.5 };
                            rot(V3::new(o.x + d(0), o.y + d(1), o.z + d(2)))
                        })
                        .collect();
                    for (idx, nrm) in FACES {
                        let n = rot(V3::new(nrm.0, nrm.1, nrm.2));
                        if n.z <= 0.02 {
                            continue; // facing away
                        }
                        let z = (idx.iter().map(|&i| corners[i].z).sum::<f32>() / 4.0 / 1.6)
                            .clamp(-1.0, 1.0);
                        let lambert = (n.x * light.x + n.y * light.y + n.z * light.z).max(0.0);
                        let points: Vec<_> = idx.iter().map(|&i| corners[i].screen(c, s)).collect();
                        for e in 0..4 {
                            let (a, b) = (points[e], points[(e + 1) % 4]);
                            let width = pen.px(HAIR + (MID - HAIR) * (z + 1.0) / 2.0);
                            layers.push(
                                z + 0.001,
                                Shape::Line {
                                    a,
                                    b,
                                    width,
                                    level: shade(z, 110.0, 225.0),
                                },
                            );
                        }
                        layers.push(
                            z,
                            Shape::Polygon {
                                points,
                                level: (14.0 + 115.0 * lambert) as u8,
                            },
                        );
                    }
                }
            }
        }
        layers.flush(out);
    }
}

impl Sunburst {
    /// A radial waveform: many hairline rays around an empty center, like a ring of sound.
    /// Mostly a circle with a gentle drifting undulation plus fine per-ray detail. Quality
    /// from precision and repetition, not ornament; weight and brightness rise with length.
    fn frame(&self, out: &mut Vec<Shape>) {
        const RAYS: usize = 240;
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = optical_center(focus);
        let pen = Pen::new(unit);
        let (r_in, reach) = (unit * 0.12, unit * 0.13);
        let spin = t * 0.04;
        for k in 0..RAYS {
            let a = k as f32 / RAYS as f32 * TAU;
            // Gentle low lobes, finer harmonics at unrelated speeds, and a little per-ray
            // grain that shimmers slowly: organic, never jittery.
            let grain = (hash3(k as u32, 3, 0) & 0xFFFF) as f32 / 65536.0;
            let wave = 0.3 * (2.0 * a + t * 0.35).sin()
                + 0.25 * (5.0 * a - t * 0.6).sin()
                + 0.2 * (9.0 * a + t * 0.9).sin()
                + 0.25 * (grain * TAU + t * 1.3).sin();
            let len = (0.3 + 0.7 * (wave + 1.0) / 2.0).clamp(0.0, 1.0).powf(1.4);
            let (sa, ca) = (a + spin).sin_cos();
            let (r0, r1) = (r_in, r_in + reach * len);
            pen.line(
                out,
                (c.0 + ca * r0, c.1 + sa * r0),
                (c.0 + ca * r1, c.1 + sa * r1),
                HAIR + (MID - HAIR) * len * len,
                (40.0 + 205.0 * len) as u8,
            );
        }
        pen.path(
            out,
            &arc(c, r_in * 0.85, r_in * 0.85, 0.0, 0.0, TAU, 120),
            HAIR,
            70,
        );
    }
}

impl Moon {
    /// The moon as a halftone print: a dot grid where each dot's area is its brightness,
    /// lit by a sun that circles round, so crescent, gibbous and the soft terminator all
    /// come from the lighting. Dark maria vary the surface; the night side shows only as
    /// faint earthshine dots. A few tiny stars twinkle slowly.
    fn frame(&self, out: &mut Vec<Shape>) {
        const CYCLE: f32 = 36.0; // seconds, new moon to new moon
        const TILT: f32 = 0.35;
        const STARS: u32 = 14;
        const ACROSS: f32 = 46.0; // halftone cells across the disc
                                  // Maria: (x, y, radius) in moon radii; darker regions of the surface.
        const MARIA: [(f32, f32, f32); 5] = [
            (-0.3, -0.35, 0.32),
            (0.2, -0.1, 0.26),
            (0.05, 0.3, 0.22),
            (-0.45, 0.2, 0.18),
            (0.42, 0.35, 0.14),
        ];
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = optical_center(focus);
        let pen = Pen::new(unit);
        let r = unit * 0.2;

        for i in 0..STARS {
            let hh = hash3(i, 7, 0);
            let bits = |shift: u32, n: u64| ((hh >> shift) & n) as f32 / n as f32;
            let (a, d) = (bits(0, 0xFFFF) * TAU, r * (1.6 + 2.4 * bits(16, 0xFFFF)));
            let p = (c.0 + a.cos() * d * 1.5, c.1 + a.sin() * d * 0.8); // wider than tall
            let twinkle =
                0.5 + 0.5 * (t * (0.3 + 0.6 * bits(32, 0xFF)) + 6.0 * bits(40, 0xFF)).sin();
            pen.dot(
                out,
                p,
                0.7 + 0.6 * bits(48, 0xFF),
                (30.0 + 100.0 * twinkle) as u8,
            );
        }

        // Sun direction: behind the moon at new (phase 0), in front at full (PI); eased so
        // the cycle lingers at both.
        let halves = (t / CYCLE).fract() * 2.0;
        let phase = (halves.floor() + ease(halves.fract())) * PI;
        let sun = V3::new(phase.sin(), 0.0, -phase.cos());
        // Libration: the real moon's slow nod, so the surface visibly slides over the limb.
        let (lib_x, lib_y) = (0.1 * (t * 0.21).sin(), 0.07 * (t * 0.17 + 1.0).sin());
        // The halftone screen lives on the sphere, not the screen: a hex grid in angular
        // distance from the disc center (azimuthal equidistant), wrapped onto the surface.
        // Projected, its spacing compresses toward the limb, and dots shrink as the surface
        // turns away: the eye reads curvature. Rows reach a little past the limb so
        // libration never exposes an edge.
        let step = PI / ACROSS; // angular spacing
        let reach = PI / 2.0 + 0.15;
        let (nx, ny) = (
            (reach / step) as i32 + 1,
            (reach / (step * 0.866)) as i32 + 1,
        );
        for j in -ny..=ny {
            for i in -nx..=nx {
                let gx = (i as f32 + if j % 2 != 0 { 0.5 } else { 0.0 }) * step;
                let gy = j as f32 * step * 0.866;
                let theta = (gx * gx + gy * gy).sqrt();
                if theta > reach {
                    continue;
                }
                let phi = gy.atan2(gx);
                // Moon-fixed surface point (maria are fixed to it), then as seen.
                let fixed = V3::new(
                    theta.sin() * phi.cos(),
                    theta.sin() * phi.sin(),
                    theta.cos(),
                );
                let seen = fixed.rot_y(lib_x).rot_x(lib_y);
                if seen.z <= 0.0 {
                    continue; // far side
                }
                let lum = (seen.x * sun.x + seen.y * sun.y + seen.z * sun.z).max(0.0);
                let mare: f32 = MARIA
                    .iter()
                    .map(|&(mx, my, mr)| {
                        (-((fixed.x - mx).powi(2) + (fixed.y - my).powi(2)) / (mr * mr)).exp()
                    })
                    .sum();
                let darken = 0.55 + 0.45 * seen.z; // limb darkening
                let tone = lum * (1.0 - 0.4 * mare.min(1.0)) * darken;
                let foreshorten = seen.z.powf(0.4);
                let p = turn(c, TILT, seen.x * r, seen.y * r);
                if tone > 0.02 {
                    // Dot area proportional to tone.
                    out.push(Shape::Circle {
                        c: p,
                        r: r * step * 0.5 * tone.sqrt() * foreshorten,
                        fill: true,
                        level: (90.0 + 160.0 * tone) as u8,
                    });
                } else {
                    pen.dot(out, p, 0.5 * foreshorten.max(0.5), 26); // earthshine
                }
            }
        }
        // A fine rim that catches the light on the lit side; quieter near new moon.
        let lit = (1.0 - phase.cos()) / 2.0;
        let toward_sun = turn((0.0, 0.0), TILT, phase.sin().signum(), 0.0);
        pen.rim(out, c, r, toward_sun, 0.15 + 0.45 * lit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashed_terminates_and_is_bounded_on_long_paths() {
        // Far from the origin and thousands of px long: where f32 phase math used to stall.
        let pts = arc((12_000.0, 9_000.0), 5_000.0, 5_000.0, 0.0, 0.0, TAU, 2_000);
        let mut out = Vec::new();
        Pen::new(2160.0).dashed(&mut out, &pts, 6.0, 5.0, HAIR, 40);
        let length = TAU * 5_000.0;
        let dash_period = (6.0 + 5.0) * 2.16;
        assert!(!out.is_empty());
        assert!(
            out.len() < (length / dash_period) as usize + pts.len() + 10,
            "{}",
            out.len()
        );
    }
}
