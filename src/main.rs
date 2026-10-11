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
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--screenshots") {
        // Website images, drawn offscreen; the running wallpaper is left alone.
        let result = match args.get(2) {
            Some(dir) => glyphdrift::macos::screenshots(dir.as_ref()),
            None => Err("usage: glyphdrift --screenshots <dir>".into()),
        };
        if let Err(e) = result {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
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
