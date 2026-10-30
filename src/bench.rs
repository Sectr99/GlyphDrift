//! Bench, our desktop design system (github.com/Sectr99/bench-design-system), as the settings
//! windows use it: color and size tokens from its `tokens.json` (v1). A look is a style (Native
//! or Instrument) in a light or dark appearance; the hosts draw their controls from these.

use crate::config::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// System font, cobalt accent, rounded: feels shipped with the OS.
    Native,
    /// Graphite, amber accent, square, uppercase headings, numbers in mono.
    Instrument,
}

/// Saved names, in the order of `ui::STYLES`.
pub const STYLE_NAMES: [&str; 2] = ["native", "instrument"];
/// Saved names, in the order of `ui::APPEARANCES`.
pub const APPEARANCE_NAMES: [&str; 3] = ["system", "light", "dark"];

/// Colors by role. Use them by role, never by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The content area.
    pub window: Rgb,
    /// Title bar and footer: one step back from the content.
    pub chrome: Rgb,
    /// Secondary buttons, hovered rows.
    pub panel: Rgb,
    /// Editable wells: dropdowns, the theme list, the switch's off track.
    pub field: Rgb,
    /// The selected list row, with a 3px accent edge.
    pub selected: Rgb,
    pub ink: Rgb,
    /// Labels and secondary values.
    pub ink_muted: Rgb,
    /// Between regions.
    pub border: Rgb,
    /// Between rows.
    pub border_soft: Rgb,
    /// Around inputs, and the slider track (3:1).
    pub border_control: Rgb,
    /// The one accent: on switches, the slider fill, selection and focus.
    pub accent: Rgb,
    /// Text and marks on the accent.
    pub on_accent: Rgb,
}

/// Sizes in px at 96 DPI, and type, for a style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Height of buttons and dropdowns.
    pub ctl: f32,
    /// Height of list and switch rows.
    pub row: f32,
    /// Corner radius of controls.
    pub radius: f32,
    /// Body text size.
    pub body: f32,
    /// Label text size (uppercase, semibold, muted).
    pub label: f32,
    /// Label letter spacing, in em.
    pub label_track: f32,
    /// Numbers in the monospaced face.
    pub mono_numbers: bool,
    /// Switch thumbs and radios round (Native) or square (Instrument).
    pub round: bool,
}

const fn hex(v: u32) -> Rgb {
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

macro_rules! palette {
    ($($role:ident: $v:literal),* $(,)?) => { Palette { $($role: hex($v)),* } };
}

const NATIVE_LIGHT: Palette = palette! {
    window: 0xffffff, chrome: 0xf3f4f7, panel: 0xf8f9fb, field: 0xffffff, selected: 0xeef2fc,
    ink: 0x1b1f27, ink_muted: 0x636a77, border: 0xe0e3e8, border_soft: 0xeef0f3,
    border_control: 0x8a919e, accent: 0x2f5bd3, on_accent: 0xffffff,
};
const NATIVE_DARK: Palette = palette! {
    window: 0x202227, chrome: 0x1a1c20, panel: 0x25282e, field: 0x2a2d34, selected: 0x28304a,
    ink: 0xe7e9ee, ink_muted: 0x9aa1ae, border: 0x363a42, border_soft: 0x2c2f36,
    border_control: 0x767c88, accent: 0x6e93f5, on_accent: 0x0e1630,
};
const INSTRUMENT_DARK: Palette = palette! {
    window: 0x14171a, chrome: 0x0e1012, panel: 0x191c20, field: 0x0f1114, selected: 0x2a2216,
    ink: 0xd8dce1, ink_muted: 0x7d8590, border: 0x272b31, border_soft: 0x1e2226,
    border_control: 0x5d646e, accent: 0xf2a93b, on_accent: 0x14171a,
};
const INSTRUMENT_LIGHT: Palette = palette! {
    window: 0xf7f8f9, chrome: 0xe6e9ec, panel: 0xeef0f2, field: 0xffffff, selected: 0xfbefd9,
    ink: 0x16191d, ink_muted: 0x5b636d, border: 0xcdd2d8, border_soft: 0xe1e4e8,
    border_control: 0x858d97, accent: 0x9a5b00, on_accent: 0xffffff,
};

const NATIVE: Metrics = Metrics {
    ctl: 30.0,
    row: 32.0,
    radius: 5.0,
    body: 13.0,
    label: 11.0,
    label_track: 0.04,
    mono_numbers: false,
    round: true,
};
const INSTRUMENT: Metrics = Metrics {
    ctl: 26.0,
    row: 26.0,
    radius: 2.0,
    body: 12.5,
    label: 10.5,
    label_track: 0.1,
    mono_numbers: true,
    round: false,
};

/// Bench's spacing steps, in px at 96 DPI.
pub const SPACE: [f32; 7] = [0.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0];

/// A style in an appearance: everything a settings window draws from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub style: Style,
    pub dark: bool,
    pub colors: Palette,
    pub size: Metrics,
}

impl Look {
    /// From the saved names (`[settings] style` and `appearance`); "system" follows
    /// `system_dark`. Unknown names fall back to Native and to the system.
    pub fn new(style: &str, appearance: &str, system_dark: bool) -> Look {
        let style = if style == "instrument" {
            Style::Instrument
        } else {
            Style::Native
        };
        let dark = match appearance {
            "light" => false,
            "dark" => true,
            _ => system_dark,
        };
        let colors = match (style, dark) {
            (Style::Native, false) => NATIVE_LIGHT,
            (Style::Native, true) => NATIVE_DARK,
            (Style::Instrument, false) => INSTRUMENT_LIGHT,
            (Style::Instrument, true) => INSTRUMENT_DARK,
        };
        let size = match style {
            Style::Native => NATIVE,
            Style::Instrument => INSTRUMENT,
        };
        Look {
            style,
            dark,
            colors,
            size,
        }
    }

    /// Bench's theme id, as in `tokens.css`.
    pub fn id(&self) -> String {
        let style = STYLE_NAMES[self.style as usize];
        format!("{style}-{}", if self.dark { "dark" } else { "light" })
    }
}

/// Text as a label shows it: uppercase in both styles (Bench styles labels, not the source).
pub fn label(text: &str) -> String {
    text.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG contrast ratio.
    fn contrast(a: Rgb, b: Rgb) -> f32 {
        let lum = |c: Rgb| {
            let ch = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn looks_resolve_and_meet_benchs_contrast() {
        assert_eq!(Look::new("native", "system", true).id(), "native-dark");
        assert_eq!(
            Look::new("instrument", "light", true).id(),
            "instrument-light"
        );
        assert_eq!(Look::new("nope", "nope", false).id(), "native-light");
        for style in STYLE_NAMES {
            for appearance in ["light", "dark"] {
                let look = Look::new(style, appearance, false);
                let c = look.colors;
                let id = look.id();
                for surface in [c.window, c.chrome, c.panel] {
                    assert!(contrast(c.ink, surface) >= 4.5, "{id}: ink");
                    assert!(contrast(c.ink_muted, surface) >= 4.5, "{id}: ink-muted");
                }
                assert!(
                    contrast(c.border_control, c.field) >= 3.0,
                    "{id}: control border"
                );
                assert!(contrast(c.accent, c.window) >= 3.0, "{id}: accent");
                assert!(contrast(c.on_accent, c.accent) >= 4.5, "{id}: on accent");
            }
        }
    }
}
