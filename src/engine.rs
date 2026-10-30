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
    /// Show on lock screen was turned on or off.
    pub lock_screen: bool,
    /// The settings window's style or appearance: redraw it.
    pub settings_look: bool,
}

/// A setting as the settings window shows it.
pub fn tweak_value(cfg: &Config, id: usize) -> f32 {
    let g = &cfg.general;
    let on = |b: bool| f32::from(u8::from(b));
    match id {
        ui::SPEED => g.speed,
        ui::FPS => g.fps as f32,
        ui::GLASS => cfg.theme.glass,
        ui::SCALE => g.scale,
        ui::OBJECT_SCALE => g.object_scale,
        ui::CLOCK => on(g.clock),
        ui::LOCK_SCREEN => on(g.lock_screen),
        ui::PAUSE_FULLSCREEN => on(g.pause_on_fullscreen),
        ui::PAUSE_BATTERY => on(g.pause_on_battery_saver),
        ui::STYLE => index(&crate::bench::STYLE_NAMES, &cfg.settings.style),
        ui::APPEARANCE => index(&crate::bench::APPEARANCE_NAMES, &cfg.settings.appearance),
        _ => 0.0,
    }
}

/// Position of `name` in `names`, as a setting value; unknown names give 0.
fn index(names: &[&str], name: &str) -> f32 {
    names.iter().position(|n| *n == name).unwrap_or(0) as f32
}

/// Apply a setting from the settings window or menu, clamped; with `commit` (a slider
/// released, a box ticked) save it too.
pub fn tweak(cfg: &mut Config, id: usize, value: f32, commit: bool) -> Changed {
    let mut changed = Changed::default();
    let (lo, hi) = ui::range(id);
    let v = value.clamp(lo, hi);
    let on = value > 0.5;
    let g = &mut cfg.general;
    match id {
        ui::SPEED => g.speed = v,
        ui::FPS => {
            g.fps = v.round() as u32;
            changed.frame_rate = true;
        }
        ui::GLASS => {
            cfg.theme.glass = v;
            changed.colors = true;
        }
        ui::SCALE => {
            changed.font = v != g.scale; // the font size follows the scale
            g.scale = v;
        }
        ui::OBJECT_SCALE => {
            changed.layout = v != g.object_scale;
            g.object_scale = v;
        }
        ui::CLOCK => {
            g.clock = on;
            changed.frame = true;
        }
        ui::LOCK_SCREEN => {
            g.lock_screen = on;
            changed.lock_screen = true;
        }
        // Read on the next once-a-second pause check.
        ui::PAUSE_FULLSCREEN => g.pause_on_fullscreen = on,
        ui::PAUSE_BATTERY => g.pause_on_battery_saver = on,
        ui::STYLE | ui::APPEARANCE => {
            let (names, field): (&[&str], _) = if id == ui::STYLE {
                (&crate::bench::STYLE_NAMES, &mut cfg.settings.style)
            } else {
                (
                    &crate::bench::APPEARANCE_NAMES,
                    &mut cfg.settings.appearance,
                )
            };
            *field = names[(v.round() as usize).min(names.len() - 1)].into();
            changed.settings_look = true;
        }
        _ => return changed,
    }
    if commit {
        save_tweaks(cfg, &[id]);
    }
    changed
}

/// Where a setting is saved, and its current value.
fn entry(cfg: &Config, id: usize) -> Option<(&'static str, &'static str, toml_edit::Value)> {
    let g = &cfg.general;
    Some(match id {
        ui::SPEED => ("general", "speed", decimal(g.speed).into()),
        ui::FPS => ("general", "fps", (g.fps as i64).into()),
        ui::GLASS => ("theme", "glass", decimal(cfg.theme.glass).into()),
        ui::SCALE => ("general", "scale", decimal(g.scale).into()),
        ui::OBJECT_SCALE => ("general", "object_scale", decimal(g.object_scale).into()),
        ui::CLOCK => ("general", "clock", g.clock.into()),
        ui::LOCK_SCREEN => ("general", "lock_screen", g.lock_screen.into()),
        ui::PAUSE_FULLSCREEN => (
            "general",
            "pause_on_fullscreen",
            g.pause_on_fullscreen.into(),
        ),
        ui::PAUSE_BATTERY => (
            "general",
            "pause_on_battery_saver",
            g.pause_on_battery_saver.into(),
        ),
        ui::STYLE => ("settings", "style", cfg.settings.style.as_str().into()),
        ui::APPEARANCE => (
            "settings",
            "appearance",
            cfg.settings.appearance.as_str().into(),
        ),
        _ => return None,
    })
}

/// Save the current value of each setting in `ids`, in one write.
pub fn save_tweaks(cfg: &Config, ids: &[usize]) {
    let entries: Vec<_> = ids.iter().filter_map(|&id| entry(cfg, id)).collect();
    if let Err(e) = config::save(&entries) {
        report(&format!("config: could not save settings: {e}"));
    }
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
        settings_look: (&old.settings.style, &old.settings.appearance)
            != (&new.settings.style, &new.settings.appearance),
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

/// The grid font's size in points: the config's, times the overall scale.
pub fn font_size(cfg: &Config) -> f64 {
    (cfg.font.size as f64 * cfg.general.scale as f64)
        .round()
        .max(4.0)
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
    if let Err(e) = config::save(&[(section, key, value.into())]) {
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
        assert!(tweak(&mut cfg, ui::PAUSE_BATTERY, 0.0, false) == Changed::default());
        assert_eq!(tweak_value(&cfg, ui::PAUSE_BATTERY), 0.0);
        // Every setting the window shows has a place in the config.
        assert!(tweak(&mut cfg, ui::STYLE, 1.0, false).settings_look);
        assert_eq!(cfg.settings.style, "instrument");
        tweak(&mut cfg, ui::APPEARANCE, 7.0, false);
        assert_eq!(
            cfg.settings.appearance, "dark",
            "clamped to the last choice"
        );
        assert_eq!(tweak_value(&cfg, ui::APPEARANCE), 2.0);
        for id in 0..=ui::APPEARANCE {
            assert!(entry(&cfg, id).is_some(), "setting {id} is never saved");
        }
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
