//! Animation trait and built-in animations.

mod fire;
mod matrix;
mod plasma;
mod ripples;
mod rng;
mod waves;
mod wind;

pub use fire::Fire;
pub use matrix::Matrix;
pub use plasma::Plasma;
pub use ripples::Ripples;
pub use rng::Rng;
pub use waves::Waves;
pub use wind::Wind;

use crate::grid::Grid;

pub trait Animation {
    fn name(&self) -> &'static str;
    /// Use the theme's warm gradient instead of the cool one.
    fn warm(&self) -> bool {
        false
    }
    /// Called once before the first `update`, and again on resize.
    fn init(&mut self, cols: usize, rows: usize, seed: u64);
    /// Advance simulation by `dt` seconds.
    fn update(&mut self, dt: f32);
    /// Write the current frame into `grid` (which is `cols` x `rows` from `init`).
    fn render(&self, grid: &mut Grid);
}

/// (config name, menu label)
pub const ANIMATIONS: &[(&str, &str)] = &[
    ("matrix", "Matrix rain"),
    ("plasma", "Plasma"),
    ("fire", "Fire"),
    ("waves", "Waves"),
    ("ripples", "Ripples"),
    ("wind", "Wind"),
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
        _ => Box::new(Matrix::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(name: &str, seed: u64) -> Grid {
        let mut a = by_name(name, " .:-=+*#%@");
        assert_eq!(a.name(), name);
        a.init(60, 30, seed);
        let mut g = Grid::new(60, 30);
        for _ in 0..48 {
            a.update(1.0 / 24.0);
        }
        a.render(&mut g);
        g
    }

    #[test]
    fn every_animation_draws_deterministically() {
        for &(name, _) in ANIMATIONS {
            let g = frame(name, 42);
            assert_eq!(
                g.to_text(),
                frame(name, 42).to_text(),
                "{name} not deterministic"
            );
            let lit = (0..g.rows())
                .flat_map(|y| g.row(y).iter())
                .filter(|c| c.brightness > 0 && c.ch != ' ')
                .count();
            assert!(lit > 0, "{name} drew nothing");
        }
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
