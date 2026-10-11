//! Writes the macOS app icon as an `.iconset` folder for `iconutil`: the icon shape from
//! `src/icon.rs` in green on a dark rounded square. Used by `macos/bundle.sh`.
//! `cargo run --example macos_iconset -- <dir>.iconset`

use std::io::Write;

/// Icon green, as on Windows.
const FG: [f32; 3] = [0.0, 255.0, 102.0];
const BG: [f32; 3] = [16.0, 20.0, 18.0];

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: macos_iconset <dir>.iconset");
    std::fs::create_dir_all(&dir).unwrap();
    for (n, name) in [
        (16, "icon_16x16"),
        (32, "icon_16x16@2x"),
        (32, "icon_32x32"),
        (64, "icon_32x32@2x"),
        (128, "icon_128x128"),
        (256, "icon_128x128@2x"),
        (256, "icon_256x256"),
        (512, "icon_256x256@2x"),
        (512, "icon_512x512"),
        (1024, "icon_512x512@2x"),
    ] {
        let png = png(n, &rgba(n));
        std::fs::write(format!("{dir}/{name}.png"), png).unwrap();
    }
}

/// The macOS icon grid: a rounded square 80% of the canvas, the glyph 60% of that.
fn rgba(n: usize) -> Vec<u8> {
    let s = n as f32;
    let (inset, side) = (s * 0.1, s * 0.8);
    let radius = side * 0.225;
    let glyph = (side * 0.6).round() as usize;
    let g0 = ((s - glyph as f32) / 2.0).round() as usize;
    let mut out = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            // Rounded-square coverage, 4x4 supersampled.
            let mut hits = 0;
            for k in 0..16 {
                let px = x as f32 + (k % 4) as f32 / 4.0 + 0.125 - inset;
                let py = y as f32 + (k / 4) as f32 / 4.0 + 0.125 - inset;
                let cx = px.clamp(radius, side - radius);
                let cy = py.clamp(radius, side - radius);
                let inside = (0.0..side).contains(&px)
                    && (0.0..side).contains(&py)
                    && (px - cx).hypot(py - cy) <= radius;
                hits += usize::from(inside);
            }
            let alpha = hits as f32 / 16.0;
            let fg = if (g0..g0 + glyph).contains(&x) && (g0..g0 + glyph).contains(&y) {
                glyphdrift::icon::coverage(x - g0, y - g0, glyph)
            } else {
                0.0
            };
            let c = |i: usize| (BG[i] + (FG[i] - BG[i]) * fg).round() as u8;
            out.extend([c(0), c(1), c(2), (alpha * 255.0).round() as u8]);
        }
    }
    out
}

/// A minimal PNG: 8-bit RGBA, stored (uncompressed) deflate blocks.
fn png(n: usize, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((n * 4 + 1) * n);
    for row in rgba.chunks(n * 4) {
        raw.push(0); // no filter
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i == blocks.len() - 1));
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend((b << 16 | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend((n as u32).to_be_bytes());
    ihdr.extend((n as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8-bit RGBA
    for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        out.write_all(&(data.len() as u32).to_be_bytes()).unwrap();
        let mut chunk = kind.to_vec();
        chunk.extend(&data);
        out.extend(&chunk);
        out.extend(crc32(&chunk).to_be_bytes());
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb88320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
