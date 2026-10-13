//! Screen saver (Windows): a copy of this exe named `Glyphdrift.scr`. Windows runs it with
//! `/s` (full screen), `/p <window>` (the preview in Screen Saver Settings) or `/c`
//! (its Settings button); with "On resume, display logon screen" ticked, the animation
//! leads straight to the lock screen.

use std::path::PathBuf;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// How this process runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Wallpaper,
    /// Full screen until the mouse or keyboard is used.
    Saver,
    /// Inside the given window (the Screen Saver Settings preview).
    Preview(HWND),
    /// The screen saver's Settings button.
    Configure,
}

/// The mode for these arguments (without the program name). `scr` is true when running as
/// the `.scr`, which Windows starts without arguments for its Settings button.
pub fn mode(args: &[String], scr: bool) -> Mode {
    let Some(first) = args.first() else {
        return if scr {
            Mode::Configure
        } else {
            Mode::Wallpaper
        };
    };
    // "/s", "-S", "/p 1234", "/p:1234", "/c:1234"
    let flag = first.trim_start_matches(['/', '-']).to_ascii_lowercase();
    let (key, rest) = flag.split_once(':').unwrap_or((&flag, ""));
    match key {
        "s" => Mode::Saver,
        "c" => Mode::Configure,
        "p" => {
            let n = if rest.is_empty() {
                args.get(1).map(String::as_str).unwrap_or("")
            } else {
                rest
            };
            match n.parse::<usize>() {
                Ok(n) if n != 0 => Mode::Preview(HWND(n as *mut _)),
                _ => Mode::Configure,
            }
        }
        _ => Mode::Wallpaper,
    }
}

fn installed_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("Glyphdrift")
            .join("Glyphdrift.scr"),
    )
}

/// Copy this exe to the `.scr`. A plain read and write: the copy is of the program already
/// running, so the download mark on the original isn't carried over to trigger a warning
/// each time the screen saver starts.
fn copy_to(scr: &PathBuf, exe: &[u8]) -> Result<(), String> {
    if let Some(dir) = scr.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(scr, exe).map_err(|e| format!("{}: {e}", scr.display()))
}

/// Install the screen saver and open Screen Saver Settings with it selected.
pub fn install() -> Result<(), String> {
    let scr = installed_path().ok_or("no LOCALAPPDATA folder")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    copy_to(&scr, &std::fs::read(&exe).map_err(|e| e.to_string())?)?;
    // SAFETY: constant verb, NUL-terminated path; no window needed.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("install"),
            &HSTRING::from(scr.as_os_str()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!(
            "opening Screen Saver Settings failed ({})",
            result.0 as usize
        ))
    }
}

/// After an update, bring an installed screen saver up to this version.
pub fn refresh_installed() {
    let (Some(scr), Ok(exe)) = (installed_path(), std::env::current_exe()) else {
        return;
    };
    if !scr.exists() || exe == scr {
        return;
    }
    if let (Ok(new), Ok(old)) = (std::fs::read(&exe), std::fs::read(&scr)) {
        if new != old {
            // Fails while the screen saver runs; the next start tries again.
            if let Err(e) = copy_to(&scr, &new) {
                log!("screen saver update: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(args: &[&str], scr: bool) -> Mode {
        mode(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), scr)
    }

    #[test]
    fn screen_saver_arguments() {
        assert_eq!(m(&[], false), Mode::Wallpaper);
        assert_eq!(m(&[], true), Mode::Configure);
        assert_eq!(m(&["/s"], true), Mode::Saver);
        assert_eq!(m(&["-S"], true), Mode::Saver);
        assert_eq!(m(&["/c:4242"], true), Mode::Configure);
        assert_eq!(
            m(&["/p", "1234"], true),
            Mode::Preview(HWND(1234 as *mut _))
        );
        assert_eq!(m(&["/P:99"], true), Mode::Preview(HWND(99 as *mut _)));
        assert_eq!(m(&["/p"], true), Mode::Configure);
        assert_eq!(m(&["--other"], false), Mode::Wallpaper);
    }
}
