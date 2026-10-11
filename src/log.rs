//! Append-only log at `%APPDATA%\WallMotion\wallmotion.log`. Release builds have no
//! console, so warnings and errors land here (and on stderr, for debug builds).

use std::io::Write;
use std::path::PathBuf;

use windows::Win32::System::SystemInformation::GetLocalTime;

/// Past this size at startup the log moves to `wallmotion.log.old` and starts fresh.
const MAX_BYTES: u64 = 256 * 1024;

fn path() -> Option<PathBuf> {
    Some(crate::config::default_path()?.with_file_name("wallmotion.log"))
}

/// Start a session: rotate an oversized log and record panics, which abort in release
/// builds, before the process ends.
pub fn init() {
    if let Some(p) = path() {
        if std::fs::metadata(&p).is_ok_and(|m| m.len() > MAX_BYTES) {
            let _ = std::fs::rename(&p, p.with_extension("log.old"));
        }
    }
    std::panic::set_hook(Box::new(|info| write(&format!("panic: {info}"))));
    write(&format!("WallMotion {} started", env!("CARGO_PKG_VERSION")));
}

/// Append one timestamped line.
pub fn write(msg: &str) {
    eprintln!("{msg}");
    let Some(p) = path() else {
        return;
    };
    // SAFETY: no preconditions.
    let t = unsafe { GetLocalTime() };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {msg}\n",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
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
