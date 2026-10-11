#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(e) = glyphdrift::app::run() {
        // No console in release builds: show a dialog.
        glyphdrift::app::fatal(&format!("{e}"));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Glyphdrift only runs on Windows. Run `cargo test` to exercise the portable core.");
    std::process::exit(1);
}
