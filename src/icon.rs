//! The app icon from `assets/icon.svg`: a filled square with eight rectangular holes.
//!
//! Dependency-free so `build.rs` can include it (`#[path]`) to bake the exe icon, while the
//! tray rasterizes the same shape at runtime.

/// Holes cut from the filled 2..22 square, in the SVG's 24x24 viewBox (x0, y0, x1, y1).
const HOLES: [[f32; 4]; 8] = [
    [4.0, 16.0, 8.0, 20.0],
    [10.0, 16.0, 16.0, 20.0],
    [18.0, 16.0, 20.0, 20.0],
    [13.0, 10.0, 20.0, 14.0],
    [4.0, 10.0, 11.0, 14.0],
    [4.0, 4.0, 6.0, 8.0],
    [8.0, 4.0, 14.0, 8.0],
    [16.0, 4.0, 20.0, 8.0],
];

/// Fraction of pixel (px, py) of an `n` x `n` icon covered by the shape (4x4 supersampling).
pub fn coverage(px: usize, py: usize, n: usize) -> f32 {
    const S: usize = 4;
    let inside = |x: f32, y: f32| {
        (2.0..22.0).contains(&x)
            && (2.0..22.0).contains(&y)
            && !HOLES
                .iter()
                .any(|h| x >= h[0] && x < h[2] && y >= h[1] && y < h[3])
    };
    let to_view = |p: usize, s: usize| (p as f32 + (s as f32 + 0.5) / S as f32) * 24.0 / n as f32;
    let hits = (0..S * S)
        .filter(|i| inside(to_view(px, i % S), to_view(py, i / S)))
        .count();
    hits as f32 / (S * S) as f32
}

/// `n` x `n` icon in color `rgb` as top-down BGRA rows, alpha from coverage.
pub fn bgra(n: usize, rgb: (u8, u8, u8)) -> Vec<u8> {
    let mut out = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let a = (coverage(x, y, n) * 255.0).round() as u8;
            out.extend([rgb.2, rgb.1, rgb.0, a]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_matches_svg() {
        // At 24 px one pixel is one viewBox unit.
        assert_eq!(coverage(0, 0, 24), 0.0); // outside the frame
        assert_eq!(coverage(2, 2, 24), 1.0); // frame corner
        assert_eq!(coverage(5, 5, 24), 0.0); // hole [4,4]-[6,8]
        assert_eq!(coverage(12, 14, 24), 1.0); // bar between hole rows
        let partial = coverage(1, 1, 16); // edge straddles the pixel at 16 px
        assert!(partial > 0.0 && partial < 1.0);
        assert_eq!(bgra(16, (1, 2, 3)).len(), 16 * 16 * 4);
    }
}
