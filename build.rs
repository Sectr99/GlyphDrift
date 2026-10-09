// Embed app.manifest and the exe icon with the MSVC linker; no build dependencies needed.
// The icon is rasterized from src/icon.rs and written straight into a .res file, so no
// resource compiler (rc.exe) is required either.

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

/// Exe icon color: the default theme's green reads on light and dark Explorer backgrounds.
const EXE_ICON_RGB: (u8, u8, u8) = (0x00, 0xff, 0x66);
const SIZES: [usize; 6] = [16, 24, 32, 48, 64, 256];
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=src/icon.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{dir}\\app.manifest");

    let res = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("icon.res");
    std::fs::write(&res, icon_res()).unwrap();
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// A .res file holding one RT_ICON per size plus the RT_GROUP_ICON (id 1) that lists them.
fn icon_res() -> Vec<u8> {
    let mut out = Vec::new();
    res_entry(&mut out, 0, 0, &[]); // a .res file starts with an empty entry
    let mut group = Vec::new();
    for v in [0u16, 1, SIZES.len() as u16] {
        group.extend(v.to_le_bytes()); // reserved, type = icon, count
    }
    for (i, &n) in SIZES.iter().enumerate() {
        let id = i as u16 + 1;
        let image = dib(n);
        group.extend([n as u8, n as u8, 0, 0]); // width, height (256 -> 0), colors, reserved
        group.extend(1u16.to_le_bytes()); // planes
        group.extend(32u16.to_le_bytes()); // bits per pixel
        group.extend((image.len() as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
        res_entry(&mut out, RT_ICON, id, &image);
    }
    res_entry(&mut out, RT_GROUP_ICON, 1, &group);
    out
}

/// One resource: 32-byte header with numeric type and name, then data padded to 4 bytes.
fn res_entry(out: &mut Vec<u8>, ty: u16, id: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes());
    for v in [0xFFFF, ty, 0xFFFF, id] {
        out.extend(v.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes()); // data version
    let (flags, lang): (u16, u16) = if ty == 0 { (0, 0) } else { (0x1010, 0x0409) };
    out.extend(flags.to_le_bytes()); // moveable | discardable
    out.extend(lang.to_le_bytes()); // en-US
    out.extend([0u8; 8]); // version, characteristics
    out.extend(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

/// Icon image as a DIB: BITMAPINFOHEADER (double height), bottom-up BGRA, zero AND mask.
fn dib(n: usize) -> Vec<u8> {
    let mask_row = n.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40u32, n as u32, 2 * n as u32] {
        out.extend(v.to_le_bytes()); // size, width, height (XOR + AND)
    }
    out.extend(1u16.to_le_bytes()); // planes
    out.extend(32u16.to_le_bytes()); // bits per pixel
    out.extend(0u32.to_le_bytes()); // BI_RGB
    out.extend(((n * n * 4 + mask_row * n) as u32).to_le_bytes());
    out.extend([0u8; 16]); // resolution, palette
    let pixels = icon::bgra(n, EXE_ICON_RGB);
    for row in pixels.chunks(n * 4).rev() {
        out.extend(row);
    }
    out.resize(out.len() + mask_row * n, 0);
    out
}
