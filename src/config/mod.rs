//! TOML configuration. Invalid values never abort startup: they're clamped or defaulted.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(v(0)?, v(2)?, v(4)?))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct General {
    pub fps: u32,
    pub animation: String,
    /// Speed multiplier.
    pub speed: f32,
    /// ASCII font size, line weights and feature sizes.
    pub scale: f32,
    /// Extra scale for centered objects.
    pub object_scale: f32,
    /// Time and date in Topographic drift.
    pub clock: bool,
    /// macOS: stop below the menu bar and set the desktop picture to the theme background.
    pub match_menu_bar: bool,
    /// Show a still frame of the animation on the lock screen (macOS: as the desktop
    /// picture; Windows: as the lock screen picture). On by default only on macOS, where
    /// it was introduced; on Windows it replaces a picture the user chose.
    pub lock_screen: bool,
    pub pause_on_fullscreen: bool,
    pub pause_on_battery_saver: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Font {
    pub family: String,
    pub size: i32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Theme {
    /// One of `theme::THEMES`.
    pub name: String,
    pub ramp: String,
    /// Veil opacity (Settings: Icon readability).
    pub glass: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub font: Font,
    pub theme: Theme,
}

impl Default for General {
    fn default() -> Self {
        Self {
            fps: 0,
            animation: "matrix".into(),
            speed: 1.0,
            scale: 1.0,
            object_scale: 1.0,
            clock: false,
            match_menu_bar: false,
            lock_screen: cfg!(target_os = "macos"),
            pause_on_fullscreen: true,
            pause_on_battery_saver: true,
        }
    }
}

/// A monospaced font every install has.
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_FONT: &str = "Consolas";
#[cfg(target_os = "macos")]
pub const DEFAULT_FONT: &str = "Menlo";

impl Default for Font {
    fn default() -> Self {
        Self {
            family: DEFAULT_FONT.into(),
            size: 16,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "matrix".into(),
            glass: 0.0,
            ramp: " .:-=+*#%@".into(),
        }
    }
}

impl Config {
    /// Parse and validate; returns warnings too.
    pub fn from_toml(text: &str) -> (Config, Vec<String>) {
        let mut warnings = Vec::new();
        let mut cfg: Config = match toml_edit::de::from_str(text) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("config parse error, using defaults: {e}"));
                Config::default()
            }
        };
        cfg.validate(&mut warnings);
        (cfg, warnings)
    }

    fn validate(&mut self, w: &mut Vec<String>) {
        // 0 = auto.
        let fps = if self.general.fps == 0 {
            0
        } else {
            self.general.fps.clamp(1, 60)
        };
        if fps != self.general.fps {
            w.push(format!(
                "fps {} out of range, using {fps}",
                self.general.fps
            ));
            self.general.fps = fps;
        }
        let speed = if self.general.speed.is_finite() {
            self.general.speed.clamp(0.1, 4.0)
        } else {
            1.0
        };
        if speed != self.general.speed {
            w.push(format!(
                "speed {} out of range, using {speed}",
                self.general.speed
            ));
            self.general.speed = speed;
        }
        let glass = if self.theme.glass.is_finite() {
            self.theme.glass.clamp(0.0, 0.9)
        } else {
            0.0
        };
        if glass != self.theme.glass {
            w.push(format!(
                "glass {} out of range, using {glass}",
                self.theme.glass
            ));
            self.theme.glass = glass;
        }
        for (name, value) in [
            ("scale", &mut self.general.scale),
            ("object_scale", &mut self.general.object_scale),
        ] {
            let clamped = if value.is_finite() {
                value.clamp(0.5, 2.0)
            } else {
                1.0
            };
            if clamped != *value {
                w.push(format!("{name} {value} out of range, using {clamped}"));
                *value = clamped;
            }
        }
        let size = self.font.size.clamp(6, 96);
        if size != self.font.size {
            w.push(format!(
                "font size {} out of range, using {size}",
                self.font.size
            ));
            self.font.size = size;
        }
        if !super::anim::ANIMATIONS
            .iter()
            .any(|a| a.0 == self.general.animation)
        {
            w.push(format!(
                "unknown animation '{}', using matrix",
                self.general.animation
            ));
            self.general.animation = "matrix".into();
        }
        let d = Theme::default();
        if !super::theme::THEMES
            .iter()
            .any(|t| t.name == self.theme.name)
        {
            w.push(format!("unknown theme '{}', using matrix", self.theme.name));
            self.theme.name = d.name;
        }
        if self.theme.ramp.chars().count() < 2 || !self.theme.ramp.is_ascii() {
            w.push("ramp must be 2+ ASCII characters, using default".into());
            self.theme.ramp = d.ramp;
        }
    }
}

/// Set `[section] key = value` in `text`, keeping everything else.
pub fn set_value(
    text: &str,
    section: &str,
    key: &str,
    value: impl Into<toml_edit::Value>,
) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{e}"))?;
    let table = doc
        .entry(section)
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or_else(|| format!("[{section}] is not a table"))?;
    table[key] = toml_edit::value(value);
    Ok(doc.to_string())
}

/// Save one setting, creating the file if needed; an unparseable file is left alone.
pub fn save(section: &str, key: &str, value: impl Into<toml_edit::Value>) -> Result<(), String> {
    let path = default_path().ok_or("no settings folder")?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let out = set_value(&text, section, key, value)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{e}"))?;
    }
    std::fs::write(&path, out).map_err(|e| format!("{e}"))
}

/// The first-run config: no settings (all defaults). Its existence marks the first-run
/// hint as shown.
const NEW_CONFIG: &str = "\
# Glyphdrift settings. Changes made in Settings or the tray menu are saved here, and edits
# to this file apply within a second. Every option is listed in config.example.toml:
# https://github.com/Sectr99/GlyphDrift/blob/main/config.example.toml
";

/// Create the config if missing; true if created (the first run).
pub fn create_if_missing() -> bool {
    let Some(path) = default_path() else {
        return false;
    };
    if path.exists() {
        return false;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, NEW_CONFIG).is_ok()
}

/// `%APPDATA%\Glyphdrift\config.toml`, or on macOS
/// `~/Library/Application Support/Glyphdrift/config.toml`
pub fn default_path() -> Option<std::path::PathBuf> {
    Some(settings_dir()?.join("Glyphdrift").join("config.toml"))
}

#[cfg(not(target_os = "macos"))]
fn settings_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(Into::into)
}

#[cfg(target_os = "macos")]
fn settings_dir() -> Option<std::path::PathBuf> {
    let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
    Some(home.join("Library").join("Application Support"))
}

/// Config file mtime, for hot reload.
pub fn modified() -> Option<std::time::SystemTime> {
    std::fs::metadata(default_path()?).ok()?.modified().ok()
}

/// Load from disk; a missing file means defaults.
pub fn load() -> (Config, Vec<String>) {
    match default_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => Config::from_toml(&text),
        None => (Config::default(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_defaults() {
        let (c, w) = Config::from_toml("");
        assert!(w.is_empty());
        assert_eq!(c.general.fps, 0);
        assert_eq!(c.theme.name, "matrix");
    }

    #[test]
    fn clamps_and_falls_back() {
        let (c, w) = Config::from_toml(
            "[general]\nfps = 500\nspeed = -1.0\nanimation = \"nope\"\n[theme]\nname = \"nope\"\nramp = \"x\"\n",
        );
        assert_eq!(c.general.fps, 60);
        assert_eq!(c.general.speed, 0.1);
        assert_eq!(c.general.animation, "matrix");
        assert_eq!(c.theme.name, "matrix");
        assert_eq!(c.theme.ramp, " .:-=+*#%@");
        assert_eq!(w.len(), 5);
    }

    #[test]
    fn syntax_error_gives_defaults() {
        let (c, w) = Config::from_toml("[general\nfps = ");
        assert_eq!(c.general.fps, 0);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn example_file_parses_cleanly() {
        let text = include_str!("../../config.example.toml");
        let (_, w) = Config::from_toml(text);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn set_value_keeps_comments_and_adds_sections() {
        let text = "# mine\n[general]\nfps = 30 # smooth\n";
        let out = set_value(text, "theme", "name", "nord").unwrap();
        assert!(
            out.contains("# mine") && out.contains("fps = 30 # smooth"),
            "{out}"
        );
        let out = set_value(&out, "theme", "name", "dracula").unwrap();
        let (c, w) = Config::from_toml(&out);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((c.general.fps, c.theme.name.as_str()), (30, "dracula"));
        assert!(set_value("[broken", "theme", "name", "x").is_err());
    }

    #[test]
    fn new_config_is_all_defaults() {
        let (c, w) = Config::from_toml(NEW_CONFIG);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.general.animation, Config::default().general.animation);
    }

    #[test]
    fn rgb_parse() {
        assert_eq!(Rgb::parse("#0a0B0c"), Some(Rgb(10, 11, 12)));
        assert_eq!(Rgb::parse("0a0b0c"), None);
        assert_eq!(Rgb::parse("#12345"), None);
    }
}
