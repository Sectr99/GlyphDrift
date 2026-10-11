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
    /// Smooth curve through `points` (Catmull-Rom); closed if the last point is the first.
    Curve {
        points: Vec<(f32, f32)>,
        width: f32,
        level: u8,
    },
    /// Text centered on `c`; `size` is the em height in px, `weight` 300 light or 400.
    Text {
        c: (f32, f32),
        size: f32,
        weight: u16,
        text: String,
        level: u8,
    },
}

type P = (f32, f32);

/// Cubic Bezier segments (control 1, control 2, end) for a centripetal Catmull-Rom curve
/// through `points`, starting at `points[0]`, and whether it's closed (last point = first).
/// Centripetal spacing never forms cusps or loops, even with unevenly spaced points.
pub fn catmull_rom(points: &[P]) -> (Vec<[P; 3]>, bool) {
    let n = points.len();
    let closed = n > 3 && points[0] == points[n - 1];
    let at = |i: isize| -> P {
        if closed {
            // Skip the repeated last point when wrapping.
            points[i.rem_euclid(n as isize - 1) as usize]
        } else {
            points[i.clamp(0, n as isize - 1) as usize]
        }
    };
    // Square root of the distance: the centripetal knot spacing.
    let knot = |a: P, b: P| (b.0 - a.0).hypot(b.1 - a.1).sqrt().max(1e-4);
    let segments = (0..n.saturating_sub(1) as isize)
        .map(|i| {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let (d1, d2, d3) = (knot(p0, p1), knot(p1, p2), knot(p2, p3));
            // Tangents at p1 and p2, scaled to this segment's knot interval.
            let tangent = |a: P, b: P, c: P, dab: f32, dbc: f32| -> P {
                let f = |ua: f32, ub: f32, uc: f32| {
                    ((ub - ua) / dab - (uc - ua) / (dab + dbc) + (uc - ub) / dbc) * d2
                };
                (f(a.0, b.0, c.0), f(a.1, b.1, c.1))
            };
            let (m1, m2) = (tangent(p0, p1, p2, d1, d2), tangent(p1, p2, p3, d2, d3));
            [
                (p1.0 + m1.0 / 3.0, p1.1 + m1.1 / 3.0),
                (p2.0 - m2.0 / 3.0, p2.1 - m2.1 / 3.0),
                p2,
            ]
        })
        .collect();
    (segments, closed)
}

#[cfg(test)]
mod tests {
    use super::catmull_rom;

    #[test]
    fn catmull_rom_passes_through_the_points() {
        let pts = [(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 5.0)];
        let (segs, closed) = catmull_rom(&pts);
        assert!(!closed);
        assert_eq!(segs.len(), 3);
        for (seg, p) in segs.iter().zip(&pts[1..]) {
            assert_eq!(seg[2], *p);
        }
        // A straight line stays straight: controls lie on it.
        let (line, _) = catmull_rom(&[(0.0, 0.0), (5.0, 5.0), (10.0, 10.0)]);
        assert!(line
            .iter()
            .all(|s| s.iter().all(|c| (c.0 - c.1).abs() < 1e-5)));
        let square = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)];
        assert!(catmull_rom(&square).1);
        // Two nearly coincident points don't make a cusp: controls stay near the path.
        let (segs, _) = catmull_rom(&[(0.0, 0.0), (10.0, 0.0), (10.01, 0.01), (20.0, 0.0)]);
        for s in &segs {
            for c in &s[..2] {
                assert!(c.1.abs() < 1.0, "control {c:?} strays off the line");
            }
        }
    }
}
