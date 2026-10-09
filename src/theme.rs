//! Built-in color themes: terminal classics, the published palettes of popular editor
//! themes, and a couple of neon ones.
//!
//! Each theme has a background plus two three-stop gradients (dim to bright): `warm` for
//! fire-like animations and `cool` for everything else. The renderer blends from the
//! background through the stops, so low-brightness cells fade into the background.

use crate::config::Rgb;

pub struct Theme {
    pub name: &'static str,
    pub label: &'static str,
    /// Menu group; the tray separates groups.
    pub group: &'static str,
    pub bg: Rgb,
    pub cool: [Rgb; 3],
    pub warm: [Rgb; 3],
}

const fn hex(v: u32) -> Rgb {
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

macro_rules! theme {
    ($group:literal, $name:literal, $label:literal, $bg:literal, [$($c:literal),*], [$($w:literal),*]) => {
        Theme {
            name: $name,
            label: $label,
            group: $group,
            bg: hex($bg),
            cool: [$(hex($c)),*],
            warm: [$(hex($w)),*],
        }
    };
}

pub const THEMES: &[Theme] = &[
    // Green phosphor.
    theme!(
        "terminal",
        "matrix",
        "Matrix",
        0x000000,
        [0x005522, 0x00aa44, 0x00ff66],
        [0x7a1f00, 0xff6a00, 0xffd75e]
    ),
    // Pure black and white.
    theme!(
        "terminal",
        "mono",
        "Mono",
        0x000000,
        [0x3c3c3c, 0x9a9a9a, 0xffffff],
        [0x3c3c3c, 0x9a9a9a, 0xffffff]
    ),
    // Amber phosphor terminal.
    theme!(
        "terminal",
        "amber",
        "Amber CRT",
        0x0a0600,
        [0x5a3200, 0xc77800, 0xffb000],
        [0x7a2400, 0xe06800, 0xffc040]
    ),
    // base | blue, sapphire, sky | red, peach, yellow
    theme!(
        "editor",
        "catppuccin-mocha",
        "Catppuccin Mocha",
        0x1e1e2e,
        [0x89b4fa, 0x74c7ec, 0x89dceb],
        [0xf38ba8, 0xfab387, 0xf9e2af]
    ),
    // bg | blue, cyan, fg | red, orange, yellow
    theme!(
        "editor",
        "tokyo-night",
        "Tokyo Night",
        0x1a1b26,
        [0x7aa2f7, 0x7dcfff, 0xc0caf5],
        [0xf7768e, 0xff9e64, 0xe0af68]
    ),
    // nord0 | nord10, nord9, nord8 | nord11, nord12, nord13
    theme!(
        "editor",
        "nord",
        "Nord",
        0x2e3440,
        [0x5e81ac, 0x81a1c1, 0x88c0d0],
        [0xbf616a, 0xd08770, 0xebcb8b]
    ),
    // bg | blue, bright blue, bright aqua | red, bright orange, bright yellow
    theme!(
        "editor",
        "gruvbox",
        "Gruvbox Dark",
        0x282828,
        [0x458588, 0x83a598, 0x8ec07c],
        [0xcc241d, 0xfe8019, 0xfabd2f]
    ),
    // bg | comment, purple, cyan | red, orange, yellow
    theme!(
        "editor",
        "dracula",
        "Dracula",
        0x282a36,
        [0x6272a4, 0xbd93f9, 0x8be9fd],
        [0xff5555, 0xffb86c, 0xf1fa8c]
    ),
    // base | pine, foam, text | love, rose, gold
    theme!(
        "editor",
        "rose-pine",
        "Rosé Pine",
        0x191724,
        [0x31748f, 0x9ccfd8, 0xe0def4],
        [0xeb6f92, 0xebbcba, 0xf6c177]
    ),
    // sumiInk1 | crystalBlue, springBlue, fujiWhite | autumnRed, surimiOrange, carpYellow
    theme!(
        "editor",
        "kanagawa",
        "Kanagawa",
        0x1f1f28,
        [0x7e9cd8, 0x7fb4ca, 0xdcd7ba],
        [0xc34043, 0xffa066, 0xe6c384]
    ),
    // bg | purple, blue, cyan | red, orange, yellow
    theme!(
        "editor",
        "one-dark",
        "One Dark",
        0x282c34,
        [0xc678dd, 0x61afef, 0x56b6c2],
        [0xe06c75, 0xd19a66, 0xe5c07b]
    ),
    // base03 | blue, cyan, base1 | red, orange, yellow
    theme!(
        "editor",
        "solarized-dark",
        "Solarized Dark",
        0x002b36,
        [0x268bd2, 0x2aa198, 0x93a1a1],
        [0xdc322f, 0xcb4b16, 0xb58900]
    ),
    // bg | blue, aqua, fg | red, orange, yellow
    theme!(
        "editor",
        "everforest",
        "Everforest",
        0x2d353b,
        [0x7fbbb3, 0x83c092, 0xd3c6aa],
        [0xe67e80, 0xe69875, 0xdbbc7f]
    ),
    // bg | purple, blue, green | pink, orange, yellow
    theme!(
        "editor",
        "monokai",
        "Monokai",
        0x272822,
        [0xae81ff, 0x66d9ef, 0xa6e22e],
        [0xf92672, 0xfd971f, 0xe6db74]
    ),
    // bg | entity blue, regexp cyan, fg | markup red, keyword orange, func yellow
    theme!(
        "editor",
        "ayu-dark",
        "Ayu Dark",
        0x0b0e14,
        [0x39bae6, 0x95e6cb, 0xbfbdb6],
        [0xf07178, 0xff8f40, 0xffb454]
    ),
    // bg | blue, cyan, fg | red, orange, yellow
    theme!(
        "editor",
        "night-owl",
        "Night Owl",
        0x011627,
        [0x82aaff, 0x7fdbca, 0xd6deeb],
        [0xef5350, 0xf78c6c, 0xecc48d]
    ),
    // bg | purple, blue, cyan | red, orange, yellow
    theme!(
        "editor",
        "palenight",
        "Palenight",
        0x292d3e,
        [0xc792ea, 0x82aaff, 0x89ddff],
        [0xf07178, 0xf78c6c, 0xffcb6b]
    ),
    // canvas | blue, light blue, fg | red, orange, yellow
    theme!(
        "editor",
        "github-dark",
        "GitHub Dark",
        0x0d1117,
        [0x58a6ff, 0x79c0ff, 0xc9d1d9],
        [0xff7b72, 0xffa657, 0xd29922]
    ),
    // bg | blue, sky, cyan | magenta, pink, purple (no orange in the palette)
    theme!(
        "editor",
        "oxocarbon",
        "Oxocarbon",
        0x161616,
        [0x78a9ff, 0x33b1ff, 0x3ddbd9],
        [0xee5396, 0xff7eb6, 0xbe95ff]
    ),
    // bg | teal, cyan, blue | red, pink, yellow
    theme!(
        "editor",
        "poimandres",
        "Poimandres",
        0x1b1e28,
        [0x5de4c7, 0x89ddff, 0xadd7ff],
        [0xd0679d, 0xfcc5e9, 0xfffac2]
    ),
    // bg | purple, pink, cyan | red, orange, yellow
    theme!(
        "neon",
        "synthwave-84",
        "Synthwave '84",
        0x262335,
        [0xb893ce, 0xff7edb, 0x36f9f6],
        [0xfe4450, 0xf97e72, 0xfede5d]
    ),
    // slate | book cloth, coral, ivory | error red, coral, manilla
    theme!(
        "neon",
        "claude",
        "Claude",
        0x262624,
        [0xcc785c, 0xd97757, 0xf0efea],
        [0xbf4d43, 0xd97757, 0xebdbbc]
    ),
];

/// Glass veil presets offered in the tray: (opacity, menu label).
pub const GLASS: &[(f32, &str)] = &[
    (0.0, "Off"),
    (0.3, "Light"),
    (0.5, "Medium"),
    (0.7, "Strong"),
];

/// Blend `a` toward `b` by `f` (0 = a, 1 = b).
pub fn mix(a: Rgb, b: Rgb, f: f32) -> Rgb {
    let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// `c` seen through a frosted veil of opacity `glass` over background `bg`: the veil is
/// the background lifted slightly toward white, which reads as a milky haze. Because the
/// background is solid this equals drawing a translucent layer on top, at no cost.
pub fn frost(c: Rgb, bg: Rgb, glass: f32) -> Rgb {
    mix(c, mix(bg, Rgb(255, 255, 255), 0.08), glass)
}

/// Index into `THEMES`; unknown names give 0 (matrix).
pub fn index_of(name: &str) -> usize {
    THEMES.iter().position(|t| t.name == name).unwrap_or(0)
}

/// `levels` colors running from just above `bg` up to the last stop.
pub fn gradient(bg: Rgb, stops: &[Rgb; 3], levels: usize) -> Vec<Rgb> {
    let points = [bg, stops[0], stops[1], stops[2]];
    let segs = (points.len() - 1) as f32;
    (1..=levels)
        .map(|i| {
            let t = i as f32 / levels as f32 * segs;
            let k = (t.ceil() as usize).clamp(1, points.len() - 1);
            mix(points[k - 1], points[k], t - (k - 1) as f32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_unique_and_lookup() {
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(index_of(t.name), i, "duplicate theme name {}", t.name);
        }
        assert_eq!(index_of("nope"), 0);
    }

    #[test]
    fn frost_flattens_contrast() {
        let (bg, fg) = (Rgb(0, 0, 0), Rgb(200, 200, 200));
        assert_eq!(frost(fg, bg, 0.0), fg);
        let (f, b) = (frost(fg, bg, 0.5), frost(bg, bg, 0.5));
        assert!(f.0 - b.0 < fg.0 - bg.0 && b.0 > 0, "{f:?} {b:?}");
    }

    #[test]
    fn gradient_hits_stops_and_ends_bright() {
        let g = gradient(
            Rgb(0, 0, 0),
            &[hex(0x300000), hex(0x600000), hex(0x900000)],
            6,
        );
        assert_eq!(g.len(), 6);
        assert_eq!(g[1], hex(0x300000)); // t = 1/3
        assert_eq!(g[3], hex(0x600000)); // t = 2/3
        assert_eq!(g[5], hex(0x900000)); // t = 1
        assert!(
            g.windows(2).all(|w| w[0].0 < w[1].0),
            "not increasing: {g:?}"
        );
    }
}
