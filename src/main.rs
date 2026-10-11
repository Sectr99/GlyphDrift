#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(e) = glyphdrift::app::run() {
        // No console in release builds: show a dialog.
        glyphdrift::app::fatal(&format!("{e}"));
    }
}

#[cfg(target_os = "macos")]
fn main() {
    if let Err(e) = glyphdrift::macos::run() {
        glyphdrift::macos::fatal(&e);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!(
        "Glyphdrift runs on Windows and macOS. Run `cargo test` to exercise the portable core."
    );
    std::process::exit(1);
}
