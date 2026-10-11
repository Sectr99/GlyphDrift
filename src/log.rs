//! Append-only log next to the config: `%APPDATA%\Glyphdrift\glyphdrift.log`, or
//! `~/Library/Application Support/Glyphdrift/glyphdrift.log` (release builds have no console).

use std::io::Write;
use std::path::PathBuf;

#[cfg(windows)]
use windows::Win32::System::SystemInformation::GetLocalTime;

/// Past this size at startup, the log moves to `glyphdrift.log.old`.
const MAX_BYTES: u64 = 256 * 1024;

fn path() -> Option<PathBuf> {
    Some(crate::config::default_path()?.with_file_name("glyphdrift.log"))
}

/// Rotate an oversized log and record panics (which abort in release builds).
pub fn init() {
    if let Some(p) = path() {
        if std::fs::metadata(&p).is_ok_and(|m| m.len() > MAX_BYTES) {
            let _ = std::fs::rename(&p, p.with_extension("log.old"));
        }
    }
    std::panic::set_hook(Box::new(|info| write(&format!("panic: {info}"))));
    write(&format!("Glyphdrift {} started", env!("CARGO_PKG_VERSION")));
}

/// Append one timestamped line.
pub fn write(msg: &str) {
    eprintln!("{msg}");
    let Some(p) = path() else {
        return;
    };
    let line = format!("{} {msg}\n", timestamp());
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Local time as `YYYY-MM-DD hh:mm:ss`.
#[cfg(windows)]
fn timestamp() -> String {
    // SAFETY: no preconditions.
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

#[cfg(target_os = "macos")]
fn timestamp() -> String {
    crate::macos::timestamp()
}
