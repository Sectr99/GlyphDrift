//! TOML configuration with defaults and validation.
//!
//! Invalid values never abort startup: they are clamped or replaced by defaults.

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
    pub pause_on_fullscreen: bool,
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
    pub foreground: String,
    pub background: String,
    pub ramp: String,
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
            fps: 24,
            animation: "matrix".into(),
            pause_on_fullscreen: true,
        }
    }
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "Consolas".into(),
            size: 16,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            foreground: "#00ff66".into(),
            background: "#000000".into(),
            ramp: " .:-=+*#%@".into(),
        }
    }
}

impl Config {
    /// Parse and validate. Returns the config plus human-readable warnings.
    pub fn from_toml(text: &str) -> (Config, Vec<String>) {
        let mut warnings = Vec::new();
        let mut cfg: Config = match toml::from_str(text) {
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
        let fps = self.general.fps.clamp(1, 60);
        if fps != self.general.fps {
            w.push(format!(
                "fps {} out of range, using {fps}",
                self.general.fps
            ));
            self.general.fps = fps;
        }
        let size = self.font.size.clamp(6, 96);
        if size != self.font.size {
            w.push(format!(
                "font size {} out of range, using {size}",
                self.font.size
            ));
            self.font.size = size;
        }
        if !super::anim::NAMES.contains(&self.general.animation.as_str()) {
            w.push(format!(
                "unknown animation '{}', using matrix",
                self.general.animation
            ));
            self.general.animation = "matrix".into();
        }
        let d = Theme::default();
        if Rgb::parse(&self.theme.foreground).is_none() {
            w.push(format!("bad foreground color '{}'", self.theme.foreground));
            self.theme.foreground = d.foreground;
        }
        if Rgb::parse(&self.theme.background).is_none() {
            w.push(format!("bad background color '{}'", self.theme.background));
            self.theme.background = d.background;
        }
        if self.theme.ramp.chars().count() < 2 || !self.theme.ramp.is_ascii() {
            w.push("ramp must be 2+ ASCII characters, using default".into());
            self.theme.ramp = d.ramp;
        }
    }

    pub fn foreground(&self) -> Rgb {
        Rgb::parse(&self.theme.foreground).unwrap_or(Rgb(0, 255, 102))
    }

    pub fn background(&self) -> Rgb {
        Rgb::parse(&self.theme.background).unwrap_or(Rgb(0, 0, 0))
    }
}

/// `%APPDATA%\ascii-wallpaper\config.toml`
pub fn default_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(
        std::path::PathBuf::from(base)
            .join("ascii-wallpaper")
            .join("config.toml"),
    )
}

/// Load from disk; missing file means defaults, silently.
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
        assert_eq!(c.general.fps, 24);
        assert_eq!(c.foreground(), Rgb(0, 0xff, 0x66));
    }

    #[test]
    fn clamps_and_falls_back() {
        let (c, w) = Config::from_toml(
            "[general]\nfps = 500\nanimation = \"nope\"\n[theme]\nforeground = \"green\"\nramp = \"x\"\n",
        );
        assert_eq!(c.general.fps, 60);
        assert_eq!(c.general.animation, "matrix");
        assert_eq!(c.foreground(), Rgb(0, 0xff, 0x66));
        assert_eq!(c.theme.ramp, " .:-=+*#%@");
        assert_eq!(w.len(), 4);
    }

    #[test]
    fn syntax_error_gives_defaults() {
        let (c, w) = Config::from_toml("[general\nfps = ");
        assert_eq!(c.general.fps, 24);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn example_file_parses_cleanly() {
        let text = include_str!("../../config.example.toml");
        let (_, w) = Config::from_toml(text);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn rgb_parse() {
        assert_eq!(Rgb::parse("#0a0B0c"), Some(Rgb(10, 11, 12)));
        assert_eq!(Rgb::parse("0a0b0c"), None);
        assert_eq!(Rgb::parse("#12345"), None);
    }
}
