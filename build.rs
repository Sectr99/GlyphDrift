// Embed the manifest, exe icon and version info via the MSVC linker. The icon (from
// src/icon.rs) and version info are written straight into a .res file: no rc.exe needed.

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

/// Exe icon color: reads on light and dark backgrounds.
const EXE_ICON_RGB: (u8, u8, u8) = (0x00, 0xff, 0x66);
const SIZES: [usize; 6] = [16, 24, 32, 48, 64, 256];
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;

/// App name and publisher as Windows shows them; versions come from Cargo.toml.
const PRODUCT: &str = "Glyphdrift";
const PUBLISHER: &str = "Sectr99";
const COPYRIGHT: &str = "© 2026 Sectr99";

fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{dir}\\app.manifest");
    // Load imported DLLs (d2d1, dwrite, ...) from System32 only, never from the exe's own
    // folder: a portable exe often runs from Downloads, where a planted DLL could wait.
    // LOAD_LIBRARY_SEARCH_SYSTEM32; honored from Windows 10 1607.
    println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");

    let res = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("resources.res");
    let mut data = icon_res();
    res_entry(&mut data, RT_VERSION, 1, &version_info());
    std::fs::write(&res, data).unwrap();
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// A .res file: one RT_ICON per size plus the RT_GROUP_ICON (id 1) listing them.
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

/// One resource: 32-byte header (numeric type and name), data padded to 4 bytes.
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

/// VS_VERSIONINFO: fixed versions plus the strings Windows shows.
fn version_info() -> Vec<u8> {
    let part = |name: &str| -> u32 {
        std::env::var(format!("CARGO_PKG_VERSION_{name}"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    let (major, minor, patch) = (part("MAJOR"), part("MINOR"), part("PATCH"));
    let version = format!("{major}.{minor}.{patch}");

    let mut fixed = Vec::new();
    for v in [
        0xFEEF_04BD,         // signature
        0x0001_0000,         // structure version
        major << 16 | minor, // file version
        patch << 16,
        major << 16 | minor, // product version
        patch << 16,
        0x3F,        // flags mask
        0,           // flags
        0x0004_0004, // VOS_NT_WINDOWS32
        1,           // VFT_APP
        0,           // subtype
        0,           // date
        0,
    ] {
        fixed.extend(u32::to_le_bytes(v));
    }

    let exe = format!("{}.exe", PRODUCT.to_lowercase());
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", PUBLISHER),
        ("FileDescription", PRODUCT),
        ("FileVersion", &version),
        ("InternalName", &PRODUCT.to_lowercase()),
        ("LegalCopyright", COPYRIGHT),
        ("OriginalFilename", &exe),
        ("ProductName", PRODUCT),
        ("ProductVersion", &version),
    ]
    .iter()
    .map(|(key, value)| {
        let text = utf16z(value);
        version_node(key, 1, &text, (text.len() / 2) as u16, &[])
    })
    .collect();
    // en-US (0x0409), Unicode (0x04B0).
    let table = version_node("040904B0", 1, &[], 0, &strings);
    let string_info = version_node("StringFileInfo", 1, &[], 0, &[table]);
    let translation = [0x0409u16, 0x04B0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    let var = version_node("Translation", 0, &translation, 4, &[]);
    let var_info = version_node("VarFileInfo", 1, &[], 0, &[var]);
    version_node(
        "VS_VERSION_INFO",
        0,
        &fixed,
        fixed.len() as u16,
        &[string_info, var_info],
    )
}

/// A version-info block: length, value length (bytes, or chars for text), type (1 = text),
/// UTF-16 key, then value and children, each 4-byte aligned.
fn version_node(key: &str, ty: u16, value: &[u8], value_len: u16, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0u8; 6];
    out.extend(utf16z(key));
    out.resize(out.len().next_multiple_of(4), 0);
    out.extend(value);
    for child in children {
        out.resize(out.len().next_multiple_of(4), 0);
        out.extend(child);
    }
    let len = out.len() as u16;
    out[0..2].copy_from_slice(&len.to_le_bytes());
    out[2..4].copy_from_slice(&value_len.to_le_bytes());
    out[4..6].copy_from_slice(&ty.to_le_bytes());
    out
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}
