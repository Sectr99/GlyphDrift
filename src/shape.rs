//! Vector shapes drawn over the character grid, in window px. `level` is a brightness
//! mapped through the theme palette like grid cells.

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
        /// Filled, or a 1 px outline.
        fill: bool,
        level: u8,
    },
    /// Filled polygon, closed automatically; 3+ points.
    Polygon { points: Vec<(f32, f32)>, level: u8 },
    /// Text centered on `c`; `size` is the em height in px, `weight` 300 light or 400.
    Text {
        c: (f32, f32),
        size: f32,
        weight: u16,
        text: String,
        level: u8,
    },
}
