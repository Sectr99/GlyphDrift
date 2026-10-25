//! Minimal geometric loops: one line-art object each, technical-illustration style.
//! Frames are pure functions of `t`. Light comes from the upper left; depth drives line
//! weight and brightness.

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
        self.t = (seed % 1000) as f32;
    }
}

/// Line weights in px at a 1000 px screen.
pub(super) const HERO: f32 = 2.4;
pub(super) const MID: f32 = 1.3;
pub(super) const HAIR: f32 = 0.55;

/// Light direction on screen (upper left).
pub(super) const LIGHT_2D: (f32, f32) = (-0.6, -0.8);

/// Smoothstep, 0..1.
pub(super) fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// 0 at `a`, 1 at `b`, eased (a > b works too).
fn ramp(a: f32, b: f32, x: f32) -> f32 {
    ease((x - a) / (b - a))
}

/// Centered, slightly high so the object doesn't sag.
pub(super) fn optical_center(f: Rect) -> ((f32, f32), f32) {
    ((f.x + f.w * 0.5, f.y + f.h * 0.47), f.object_unit())
}

/// Rotate screen offset (dx, dy) by `angle` about `c`.
fn turn(c: (f32, f32), angle: f32, dx: f32, dy: f32) -> (f32, f32) {
    let (s, k) = angle.sin_cos();
    (c.0 + dx * k - dy * s, c.1 + dx * s + dy * k)
}

fn lerp(a: (f32, f32), b: (f32, f32), f: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)
}

/// Draws with weights and spacings given at a 1000 px screen.
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

    pub(super) fn dot(self, out: &mut Vec<Shape>, c: (f32, f32), radius: f32, level: u8) {
        out.push(Shape::Circle {
            c,
            r: self.px(radius),
            fill: true,
            level,
        });
    }

    /// Circle outline, HERO where lit and HAIR in shadow; `strength` scales the lit side.
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

    /// Halftone shadow: staggered dots in an ellipse, shrinking toward the rim.
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

/// Shapes sorted by depth, emitted far to near.
struct Layers {
    items: Vec<(f32, Shape)>,
}

impl Layers {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn push(&mut self, z: f32, s: Shape) {
        self.items.push((z, s));
    }

    fn flush(mut self, out: &mut Vec<Shape>) {
        self.items.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.extend(self.items.into_iter().map(|(_, s)| s));
    }
}

macro_rules! loop_animation {
    ($ty:ident) => {
        #[derive(Default)]
        pub struct $ty(Clock);

        impl $ty {
            pub fn new() -> Self {
                Self::default()
            }
        }

        impl Animation for $ty {
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

loop_animation!(Helix);
loop_animation!(Globe);
loop_animation!(DotWave);
loop_animation!(Cubes);
loop_animation!(Sunburst);
loop_animation!(Moon);

impl Helix {
    /// Double helix of dots, level and centered, turning slowly, with fainter dotted base
    /// pairs. Dot size and brightness come from depth; both ends fade out.
    fn frame(&self, out: &mut Vec<Shape>) {
        const DOTS: usize = 260; // per strand
        const RUNGS: usize = 30;
        const TURNS: f32 = 3.0;
        const TILT: f32 = -0.12;
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = optical_center(focus);
        let pen = Pen::new(unit);
        let (half_len, r) = ((focus.w * 0.42).min(unit * 0.95), unit * 0.11);
        let spin = t * 0.35;
        // `f` runs 0..1 along the axis: screen point and depth (-1 far, 1 near).
        let point = |f: f32, phase: f32| {
            let (z, y) = (f * TAU * TURNS + spin + phase).sin_cos();
            (turn(c, TILT, (f * 2.0 - 1.0) * half_len, y * r), z)
        };
        let fade = |f: f32| ramp(0.0, 0.12, f) * ramp(1.0, 0.88, f);
        let mut layers = Layers::new();
        for phase in [0.0, PI] {
            for i in 0..=DOTS {
                let f = i as f32 / DOTS as f32;
                let (p, z) = point(f, phase);
                let n = (z + 1.0) / 2.0;
                layers.push(
                    z,
                    Shape::Circle {
                        c: p,
                        r: pen.px(1.1 + 2.3 * n * n),
                        fill: true,
                        level: (shade(z, 30.0, 240.0) as f32 * fade(f)) as u8,
                    },
                );
            }
        }
        for k in 0..RUNGS {
            let f = (k as f32 + 0.5) / RUNGS as f32;
            let ((a, za), (b, zb)) = (point(f, 0.0), point(f, PI));
            for d in 1..8 {
                let fd = d as f32 / 8.0;
                let z = za + (zb - za) * fd;
                layers.push(
                    z - 0.02, // under the strands
                    Shape::Circle {
                        c: lerp(a, b, fd),
                        r: pen.px(0.8),
                        fill: true,
                        level: (shade(z, 18.0, 120.0) as f32 * fade(f)) as u8,
                    },
                );
            }
        }
        layers.flush(out);
    }
}

impl Globe {
    /// Wireframe globe, centered: a 15 degree graticule whose far side shows faintly
    /// through, a rim lit from the upper left, and a satellite on a dotted orbit.
    fn frame(&self, out: &mut Vec<Shape>) {
        const SEG: usize = 96;
        const ORBIT: f32 = 1.45; // in globe radii
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = optical_center(focus);
        let pen = Pen::new(unit);
        let r = unit * 0.2; // as Moon; the orbit reaches 0.29 unit
        let (spin, tilt) = (t * 0.12, 0.4 + 0.05 * (t * 0.13).sin());
        let view = |p: V3| p.rot_y(spin).rot_x(tilt);
        let mut layers = Layers::new();
        // The equator and prime meridian are a little heavier.
        let mut curve = |f: &dyn Fn(f32) -> V3, major: bool| {
            for k in 0..SEG {
                let (a0, a1) = (
                    k as f32 / SEG as f32 * TAU,
                    (k + 1) as f32 / SEG as f32 * TAU,
                );
                let (p, q) = (view(f(a0)), view(f(a1)));
                let z = (p.z + q.z) / 2.0;
                let (weight, level) = match (z < 0.0, major) {
                    (true, _) => (HAIR, 24), // the far side, faint
                    (false, true) => (MID, shade(z, 90.0, 225.0)),
                    (false, false) => (HAIR + 0.5 * z, shade(z, 55.0, 165.0)),
                };
                layers.push(
                    z,
                    Shape::Line {
                        a: p.screen(c, r),
                        b: q.screen(c, r),
                        width: pen.px(weight),
                        level,
                    },
                );
            }
        };
        for m in 0..12 {
            let lon = m as f32 * PI / 12.0;
            curve(
                &|a| V3::new(a.cos() * lon.cos(), a.sin(), a.cos() * lon.sin()),
                m == 0,
            );
        }
        for i in -5i32..=5 {
            let (y, rr) = (i as f32 * PI / 12.0).sin_cos();
            curve(&|a| V3::new(a.cos() * rr, y, a.sin() * rr), i == 0);
        }
        // Orbit and satellite, hidden behind the globe.
        let orbit = |a: f32| view(V3::new(a.cos() * ORBIT, 0.0, a.sin() * ORBIT)).rot_z(0.3);
        let hidden = |p: V3| p.z < 0.0 && p.x * p.x + p.y * p.y < 1.0;
        const ORBIT_DOTS: usize = 140;
        for k in 0..ORBIT_DOTS {
            let p = orbit(k as f32 / ORBIT_DOTS as f32 * TAU - spin);
            if !hidden(p) {
                layers.push(
                    p.z / ORBIT,
                    Shape::Circle {
                        c: p.screen(c, r),
                        r: pen.px(0.7),
                        fill: true,
                        level: shade(p.z / ORBIT, 30.0, 95.0),
                    },
                );
            }
        }
        let sat = orbit(t * 0.3);
        if !hidden(sat) {
            layers.push(
                sat.z / ORBIT + 0.01,
                Shape::Circle {
                    c: sat.screen(c, r),
                    r: pen.px(2.2),
                    fill: true,
                    level: shade(sat.z / ORBIT, 120.0, 230.0),
                },
            );
        }
        layers.flush(out);
        pen.rim(out, c, r, LIGHT_2D, 0.8);
    }
}

impl DotWave {
    /// Circular dot field in perspective with ripples from the center; the rim fades out.
    fn frame(&self, out: &mut Vec<Shape>) {
        const RADIUS: f32 = 3.0; // world units
        const SPACING: f32 = 0.22;
        const DOT: f32 = 0.025; // dot radius, world units
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = center(focus);
        // Camera framing the whole disc, centered vertically.
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
                let depth = dist + z;
                let p = (
                    c.0 + x / depth * focal,
                    c.1 + (cam_h - y) / depth * focal - y0,
                );
                let near = (RADIUS - z) / (2.0 * RADIUS);
                let rim = ramp(RADIUS, RADIUS * 0.65, d);
                let lift = (crest + 1.0) / 2.0;
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
    /// A 2x2x2 block of cubes, seen three-quarters on and swaying gently, whose halves
    /// take turns making quarter twists like a puzzle cube: ease in, turn, settle, hold. Each cube's faces are halftone dots that turn with them, shaded by light
    /// and depth. Dots behind another cube aren't drawn, so the twisting half passes
    /// cleanly in front of and behind the rest.
    fn frame(&self, out: &mut Vec<Shape>) {
        const CELLS: usize = 16; // halftone cells across one cube's face
        const INSET: f32 = 0.07; // empty margin on each face, as a fraction of the cube
        const PERIOD: f32 = 5.0; // seconds per twist, including the hold
        const MOVE: f32 = 0.55; // fraction of the period spent twisting
        let light = V3::new(-0.45, -0.7, 0.55);
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = optical_center(focus);
        let pen = Pen::new(unit);
        let s = unit * 0.13; // half the block's side

        // This period's twist: axis, which half, direction and progress.
        let step = (t / PERIOD).floor();
        let progress = ((t / PERIOD - step) / MOVE).min(1.0);
        let n = step as i64;
        let axis = n.rem_euclid(3);
        let side = if (n / 3).rem_euclid(2) == 0 {
            1.0
        } else {
            -1.0
        };
        let turn_dir = if n.rem_euclid(4) < 2 { 1.0 } else { -1.0 };
        let angle = ease(progress) * PI / 2.0 * turn_dir;
        let basis = [
            V3::new(1.0, 0.0, 0.0),
            V3::new(0.0, 1.0, 0.0),
            V3::new(0.0, 0.0, 1.0),
        ];
        let twist = |p: V3| match axis {
            0 => p.rot_x(angle),
            1 => p.rot_y(angle),
            _ => p.rot_z(angle),
        };
        // Three faces in view, as in a technical drawing, with a slow sway.
        let tumble = |p: V3| {
            p.rot_y(0.62 + 0.22 * (t * 0.1).sin())
                .rot_x(0.5 + 0.08 * (t * 0.13).sin())
        };

        // Each cube in view space: center, its three unit axes, and which side of the cut
        // it's on (its center's coordinate along the twist axis, +-0.5).
        let cubes: Vec<(V3, [V3; 3], f32)> = (0..8)
            .map(|k| {
                let half = |bit: usize| if k >> bit & 1 == 1 { 0.5 } else { -0.5 };
                let o = V3::new(half(0), half(1), half(2));
                let along = [o.x, o.y, o.z][axis as usize];
                if along * side > 0.0 {
                    (tumble(twist(o)), basis.map(|e| tumble(twist(e))), along)
                } else {
                    (tumble(o), basis.map(tumble), along)
                }
            })
            .collect();
        let dot3 = |a: V3, b: V3| a.x * b.x + a.y * b.y + a.z * b.z;
        // Whether cube `k` lies between point `p` and the viewer (toward +z).
        let hides = |k: usize, p: V3| {
            let (center, axes, _) = cubes[k];
            let d = V3::new(p.x - center.x, p.y - center.y, p.z - center.z);
            let (mut near, mut far) = (1e-3f32, f32::INFINITY);
            for a in axes {
                let (o, dir) = (dot3(d, a), a.z);
                if dir.abs() < 1e-6 {
                    if o.abs() > 0.5 {
                        return false;
                    }
                    continue;
                }
                let (ta, tb) = ((-0.5 - o) / dir, (0.5 - o) / dir);
                near = near.max(ta.min(tb));
                far = far.min(ta.max(tb));
                if near > far {
                    return false;
                }
            }
            true
        };

        pen.halftone(out, (c.0, c.1 + s * 2.6), s * 1.9, s * 0.4, 34);
        let cell = (1.0 - 2.0 * INSET) / CELLS as f32;
        for (k, &(center, axes, along)) in cubes.iter().enumerate() {
            for i in 0..3 {
                for sign in [1.0f32, -1.0] {
                    let nrm = V3::new(axes[i].x * sign, axes[i].y * sign, axes[i].z * sign);
                    if nrm.z <= 0.02 {
                        continue; // facing away
                    }
                    let (u, w) = (axes[(i + 1) % 3], axes[(i + 2) % 3]);
                    let mut tone = 0.15 + 0.85 * dot3(nrm, light).max(0.0);
                    // Faces on the cut between the halves are inside the block: shadowed,
                    // so mid-twist they read as a gap rather than as stray dots.
                    if i == axis as usize && sign * along < 0.0 {
                        tone *= 0.3;
                    }
                    for j in 0..CELLS {
                        for m in 0..CELLS {
                            let a = -0.5 + INSET + (m as f32 + 0.5) * cell;
                            let b = -0.5 + INSET + (j as f32 + 0.5) * cell;
                            let p = V3::new(
                                center.x + nrm.x * 0.5 + u.x * a + w.x * b,
                                center.y + nrm.y * 0.5 + u.y * a + w.y * b,
                                center.z + nrm.z * 0.5 + u.z * a + w.z * b,
                            );
                            if (0..cubes.len()).any(|o| o != k && hides(o, p)) {
                                continue;
                            }
                            // Nearer points a little brighter: a gradient across faces.
                            let depth = (p.z / 3f32.sqrt() + 1.0) / 2.0;
                            let level = tone * (0.65 + 0.35 * depth);
                            out.push(Shape::Circle {
                                c: p.screen(c, s),
                                r: s * cell * 0.5 * level.sqrt() * nrm.z.sqrt(),
                                fill: true,
                                level: (50.0 + 190.0 * level) as u8,
                            });
                        }
                    }
                }
            }
        }
    }
}

impl Sunburst {
    /// Dotted rays around an empty center. Ray lengths follow three slow waves, so
    /// neighbors move together; dots shrink and dim toward the tips.
    fn frame(&self, out: &mut Vec<Shape>) {
        const RAYS: usize = 120;
        const STEP: f32 = 7.0; // between dots along a ray, at a 1000 px screen
        let Clock { t, focus, .. } = self.0;
        let (c, unit) = optical_center(focus);
        let pen = Pen::new(unit);
        let (r_in, reach) = (unit * 0.11, unit * 0.17);
        let step = pen.px(STEP);
        let spin = t * 0.02;
        for k in 0..RAYS {
            let a = k as f32 / RAYS as f32 * TAU;
            let wave = 0.45 * (2.0 * a + t * 0.16).sin()
                + 0.35 * (3.0 * a - t * 0.11).sin()
                + 0.2 * (5.0 * a + t * 0.07).sin();
            let len = 0.25 + 0.75 * ((wave + 1.0) / 2.0).clamp(0.0, 1.0);
            let (sa, ca) = (a + spin).sin_cos();
            let dots = ((reach * len) / step) as usize;
            for i in 0..=dots {
                let f = i as f32 / dots.max(1) as f32; // 0 inside, 1 at the tip
                let d = r_in + i as f32 * step;
                pen.dot(
                    out,
                    (c.0 + ca * d, c.1 + sa * d),
                    1.6 * (1.0 - 0.6 * f),
                    ((60.0 + 180.0 * len) * (1.0 - 0.55 * f)) as u8,
                );
            }
        }
    }
}

impl Moon {
    /// Halftone moon lit by a circling sun, so phases come from the lighting.
    fn frame(&self, out: &mut Vec<Shape>) {
        const CYCLE: f32 = 36.0; // seconds, new moon to new moon
        const TILT: f32 = 0.35;
        const STARS: u32 = 14;
        const ACROSS: f32 = 46.0; // halftone cells across the disc
                                  // Maria: (x, y, radius) in moon radii.
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
            let p = (c.0 + a.cos() * d * 1.5, c.1 + a.sin() * d * 0.8);
            let twinkle =
                0.5 + 0.5 * (t * (0.3 + 0.6 * bits(32, 0xFF)) + 6.0 * bits(40, 0xFF)).sin();
            pen.dot(
                out,
                p,
                0.7 + 0.6 * bits(48, 0xFF),
                (30.0 + 100.0 * twinkle) as u8,
            );
        }

        // Sun behind at new moon (0), in front at full (PI); eased to linger at both.
        let halves = (t / CYCLE).fract() * 2.0;
        let phase = (halves.floor() + ease(halves.fract())) * PI;
        let sun = V3::new(phase.sin(), 0.0, -phase.cos());
        // Libration: a slow nod.
        let (lib_x, lib_y) = (0.1 * (t * 0.21).sin(), 0.07 * (t * 0.17 + 1.0).sin());
        // Hex grid on the sphere (azimuthal equidistant), so spacing compresses toward the
        // limb; it reaches past the limb so libration never shows an edge.
        let step = PI / ACROSS;
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
                // Moon-fixed point, then as seen.
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
                let darken = 0.55 + 0.45 * seen.z;
                let tone = lum * (1.0 - 0.4 * mare.min(1.0)) * darken;
                let foreshorten = seen.z.powf(0.4);
                let p = turn(c, TILT, seen.x * r, seen.y * r);
                if tone > 0.02 {
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
        let lit = (1.0 - phase.cos()) / 2.0;
        let toward_sun = turn((0.0, 0.0), TILT, phase.sin().signum(), 0.0);
        pen.rim(out, c, r, toward_sun, 0.15 + 0.45 * lit);
    }
}
