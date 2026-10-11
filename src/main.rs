#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(e) = glyphdrift::app::run() {
        // Release builds have no console; surface the failure in a dialog.
        glyphdrift::app::fatal(&format!("{e}"));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Glyphdrift only runs on Windows. Run `cargo test` to exercise the portable core.");
    std::process::exit(1);
}
