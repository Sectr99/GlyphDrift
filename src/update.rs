//! Updates: asks GitHub for the latest release and compares it with this build, and on request
//! installs it. Nothing runs in the background (Windows: the tray's Check for updates…, then a
//! click on the notification it shows).

/// Where the latest version is downloaded.
pub const RELEASES_URL: &str = "https://github.com/Sectr99/GlyphDrift/releases/latest";
#[cfg(windows)]
const API_URL: &str = "https://api.github.com/repos/Sectr99/GlyphDrift/releases/latest";
/// A release's files are at `<this>/<tag>/<name>`.
#[cfg(windows)]
const DOWNLOAD_URL: &str = "https://github.com/Sectr99/GlyphDrift/releases/download";
/// The release file that's installed, as named in the release's `SHA256SUMS.txt`.
#[cfg(windows)]
const EXE_ASSET: &str = "glyphdrift.exe";
/// The updated exe to start once this process has stopped.
#[cfg(windows)]
static RESTART: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

/// What a check found.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// A newer release, by its tag (`v1.2.3`).
    Newer(String),
    Current,
    Failed(String),
}

/// The `tag_name` of a GitHub release, from its JSON.
fn tag_name(json: &str) -> Option<&str> {
    let key = "\"tag_name\"";
    let rest = &json[json.find(key)? + key.len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

/// `v1.2.3` or `1.2.3` as numbers; None for anything else.
fn version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.strip_prefix('v').unwrap_or(s).split('.');
    let mut next = || parts.next()?.parse().ok();
    let v = (next()?, next()?, next()?);
    parts.next().is_none().then_some(v)
}

/// What release JSON from GitHub says, compared with version `current`.
pub fn compare(json: &str, current: &str) -> Outcome {
    let Some(tag) = tag_name(json) else {
        return Outcome::Failed("GitHub's reply had no version".into());
    };
    match (version(tag), version(current)) {
        (Some(latest), Some(this)) if latest > this => Outcome::Newer(tag.into()),
        (Some(_), Some(_)) => Outcome::Current,
        _ => Outcome::Failed(format!("can't compare version {tag} with {current}")),
    }
}

/// The SHA-256 listed for file `name` in a `sha256sum` listing.
#[cfg(any(windows, test))]
fn listed_hash(sums: &str, name: &str) -> Option<[u8; 32]> {
    let line = sums.lines().find(|line| {
        let file = line.split_whitespace().nth(1);
        file.map(|f| f.strip_prefix('*').unwrap_or(f)) == Some(name)
    })?;
    let hex = line.split_whitespace().next()?;
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut hash = [0u8; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(hash)
}

/// Run `work` on a new thread in the Windows Runtime's multithreaded apartment, then call
/// `done` there with its result.
#[cfg(windows)]
fn in_background<T>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + Send + 'static,
) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    std::thread::spawn(move || {
        // SAFETY: initializes COM for this new thread only; it ends with the thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        done(work());
    });
}

/// Check on a background thread, then call `done` there with the outcome.
#[cfg(windows)]
pub fn check_in_background(done: impl FnOnce(Outcome) + Send + 'static) {
    let check = || match fetch_text(API_URL) {
        Ok(json) => compare(&json, env!("CARGO_PKG_VERSION")),
        Err(e) => Outcome::Failed(e),
    };
    in_background(check, done);
}

/// Install release `tag` (see `install`) on a background thread, then call `done` there.
#[cfg(windows)]
pub fn install_in_background(
    tag: String,
    done: impl FnOnce(Result<std::path::PathBuf, String>) + Send + 'static,
) {
    in_background(move || install(&tag), done);
}

/// Download release `tag`'s exe, check it against the SHA-256 the release lists, and put it
/// in place of this exe. Returns the path to start (`restart_after_exit`); this process keeps
/// running from the old exe, renamed to `.exe.old` until `clean_up` deletes it.
#[cfg(windows)]
fn install(tag: &str) -> Result<std::path::PathBuf, String> {
    let base = format!("{DOWNLOAD_URL}/{tag}");
    let sums = fetch_text(&format!("{base}/SHA256SUMS.txt"))?;
    let expected = listed_hash(&sums, EXE_ASSET)
        .ok_or_else(|| format!("the release lists no checksum for {EXE_ASSET}"))?;
    let exe = fetch_bytes(&format!("{base}/{EXE_ASSET}"))?;
    if sha256(&exe)? != expected {
        return Err("the download doesn't match the release's checksum".into());
    }
    if !exe.starts_with(b"MZ") {
        return Err("the download isn't a Windows program".into());
    }
    let path = std::env::current_exe().map_err(|e| e.to_string())?;
    replace(&path, &exe)?;
    Ok(path)
}

/// Swap `bytes` in for the exe at `path`. A running exe can't be overwritten but can be
/// renamed, so the old one moves aside to `.exe.old`; on failure it's put back.
#[cfg(windows)]
fn replace(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let (new, old) = (
        path.with_extension("exe.new"),
        path.with_extension("exe.old"),
    );
    let failed = |p: &std::path::Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::write(&new, bytes).map_err(|e| failed(&new, e))?;
    let _ = std::fs::remove_file(&old); // left by an update that couldn't clean up
    if let Err(e) = std::fs::rename(path, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(failed(path, e));
    }
    if let Err(e) = std::fs::rename(&new, path) {
        let _ = std::fs::rename(&old, path);
        let _ = std::fs::remove_file(&new);
        return Err(failed(path, e));
    }
    Ok(())
}

/// Start the updated exe at `path` once this process has stopped (see `take_restart`), not
/// beside it: it then starts as the only copy, whatever its version.
#[cfg(windows)]
pub fn restart_after_exit(path: std::path::PathBuf) {
    if let Ok(mut restart) = RESTART.lock() {
        *restart = Some(path);
    }
}

/// The exe `restart_after_exit` asked for, for the caller to start with `restart` once it
/// has stopped and released its single-instance mutex.
#[cfg(windows)]
pub fn take_restart() -> Option<std::path::PathBuf> {
    RESTART.lock().ok()?.take()
}

/// Start the updated exe at `path`.
#[cfg(windows)]
pub fn restart(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(path)
        .spawn()
        .map(drop)
        .map_err(|e| format!("starting {}: {e}", path.display()))
}

/// Delete the exe an update replaced, once the old process has quit.
#[cfg(windows)]
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }
}

#[cfg(windows)]
fn sha256(data: &[u8]) -> Result<[u8; 32], String> {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut hash = [0u8; 32];
    // SAFETY: Windows' built-in SHA-256 provider; the slices outlive the call.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut hash) };
    status
        .is_ok()
        .then_some(hash)
        .ok_or_else(|| format!("SHA-256 failed ({:#x})", status.0))
}

/// The Windows Runtime's HTTP client, named to GitHub.
#[cfg(windows)]
fn client() -> windows::core::Result<windows::Web::Http::HttpClient> {
    use windows::core::HSTRING;
    let client = windows::Web::Http::HttpClient::new()?;
    // GitHub's API refuses requests without one.
    let agent = concat!("Glyphdrift/", env!("CARGO_PKG_VERSION"));
    client
        .DefaultRequestHeaders()?
        .UserAgent()?
        .TryParseAdd(&HSTRING::from(agent))?;
    Ok(client)
}

#[cfg(windows)]
fn uri(url: &str) -> windows::core::Result<windows::Foundation::Uri> {
    windows::Foundation::Uri::CreateUri(&windows::core::HSTRING::from(url))
}

#[cfg(windows)]
fn fetch_text(url: &str) -> Result<String, String> {
    let get = || client()?.GetStringAsync(&uri(url)?)?.join();
    get()
        .map(|s| s.to_string())
        .map_err(|e| format!("{url}: {}", e.message()))
}

#[cfg(windows)]
fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    use windows::Storage::Streams::DataReader;
    let get = || {
        let buffer = client()?.GetBufferAsync(&uri(url)?)?.join()?;
        let mut bytes = vec![0; buffer.Length()? as usize];
        DataReader::FromBuffer(&buffer)?.ReadBytes(&mut bytes)?;
        Ok(bytes)
    };
    get().map_err(|e: windows::core::Error| format!("{url}: {}", e.message()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_the_release_tag() {
        let json = r#"{"url":"x","tag_name": "v0.3.0","name":"Glyphdrift 0.3.0"}"#;
        assert_eq!(compare(json, "0.2.6"), Outcome::Newer("v0.3.0".into()));
        assert_eq!(compare(json, "0.3.0"), Outcome::Current);
        // Numbers, not text: 0.10 is newer than 0.9.
        let json = r#"{"tag_name":"v0.10.0"}"#;
        assert_eq!(compare(json, "0.9.9"), Outcome::Newer("v0.10.0".into()));
        // A local build ahead of the release.
        assert_eq!(compare(json, "0.11.0"), Outcome::Current);
        assert!(matches!(compare("{}", "0.2.6"), Outcome::Failed(_)));
        assert!(matches!(
            compare(r#"{"tag_name":"nightly"}"#, "0.2.6"),
            Outcome::Failed(_)
        ));
        assert_eq!(version("v1.2"), None);
        assert_eq!(version("1.2.3.4"), None);
    }

    #[test]
    fn finds_the_listed_checksum() {
        let a = "ab".repeat(32);
        let sums = format!("{}  other.zip\n{a}  glyphdrift.exe\n", "cd".repeat(32));
        assert_eq!(listed_hash(&sums, "glyphdrift.exe"), Some([0xab; 32]));
        // Binary-mode listings mark the name with `*`.
        assert_eq!(
            listed_hash(&format!("{a} *glyphdrift.exe"), "glyphdrift.exe"),
            Some([0xab; 32])
        );
        assert_eq!(listed_hash(&sums, "missing.exe"), None);
        assert_eq!(listed_hash("abc  glyphdrift.exe", "glyphdrift.exe"), None);
    }

    #[cfg(windows)]
    #[test]
    fn sha256_matches_the_standard() {
        let hash = sha256(b"abc").unwrap();
        assert_eq!(
            listed_hash(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  abc",
                "abc"
            ),
            Some(hash)
        );
    }

    #[cfg(windows)]
    #[test]
    fn replace_swaps_and_keeps_the_old_exe() {
        let dir = std::env::temp_dir().join(format!("glyphdrift-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("glyphdrift.exe");
        std::fs::write(&exe, b"old").unwrap();
        replace(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(
            std::fs::read(exe.with_extension("exe.old")).unwrap(),
            b"old"
        );
        assert!(!exe.with_extension("exe.new").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Downloads the latest release's exe and checks it against the release's checksum,
    /// without installing it. `cargo test live_download -- --ignored --nocapture`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_download() {
        // Like the app: web calls only on an `in_background` thread.
        let download = || -> Result<String, String> {
            let json = fetch_text(API_URL)?;
            let tag = tag_name(&json).ok_or("no tag")?.to_string();
            let sums = fetch_text(&format!("{DOWNLOAD_URL}/{tag}/SHA256SUMS.txt"))?;
            let exe = fetch_bytes(&format!("{DOWNLOAD_URL}/{tag}/{EXE_ASSET}"))?;
            let matches = Some(sha256(&exe)?) == listed_hash(&sums, EXE_ASSET);
            matches
                .then(|| format!("{tag}: {} bytes, checksum matches", exe.len()))
                .ok_or_else(|| format!("{tag}: checksum doesn't match"))
        };
        let (tx, rx) = std::sync::mpsc::channel();
        in_background(download, move |result| tx.send(result).unwrap());
        println!("{}", rx.recv().unwrap().unwrap());
    }

    /// Asks GitHub for real. `cargo test live_check -- --ignored --nocapture`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_check() {
        let (tx, rx) = std::sync::mpsc::channel();
        check_in_background(move |outcome| tx.send(outcome).unwrap());
        let outcome = rx.recv().unwrap();
        println!("{outcome:?}");
        assert!(!matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    }
}
