//! Settings logic both apps share: reading and applying a setting, what a config edit
//! affects, the frame rate, and the tray or menu bar hover text. Each function changes only
//! the config and reports what changed; the host acts on it (timers, renderers, windows).

use crate::anim::ANIMATIONS;
use crate::config::{self, Config};
use crate::theme::{self, THEMES};
use crate::ui;

/// What a change affects, for the host to act on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changed {
    /// The frame rate or the animation's own rate: restart the frame timer.
    pub frame_rate: bool,
    /// Theme or readability veil: recolor.
    pub colors: bool,
    /// Grid font or overall scale: rebuild the renderer's font, then lay out.
    pub font: bool,
    /// Another animation: create it, then lay out.
    pub animation: bool,
    /// Object scale: lay out (re-init the animation at the same size).
    pub layout: bool,
    /// Something drawn changed without a new layout (the clock): render a frame.
    pub frame: bool,
    /// macOS: Match menu bar to theme was turned on or off.
    pub menu_bar: bool,
    /// macOS: Show on lock screen was turned on or off.
    pub lock_screen: bool,
}

/// A setting as the settings window shows it.
pub fn tweak_value(cfg: &Config, id: usize) -> f32 {
    let g = &cfg.general;
    match id {
        ui::SPEED => g.speed,
        ui::FPS => g.fps as f32,
        ui::GLASS => cfg.theme.glass,
        ui::SCALE => g.scale,
        ui::OBJECT_SCALE => g.object_scale,
        ui::CLOCK => f32::from(u8::from(g.clock)),
        _ => 0.0,
    }
}

/// Apply a setting from the settings window or menu, clamped; with `commit` (a slider
/// released, a box ticked) save it too.
pub fn tweak(cfg: &mut Config, id: usize, value: f32, commit: bool) -> Changed {
    let mut changed = Changed::default();
    match id {
        ui::SPEED => {
            cfg.general.speed = value.clamp(0.1, 4.0);
            if commit {
                save("general", "speed", decimal(cfg.general.speed));
            }
        }
        ui::FPS => {
            cfg.general.fps = value.round().clamp(0.0, 60.0) as u32;
            changed.frame_rate = true;
            if commit {
                save("general", "fps", cfg.general.fps as i64);
            }
        }
        ui::GLASS => {
            cfg.theme.glass = value.clamp(0.0, 0.9);
            changed.colors = true;
            if commit {
                save("theme", "glass", decimal(cfg.theme.glass));
            }
        }
        ui::SCALE => {
            let scale = value.clamp(0.5, 2.0);
            if scale != cfg.general.scale {
                cfg.general.scale = scale;
                changed.font = true; // the font size follows the scale
            }
            if commit {
                save("general", "scale", decimal(scale));
            }
        }
        ui::OBJECT_SCALE => {
            let scale = value.clamp(0.5, 2.0);
            if scale != cfg.general.object_scale {
                cfg.general.object_scale = scale;
                changed.layout = true;
            }
            if commit {
                save("general", "object_scale", decimal(scale));
            }
        }
        ui::CLOCK => {
            cfg.general.clock = value > 0.5;
            changed.frame = true;
            if commit {
                save("general", "clock", cfg.general.clock);
            }
        }
        _ => {}
    }
    changed
}

/// What a config file edit affects, from the config before (`old`) and after (`new`).
/// Colors are always worth reapplying: they're cheap and the theme may have changed.
pub fn reloaded(old: &Config, new: &Config) -> Changed {
    let (o, n) = (&old.general, &new.general);
    Changed {
        frame_rate: o.fps != n.fps || o.animation != n.animation,
        colors: true,
        font: (&old.font.family, old.font.size, o.scale)
            != (&new.font.family, new.font.size, n.scale),
        animation: (&o.animation, &old.theme.ramp) != (&n.animation, &new.theme.ramp),
        layout: o.object_scale != n.object_scale,
        frame: o.clock != n.clock,
        menu_bar: o.match_menu_bar != n.match_menu_bar,
        lock_screen: o.lock_screen != n.lock_screen,
    }
}

/// Frames per second: the config's, or on Auto (0) the animation's own.
pub fn frame_rate(cfg: &Config, animation_fps: u32) -> u32 {
    match cfg.general.fps {
        0 => animation_fps,
        n => n,
    }
    .max(1)
}

/// Tray or menu bar hover text: animation and theme, or why it's paused.
pub fn status(cfg: &Config, user_paused: bool, auto_paused: Option<&str>) -> String {
    if user_paused {
        "Glyphdrift: paused".to_string()
    } else if let Some(reason) = auto_paused {
        format!("Glyphdrift: paused while {reason}")
    } else {
        let anim = ANIMATIONS[crate::anim::index_of(&cfg.general.animation)].1;
        let theme = THEMES[theme::index_of(&cfg.theme.name)].label;
        format!("Glyphdrift: {anim} · {theme}")
    }
}

/// Rounded to 3 decimals for the config (0.3f32 would save as 0.30000001192092896).
pub fn decimal(v: f32) -> f64 {
    (v as f64 * 1000.0).round() / 1000.0
}

/// Save one setting; a failure only loses it on restart, so it's logged, not returned.
pub fn save(section: &str, key: &str, value: impl Into<toml_edit::Value>) {
    if let Err(e) = config::save(section, key, value) {
        report(&format!("config: could not save {section}.{key}: {e}"));
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn report(message: &str) {
    crate::log::write(message);
}

#[cfg(not(any(windows, target_os = "macos")))]
fn report(message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    // `commit` stays false throughout: a commit writes the real config file.

    #[test]
    fn tweaks_clamp_and_report_what_changed() {
        let mut cfg = Config::default();
        assert_eq!(tweak(&mut cfg, ui::SPEED, 9.0, false), Changed::default());
        assert_eq!(cfg.general.speed, 4.0);
        let c = tweak(&mut cfg, ui::FPS, 75.4, false);
        assert!(c.frame_rate && cfg.general.fps == 60);
        assert!(tweak(&mut cfg, ui::GLASS, 2.0, false).colors);
        assert_eq!(cfg.theme.glass, 0.9);
        assert!(tweak(&mut cfg, ui::SCALE, 1.5, false).font);
        // The same scale again: no font rebuild.
        assert!(!tweak(&mut cfg, ui::SCALE, 1.5, false).font);
        assert!(tweak(&mut cfg, ui::OBJECT_SCALE, 0.1, false).layout);
        assert_eq!(cfg.general.object_scale, 0.5);
        assert!(tweak(&mut cfg, ui::CLOCK, 1.0, false).frame);
        assert!(cfg.general.clock);
        // What the settings window reads back is what was set.
        assert_eq!(tweak_value(&cfg, ui::SPEED), 4.0);
        assert_eq!(tweak_value(&cfg, ui::FPS), 60.0);
        assert_eq!(tweak_value(&cfg, ui::CLOCK), 1.0);
        assert_eq!(tweak(&mut cfg, 99, 1.0, false), Changed::default());
    }

    #[test]
    fn reload_reports_only_what_differs() {
        let old = Config::default();
        let same = reloaded(&old, &old.clone());
        assert_eq!(
            same,
            Changed {
                colors: true,
                ..Changed::default()
            }
        );
        let mut new = old.clone();
        new.general.animation = "topo".into();
        new.general.scale = 1.5;
        new.general.match_menu_bar = true;
        new.general.lock_screen = !old.general.lock_screen;
        let c = reloaded(&old, &new);
        assert!(c.animation && c.frame_rate && c.font && c.menu_bar && c.lock_screen);
        assert!(!c.layout && !c.frame);
        new = old.clone();
        new.theme.ramp = " .#".into();
        assert!(
            reloaded(&old, &new).animation,
            "a new ramp rebuilds the animation"
        );
    }

    #[test]
    fn frame_rate_status_and_decimal() {
        let mut cfg = Config::default();
        assert_eq!(frame_rate(&cfg, 24), 24);
        cfg.general.fps = 15;
        assert_eq!(frame_rate(&cfg, 24), 15);
        cfg.general.fps = 0;
        assert_eq!(frame_rate(&cfg, 0), 1, "never zero");
        cfg.general.animation = "cubes".into();
        cfg.theme.name = "mono".into();
        assert_eq!(status(&cfg, false, None), "Glyphdrift: Cubes · Mono");
        assert_eq!(status(&cfg, true, Some("x")), "Glyphdrift: paused");
        assert_eq!(
            status(&cfg, false, Some("Low Power Mode is on")),
            "Glyphdrift: paused while Low Power Mode is on"
        );
        assert_eq!(decimal(0.3), 0.3);
    }
}
