//! Settings shared by every platform's settings window and menu: tweak ids, slider ranges,
//! hover help and the grouped animation and theme lists.

use crate::anim::ANIMATIONS;
use crate::theme::THEMES;

pub const SPEED: usize = 0;
pub const FPS: usize = 1;
pub const GLASS: usize = 2;
pub const SCALE: usize = 3;
pub const OBJECT_SCALE: usize = 4;
/// Clock on/off (1 or 0).
pub const CLOCK: usize = 5;
/// Show on lock screen on/off (1 or 0).
pub const LOCK_SCREEN: usize = 6;
/// Pause while a fullscreen app is open, on/off (1 or 0).
pub const PAUSE_FULLSCREEN: usize = 7;
/// Pause while battery saver is on, on/off (1 or 0).
pub const PAUSE_BATTERY: usize = 8;
/// The settings window's Bench style: an index into `STYLES`.
pub const STYLE: usize = 9;
/// The settings window's appearance: an index into `APPEARANCES`.
pub const APPEARANCE: usize = 10;

/// Style choices, in the order of `bench::STYLE_NAMES`.
pub const STYLES: [&str; 2] = ["Native", "Instrument"];
/// Appearance choices, in the order of `bench::APPEARANCE_NAMES`.
pub const APPEARANCES: [&str; 3] = ["Match system", "Light", "Dark"];

/// The allowed range of a setting; toggles are 0 to 1.
pub fn range(id: usize) -> (f32, f32) {
    match id {
        SPEED => (0.1, 4.0),
        FPS => (0.0, 60.0),
        GLASS => (0.0, 0.9),
        SCALE | OBJECT_SCALE => (0.5, 2.0),
        APPEARANCE => (0.0, 2.0),
        _ => (0.0, 1.0),
    }
}

/// (tweak id, label, slider min, slider max, default value); min and max are `range` in
/// hundredths.
pub const SLIDERS: [(usize, &str, i32, i32, f32); 4] = [
    (SPEED, "Speed", 10, 400, 1.0),
    (GLASS, "Icon readability", 0, 90, 0.0),
    (SCALE, "Overall scale", 50, 200, 1.0),
    (OBJECT_SCALE, "Object scale", 50, 200, 1.0),
];

/// (fps, label); 0 = each animation's own rate.
pub const FRAME_RATES: [(f32, &str); 5] = [
    (0.0, "Auto (recommended)"),
    (15.0, "15 fps (lightest)"),
    (24.0, "24 fps"),
    (30.0, "30 fps"),
    (60.0, "60 fps (smoothest)"),
];

/// Hover help per setting.
pub fn hint(id: usize) -> &'static str {
    match id {
        SPEED => "How fast the animation moves.",
        GLASS => {
            "Fades the animation toward the background color so desktop icon labels stay \
             easy to read."
        }
        SCALE => "Size of everything: ASCII characters, line weights and terrain features.",
        OBJECT_SCALE => {
            "Size of the centered shape in the Loops animations, on top of the overall scale."
        }
        FPS => {
            "Auto uses each animation's own rate (20 to 30 fps). Higher is smoother but uses \
             more CPU."
        }
        CLOCK => {
            "Time and date at the center of Topographic drift; the contour lines flow \
             around it."
        }
        LOCK_SCREEN => {
            "Sets the lock screen picture to a still frame of the animation, updated every few \
             minutes. Turning it off brings back your own picture."
        }
        PAUSE_FULLSCREEN => "Pauses while a game, video or presentation fills the screen.",
        PAUSE_BATTERY => "Pauses while battery saver is on.",
        STYLE => {
            "How this window looks: Native fits in with the system; Instrument is darker and \
             denser."
        }
        APPEARANCE => "Light or dark, or the same as the system.",
        _ => "",
    }
}

/// Index of the choice nearest `fps`.
pub fn frame_rate_index(fps: f32) -> usize {
    (0..FRAME_RATES.len())
        .min_by(|&a, &b| {
            let d = |i: usize| (FRAME_RATES[i].0 - fps).abs();
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(0)
}

/// Sliders work in hundredths.
pub fn slider_pos(value: f32) -> i32 {
    (value * 100.0).round() as i32
}

pub fn slider_value(pos: i32) -> f32 {
    pos as f32 / 100.0
}

pub fn value_label(id: usize, value: f32) -> String {
    match id {
        SPEED => format!("{value:.2}×"),
        _ => format!("{:.0}%", value * 100.0),
    }
}

/// A row of a grouped list (the animation dropdown, the theme list).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    /// A group heading, not selectable.
    Heading(&'static str),
    /// An index into `ANIMATIONS` or `THEMES`.
    Item(usize),
}

/// A heading wherever the group changes, then the items.
pub fn grouped_rows(groups: impl Iterator<Item = &'static str>) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut last = None;
    for (i, group) in groups.enumerate() {
        if last != Some(group) {
            rows.push(Row::Heading(match group {
                "ascii" => "ASCII",
                "graphics" => "Graphics",
                "loops" => "Loops",
                "fluid" => "Fluid",
                "contour" => "Contour",
                "terminal" => "Terminal",
                "editor" => "Editor",
                "neon" => "Neon",
                other => other,
            }));
            last = Some(group);
        }
        rows.push(Row::Item(i));
    }
    rows
}

pub fn anim_rows() -> Vec<Row> {
    grouped_rows(ANIMATIONS.iter().map(|a| a.2))
}

pub fn theme_rows() -> Vec<Row> {
    grouped_rows(THEMES.iter().map(|t| t.group))
}

pub fn row_of(rows: &[Row], item: usize) -> usize {
    rows.iter().position(|&r| r == Row::Item(item)).unwrap_or(1)
}

/// Nearest item row, searching in the direction of travel first.
pub fn nearest_item_row(rows: &[Row], row: usize, up: bool) -> usize {
    let is_item = |i: &usize| matches!(rows.get(*i), Some(Row::Item(_)));
    let below = || (row..rows.len()).find(is_item);
    let above = || (0..=row.min(rows.len() - 1)).rev().find(is_item);
    let found = if up {
        above().or_else(below)
    } else {
        below().or_else(above)
    };
    found.unwrap_or(row)
}

/// Which optional controls apply: (object scale, clock).
pub fn applies(anim: usize) -> (bool, bool) {
    let (name, _, group) = ANIMATIONS.get(anim).copied().unwrap_or(ANIMATIONS[0]);
    (group == "loops", name == "topo")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_positions_round_trip() {
        for (id, _, min, max, default) in SLIDERS {
            assert!(
                (min..=max).contains(&slider_pos(default)),
                "default out of range for {id}"
            );
            for pos in min..=max {
                assert_eq!(
                    slider_pos(slider_value(pos)),
                    pos,
                    "slider {id} position {pos}"
                );
            }
        }
        for (id, _, min, max, _) in SLIDERS {
            let (lo, hi) = range(id);
            assert_eq!(
                (min, max),
                (slider_pos(lo), slider_pos(hi)),
                "slider {id} range"
            );
        }
        assert_eq!(FRAME_RATES[frame_rate_index(0.0)].0, 0.0);
        assert_eq!(FRAME_RATES[frame_rate_index(45.0)].0, 30.0); // ties go to the lower
        assert_eq!(FRAME_RATES[frame_rate_index(100.0)].0, 60.0);
    }

    #[test]
    fn grouped_lists_have_every_item_once() {
        for (rows, n, first) in [
            (anim_rows(), ANIMATIONS.len(), "ASCII"),
            (theme_rows(), THEMES.len(), "Terminal"),
        ] {
            assert_eq!(rows[0], Row::Heading(first));
            assert_eq!(rows.iter().filter(|r| matches!(r, Row::Item(_))).count(), n);
            for i in 0..n {
                assert_eq!(rows[row_of(&rows, i)], Row::Item(i));
            }
        }
    }

    #[test]
    fn headings_are_skipped() {
        let rows = anim_rows();
        let second = rows
            .iter()
            .skip(1)
            .position(|r| matches!(r, Row::Heading(_)))
            .unwrap()
            + 1;
        // Onto a heading: down goes below it, up goes above.
        assert_eq!(nearest_item_row(&rows, second, false), second + 1);
        assert_eq!(nearest_item_row(&rows, second, true), second - 1);
        // Nowhere to go above the first heading.
        assert_eq!(nearest_item_row(&rows, 0, true), 1);
        assert_eq!(value_label(SCALE, 1.25), "125%");
    }
}
