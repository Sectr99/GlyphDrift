//! Drifting points, joined by faint lines when close.

use super::{Animation, Rng, Size};
use crate::shape::Shape;

// Sizes in px at a 1440 px screen, scaled by `scale`.
const PX_PER_STAR: f32 = 18_000.0; // one star per this many square px
const LINK: f32 = 140.0; // max link distance

struct Star {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    r: f32,
}

pub struct Constellation {
    stars: Vec<Star>,
    w: f32,
    h: f32,
    /// Pixel scale: focus unit / 1440.
    scale: f32,
}

impl Constellation {
    pub fn new() -> Self {
        Self {
            stars: Vec::new(),
            w: 0.0,
            h: 0.0,
            scale: 1.0,
        }
    }
}

impl Default for Constellation {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for Constellation {
    fn name(&self) -> &'static str {
        "constellation"
    }

    fn fps(&self) -> u32 {
        30
    }

    fn init(&mut self, size: Size, seed: u64) {
        let mut rng = Rng::new(seed);
        (self.w, self.h) = (size.width, size.height);
        // Tuned at 1440 px tall; same proportions elsewhere.
        let k = (size.focus.unit() / 1440.0).max(0.25);
        self.scale = k;
        let n = (size.width * size.height / (PX_PER_STAR * k * k)).ceil() as usize;
        self.stars = (0..n)
            .map(|_| {
                let angle = rng.range(0.0, std::f32::consts::TAU);
                let speed = rng.range(6.0, 20.0) * k; // px per second
                Star {
                    x: rng.range(0.0, size.width),
                    y: rng.range(0.0, size.height),
                    vx: angle.cos() * speed,
                    vy: angle.sin() * speed,
                    r: rng.range(1.2, 2.6) * k,
                }
            })
            .collect();
    }

    fn update(&mut self, dt: f32) {
        // Wrap with a margin so stars leave and re-enter off screen.
        let m = LINK * self.scale;
        for s in &mut self.stars {
            s.x = (s.x + s.vx * dt + m).rem_euclid(self.w + 2.0 * m) - m;
            s.y = (s.y + s.vy * dt + m).rem_euclid(self.h + 2.0 * m) - m;
        }
    }

    fn draw(&self, out: &mut Vec<Shape>) {
        // ponytail: O(n^2) pair scan (~10k pairs at 1440p); a spatial grid if density grows.
        for (i, a) in self.stars.iter().enumerate() {
            for b in &self.stars[i + 1..] {
                let d = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
                let link = LINK * self.scale;
                if d < link {
                    out.push(Shape::Line {
                        a: (a.x, a.y),
                        b: (b.x, b.y),
                        width: self.scale.max(0.75),
                        level: ((1.0 - d / link) * 150.0) as u8,
                    });
                }
            }
        }
        for s in &self.stars {
            out.push(Shape::Circle {
                c: (s.x, s.y),
                r: s.r,
                fill: true,
                level: 230,
            });
        }
    }
}
