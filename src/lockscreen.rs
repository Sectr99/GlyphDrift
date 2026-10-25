//! Show on lock screen (Windows): a still frame of the animation as the lock screen picture,
//! through the Windows Runtime `LockScreen` API (it works without an installer). The user's
//! own picture is remembered when this is first turned on and put back when it's turned off.
//! The Windows calls run in order on a worker thread, so the wallpaper never waits on them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;

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

/// The user's own picture, remembered while ours is shown; empty if it couldn't be read.
fn original_note() -> Option<PathBuf> {
    Some(dir()?.join("lock-screen-original.txt"))
}

/// Show `frame` on the lock screen, first remembering the user's picture if that hasn't
/// been done yet.
pub fn show(frame: PathBuf) {
    run(move || {
        if let Some(note) = original_note().filter(|n| !n.exists()) {
            let own = current_picture().filter(|p| !is_ours(p));
            if own.is_none() {
                log!(
                    "lock screen: your current picture isn't a file (Windows Spotlight?), so                      turning this off can't bring it back"
                );
            }
            let text = own.map(|p| p.to_string_lossy().into_owned());
            std::fs::write(&note, text.unwrap_or_default())
                .map_err(|e| format!("{}: {e}", note.display()))?;
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
        let own = std::fs::read_to_string(&note).map_err(|e| format!("{e}"))?;
        let own = PathBuf::from(own);
        if own.as_os_str().is_empty() {
            // Not readable when first shown, already logged.
        } else if own.exists() {
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

type Job = Box<dyn FnOnce() -> Result<(), String> + Send>;

/// Run `f` on the worker thread, in the Windows Runtime's multithreaded apartment, logging
/// any error. One thread runs the jobs in order: a `restore` never loses to a `show` sent
/// before it.
fn run(f: impl FnOnce() -> Result<(), String> + Send + 'static) {
    static WORKER: OnceLock<Sender<Job>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::spawn(move || {
            // SAFETY: initializes COM for this thread only; it lives as long as the process.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            for job in rx {
                if let Err(e) = job() {
                    log!("lock screen: {e}");
                }
            }
        });
        tx
    });
    let _ = worker.send(Box::new(f));
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
