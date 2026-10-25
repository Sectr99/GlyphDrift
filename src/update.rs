//! Update check: asks GitHub for the latest release and compares it with this build. It runs
//! only when asked (Windows: the tray's Check for updates…), never in the background.

/// Where the latest version is downloaded.
pub const RELEASES_URL: &str = "https://github.com/Sectr99/GlyphDrift/releases/latest";
#[cfg(windows)]
const API_URL: &str = "https://api.github.com/repos/Sectr99/GlyphDrift/releases/latest";

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

/// Check on a background thread, then call `done` there with the outcome.
#[cfg(windows)]
pub fn check_in_background(done: impl FnOnce(Outcome) + Send + 'static) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    std::thread::spawn(move || {
        // SAFETY: initializes COM for this new thread only; it ends with the thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        done(match fetch() {
            Ok(json) => compare(&json, env!("CARGO_PKG_VERSION")),
            Err(e) => Outcome::Failed(e.message()),
        });
    });
}

/// The latest release's JSON, through the Windows Runtime's HTTP client.
#[cfg(windows)]
fn fetch() -> windows::core::Result<String> {
    use windows::core::HSTRING;
    use windows::Foundation::Uri;
    use windows::Web::Http::HttpClient;
    let client = HttpClient::new()?;
    // GitHub's API refuses requests without one.
    let agent = concat!("Glyphdrift/", env!("CARGO_PKG_VERSION"));
    client
        .DefaultRequestHeaders()?
        .UserAgent()?
        .TryParseAdd(&HSTRING::from(agent))?;
    let uri = Uri::CreateUri(&HSTRING::from(API_URL))?;
    Ok(client.GetStringAsync(&uri)?.join()?.to_string())
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
