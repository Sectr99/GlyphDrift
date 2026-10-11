//! Animation trait and built-in animations.

mod constellation;
mod contour;
mod fire;
mod flow;
mod fluid;
mod geom;
mod isolines;
mod loops;
mod matrix;
mod plasma;
mod ripples;
mod rng;
mod waves;
mod wind;

pub use constellation::Constellation;
pub use fire::Fire;
pub use flow::Flow;
pub use fluid::{Ink, LavaLamp, Topographic};
pub use isolines::{Dunes, Hillshade, Interference, Isobars, Ridgelines, WoodGrain};
pub use loops::{Cubes, DotWave, Globe, Helix, Moon, Sunburst};
pub use matrix::Matrix;
pub use plasma::Plasma;
pub use ripples::Ripples;
pub use rng::Rng;
pub use waves::Waves;
pub use wind::Wind;

use crate::grid::Grid;
use crate::shape::Shape;

/// A rectangle in window px, with the user's scale settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Overall scale.
    pub zoom: f32,
    /// Extra scale for centered objects.
    pub object_zoom: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            zoom: 1.0,
            object_zoom: 1.0,
        }
    }

    /// Scale unit for sizes: the smaller side times the overall scale.
    pub fn unit(&self) -> f32 {
        self.w.min(self.h) * self.zoom
    }

    /// Scale unit for centered objects.
    pub fn object_unit(&self) -> f32 {
        self.unit() * self.object_zoom
    }
}

impl Default for Rect {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }
}

/// Drawing area for `init`: character grid, window size (px) and `focus`, the primary
/// monitor within the window. Full-bleed animations fill the window, centered ones center
/// on `focus`; both scale by `focus.unit()`.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub cols: usize,
    pub rows: usize,
    pub width: f32,
    pub height: f32,
    pub focus: Rect,
}

impl Size {
    /// A `cols` x `rows` grid of nominal 8x16 px cells (tests).
    pub fn cells(cols: usize, rows: usize) -> Self {
        let (width, height) = (cols as f32 * 8.0, rows as f32 * 16.0);
        Self {
            cols,
            rows,
            width,
            height,
            focus: Rect::new(0.0, 0.0, width, height),
        }
    }
}

pub trait Animation {
    fn name(&self) -> &'static str;
    /// Use the theme's warm gradient instead of the cool one.
    fn warm(&self) -> bool {
        false
    }
    /// Frame rate when the config says auto (fps = 0).
    fn fps(&self) -> u32 {
        24
    }
    /// Called before the first `update`, and on resize.
    fn init(&mut self, size: Size, seed: u64);
    /// Advance by `dt` seconds.
    fn update(&mut self, dt: f32);
    /// Write the current frame into `grid`. Shape-only animations leave it empty.
    fn render(&self, grid: &mut Grid) {
        grid.clear();
    }
    /// Append the current frame's vector shapes.
    fn draw(&self, _out: &mut Vec<Shape>) {}
    /// Local time and long date, or None for no clock. Called every frame.
    fn set_clock(&mut self, _clock: Option<&[String; 2]>) {}
}

/// (config name, menu label, menu group).
pub const ANIMATIONS: &[(&str, &str, &str)] = &[
    ("matrix", "Matrix rain", "ascii"),
    ("plasma", "Plasma", "ascii"),
    ("fire", "Fire", "ascii"),
    ("waves", "Waves", "ascii"),
    ("ripples", "Ripples", "ascii"),
    ("wind", "Wind", "ascii"),
    ("constellation", "Constellation", "graphics"),
    ("flow", "Flow field", "graphics"),
    ("helix", "Helix", "loops"),
    ("globe", "Wire globe", "loops"),
    ("dotwave", "Dot wave", "loops"),
    ("cubes", "Cubes", "loops"),
    ("sunburst", "Sunburst", "loops"),
    ("moon", "Moon", "loops"),
    ("lava", "Lava lamp", "fluid"),
    ("ink", "Ink in water", "fluid"),
    ("topo", "Topographic drift", "contour"),
    ("ridges", "Ridgelines", "contour"),
    ("isobars", "Isobars", "contour"),
    ("wood", "Wood grain", "contour"),
    ("interference", "Interference", "contour"),
    ("hillshade", "Hillshade topo", "contour"),
    ("dunes", "Dune ripples", "contour"),
];

/// Index into `ANIMATIONS`; unknown names give 0 (matrix).
pub fn index_of(name: &str) -> usize {
    ANIMATIONS.iter().position(|a| a.0 == name).unwrap_or(0)
}

/// Build an animation by config name. Unknown names fall back to matrix.
pub fn by_name(name: &str, ramp: &str) -> Box<dyn Animation> {
    match name {
        "plasma" => Box::new(Plasma::new(ramp)),
        "fire" => Box::new(Fire::new(ramp)),
        "waves" => Box::new(Waves::new()),
        "ripples" => Box::new(Ripples::new()),
        "wind" => Box::new(Wind::new()),
        "constellation" => Box::new(Constellation::new()),
        "flow" => Box::new(Flow::new()),
        "helix" => Box::new(Helix::new()),
        "globe" => Box::new(Globe::new()),
        "dotwave" => Box::new(DotWave::new()),
        "cubes" => Box::new(Cubes::new()),
        "sunburst" => Box::new(Sunburst::new()),
        "moon" => Box::new(Moon::new()),
        "lava" => Box::new(LavaLamp::new()),
        "topo" => Box::new(Topographic::new()),
        "ink" => Box::new(Ink::new()),
        "ridges" => Box::new(Ridgelines::new()),
        "isobars" => Box::new(Isobars::new()),
        "wood" => Box::new(WoodGrain::new()),
        "interference" => Box::new(Interference::new()),
        "hillshade" => Box::new(Hillshade::new()),
        "dunes" => Box::new(Dunes::new()),
        _ => Box::new(Matrix::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(name: &str, seed: u64) -> Grid {
        frame_and_shapes(name, seed).0
    }

    fn frame_and_shapes(name: &str, seed: u64) -> (Grid, Vec<Shape>) {
        let mut a = by_name(name, " .:-=+*#%@");
        assert_eq!(a.name(), name);
        a.init(Size::cells(60, 30), seed);
        let mut g = Grid::new(60, 30);
        for _ in 0..48 {
            a.update(1.0 / 24.0);
        }
        a.render(&mut g);
        let mut shapes = Vec::new();
        a.draw(&mut shapes);
        (g, shapes)
    }

    #[test]
    fn every_animation_draws_deterministically() {
        for &(name, ..) in ANIMATIONS {
            let (g, shapes) = frame_and_shapes(name, 42);
            assert_eq!(
                shapes,
                frame_and_shapes(name, 42).1,
                "{name} shapes not deterministic"
            );
            assert_eq!(
                g.to_text(),
                frame(name, 42).to_text(),
                "{name} not deterministic"
            );
            let lit = (0..g.rows())
                .flat_map(|y| g.row(y).iter())
                .filter(|c| c.brightness > 0 && c.ch != ' ')
                .count();
            assert!(lit + shapes.len() > 0, "{name} drew nothing");
        }
    }

    /// Frame cost per animation at 3440x1440, without painting. GLYPHDRIFT_SCALE=0.5 runs
    /// it at that overall scale. `cargo test --release frame_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn frame_cost() {
        let zoom: f32 = std::env::var("GLYPHDRIFT_SCALE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0);
        let size = Size {
            cols: (430.0 / zoom) as usize,
            rows: (90.0 / zoom) as usize,
            width: 3440.0,
            height: 1440.0,
            focus: Rect {
                zoom,
                ..Rect::new(0.0, 0.0, 3440.0, 1440.0)
            },
        };
        for &(name, ..) in ANIMATIONS {
            let mut a = by_name(name, " .:-=+*#%@");
            a.init(size, 7);
            let mut g = Grid::new(size.cols, size.rows);
            let mut shapes = Vec::new();
            let frames = 60;
            let (mut update, mut render, mut draw) = (0.0, 0.0, 0.0);
            let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0 / frames as f64;
            for _ in 0..frames {
                let t = std::time::Instant::now();
                a.update(1.0 / 30.0);
                update += ms(t);
                let t = std::time::Instant::now();
                a.render(&mut g);
                render += ms(t);
                let t = std::time::Instant::now();
                shapes.clear();
                a.draw(&mut shapes);
                draw += ms(t);
            }
            println!(
                "{name:>13}: {:6.2} ms/frame (update {update:5.2}, grid {render:5.2}, shapes {draw:5.2}), {:6} shapes",
                update + render + draw,
                shapes.len()
            );
        }
    }

    #[test]
    fn every_animation_stays_bounded_at_4k() {
        // Small grids once hid a loop that never ended at full-screen coordinates.
        let size = Size {
            cols: 480,
            rows: 135,
            width: 3840.0,
            height: 2160.0,
            focus: Rect::new(0.0, 0.0, 3840.0, 2160.0),
        };
        for &(name, ..) in ANIMATIONS {
            let mut a = by_name(name, " .:-=+*#%@");
            a.init(size, 9);
            let mut g = Grid::new(size.cols, size.rows);
            let mut shapes = Vec::new();
            for _ in 0..3 {
                a.update(7.3); // big steps reach late phases quickly
                a.render(&mut g);
                shapes.clear();
                a.draw(&mut shapes);
                assert!(shapes.len() < 100_000, "{name}: {} shapes", shapes.len());
            }
        }
    }

    #[test]
    fn centered_loops_follow_the_primary_monitor_and_full_bleed_fills_the_desktop() {
        // Two 1600x1000 monitors, the primary on the right.
        let size = Size {
            cols: 400,
            rows: 70,
            width: 3200.0,
            height: 1000.0,
            focus: Rect::new(1600.0, 0.0, 1600.0, 1000.0),
        };
        let xs = |name: &str| -> Vec<f32> {
            let mut a = by_name(name, " .:-=+*#%@");
            a.init(size, 3);
            a.update(2.0);
            let mut shapes = Vec::new();
            a.draw(&mut shapes);
            shapes
                .iter()
                .map(|s| match s {
                    Shape::Line { a, b, .. } => (a.0 + b.0) / 2.0,
                    Shape::Circle { c, .. } => c.0,
                    Shape::Polygon { points, .. } => points[0].0,
                    Shape::Text { c, .. } => c.0,
                })
                .collect()
        };
        for &(name, _, group) in ANIMATIONS {
            if group != "loops" || name == "globe" {
                continue; // the globe is deliberately off-center
            }
            let x = xs(name);
            let mean = x.iter().sum::<f32>() / x.len() as f32;
            assert!(
                (mean - 2400.0).abs() < 250.0,
                "{name} centered at {mean}, not on the primary"
            );
        }
        let ridges = xs("ridges");
        let (lo, hi) = ridges
            .iter()
            .fold((f32::MAX, f32::MIN), |m, &x| (m.0.min(x), m.1.max(x)));
        assert!(
            lo < 20.0 && hi > 3180.0,
            "ridgelines span {lo}..{hi}, not the whole desktop"
        );
    }

    #[test]
    fn gentle_animations_stay_sparse() {
        for name in ["waves", "ripples", "wind"] {
            let g = frame(name, 7);
            let lit = g.to_text().chars().filter(|c| !c.is_whitespace()).count();
            assert!(lit < 60 * 30 / 3, "{name} lit {lit} cells, not gentle");
        }
    }
}
