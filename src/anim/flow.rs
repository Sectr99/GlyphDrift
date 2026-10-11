//! Flow field: thin trails following a slowly changing invisible current.

use super::{Animation, Rng, Size};
use crate::shape::Shape;

const STEP: f32 = 1.0 / 30.0; // fixed simulation step, so trails look the same at any FPS
const TRAIL: usize = 14; // points per trail
const PX_PER_PARTICLE: f32 = 9_000.0;

struct Particle {
    /// Newest point first.
    trail: [(f32, f32); TRAIL],
    speed: f32,
    age: f32,
    life: f32,
}

pub struct Flow {
    particles: Vec<Particle>,
    rng: Rng,
    w: f32,
    h: f32,
    /// Pixel scale: focus unit / 1440.
    scale: f32,
    t: f32,
    accum: f32,
}

impl Flow {
    pub fn new() -> Self {
        Self {
            particles: Vec::new(),
            rng: Rng::new(1),
            w: 0.0,
            h: 0.0,
            scale: 1.0,
            t: 0.0,
            accum: 0.0,
        }
    }

    /// Direction of the current at (x, y), in radians: a few slow, smooth waves.
    fn angle(&self, x: f32, y: f32) -> f32 {
        let t = self.t;
        let (x, y) = (x / self.scale, y / self.scale);
        2.0 * (x * 0.0025 + t * 0.05).sin()
            + 2.0 * (y * 0.003 - t * 0.04).cos()
            + ((x + y) * 0.0015 + t * 0.03).sin()
    }

    fn spawn(rng: &mut Rng, w: f32, h: f32, scale: f32, age: f32) -> Particle {
        let p = (rng.range(0.0, w), rng.range(0.0, h));
        Particle {
            trail: [p; TRAIL],
            speed: rng.range(25.0, 45.0) * scale, // px per second
            age,
            life: rng.range(4.0, 10.0),
        }
    }

    fn step(&mut self) {
        self.t += STEP;
        for i in 0..self.particles.len() {
            let (x, y) = self.particles[i].trail[0];
            let a = self.angle(x, y);
            let p = &mut self.particles[i];
            p.age += STEP;
            p.trail.copy_within(0..TRAIL - 1, 1);
            p.trail[0] = (x + a.cos() * p.speed * STEP, y + a.sin() * p.speed * STEP);
            let (nx, ny) = p.trail[0];
            // Respawn rather than wrap, so trails never streak across the screen.
            if p.age >= p.life || nx < 0.0 || ny < 0.0 || nx > self.w || ny > self.h {
                *p = Self::spawn(&mut self.rng, self.w, self.h, self.scale, 0.0);
            }
        }
    }
}

impl Default for Flow {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for Flow {
    fn name(&self) -> &'static str {
        "flow"
    }

    fn fps(&self) -> u32 {
        30
    }

    fn init(&mut self, size: Size, seed: u64) {
        self.rng = Rng::new(seed);
        (self.w, self.h) = (size.width, size.height);
        // Tuned on a 1440 px-tall screen: keep that look there, and the same proportions elsewhere.
        self.scale = (size.focus.unit() / 1440.0).max(0.25);
        self.t = (seed % 1000) as f32;
        self.accum = 0.0;
        let n = (size.width * size.height / (PX_PER_PARTICLE * self.scale * self.scale)).ceil()
            as usize;
        self.particles = (0..n)
            .map(|_| {
                let age = self.rng.range(0.0, 4.0);
                Self::spawn(&mut self.rng, size.width, size.height, self.scale, age)
            })
            .collect();
        for _ in 0..TRAIL {
            self.step(); // grow the trails before the first frame
        }
    }

    fn update(&mut self, dt: f32) {
        self.accum += dt;
        while self.accum >= STEP {
            self.accum -= STEP;
            self.step();
        }
    }

    fn draw(&self, out: &mut Vec<Shape>) {
        for p in &self.particles {
            // Fade in over the first second and out over the last.
            let life = p.age.min(p.life - p.age).clamp(0.0, 1.0);
            for (i, seg) in p.trail.windows(2).enumerate() {
                let tail = 1.0 - i as f32 / (TRAIL - 1) as f32;
                let level = (life * tail * 210.0) as u8;
                if level > 0 && seg[0] != seg[1] {
                    out.push(Shape::Line {
                        a: seg[0],
                        b: seg[1],
                        width: 1.2 * self.scale.max(0.75),
                        level,
                    });
                }
            }
        }
    }
}
