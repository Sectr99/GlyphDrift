//! Iso-contours of sampled scalar fields (marching squares) and smooth value noise, shared
//! by the fluid animations.

use super::rng::hash3;

type P = (f32, f32);

/// A scalar field sampled on a regular grid of `nx` x `ny` vertices, `cell` px apart.
pub struct Field {
    pub nx: usize,
    pub ny: usize,
    pub cell: f32,
    pub values: Vec<f32>,
}

impl Field {
    /// Sample `f(x, y)` (screen px) over a `w` x `h` area.
    pub fn sample(w: f32, h: f32, cell: f32, f: impl Fn(f32, f32) -> f32) -> Self {
        let (nx, ny) = (
            (w / cell).ceil() as usize + 1,
            (h / cell).ceil() as usize + 1,
        );
        let mut values = Vec::with_capacity(nx * ny);
        for j in 0..ny {
            for i in 0..nx {
                values.push(f(i as f32 * cell, j as f32 * cell));
            }
        }
        Self {
            nx,
            ny,
            cell,
            values,
        }
    }

    fn at(&self, i: usize, j: usize) -> f32 {
        self.values[j * self.nx + i]
    }

    /// Call `emit(a, b)` for every segment of the iso-line where the field equals `level`.
    /// Saddle cells are resolved by the cell's center value.
    pub fn contour(&self, level: f32, mut emit: impl FnMut(P, P)) {
        for j in 0..self.ny.saturating_sub(1) {
            for i in 0..self.nx.saturating_sub(1) {
                let (tl, tr) = (self.at(i, j), self.at(i + 1, j));
                let (br, bl) = (self.at(i + 1, j + 1), self.at(i, j + 1));
                let case = (usize::from(tl > level) << 3)
                    | (usize::from(tr > level) << 2)
                    | (usize::from(br > level) << 1)
                    | usize::from(bl > level);
                if case == 0 || case == 15 {
                    continue;
                }
                let (x, y, c) = (i as f32 * self.cell, j as f32 * self.cell, self.cell);
                // Where the level crosses an edge between values a and b.
                let cross = |a: f32, b: f32| ((level - a) / (b - a)).clamp(0.0, 1.0);
                let top = (x + c * cross(tl, tr), y);
                let right = (x + c, y + c * cross(tr, br));
                let bottom = (x + c * cross(bl, br), y + c);
                let left = (x, y + c * cross(tl, bl));
                let center_above = (tl + tr + br + bl) / 4.0 > level;
                match case {
                    1 | 14 => emit(left, bottom),
                    2 | 13 => emit(bottom, right),
                    3 | 12 => emit(left, right),
                    4 | 11 => emit(top, right),
                    6 | 9 => emit(top, bottom),
                    7 | 8 => emit(top, left),
                    5 => {
                        // tr and bl above.
                        if center_above {
                            emit(top, left);
                            emit(bottom, right);
                        } else {
                            emit(top, right);
                            emit(left, bottom);
                        }
                    }
                    10 => {
                        // tl and br above.
                        if center_above {
                            emit(top, right);
                            emit(left, bottom);
                        } else {
                            emit(top, left);
                            emit(bottom, right);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Smooth 3D value noise in -1..1: hashed lattice values, smoothstep-interpolated.
pub fn noise3(x: f32, y: f32, z: f32) -> f32 {
    let (xf, yf, zf) = (x.floor(), y.floor(), z.floor());
    let (xi, yi, zi) = (xf as i32, yf as i32, zf as i32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v, w) = (s(x - xf), s(y - yf), s(z - zf));
    let corner = |dx: i32, dy: i32, dz: i32| {
        let h = hash3((xi + dx) as u32, (yi + dy) as u32, (zi + dz) as u32);
        (h & 0xFFFF) as f32 / 32767.5 - 1.0
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let plane = |dz: i32| {
        lerp(
            lerp(corner(0, 0, dz), corner(1, 0, dz), u),
            lerp(corner(0, 1, dz), corner(1, 1, dz), u),
            v,
        )
    };
    lerp(plane(0), plane(1), w)
}

/// Three octaves of `noise3`, roughly -1..1.
pub fn fbm3(x: f32, y: f32, z: f32) -> f32 {
    (noise3(x, y, z)
        + 0.5 * noise3(2.0 * x + 17.3, 2.0 * y, z * 1.3)
        + 0.25 * noise3(4.0 * x, 4.0 * y + 31.7, z * 1.7))
        / 1.75
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contour_of_a_disc_is_closed_and_on_the_circle() {
        // Field = distance from (50, 50); the level-20 contour is a circle of radius 20.
        let f = Field::sample(100.0, 100.0, 5.0, |x, y| {
            ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt()
        });
        let mut segs = Vec::new();
        f.contour(20.0, |a, b| segs.push((a, b)));
        assert!(segs.len() > 20);
        for (a, _) in &segs {
            let r = ((a.0 - 50.0).powi(2) + (a.1 - 50.0).powi(2)).sqrt();
            assert!((r - 20.0).abs() < 1.0, "point off the circle: r = {r}");
        }
    }

    #[test]
    fn noise_is_smooth_and_bounded() {
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.137, i as f32 * 0.071);
            let n = noise3(x, y, 0.5);
            assert!((-1.0..=1.0).contains(&n));
            assert!(
                (noise3(x + 0.001, y, 0.5) - n).abs() < 0.01,
                "not continuous at {x}"
            );
        }
    }
}
