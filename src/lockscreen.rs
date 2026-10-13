//! Show on lock screen (Windows): a still frame of the animation as the lock screen picture,
//! through the Windows Runtime `LockScreen` API (it works without an installer). The user's
//! own picture is remembered when this is first turned on and put back when it's turned off.
//! The Windows calls run on a short-lived thread, so the wallpaper never waits on them.

use std::path::{Path, PathBuf};

use windows::core::HSTRING;
use windows::Storage::StorageFile;
use windows::System::UserProfile::LockScreen;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

fn dir() -> Option<PathBuf> {
    Some(crate::config::default_path()?.parent()?.to_path_buf())
}

/// Where frame `n` goes. Names alternate: Windows ignores a new picture whose file name
/// matches the current one.
pub fn frame_path(n: u32) -> Option<PathBuf> {
    Some(dir()?.join(format!("lock-screen-{}.png", n % 2)))
}

/// The user's own picture, remembered while ours is shown.
fn original_note() -> Option<PathBuf> {
    Some(dir()?.join("lock-screen-original.txt"))
}

/// Show `frame` on the lock screen, first remembering the user's picture if that hasn't
/// been done yet.
pub fn show(frame: PathBuf) {
    run(move || {
        if let Some(note) = original_note().filter(|n| !n.exists()) {
            match current_picture() {
                Some(own) if !is_ours(&own) => {
                    std::fs::write(&note, own.to_string_lossy().as_bytes())
                        .map_err(|e| format!("{}: {e}", note.display()))?;
                }
                _ => {}
            }
        }
        set(&frame)
    });
}

/// Put the user's own picture back, if one was remembered and still exists.
pub fn restore() {
    run(|| {
        let Some(note) = original_note().filter(|n| n.exists()) else {
            return Ok(());
        };
        let own = PathBuf::from(std::fs::read_to_string(&note).map_err(|e| format!("{e}"))?);
        if own.exists() {
            set(&own)?;
        } else {
            log!(
                "lock screen: your picture {} no longer exists",
                own.display()
            );
        }
        let _ = std::fs::remove_file(note);
        Ok(())
    });
}

/// Run `f` on its own thread in the Windows Runtime's multithreaded apartment, logging
/// any error.
fn run(f: impl FnOnce() -> Result<(), String> + Send + 'static) {
    std::thread::spawn(move || {
        // SAFETY: initializes COM for this new thread only; it ends with the thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if let Err(e) = f() {
            log!("lock screen: {e}");
        }
    });
}

fn set(path: &Path) -> Result<(), String> {
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
        .and_then(|op| op.join())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    LockScreen::SetImageFileAsync(&file)
        .and_then(|op| op.join())
        .map_err(|e| format!("setting the picture: {e}"))
}

/// The lock screen's current picture as a file path.
fn current_picture() -> Option<PathBuf> {
    let uri = LockScreen::OriginalImageFile().ok()?.AbsoluteUri().ok()?;
    file_uri_to_path(&uri.to_string())
}

fn is_ours(path: &Path) -> bool {
    dir().is_some_and(|d| path.starts_with(d))
}

/// `file:///C:/Some%20Dir/x.png` to `C:\Some Dir\x.png`.
fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file:///")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'/' => {
                out.push(b'\\');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            file_uri_to_path("file:///C:/Users/A%20B/Pictures/caf%C3%A9.jpg"),
            Some(PathBuf::from(r"C:\Users\A B\Pictures\café.jpg"))
        );
        assert_eq!(file_uri_to_path("ms-appdata:///x.png"), None);
    }

    #[test]
    fn frame_names_alternate() {
        assert_ne!(frame_path(0), frame_path(1));
        assert_eq!(frame_path(0), frame_path(2));
    }
}
