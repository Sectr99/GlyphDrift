# Glyphdrift

A quiet live wallpaper for Windows 10/11: contour maps, ASCII and minimal motion graphics drifting behind your desktop icons.

**[glyphdrift.com](https://glyphdrift.com)** · **[Download the latest release](https://github.com/Sectr99/GlyphDrift/releases/latest)**

- Single portable exe (~0.7 MB), no installer, no admin rights, no network access
- Animations: Matrix rain, Plasma, Fire, Waves, Ripples, Wind (ASCII); Constellation, Flow field (vector graphics); Helix, Wire globe, Dot wave, Cubes, Sunburst, Moon (minimal geometric loops); Lava lamp, Ink in water (fluid); Topographic drift, Ridgelines, Isobars, Wood grain, Interference, Hillshade topo, Dune ripples (contour)
- Themes (22): Matrix, Mono, Amber CRT; Catppuccin Mocha, Tokyo Night, Nord, Gruvbox Dark, Dracula, Rosé Pine, Kanagawa, One Dark, Solarized Dark, Everforest, Monokai, Ayu Dark, Night Owl, Palenight, GitHub Dark, Oxocarbon, Poimandres; Synthwave '84, Claude
- Pauses automatically for fullscreen apps and Windows battery saver
- About 0.3–0.7% CPU at the default frame rate, under 2% at 60 FPS; around 45 MB of memory, most of it the GPU driver

## Use

Run `glyphdrift.exe`. Left-click its tray icon (or run it again) to open **Settings**, which opens beside the tray so the wallpaper stays visible as a live preview:

- **Animation**, grouped (ASCII, Graphics, Loops, Fluid, Contour)
- **Theme**, with swatches of each theme's colors
- Sliders for **speed**, **icon readability** (fades the animation so icon labels stay legible), **overall scale** and **object scale**, plus a **frame rate** choice (Auto, 15 to 60 fps)
- **Show clock**: time and date (your regional format) in a framed box at the center of Topographic drift; the contour lines flow around it

Hover over any setting for a short explanation. Controls that don't apply to the current animation are greyed out. Right-click the tray icon for quick actions:

| Menu | What it does |
|---|---|
| Settings… | Open the settings window |
| Animation | Switch animation |
| Show clock | Toggle the Topographic drift clock |
| Start with Windows | Run at sign-in (also listed in Settings > Apps > Startup) |
| Pause / Resume | Stop or restart the animation. While paused (by you, a fullscreen app or battery saver) the icon turns grey and its hover text says why |
| Quit | Exit and restore the normal wallpaper |

Changes apply immediately and are remembered.

## Configuration

Settings live in `%APPDATA%\Glyphdrift\config.toml`, created when you first change something in Settings or the tray. You can also edit it by hand; changes apply within a second, no restart needed. Invalid values fall back to defaults instead of stopping the app. See [`config.example.toml`](config.example.toml) for every option (fps, font, character ramp, pause behavior).

## Uninstall

1. Untick **Start with Windows** in the tray menu, then **Quit**.
2. Delete `glyphdrift.exe` and the `%APPDATA%\Glyphdrift` folder.

## Build

Requires Rust with the MSVC toolchain (`x86_64-pc-windows-msvc`).

```
cargo build --release      # target\release\glyphdrift.exe, no console window
cargo run                  # debug build with a console for log output; Ctrl+C quits
cargo test                 # portable core (animations, themes, config); runs on any OS
cargo clippy --all-targets
```

Design notes are in [`DESIGN.md`](DESIGN.md).
