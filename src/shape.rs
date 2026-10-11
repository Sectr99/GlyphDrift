//! Vector shapes that animations can emit alongside (or instead of) the character grid.
//!
//! Coordinates are pixels in the wallpaper window. `level` is a brightness 0..=255 mapped
//! through the same theme palette as grid cells, so themes and glass apply unchanged.

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Line {
        a: (f32, f32),
        b: (f32, f32),
        width: f32,
        level: u8,
    },
    Circle {
        c: (f32, f32),
        r: f32,
        /// Filled disc if true, otherwise a 1 px outline.
        fill: bool,
        level: u8,
    },
    /// Filled polygon, closed automatically. Needs at least 3 points.
    Polygon { points: Vec<(f32, f32)>, level: u8 },
    /// One line of text centered on `c`. `size` is the em height in px; `weight` is a
    /// font weight (300 light, 400 regular).
    Text {
        c: (f32, f32),
        size: f32,
        weight: u16,
        text: String,
        level: u8,
    },
}
