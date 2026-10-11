//! Open at login: a per-user LaunchAgent that runs this executable (no admin; listed in
//! System Settings > General > Login Items & Extensions).

use std::path::PathBuf;

const LABEL: &str = "com.sectr99.glyphdrift";

fn plist_path() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(
        home.join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist")),
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The agent for this executable.
fn plist() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        escape(&exe.display().to_string())
    ))
}

/// True if the agent runs this executable.
pub fn enabled() -> bool {
    let (Some(path), Some(want)) = (plist_path(), plist()) else {
        return false;
    };
    std::fs::read_to_string(path).is_ok_and(|have| have == want)
}

pub fn set(on: bool) -> Result<(), String> {
    let path = plist_path().ok_or("HOME not set")?;
    if on {
        let text = plist().ok_or("cannot locate own executable")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{e}"))?;
        }
        std::fs::write(&path, text).map_err(|e| format!("{e}"))
    } else {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{e}")),
            _ => Ok(()),
        }
    }
}
