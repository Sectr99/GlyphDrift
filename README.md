# ASCII Wallpaper

A lightweight live wallpaper for Windows 10/11 that draws ASCII animations behind your desktop icons.

- Single portable exe (~0.7 MB), no installer, no admin rights, no network access
- Animations: Matrix rain, Plasma, Fire, Waves, Ripples, Wind (ASCII); Constellation, Flow field (vector graphics); Helix, Wire globe, Dot wave, Cubes, Striped orb, Sunburst, Moon, Skull (minimal geometric loops); Lava lamp, Ink in water (fluid); Topographic drift, Ridgelines, Isobars, Wood grain, Interference (contour)
- Themes (22): Matrix, Mono, Amber CRT; Catppuccin Mocha, Tokyo Night, Nord, Gruvbox Dark, Dracula, Rosé Pine, Kanagawa, One Dark, Solarized Dark, Everforest, Monokai, Ayu Dark, Night Owl, Palenight, GitHub Dark, Oxocarbon, Poimandres; Synthwave '84, Claude
- Pauses automatically for fullscreen apps and Windows battery saver
- About 0.3–0.7% CPU at the default frame rate, under 2% at 60 FPS; around 45 MB of memory, most of it the GPU driver

## Use

Run `ascii-wallpaper.exe`. Everything is controlled from its tray icon (right-click):

| Menu | What it does |
|---|---|
| Animation | Switch animation |
| Theme | Switch color theme |
| Speed | 0.25× to 2× |
| Frame rate | Auto (24 FPS for ASCII, 30 for graphics), 30 or 60 FPS; higher is smoother but uses more CPU |
| Glass | Frosted veil over the animation (Off / Light / Medium / Strong) so icon labels stay readable |
| Start with Windows | Run at sign-in (also listed in Settings > Apps > Startup) |
| Pause / Resume | Stop or restart the animation |
| Quit | Exit and restore the normal wallpaper |

Changes apply immediately and are remembered.

## Configuration

Settings live in `%APPDATA%\ascii-wallpaper\config.toml`, created when you first change something from the tray. You can also edit it by hand; changes apply within a second, no restart needed. Invalid values fall back to defaults instead of stopping the app. See [`config.example.toml`](config.example.toml) for every option (fps, font, character ramp, pause behavior).

## Uninstall

1. Untick **Start with Windows** in the tray menu, then **Quit**.
2. Delete `ascii-wallpaper.exe` and the `%APPDATA%\ascii-wallpaper` folder.

## Build

Requires Rust with the MSVC toolchain (`x86_64-pc-windows-msvc`).

```
cargo build --release      # target\release\ascii-wallpaper.exe, no console window
cargo run                  # debug build with a console for log output; Ctrl+C quits
cargo test                 # portable core (animations, themes, config); runs on any OS
cargo clippy --all-targets
```

Design notes are in [`DESIGN.md`](DESIGN.md).
