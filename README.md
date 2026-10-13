# Glyphdrift

A live wallpaper for Windows 10/11 and macOS 12+: contour maps, ASCII art and geometric animations that move slowly behind your desktop icons.

**[glyphdrift.com](https://glyphdrift.com)** · **[Download the latest release](https://github.com/Sectr99/GlyphDrift/releases/latest)**

- Small: one portable exe (about 1 MB) on Windows and a universal app (about 1.4 MB zipped) on macOS. No installer, no admin rights, and it never connects to the internet.
- 23 animations: Matrix rain, Plasma, Fire, Waves, Ripples, Wind (ASCII); Constellation, Flow field (graphics); Helix, Wire globe, Dot wave, Cubes, Sunburst, Moon (geometric loops); Lava lamp, Ink in water (fluid); Topographic drift, Ridgelines, Isobars, Wood grain, Interference, Hillshade topo, Dune ripples (contour).
- 22 themes: Matrix, Mono, Amber CRT; Catppuccin Mocha, Tokyo Night, Nord, Gruvbox Dark, Dracula, Rosé Pine, Kanagawa, One Dark, Solarized Dark, Everforest, Monokai, Ayu Dark, Night Owl, Palenight, GitHub Dark, Oxocarbon, Poimandres; Synthwave '84, Claude.
- Also as a screen saver, and as a still frame on the lock screen.
- Pauses while a fullscreen app is open or battery saver is on (Windows), or while the desktop is covered or Low Power Mode is on (macOS).
- Low CPU use: on an 8-thread CPU at 3440×1440, the ASCII and loop animations use under 1% and the contour and fluid ones 1 to 3% (Dune ripples about 4.5%), with 45 to 70 MB of memory, most of it the GPU driver. Smaller overall scales draw more detail and cost more.

## Use

Run `glyphdrift.exe`. Left-click its tray icon (or run it again) to open **Settings**, which opens beside the tray so the wallpaper stays visible as a live preview:

- **Animation**, grouped (ASCII, Graphics, Loops, Fluid, Contour)
- **Theme**, with swatches of each theme's colors
- Sliders for **speed**, **icon readability** (fades the animation so icon labels stay legible), **overall scale** and **object scale**, plus a **frame rate** choice (Auto, 15 to 60 fps)
- **Show clock**: time and date (your regional format) in a framed box at the center of Topographic drift; the contour lines flow around it
- **Show on lock screen** (off by default): sets the lock screen picture to a still frame of the current animation and theme, without the clock. It's redrawn when you change animation, theme or scale, and every 5 minutes. Your own picture is saved first and comes back when you turn this off. Windows doesn't let apps animate the lock screen; the screen saver is the way to see it moving

Hover over any setting for a short explanation. Controls that don't apply to the current animation are greyed out. Right-click the tray icon for quick actions:

| Menu | What it does |
|---|---|
| Settings… | Open the settings window |
| Animation | Switch animation |
| Show clock | Toggle the Topographic drift clock |
| Show on lock screen | Toggle the lock screen frame |
| Install screen saver… | Install Glyphdrift as your screen saver (see below) |
| Start with Windows | Run at sign-in (also listed in Settings > Apps > Startup) |
| Pause / Resume | Stop or restart the animation. While paused (by you, a fullscreen app or battery saver) the icon turns grey and its hover text says why |
| Quit | Exit and restore the normal wallpaper |

Changes apply immediately and are remembered.

**Screen saver:** **Install screen saver…** copies Glyphdrift to `%LOCALAPPDATA%\Glyphdrift\Glyphdrift.scr` and opens Screen Saver Settings with it selected; click **OK** to keep it. It shows the animation and theme set in Glyphdrift on all screens, and any key, click or mouse movement ends it. Tick **On resume, display logon screen** there to go from the animation straight to the lock screen. Each Glyphdrift update brings the screen saver up to date the next time Glyphdrift starts.

## macOS

Unzip `Glyphdrift-macos.zip` from the [latest release](https://github.com/Sectr99/GlyphDrift/releases/latest) and move **Glyphdrift.app** to Applications. It isn't notarized yet, so macOS blocks the first launch: open **System Settings > Privacy & Security**, scroll down and click **Open Anyway**. It lives in the menu bar, with no Dock icon:

- The menu bar icon's menu has **Settings…**, **Animation**, **Theme**, **Show clock**, **Match Menu Bar to Theme**, **Show on Lock Screen**, **Install Screen Saver…**, **Open at Login**, **Pause / Resume** and **Quit**. Opening the app again also opens Settings.
- macOS draws a pale frosted strip behind the menu bar wherever a window, including the wallpaper, lies under it. **Match menu bar to theme** (off by default) stops the wallpaper below the menu bar and sets the desktop picture to the theme's background color, so the menu bar sits on the theme. Your own picture is saved first and comes back when you turn the option off or quit, or on the next launch after a crash. macOS keeps a picture per Space and an app can only set the current one: other Spaces switch within a second of visiting them, and quitting restores the Space you're on. On macOS 26 you can instead turn off **System Settings > Menu Bar > Show menu bar background**.
- **Show on lock screen** (on by default): the lock screen shows the desktop picture, not app windows, so Glyphdrift sets the picture to a still frame of the current animation and theme (without the clock, which would go stale). It's redrawn when you change animation, theme or scale, and every 5 minutes. Your own picture is saved first and comes back when you turn it off or quit, as with Match menu bar to theme; with both on, the frame gets a strip of the theme color behind the menu bar. An *animated* lock screen isn't possible with Apple's public APIs; the screen saver is the way to see it moving while the Mac is locked.
- **Screen saver:** **Install Screen Saver…** in the menu installs `Glyphdrift.saver`, which comes inside the app; macOS asks whether to install it for you or for all users, then opens Screen Saver settings, where you pick **Glyphdrift**. It shows the animation, theme and clock set in the app (read each time it starts), on every screen. Once it's installed for you, each app update brings the saver up to date too. With **Require password after screen saver begins** on, it keeps running behind the lock until you unlock.
- Settings has the same controls as on Windows and opens at the top right, so the wallpaper stays visible.
- Each screen gets its own window, so centered animations center on every screen. A screen that's fully covered (a fullscreen app, a locked screen) stops drawing; when all are covered, or Low Power Mode is on, the wallpaper pauses and the icon dims.
- Settings live in `~/Library/Application Support/Glyphdrift/config.toml` (same format; the default font is Menlo). **Open at Login** adds `~/Library/LaunchAgents/com.sectr99.glyphdrift.plist`.

To uninstall: untick **Open at Login**, **Quit**, then delete the app, `~/Library/Application Support/Glyphdrift` and, if you installed it, `~/Library/Screen Savers/Glyphdrift.saver`.

## Configuration

Settings live in `%APPDATA%\Glyphdrift\config.toml`, created on first launch. You can also edit it by hand; changes apply within a second without a restart. Invalid values fall back to defaults instead of stopping the app. See [`config.example.toml`](config.example.toml) for every option (fps, font, character ramp, pause behavior).

## Uninstall

1. In the tray menu, untick **Show on lock screen** (to get your own picture back) and **Start with Windows**, then **Quit**.
2. If you installed the screen saver, pick another one (or None) in Screen Saver Settings.
3. Delete `glyphdrift.exe`, the `%APPDATA%\Glyphdrift` folder and, if it exists, `%LOCALAPPDATA%\Glyphdrift`.

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
macos/bundle.sh            # dist/Glyphdrift.app (with Glyphdrift.saver inside) and a zip, universal (needs both macOS targets)
macos/bundle.sh --native   # this Mac's architecture only
cargo test --release snapshots -- --ignored --nocapture   # every animation through Metal, to target/snapshots
cargo run --release -- --screenshots <dir>                # website images, drawn offscreen (see below)
```

The website's gallery runs the real animations: `web/` compiles them to WebAssembly, and `web/build.sh` writes `site/glyphdrift.wasm` (commit it; the site has no build step). After changing animations, `web/build-assets.sh <name> ...` rebuilds that file and those animations' gallery and viewer images in one go (no names rebuilds every image). Name only the animations that changed: each image regeneration adds to the git history.

`--screenshots` writes a gallery image of every animation, the Settings window, and the Settings window over Cubes. Everything is drawn offscreen with built-in settings and a fixed clock: nothing is read from the screen, your config or your desktop, and a running Glyphdrift is left alone.

To check the Windows build from a Mac: `rustup target add x86_64-pc-windows-msvc`, then `cargo clippy --target x86_64-pc-windows-msvc --all-targets`.

Design notes are in [`DESIGN.md`](DESIGN.md).
