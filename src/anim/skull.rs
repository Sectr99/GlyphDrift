//! Skull: a hacker-core loop. A classical jawless skull, halftone-shaded from a 3D height
//! field and turning slowly, with a hacker layer on top.
//!
//! Depth tricks (shared with the other loops): the surface is a height field (cranium and
//! face domes with recessed sockets and nasal cavity), so dots are lit from its normals and
//! shrink as it turns away; every point is rotated in 3D as the head turns and nods, so
//! front features shift more than the silhouette (parallax) and the light moves across the
//! form; inside the voids, the walls facing the light catch it dimly, and the pupils sit
//! deep in the sockets; teeth sit on a curved arch, foreshortened toward the sides.
//!
//! Hacker layer: scanning pupils and a reticle, a sweeping scanline, glitch bands, HUD
//! brackets, and character-grid readouts with a typed status line.

use std::f32::consts::{PI, TAU};

use super::geom::V3;
use super::loops::{ease, optical_center, Pen, HAIR, HERO, LIGHT_2D, MID};
use super::rng::hash3;
use super::{Animation, Size};
use crate::grid::{Cell, Grid};
use crate::shape::Shape;

type P = (f32, f32);

// Geometry in skull units: half-width 1, origin at eye level, y down. Right halves run
// from the top center to the bottom center and are mirrored.
const SILHOUETTE: [P; 18] = [
    (0.0, -1.39),
    (0.3, -1.36),
    (0.58, -1.25),
    (0.8, -1.04),
    (0.94, -0.77),
    (0.975, -0.55), // widest across the upper sides
    (0.95, -0.36),
    (0.885, -0.17), // temple hollow
    (0.9, -0.02),
    (0.97, 0.14),
    (1.02, 0.27), // cheekbone flare
    (0.98, 0.37),
    (0.87, 0.45), // corner where the side cuts back in
    (0.81, 0.57),
    (0.71, 0.74),
    (0.6, 0.87),
    (0.4, 0.92),
    (0.0, 0.92),
];
/// Right eye socket: big and rounded, arched on top, with a pointed inner-top corner and
/// the outer side dropping lower.
const SOCKET: [P; 11] = [
    (0.14, -0.22),
    (0.3, -0.33),
    (0.5, -0.36),
    (0.68, -0.31),
    (0.82, -0.17),
    (0.86, 0.02),
    (0.8, 0.2),
    (0.66, 0.32),
    (0.46, 0.35),
    (0.28, 0.29),
    (0.14, 0.12),
];
/// Nasal opening: a tall inverted heart starting between the sockets.
const NOSE: [P; 12] = [
    (0.0, 0.08),
    (0.07, 0.18),
    (0.14, 0.32),
    (0.19, 0.46),
    (0.15, 0.57),
    (0.06, 0.6),
    (0.0, 0.55),
    (-0.06, 0.6),
    (-0.15, 0.57),
    (-0.19, 0.46),
    (-0.14, 0.32),
    (-0.07, 0.18),
];
const EYE: P = (0.5, 0.0); // right socket center; the left mirrors it
const NOSE_CENTER: P = (0.0, 0.4);
/// Upper teeth arch: (radius, half the arch angle in radians, top at the front).
const ARCH: (f32, f32, f32) = (0.6, 1.15, 0.9);
#[derive(Clone, Copy)]
enum Tooth {
    Incisor,
    Canine,
    Premolar,
    Molar,
}
/// One side of the upper row, midline outward: (type, relative width, length).
const ROW: [(Tooth, f32, f32); 6] = [
    (Tooth::Incisor, 1.3, 0.25),  // central
    (Tooth::Incisor, 0.95, 0.21), // lateral
    (Tooth::Canine, 1.0, 0.235),
    (Tooth::Premolar, 0.9, 0.18),
    (Tooth::Premolar, 0.9, 0.17),
    (Tooth::Molar, 1.15, 0.14),
];
/// Depth scale of the height field relative to the half-width.
const DEPTH: f32 = 0.9;
/// Light direction in view space (upper left, toward the viewer), unit length.
const LIGHT: V3 = V3 {
    x: -0.45,
    y: -0.55,
    z: 0.70,
};
/// Teeth halftone spacing relative to the face's.
const TOOTH_DOTS: f32 = 0.6;
/// Halftone spacing in px at a 1000 px screen.
const DOT_STEP: f32 = 11.0;
/// Screen scale: skull units per unit of the smaller screen dimension.
const SCALE: f32 = 0.235;

const MESSAGES: [&str; 5] = [
    "handshake",
    "bypass firewall",
    "inject payload",
    "decrypt core",
    "access granted",
];
const GLITCH_EVERY: f32 = 6.5; // seconds between glitch bursts
const GLITCH_FOR: f32 = 0.35;
const SCAN_PERIOD: f32 = 4.5;
const SCAN_TOP: f32 = -1.5;
const SCAN_SPAN: f32 = 2.8;

/// Mirror a right half (top center to bottom center) into a closed outline.
fn mirrored(half: &[P]) -> Vec<P> {
    let mut pts = half.to_vec();
    pts.extend(half[1..half.len() - 1].iter().rev().map(|&(x, y)| (-x, y)));
    pts
}

/// Catmull-Rom spline through `pts`, `per` samples per span, as a closed loop.
fn smooth(pts: &[P], per: usize) -> Vec<P> {
    let n = pts.len() as isize;
    let at = |i: isize| pts[i.rem_euclid(n) as usize];
    let mut out = Vec::with_capacity(pts.len() * per + 1);
    for i in 0..n {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for k in 0..per {
            let t = k as f32 / per as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            out.push((f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    out.push(out[0]);
    out
}

/// Even-odd point-in-polygon test.
fn inside(poly: &[P], p: P) -> bool {
    let mut hit = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            hit = !hit;
        }
    }
    hit
}

/// Deterministic noise in 0..1.
fn noise(a: u32, b: u32, c: u32) -> f32 {
    (hash3(a, b, c) & 0xFFFF) as f32 / 65535.0
}

/// Height of the bone surface at `p`, 0..1: the larger of a cranium dome and a face dome,
/// shaped by the landmarks that make a skull read as bone: a brow ridge over the sockets,
/// frontal bosses on the forehead, hollows at the temples, cheekbone ridges with hollows
/// beneath, and smooth dips into the sockets and nasal cavity so their rims curve inward.
fn height(p: P) -> f32 {
    let dome = |cx: f32, cy: f32, rx: f32, ry: f32| {
        let (nx, ny) = ((p.0 - cx) / rx, (p.1 - cy) / ry);
        (1.0 - nx * nx - ny * ny).max(0.0).sqrt()
    };
    // Smooth radial bump (positive) or dip (negative), mirrored left and right.
    let blob = |c: P, r: f32, amount: f32| {
        [1.0f32, -1.0]
            .iter()
            .map(|&side| {
                let d2 = ((p.0 - c.0 * side).powi(2) + (p.1 - c.1).powi(2)) / (r * r);
                amount * (1.0 - d2).max(0.0).powi(2)
            })
            .sum::<f32>()
    };
    // Both domes extend a little past the silhouette, so no edge area is left flat (a flat
    // patch would face the viewer and light up wrongly).
    // Smooth union (polynomial smooth max): a hard max would leave a crease where the domes
    // meet, which shades as a dark band across the forehead.
    let (a, b, k) = (
        dome(0.0, -0.45, 1.08, 1.0),
        0.92 * dome(0.0, 0.3, 1.08, 0.8),
        0.25,
    );
    let blend = (0.5 + 0.5 * (a - b) / k).clamp(0.0, 1.0);
    let base = b + (a - b) * blend + k * blend * (1.0 - blend);
    // Brow ridge: a gentle rise over each socket only (not across the whole forehead, where
    // its underside would shade a band).
    let over_socket = (-((p.0.abs() - EYE.0) / 0.3).powi(2)).exp();
    let brow = 0.035 * (-((p.1 + 0.43) / 0.11).powi(2)).exp() * over_socket;
    base + brow
        + blob((0.36, -0.78), 0.32, 0.05) // frontal bosses
        + blob((0.9, -0.22), 0.28, -0.12) // temple hollows
        + blob((0.78, 0.3), 0.24, 0.12) // cheekbone ridges
        + blob((0.6, 0.62), 0.22, -0.14) // hollows under the cheekbones
        + blob(EYE, 0.42, -0.36) // sockets: the rims curve in close to the openings
        - 0.3 * (1.0 - ((p.0 - NOSE_CENTER.0).powi(2) + (p.1 - NOSE_CENTER.1).powi(2)) / 0.0676).max(0.0).powi(2)
}

/// Surface normal at `p` (view space before rotation) from the height field's slope.
fn normal(p: P) -> V3 {
    const E: f32 = 0.01;
    let dx = (height((p.0 + E, p.1)) - height((p.0 - E, p.1))) / (2.0 * E) * DEPTH;
    let dy = (height((p.0, p.1 + E)) - height((p.0, p.1 - E))) / (2.0 * E) * DEPTH;
    let len = (dx * dx + dy * dy + 1.0).sqrt();
    V3::new(-dx / len, -dy / len, 1.0 / len)
}

/// The teeth as regions on the arch, in skull units: each tooth's outline, its depth and
/// its resting normal (facing out from the arch). Widths follow the arch (foreshortened
/// toward the sides); tops follow a slight smile curve; each type has its own crown.
fn teeth() -> Vec<(Vec<P>, f32, V3)> {
    let (radius, span, top) = ARCH;
    let total: f32 = ROW.iter().map(|r| r.1).sum();
    let mut out = Vec::new();
    for side in [1.0f32, -1.0] {
        let mut a = 0.0;
        for &(kind, rel, len) in &ROW {
            let (a0, a1) = (a, a + span * rel / total);
            a = a1;
            let gap = (a1 - a0) * 0.1;
            let (x0, x1) = (radius * (a0 + gap).sin(), radius * (a1 - gap).sin());
            let mid = (a0 + a1) / 2.0;
            let y0 = top - 0.05 * (1.0 - mid.cos());
            let y1 = y0 + len;
            let (w, c) = (x1 - x0, (x0 + x1) / 2.0);
            let crown: Vec<P> = match kind {
                // Slightly narrower at the root, square edge with softened corners.
                Tooth::Incisor => vec![
                    (x0 + w * 0.08, y0),
                    (x1 - w * 0.08, y0),
                    (x1, y1 - 0.03),
                    (x1 - w * 0.12, y1),
                    (x0 + w * 0.12, y1),
                    (x0, y1 - 0.03),
                ],
                // Longest, tapering to a blunt point.
                Tooth::Canine => vec![
                    (x0 + w * 0.12, y0),
                    (x1 - w * 0.12, y0),
                    (x1, y0 + len * 0.45),
                    (x1 - w * 0.12, y1 - len * 0.2),
                    (c + w * 0.12, y1 - len * 0.03),
                    (c - w * 0.12, y1 - len * 0.03),
                    (x0 + w * 0.12, y1 - len * 0.2),
                    (x0, y0 + len * 0.45),
                ],
                // Rounded, with a single cusp.
                Tooth::Premolar => vec![
                    (x0 + w * 0.12, y0),
                    (x1 - w * 0.12, y0),
                    (x1, y0 + len * 0.4),
                    (x1 - w * 0.15, y1 - len * 0.12),
                    (c, y1),
                    (x0 + w * 0.15, y1 - len * 0.12),
                    (x0, y0 + len * 0.4),
                ],
                // Short and broad, two cusps.
                Tooth::Molar => vec![
                    (x0 + w * 0.1, y0),
                    (x1 - w * 0.1, y0),
                    (x1, y0 + len * 0.45),
                    (x1 - w * 0.22, y1),
                    (c, y1 - len * 0.18),
                    (x0 + w * 0.22, y1),
                    (x0, y0 + len * 0.45),
                ],
            };
            let crown = crown.into_iter().map(|(x, y)| (x * side, y)).collect();
            out.push((
                crown,
                0.72 * mid.cos(),
                V3::new(mid.sin() * side, 0.15, mid.cos()),
            ));
        }
    }
    out
}

fn lambert(n: V3) -> f32 {
    (n.x * LIGHT.x + n.y * LIGHT.y + n.z * LIGHT.z).max(0.0)
}

/// A halftone dot with everything that doesn't change frame to frame.
struct Dot {
    p: P,
    kind: DotKind,
    /// Dot size relative to the main screen (teeth use a finer one).
    scale: f32,
}

enum DotKind {
    /// On the bone: surface height and resting normal (rotated with the head each frame).
    Bone { z: f32, n: V3 },
    /// On a void's far inner wall: its fixed tone.
    Wall(f32),
}

pub struct Skull {
    t: f32,
    w: f32,
    h: f32,
    cols: usize,
    rows: usize,
    silhouette: Vec<P>,
    sockets: [Vec<P>; 2],
    nose: Vec<P>,
    teeth: Vec<(Vec<P>, f32, V3)>,
    /// Built in `init` for the screen's dot spacing: the costly geometry (point-in-polygon
    /// tests, height-field normals) is done once, not every frame.
    dots: Vec<Dot>,
}

impl Skull {
    pub fn new() -> Self {
        let socket_r = smooth(&SOCKET, 6);
        let socket_l = socket_r.iter().map(|&(x, y)| (-x, y)).collect();
        Self {
            t: 0.0,
            w: 0.0,
            h: 0.0,
            cols: 0,
            rows: 0,
            silhouette: smooth(&mirrored(&SILHOUETTE), 6),
            sockets: [socket_r, socket_l],
            nose: smooth(&NOSE, 5),
            teeth: teeth(),
            dots: Vec::new(),
        }
    }

    /// Lay out the halftone screen in skull units (`step` apart) and classify every dot.
    fn layout(&self, step: f32) -> Vec<Dot> {
        let mut dots = Vec::new();
        let (nx, ny) = ((1.05 / step) as i32 + 1, (1.45 / (step * 0.866)) as i32 + 1);
        for j in -ny..=ny {
            for i in -nx..=nx {
                let p = (
                    (i as f32 + if j % 2 != 0 { 0.5 } else { 0.0 }) * step,
                    -0.2 + j as f32 * step * 0.866,
                );
                if !inside(&self.silhouette, p) || self.teeth.iter().any(|(o, ..)| inside(o, p)) {
                    continue;
                }
                let kind = if self.in_void(p) {
                    // Far inner wall of a socket or the nose: lit on the side away from the light.
                    let (center, radius) = if inside(&self.nose, p) {
                        (NOSE_CENTER, 0.24)
                    } else {
                        ((EYE.0 * p.0.signum(), EYE.1), 0.4)
                    };
                    let (dx, dy) = (p.0 - center.0, p.1 - center.1);
                    let r = (dx * dx + dy * dy).sqrt().max(1e-6);
                    let toward_far_wall = ((dx * -LIGHT_2D.0 + dy * -LIGHT_2D.1) / r).max(0.0);
                    let wall = ((r / radius - 0.55) / 0.4).clamp(0.0, 1.0) * toward_far_wall;
                    if wall < 0.1 {
                        continue;
                    }
                    DotKind::Wall(0.38 * wall)
                } else {
                    DotKind::Bone {
                        z: height(p),
                        n: normal(p),
                    }
                };
                dots.push(Dot {
                    p,
                    kind,
                    scale: 1.0,
                });
            }
        }
        // Teeth get their own dots, in columns centered within each tooth: the shared screen
        // would land on such narrow shapes unevenly and leave ragged teeth.
        for (outline, z, n) in &self.teeth {
            let (x0, x1) = outline
                .iter()
                .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.0), m.1.max(p.0)));
            let (y0, y1) = outline
                .iter()
                .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.1), m.1.max(p.1)));
            // A finer screen than the face, so crown shapes read.
            let fine = step * TOOTH_DOTS;
            let cols = ((x1 - x0) / fine).round().max(1.0) as usize;
            for col in 0..cols {
                let x = x0 + (x1 - x0) * (col as f32 + 0.5) / cols as f32;
                let mut y = y0 + fine * 0.5;
                while y < y1 {
                    if inside(outline, (x, y)) {
                        dots.push(Dot {
                            p: (x, y),
                            kind: DotKind::Bone { z: *z, n: *n },
                            scale: TOOTH_DOTS,
                        });
                    }
                    y += fine * 0.85;
                }
            }
        }
        dots
    }

    fn in_void(&self, p: P) -> bool {
        inside(&self.sockets[0], p) || inside(&self.sockets[1], p) || inside(&self.nose, p)
    }

    /// Screen center and skull scale (px per skull unit), centering the skull's extent
    /// (-1.39 to 1.18) on the optical center.
    fn frame_of(&self) -> (P, f32) {
        let (c, unit) = optical_center(self.w, self.h);
        let s = unit * SCALE;
        ((c.0, c.1 + 0.105 * s), s)
    }

    /// Head pose: a slow turn (yaw) and nod (pitch), in radians.
    fn pose(&self) -> (f32, f32) {
        (
            0.16 * (self.t * 0.3).sin(),
            0.06 * (self.t * 0.23 + 1.0).sin(),
        )
    }

    /// Scanline position in skull units.
    fn scan_y(&self) -> f32 {
        SCAN_TOP + SCAN_SPAN * (self.t / SCAN_PERIOD).fract()
    }

    fn glitching(&self) -> Option<u32> {
        let burst = (self.t / GLITCH_EVERY).floor();
        (self.t - burst * GLITCH_EVERY < GLITCH_FOR).then_some(burst as u32)
    }
}

impl Default for Skull {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for Skull {
    fn name(&self) -> &'static str {
        "skull"
    }

    fn fps(&self) -> u32 {
        30
    }

    fn init(&mut self, size: Size, seed: u64) {
        (self.w, self.h, self.cols, self.rows) = (size.width, size.height, size.cols, size.rows);
        self.t = (seed % 1000) as f32;
        let (_, s) = self.frame_of();
        if s > 0.0 {
            self.dots = self.layout(Pen::new(s / SCALE).px(DOT_STEP) / s);
        }
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
    }

    /// Readouts in the character grid: hex dump left, binary right, status line below.
    fn render(&self, grid: &mut Grid) {
        grid.clear();
        if self.cols == 0 || self.rows == 0 {
            return;
        }
        let (c, s) = self.frame_of();
        let (cw, ch) = (self.w / self.cols as f32, self.h / self.rows as f32);
        let mut put = |col: isize, row: isize, text: &str, level: u8| {
            for (i, chr) in text.chars().enumerate() {
                let x = col + i as isize;
                if x >= 0 && row >= 0 && chr != ' ' {
                    grid.set(
                        x as usize,
                        row as usize,
                        Cell {
                            ch: chr,
                            brightness: level,
                        },
                    );
                }
            }
        };
        let (top, bottom) = (
            ((c.1 - s * 1.35) / ch) as isize,
            ((c.1 + s * 1.15) / ch) as isize,
        );
        let scan_row = ((c.1 + self.scan_y() * s) / ch) as isize;
        let scroll = (self.t * 3.0) as isize;
        let (left_col, right_col) = (
            ((c.0 - s * 1.3) / cw) as isize - 16,
            ((c.0 + s * 1.3) / cw) as isize + 2,
        );
        for row in top..=bottom {
            let idx = (row + scroll) as u32;
            let hh = hash3(idx, 11, 0);
            let edge = ((row - top).min(bottom - row) as f32 / 3.0).min(1.0); // fade the ends
            let hot = (row - scan_row).abs() <= 1;
            let level = if hot { 170 } else { (50.0 * edge) as u8 };
            let b = |k: u32| (hh >> (8 * k)) as u8;
            let hex = format!(
                "{:04X}  {:02X} {:02X} {:02X}",
                idx.wrapping_mul(16) & 0xFFFF,
                b(0),
                b(1),
                b(2)
            );
            put(left_col, row, &hex, level);
            put(right_col, row, &format!("{:08b}", b(3)), level);
        }
        // Status line: phrases typed out one by one, with a blinking cursor.
        let per = 3.2;
        let k = (self.t / per) as usize;
        let phrase = MESSAGES[k % MESSAGES.len()];
        let typed = (((self.t % per) * 16.0) as usize).min(phrase.len());
        let cursor = if (self.t * 2.0).fract() < 0.5 {
            "_"
        } else {
            " "
        };
        let line = format!("> {}{cursor}", &phrase[..typed]);
        let row = ((c.1 + s * 1.5) / ch) as isize;
        put((c.0 / cw) as isize - 9, row, &line, 140);
    }

    fn draw(&self, out: &mut Vec<Shape>) {
        let t = self.t;
        let (c, s) = self.frame_of();
        let pen = Pen::new(s / SCALE);
        let start = out.len();
        let (yaw, pitch) = self.pose();
        let rotate = |v: V3| v.rot_y(yaw).rot_x(pitch);
        // A point on the skull at local `p` with surface height `z` (0..1), on screen.
        let project = |p: P, z: f32| {
            let q = rotate(V3::new(p.0, p.1, z * DEPTH));
            (c.0 + q.x * s, c.1 + q.y * s)
        };
        let on_surface = |p: P| project(p, height(p));

        // HUD: corner brackets and a dashed baseline, fixed to the screen.
        let hud = |x: f32, y: f32| (c.0 + x * s, c.1 + y * s);
        let (bx, by0, by1, arm) = (1.25, -1.55, 1.33, 0.22);
        for (sx, sy, y) in [
            (-1.0, 1.0, by0),
            (1.0, 1.0, by0),
            (-1.0, -1.0, by1),
            (1.0, -1.0, by1),
        ] {
            let corner = hud(sx * bx, y);
            pen.line(out, corner, hud(sx * (bx - arm), y), MID, 110);
            pen.line(out, corner, hud(sx * bx, y + sy * arm), MID, 110);
        }
        pen.dashed(
            out,
            &[hud(-0.95, 1.27), hud(0.95, 1.27)],
            4.0,
            5.0,
            HAIR,
            40,
        );
        for i in 0..=10 {
            let x = -0.95 + i as f32 * 0.19;
            let len = if i % 5 == 0 { 0.07 } else { 0.03 };
            pen.line(out, hud(x, 1.27), hud(x, 1.27 + len), HAIR, 50);
        }

        // Halftone form: dots on the surface, lit from the rotated normal, shrinking as the
        // surface turns away; brightened where the scanline passes. Void dots are the far
        // inner walls, which catch a little light.
        let scan = self.scan_y();
        let dot_r = pen.px(DOT_STEP) * 0.48;
        for dot in &self.dots {
            let (screen, tone, facing) = match dot.kind {
                DotKind::Bone { z, n } => {
                    let n = rotate(n);
                    (
                        project(dot.p, z),
                        (0.1 + 0.9 * lambert(n)).min(1.0),
                        n.z.max(0.0),
                    )
                }
                DotKind::Wall(tone) => (project(dot.p, 0.2), tone, 0.7),
            };
            let glow = (1.0 - (dot.p.1 - scan).abs() / 0.06).max(0.0);
            let tone = (tone + 0.45 * glow).min(1.0);
            out.push(Shape::Circle {
                c: screen,
                r: dot_r * dot.scale * tone.sqrt() * facing.powf(0.35),
                fill: true,
                level: (55.0 + 170.0 * tone) as u8,
            });
        }

        // Teeth: dots come from the layout; here a faint edge, a scalloped gum line arching
        // over each root, and a highlight stroke down the lit side of the crown.
        for (outline, z, n) in &self.teeth {
            let lit = 0.2 + 0.8 * lambert(rotate(*n));
            let mut edge: Vec<P> = outline.iter().map(|&p| project(p, *z)).collect();
            edge.push(edge[0]);
            pen.path(out, &edge, HAIR, (35.0 + 80.0 * lit) as u8);
            let (x0, x1) = outline
                .iter()
                .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.0), m.1.max(p.0)));
            let (y0, y1) = outline
                .iter()
                .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.1), m.1.max(p.1)));
            let gum: Vec<P> = (0..=10)
                .map(|i| {
                    let f = i as f32 / 10.0;
                    project((x0 + (x1 - x0) * f, y0 + 0.025 * (PI * f).sin()), *z)
                })
                .collect();
            pen.path(out, &gum, HAIR + 0.2, (50.0 + 110.0 * lit) as u8);
            let hx = x0 + (x1 - x0) * if (x0 + x1) > 0.0 { 0.3 } else { 0.35 };
            let (a, b) = (
                project((hx, y0 + 0.05), *z),
                project((hx, y0 + (y1 - y0) * 0.75), *z),
            );
            pen.line(out, a, b, HAIR + 0.25, (70.0 + 160.0 * lit) as u8);
        }

        // Line work: rim-lit silhouette (the hero line), mid-weight void edges.
        // The straight bottom edge above the teeth is left out, so bone flows into teeth.
        let silhouette: Vec<P> = self.silhouette.iter().map(|&p| on_surface(p)).collect();
        let ridge = c.1 + 0.85 * s;
        lit_outline(pen, out, &silhouette, project((0.0, -0.2), 0.0), ridge);
        for void in [&self.sockets[0], &self.sockets[1], &self.nose] {
            pen.path(
                out,
                &void.iter().map(|&p| on_surface(p)).collect::<Vec<_>>(),
                MID,
                150,
            );
        }

        // Eyes: pupils deep in the sockets (so they shift less than the rims as the head
        // turns), holding then darting; a reticle in the right socket.
        let gaze = {
            let k = (t / 1.6).floor();
            let from = noise(k as u32, 5, 0) - 0.5;
            let to = noise(k as u32 + 1, 5, 0) - 0.5;
            from + (to - from) * ease((t / 1.6).fract() / 0.15)
        };
        for side in [-1.0f32, 1.0] {
            let eye = project((side * EYE.0 + gaze * 0.16, EYE.1), 0.25);
            out.push(Shape::Circle {
                c: eye,
                r: pen.px(9.0),
                fill: false,
                level: 60,
            });
            pen.dot(out, eye, 3.6, 245);
        }
        let reticle = project((EYE.0 + gaze * 0.16, EYE.1), 0.25);
        let spin = t * 0.8;
        let rr = s * 0.2;
        let ring: Vec<P> = (0..=48)
            .map(|i| {
                let a = spin + i as f32 / 48.0 * TAU;
                (reticle.0 + a.cos() * rr, reticle.1 + a.sin() * rr)
            })
            .collect();
        pen.dashed(out, &ring, 5.0, 4.0, HAIR, 120);
        for q in 0..4 {
            let a = spin + q as f32 * PI / 2.0;
            let p = |r: f32| (reticle.0 + a.cos() * r, reticle.1 + a.sin() * r);
            pen.line(out, p(rr * 1.18), p(rr * 1.5), HAIR + 0.3, 150);
        }

        // Scanline with a short fading trail, across the bracketed frame.
        let sy = c.1 + scan * s;
        pen.line(out, (c.0 - bx * s, sy), (c.0 + bx * s, sy), HAIR + 0.3, 170);
        for (k, level) in [(1.0, 80), (2.0, 50), (3.0, 28)] {
            let y = sy - pen.px(3.0) * k;
            pen.line(out, (c.0 - bx * s, y), (c.0 + bx * s, y), HAIR, level);
        }

        // Glitch: during a burst, a few horizontal bands tear sideways, stuttering.
        if let Some(burst) = self.glitching() {
            let stutter = ((t % GLITCH_EVERY) * 20.0) as u32;
            for band in 0..3 {
                let y0 = c.1 + s * (SCAN_TOP + SCAN_SPAN * noise(burst, band, 7));
                let y1 = y0 + s * (0.06 + 0.12 * noise(burst, band, 8));
                let dx = s * 0.28 * (noise(burst, band, stutter) - 0.5);
                for shape in &mut out[start..] {
                    shift_if_in_band(shape, y0, y1, dx);
                }
            }
        }
    }
}

/// Closed outline whose weight follows the light: HERO where the outward normal faces it,
/// HAIR on the shadow side. `center` decides which side is outward; segments lower than
/// `below` on screen are skipped.
fn lit_outline(pen: Pen, out: &mut Vec<Shape>, pts: &[P], center: P, below: f32) {
    for seg in pts.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        if a.1 > below && b.1 > below {
            continue; // segments entirely below `below` (screen y) are skipped
        }
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt().max(1e-6);
        let mut normal = ((b.1 - a.1) / len, -(b.0 - a.0) / len);
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        if normal.0 * (mid.0 - center.0) + normal.1 * (mid.1 - center.1) < 0.0 {
            normal = (-normal.0, -normal.1);
        }
        let lit = (normal.0 * LIGHT_2D.0 + normal.1 * LIGHT_2D.1)
            .max(0.0)
            .powf(0.8);
        pen.line(
            out,
            a,
            b,
            HAIR + (HERO - HAIR) * lit,
            (70.0 + 170.0 * lit) as u8,
        );
    }
}

/// Shift a shape horizontally by `dx` if it sits within the band [y0, y1).
fn shift_if_in_band(shape: &mut Shape, y0: f32, y1: f32, dx: f32) {
    let within = |y: f32| y >= y0 && y < y1;
    match shape {
        Shape::Line { a, b, .. } if within(a.1) => {
            a.0 += dx;
            b.0 += dx;
        }
        Shape::Circle { c, .. } if within(c.1) => c.0 += dx,
        Shape::Polygon { points, .. } if points.first().is_some_and(|p| within(p.1)) => {
            for p in points {
                p.0 += dx;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anatomy_and_form() {
        let skull = Skull::new();
        assert_eq!(skull.silhouette.first(), skull.silhouette.last());
        // Sockets and nose are voids inside the skull; nothing below the teeth.
        assert!(inside(&skull.silhouette, EYE) && skull.in_void(EYE));
        assert!(skull.in_void((0.0, 0.4)) && !skull.in_void((0.0, -0.8)));
        assert!(!inside(&skull.silhouette, (0.0, 1.1)));
        // The surface is highest on the dome, recessed at the sockets, lit from the upper left.
        assert!(height((0.0, -0.6)) > height(EYE));
        assert!(lambert(normal((-0.4, -0.8))) > lambert(normal((0.8, 0.5))));
    }
}
