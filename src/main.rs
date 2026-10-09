#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(e) = ascii_wallpaper::app::run() {
        // Release builds have no console; surface the failure in a dialog.
        ascii_wallpaper::app::fatal(&format!("{e}"));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "ascii-wallpaper only runs on Windows. Run `cargo test` to exercise the portable core."
    );
    std::process::exit(1);
}
