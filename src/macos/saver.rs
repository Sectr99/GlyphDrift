//! The screen saver, `Glyphdrift.saver`, ships inside the app. Installing opens it, so macOS
//! asks where to install it and opens Screen Saver settings. Once it's installed for this
//! user, each launch replaces an older copy with the app's, so the saver keeps up with app
//! updates.

use std::path::{Path, PathBuf};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSString, NSURL};

const NAME: &str = "Glyphdrift.saver";
/// The saver's executable, inside the bundle.
const EXECUTABLE: &str = "Contents/MacOS/Glyphdrift";
const SETTINGS: &str = "x-apple.systempreferences:com.apple.ScreenSaver-Settings.extension";

/// The copy inside the app, if this is a bundled build.
fn bundled() -> Option<PathBuf> {
    let resources = NSBundle::mainBundle().resourcePath()?.to_string();
    Some(PathBuf::from(resources).join(NAME)).filter(|p| p.is_dir())
}

/// Where macOS installs it for this user.
fn installed() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(home.join("Library/Screen Savers").join(NAME))
}

/// True if the installed saver is the app's own copy.
fn current(bundled: &Path, installed: &Path) -> bool {
    match (
        std::fs::read(bundled.join(EXECUTABLE)),
        std::fs::read(installed.join(EXECUTABLE)),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// True if this build can install the saver (it's bundled).
pub fn available() -> bool {
    bundled().is_some()
}

/// True if the saver is installed for this user.
pub fn is_installed() -> bool {
    installed().is_some_and(|p| p.is_dir())
}

/// Install the saver, or if it's already current, open Screen Saver settings.
pub fn install() {
    let Some(src) = bundled() else {
        return;
    };
    let url = match installed() {
        Some(dst) if current(&src, &dst) => NSURL::URLWithString(&NSString::from_str(SETTINGS)),
        _ => Some(NSURL::fileURLWithPath(&NSString::from_str(
            &src.to_string_lossy(),
        ))),
    };
    if let Some(url) = url {
        if !NSWorkspace::sharedWorkspace().openURL(&url) {
            log!("screen saver: could not open {}", src.display());
        }
    }
}

/// Replace an installed saver that differs from the app's copy.
pub fn update() {
    let (Some(src), Some(dst)) = (bundled(), installed()) else {
        return;
    };
    if !dst.is_dir() || current(&src, &dst) {
        return;
    }
    // Copy beside it, then swap, so a failed copy leaves the old one working.
    let tmp = dst.with_extension("saver-new");
    let _ = std::fs::remove_dir_all(&tmp);
    let copied = std::process::Command::new("/usr/bin/ditto")
        .arg(&src)
        .arg(&tmp)
        .status()
        .is_ok_and(|s| s.success());
    if copied && std::fs::remove_dir_all(&dst).is_ok() && std::fs::rename(&tmp, &dst).is_ok() {
        log!("screen saver: updated {}", dst.display());
    } else {
        let _ = std::fs::remove_dir_all(&tmp);
        log!("screen saver: could not update {}", dst.display());
    }
}
