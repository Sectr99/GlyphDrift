//! Marching-squares contours of sampled fields, and smooth value noise.

use super::rng::hash3;

type P = (f32, f32);
/// A line segment between two points.
pub type Segment = (P, P);

/// Smallest sample spacing (px) for contour fields: finer costs more, looks no smoother.
pub const MIN_CELL: f32 = 8.0;

/// A scalar field sampled on an `nx` x `ny` grid, `cell` px apart.
pub struct Field {
    pub nx: usize,
    pub ny: usize,
    pub cell: f32,
    pub values: Vec<f32>,
}

impl Field {
    /// Sample `f(x, y)` (px) over `w` x `h`.
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

    /// Bilinear value at (x, y) px, clamped at the edges.
    pub fn value_at(&self, x: f32, y: f32) -> f32 {
        let fx = (x / self.cell).clamp(0.0, (self.nx - 1) as f32);
        let fy = (y / self.cell).clamp(0.0, (self.ny - 1) as f32);
        let (i, j) = (
            (fx as usize).min(self.nx.saturating_sub(2)),
            (fy as usize).min(self.ny.saturating_sub(2)),
        );
        let (u, v) = (fx - i as f32, fy - j as f32);
        let (i1, j1) = ((i + 1).min(self.nx - 1), (j + 1).min(self.ny - 1));
        let top = self.at(i, j) + (self.at(i1, j) - self.at(i, j)) * u;
        let bottom = self.at(i, j1) + (self.at(i1, j1) - self.at(i, j1)) * u;
        top + (bottom - top) * v
    }

    /// Call `emit(a, b)` for each segment of the iso-line at `level` (tests compare with
    /// `contours`).
    #[cfg(test)]
    pub fn contour(&self, level: f32, mut emit: impl FnMut(P, P)) {
        for j in 0..self.ny.saturating_sub(1) {
            for i in 0..self.nx.saturating_sub(1) {
                self.cell(i, j, level, &mut emit);
            }
        }
    }

    /// Call `emit(k, a, b)` for each segment of each ascending `levels[k]`, in one pass
    /// that visits only the levels crossing each cell.
    pub fn contours(&self, levels: &[f32], mut emit: impl FnMut(usize, P, P)) {
        for j in 0..self.ny.saturating_sub(1) {
            for i in 0..self.nx.saturating_sub(1) {
                let v = [
                    self.at(i, j),
                    self.at(i + 1, j),
                    self.at(i + 1, j + 1),
                    self.at(i, j + 1),
                ];
                let lo = v.iter().copied().fold(f32::INFINITY, f32::min);
                let hi = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                // Levels between the cell's lowest and highest corner.
                let first = levels.partition_point(|&l| l < lo);
                for (k, &level) in levels.iter().enumerate().skip(first) {
                    if level >= hi {
                        break;
                    }
                    self.cell(i, j, level, &mut |a, b| emit(k, a, b));
                }
            }
        }
    }

    /// The iso-lines of each ascending `levels[k]` as polylines for smooth curves: chained,
    /// then thinned (see `thin`).
    pub fn polylines_by_level(&self, levels: &[f32]) -> Vec<Vec<Vec<P>>> {
        let mut segs = vec![Vec::new(); levels.len()];
        self.contours(levels, |k, a, b| segs[k].push((a, b)));
        segs.iter()
            .map(|s| chain(s).into_iter().map(|l| thin(l, self.cell)).collect())
            .collect()
    }

    /// Marching squares for one cell; saddles resolved by the center value.
    fn cell(&self, i: usize, j: usize, level: f32, emit: &mut impl FnMut(P, P)) {
        let (tl, tr) = (self.at(i, j), self.at(i + 1, j));
        let (br, bl) = (self.at(i + 1, j + 1), self.at(i, j + 1));
        let case = (usize::from(tl > level) << 3)
            | (usize::from(tr > level) << 2)
            | (usize::from(br > level) << 1)
            | usize::from(bl > level);
        if case == 0 || case == 15 {
            return;
        }
        let (x, y, c) = (i as f32 * self.cell, j as f32 * self.cell, self.cell);
        // Far edges as (i + 1) * cell so neighbors get bit-identical points.
        let (x1, y1) = ((i + 1) as f32 * self.cell, (j + 1) as f32 * self.cell);
        // Where the level crosses the edge from a to b.
        let cross = |a: f32, b: f32| ((level - a) / (b - a)).clamp(0.0, 1.0);
        let top = (x + c * cross(tl, tr), y);
        let right = (x1, y + c * cross(tr, br));
        let bottom = (x + c * cross(bl, br), y1);
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

/// A cheap hasher for exact point keys (std's SipHash is several times slower here).
#[derive(Default)]
struct PointHasher(u64);

impl std::hash::Hasher for PointHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u32(b as u32);
        }
    }
    fn write_u32(&mut self, x: u32) {
        self.0 = (self.0.rotate_left(5) ^ x as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

/// Join segments that share endpoints into polylines; a closed loop ends at its first
/// point. Endpoints must match exactly, as `Field` produces them.
pub fn chain(segs: &[Segment]) -> Vec<Vec<P>> {
    use std::collections::HashMap;
    use std::hash::BuildHasherDefault;
    const NONE: usize = usize::MAX;
    let key = |p: P| (p.0.to_bits(), p.1.to_bits());
    // Each contour point joins at most two segments.
    let mut at: HashMap<(u32, u32), [usize; 2], BuildHasherDefault<PointHasher>> =
        HashMap::with_capacity_and_hasher(segs.len() * 2, Default::default());
    for (k, &(a, b)) in segs.iter().enumerate() {
        for p in [a, b] {
            let slot = at.entry(key(p)).or_insert([NONE; 2]);
            if slot[0] == NONE {
                slot[0] = k;
            } else {
                slot[1] = k;
            }
        }
    }
    let mut used = vec![false; segs.len()];
    // Extend `line` from its last point while an unused segment continues it.
    let extend = |line: &mut Vec<P>, used: &mut Vec<bool>| loop {
        let end = *line.last().unwrap();
        let Some(&k) = at
            .get(&key(end))
            .and_then(|s| s.iter().find(|&&k| k != NONE && !used[k]))
        else {
            break;
        };
        used[k] = true;
        let (a, b) = segs[k];
        line.push(if key(a) == key(end) { b } else { a });
    };
    let mut out = Vec::new();
    for k in 0..segs.len() {
        if used[k] {
            continue;
        }
        used[k] = true;
        let mut line = vec![segs[k].0, segs[k].1];
        extend(&mut line, &mut used);
        line.reverse();
        extend(&mut line, &mut used);
        out.push(line);
    }
    out
}

/// About every other point of a contour polyline (grid spacing `cell`), for half the
/// drawing cost; a smooth curve through the rest still follows it. Which points stay
/// depends only on where they sit on the grid, never on where the chain starts, so the
/// same points survive from frame to frame and the curve doesn't wobble. Ends are kept
/// (closed loops stay closed), never two points in a row are dropped, short lines keep
/// everything, and near-duplicate points (a contour grazing a grid corner) are merged.
pub fn thin(line: Vec<P>, cell: f32) -> Vec<P> {
    let n = line.len();
    let checkerboard =
        |p: P| ((p.0 / cell).floor() as i64 + (p.1 / cell).floor() as i64).rem_euclid(2) == 0;
    let near = cell * 0.15;
    let mut out: Vec<P> = Vec::with_capacity(n / 2 + 2);
    let mut dropped = false;
    for (k, &p) in line.iter().enumerate() {
        let end = k == 0 || k == n - 1;
        if let Some(&q) = out.last() {
            if !end && (p.0 - q.0).abs() < near && (p.1 - q.1).abs() < near {
                continue;
            }
        }
        if end || n < 10 || dropped || checkerboard(p) {
            out.push(p);
            dropped = false;
        } else {
            dropped = true;
        }
    }
    out
}

/// Smooth 3D value noise in -1..1.
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
    fn a_circle_chains_into_one_closed_polyline() {
        let f = Field::sample(100.0, 100.0, 5.0, |x, y| {
            ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt()
        });
        let mut segs = Vec::new();
        // 20.3, not 20: a level exactly at sample points makes ambiguous corners.
        f.contour(20.3, |a, b| segs.push((a, b)));
        let lines = chain(&segs);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), segs.len() + 1);
        assert_eq!(lines[0].first(), lines[0].last(), "closed");
        let thinned = thin(lines[0].clone(), 5.0);
        assert!(thinned.len() < lines[0].len() * 2 / 3, "{}", thinned.len());
        assert_eq!(thinned.first(), thinned.last(), "still closed");
        // The same points survive wherever the chain starts.
        let mut rotated = lines[0][7..].to_vec();
        rotated.extend_from_slice(&lines[0][1..=7]);
        let interior = |l: &[P]| -> Vec<(u32, u32)> {
            let mut v: Vec<_> = l[1..l.len() - 1]
                .iter()
                .map(|p| (p.0.to_bits(), p.1.to_bits()))
                .collect();
            v.sort();
            v
        };
        let (a, b) = (interior(&thinned), interior(&thin(rotated, 5.0)));
        let common = a.iter().filter(|p| b.binary_search(p).is_ok()).count();
        assert!(
            common + 4 >= a.len(),
            "{common} of {} kept points shared",
            a.len()
        );
    }

    #[test]
    fn value_at_interpolates_between_samples() {
        let f = Field::sample(100.0, 100.0, 10.0, |x, y| x + 2.0 * y);
        for (x, y) in [(0.0, 0.0), (15.0, 25.0), (99.0, 3.0), (100.0, 100.0)] {
            assert!(
                (f.value_at(x, y) - (x + 2.0 * y)).abs() < 1e-3,
                "({x}, {y})"
            );
        }
        assert_eq!(f.value_at(-50.0, 0.0), 0.0); // clamped
    }

    #[test]
    fn contours_match_contour_per_level() {
        let f = Field::sample(120.0, 90.0, 6.0, |x, y| fbm3(x / 40.0, y / 40.0, 0.3));
        let levels = [-0.5, -0.2, 0.0, 0.1, 0.4];
        let mut multi = vec![Vec::new(); levels.len()];
        f.contours(&levels, |k, a, b| multi[k].push((a, b)));
        for (k, &level) in levels.iter().enumerate() {
            let mut single = Vec::new();
            f.contour(level, |a, b| single.push((a, b)));
            assert_eq!(single, multi[k], "level {level}");
        }
    }

    #[test]
    fn contour_of_a_disc_is_closed_and_on_the_circle() {
        // Distance from (50, 50): the level-20 contour is a circle.
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
