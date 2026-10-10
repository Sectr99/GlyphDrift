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

/// A rectangle in window pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// The smaller side: the scale unit for sizes, so features look the same on any aspect.
    pub fn unit(&self) -> f32 {
        self.w.min(self.h)
    }
}

/// Drawing area handed to `init`: the character grid, the window size in pixels, and the
/// `focus` rectangle (the primary monitor within the window). Full-bleed animations fill the
/// whole window; centered ones center on `focus`; both take their scale from `focus.unit()`,
/// so a multi-monitor desktop doesn't split a centered object or shrink features.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub cols: usize,
    pub rows: usize,
    pub width: f32,
    pub height: f32,
    pub focus: Rect,
}

impl Size {
    /// A `cols` x `rows` grid of nominal 8x16 px cells (tests and previews).
    pub fn cells(cols: usize, rows: usize) -> Self {
        let (width, height) = (cols as f32 * 8.0, rows as f32 * 16.0);
        Self {
            cols,
            rows,
            width,
            height,
            focus: Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
            },
        }
    }
}

pub trait Animation {
    fn name(&self) -> &'static str;
    /// Use the theme's warm gradient instead of the cool one.
    fn warm(&self) -> bool {
        false
    }
    /// Frame rate used when the config says "auto" (fps = 0).
    fn fps(&self) -> u32 {
        24
    }
    /// Called once before the first `update`, and again on resize.
    fn init(&mut self, size: Size, seed: u64);
    /// Advance simulation by `dt` seconds.
    fn update(&mut self, dt: f32);
    /// Write the current frame into `grid` (`size.cols` x `size.rows` from `init`).
    /// Shape-only animations keep the default, which leaves the grid empty.
    fn render(&self, grid: &mut Grid) {
        grid.clear();
    }
    /// Append the current frame's vector shapes. Grid-only animations draw none.
    fn draw(&self, _out: &mut Vec<Shape>) {}
}

/// (config name, menu label, menu group); the tray separates groups.
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

/// Speed presets offered in the tray: (multiplier, menu label).
pub const SPEEDS: &[(f32, &str)] = &[
    (0.25, "0.25×"),
    (0.5, "0.5×"),
    (0.75, "0.75×"),
    (1.0, "1× (normal)"),
    (1.5, "1.5×"),
    (2.0, "2×"),
];

/// Frame-rate presets offered in the tray: (fps, menu label); 0 = each animation's own.
pub const FRAME_RATES: &[(f32, &str)] = &[
    (0.0, "Auto (24 ASCII / 30 graphics)"),
    (30.0, "30 fps"),
    (60.0, "60 fps"),
];

/// Index of the preset closest to `v`, so a hand-edited config still gets a check mark.
pub fn nearest(presets: &[(f32, &str)], v: f32) -> usize {
    (0..presets.len())
        .min_by(|&a, &b| {
            let d = |i: usize| (presets[i].0 - v).abs();
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(0)
}

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

    #[test]
    fn every_animation_stays_bounded_at_4k() {
        // Small test grids once hid a draw loop that never ended at full-screen coordinates.
        let size = Size {
            cols: 480,
            rows: 135,
            width: 3840.0,
            height: 2160.0,
            focus: Rect {
                x: 0.0,
                y: 0.0,
                w: 3840.0,
                h: 2160.0,
            },
        };
        for &(name, ..) in ANIMATIONS {
            let mut a = by_name(name, " .:-=+*#%@");
            a.init(size, 9);
            let mut g = Grid::new(size.cols, size.rows);
            let mut shapes = Vec::new();
            for _ in 0..3 {
                a.update(7.3); // big steps reach late loop phases quickly
                a.render(&mut g);
                shapes.clear();
                a.draw(&mut shapes);
                assert!(shapes.len() < 100_000, "{name}: {} shapes", shapes.len());
            }
        }
    }

    #[test]
    fn centered_loops_follow_the_primary_monitor_and_full_bleed_fills_the_desktop() {
        // Two 1600x1000 monitors side by side, the primary on the right.
        let size = Size {
            cols: 400,
            rows: 70,
            width: 3200.0,
            height: 1000.0,
            focus: Rect {
                x: 1600.0,
                y: 0.0,
                w: 1600.0,
                h: 1000.0,
            },
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
                })
                .collect()
        };
        for &(name, _, group) in ANIMATIONS {
            if group != "loops" || name == "globe" {
                continue; // the globe is deliberately right of center
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
    fn nearest_picks_closest_preset() {
        assert_eq!(SPEEDS[nearest(SPEEDS, 1.0)].0, 1.0);
        assert_eq!(SPEEDS[nearest(SPEEDS, 0.6)].0, 0.5);
        assert_eq!(SPEEDS[nearest(SPEEDS, 9.0)].0, 2.0);
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
