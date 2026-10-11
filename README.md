# Glyphdrift

A quiet live wallpaper for Windows 10/11 and macOS 12+: contour maps, ASCII and minimal motion graphics drifting behind your desktop icons.

**[glyphdrift.com](https://glyphdrift.com)** · **[Download the latest release](https://github.com/Sectr99/GlyphDrift/releases/latest)**

- Single portable exe (~0.7 MB) on Windows, a small universal app (~0.7 MB zipped) on macOS; no installer, no admin rights, no network access
- Animations: Matrix rain, Plasma, Fire, Waves, Ripples, Wind (ASCII); Constellation, Flow field (vector graphics); Helix, Wire globe, Dot wave, Cubes, Sunburst, Moon (minimal geometric loops); Lava lamp, Ink in water (fluid); Topographic drift, Ridgelines, Isobars, Wood grain, Interference, Hillshade topo, Dune ripples (contour)
- Themes (22): Matrix, Mono, Amber CRT; Catppuccin Mocha, Tokyo Night, Nord, Gruvbox Dark, Dracula, Rosé Pine, Kanagawa, One Dark, Solarized Dark, Everforest, Monokai, Ayu Dark, Night Owl, Palenight, GitHub Dark, Oxocarbon, Poimandres; Synthwave '84, Claude
- Pauses automatically for fullscreen apps and Windows battery saver; on macOS, while the desktop is covered and in Low Power Mode
- Light: on an 8-thread CPU at 3440×1440, the ASCII and loop animations use under 1% CPU and the contour and fluid ones 1–3% (Dune ripples about 4.5%); 45–70 MB of memory, most of it the GPU driver. Smaller overall scales draw more detail and cost more

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

## macOS

Unzip `Glyphdrift-macos.zip` from the [latest release](https://github.com/Sectr99/GlyphDrift/releases/latest) and move **Glyphdrift.app** to Applications. It isn't notarized yet, so the first time, right-click it and choose **Open** (or allow it under System Settings > Privacy & Security). It lives in the menu bar, with no Dock icon:

- The menu bar icon's menu has **Settings…**, **Animation**, **Theme**, **Show clock**, **Open at Login**, **Pause / Resume** and **Quit**. Opening the app again also opens Settings.
- Settings has the same controls as on Windows and opens at the top right, so the wallpaper stays visible.
- Each screen gets its own window, so centered animations center on every screen. A screen that's fully covered (a fullscreen app, a locked screen) stops drawing; when all are covered, or Low Power Mode is on, the wallpaper pauses and the icon dims.
- Settings live in `~/Library/Application Support/Glyphdrift/config.toml` (same format; the default font is Menlo). **Open at Login** adds `~/Library/LaunchAgents/com.sectr99.glyphdrift.plist`.

To uninstall: untick **Open at Login**, **Quit**, then delete the app and `~/Library/Application Support/Glyphdrift`.

## Configuration

Settings live in `%APPDATA%\Glyphdrift\config.toml`, created on first launch. You can also edit it by hand; changes apply within a second, no restart needed. Invalid values fall back to defaults instead of stopping the app. See [`config.example.toml`](config.example.toml) for every option (fps, font, character ramp, pause behavior).

## Uninstall

1. Untick **Start with Windows** in the tray menu, then **Quit**.
2. Delete `glyphdrift.exe` and the `%APPDATA%\Glyphdrift` folder.

## Build

Windows requires Rust with the MSVC toolchain (`x86_64-pc-windows-msvc`).

```
cargo build --release      # target\release\glyphdrift.exe, no console window
cargo run                  # debug build with a console for log output; Ctrl+C quits
cargo test                 # portable core (animations, themes, config); runs on any OS
cargo clippy --all-targets
```

macOS requires Rust and the Xcode Command Line Tools:

```
cargo run                  # run from the terminal; logs to the console
macos/bundle.sh            # dist/Glyphdrift.app and a zip, universal (needs both macOS targets)
macos/bundle.sh --native   # this Mac's architecture only
cargo test --release snapshots -- --ignored --nocapture   # every animation through Metal, to target/snapshots
```

To check the Windows build from a Mac: `rustup target add x86_64-pc-windows-msvc`, then `cargo clippy --target x86_64-pc-windows-msvc --all-targets`.

Design notes are in [`DESIGN.md`](DESIGN.md).
