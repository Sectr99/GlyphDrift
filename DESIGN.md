# Glyphdrift: Design Notes

How the app works and why. For what it does and how to use it, see the [README](README.md).

## Goals

- A live wallpaper that can run all day: low CPU, pauses itself, no surprises
- One small portable exe (or app on macOS): no installer, no admin rights, no network access
- Boring, auditable code: few dependencies (`windows`, `serde`, `toml`, `toml_edit`; on macOS the `objc2` framework bindings), minimal `unsafe`, each block with a `// SAFETY:` note

## Layout

```
src/
├── main.rs, lib.rs   entry point; module list
├── app.rs            wallpaper window, frame loop, settings plumbing, lifecycle
├── desktop/          attaching behind the desktop icons
├── render/           Direct2D + DirectWrite renderer
├── anim/             Animation trait and the 23 animations
├── settings.rs       the settings window (native Win32 controls)
├── tray/             notification-area icon and menu
├── config/           TOML config: load, validate, save, migrate
├── theme.rs          color themes and the readability veil
├── power/            fullscreen and battery-saver detection
├── autostart.rs      Start with Windows (per-user Run key)
├── log.rs            glyphdrift.log next to the config (both hosts)
├── grid.rs, shape.rs what animations draw into
├── icon.rs           the icon shape, shared with build.rs
├── ui.rs             settings shared by both hosts: tweak ids, sliders, hints, grouped lists
├── engine.rs         settings logic shared by both hosts: apply a setting, what a config edit
│                     affects, frame rate, hover text; reports changes for the host to act on
└── macos/            the macOS host (see below)
build.rs              embeds the manifest, icon and version info (no rc.exe)
macos/                Info.plist and bundle.sh for Glyphdrift.app
examples/             macos_iconset: the app icon for bundle.sh
site/                 glyphdrift.com, deployed by Cloudflare Pages
```

The core (`anim`, `grid`, `config`, `theme`, `icon`, `shape`, `ui`, `engine`) is portable and unit-tested on any platform; Windows host code is `cfg(windows)`, macOS host code `cfg(target_os = "macos")`. The hosts share no platform code, so a change to one can't break the other's build, and both can be checked from one machine (see the README).

## Desktop embedding

`desktop` asks Progman (undocumented message `0x052C`) to create a WorkerW behind the icons, then picks a parent:

- **Windows 11 24H2+:** Progman hosts the icons and has `WS_EX_NOREDIRECTIONBITMAP`, so only layered children render. The wallpaper is a layered child of Progman, kept directly below the icon view. This needs the manifest's Windows 10 `supportedOS` entry.
- **Windows 10 and earlier 11:** the WorkerW that follows the one hosting the icons.

The window spans the virtual screen. Explorer restarts are detected (parent gone) and re-attached; the tray icon is re-added on `TaskbarCreated`.

## Frame loop

- A high-resolution waitable timer (Windows 10 1803+) drives frames through `MsgWaitForMultipleObjectsEx`; plain `WM_TIMER` would round 33 ms up to the 15.6 ms system tick.
- Modal loops (scrollbar drags, window moves, the tray menu) bypass our message pump, so a fallback `WM_TIMER` at the frame rate draws frames only while the pump is stalled.
- Once a second: config hot reload (file mtime) and pause checks. While paused, the timer drops to 1 Hz and only the clock is kept current.

## Animations

```rust
pub trait Animation {
    fn name(&self) -> &'static str;
    fn warm(&self) -> bool { false }          // use the theme's warm gradient
    fn fps(&self) -> u32 { 24 }               // the Auto frame rate
    fn init(&mut self, size: Size, seed: u64);
    fn update(&mut self, dt: f32);
    fn render(&self, grid: &mut Grid) {..}    // character cells
    fn draw(&self, out: &mut Vec<Shape>) {}   // vector shapes on top
    fn set_clock(&mut self, _: Option<&[String; 2]>) {}
}
```

- **Scale:** `Size::focus` is the primary monitor within the window. `focus.unit()` (its smaller side times Overall scale) sizes everything; `object_unit()` adds Object scale for centered objects. Full-bleed animations fill the window, centered ones center on the focus, so multiple monitors neither split an object nor shrink features.
- **Determinism:** frames depend only on the seed and elapsed time, which keeps tests exact.
- **Groups:** ASCII (grid only), Graphics, Loops (line art, frames are pure functions of `t`), Fluid and Contour (marching squares over noise fields; some add ASCII shading under the lines).

## Rendering

- A Direct2D DC render target draws into the `WM_PAINT` DC, which works for the layered child.
- Each brightness level's glyphs go out in one `DrawGlyphRun`, so the grid costs at most 16 draw calls.
- Shapes (lines, circles, polygons, text) follow; runs of identical consecutive lines become one path.
- A theme is a 16-step gradient from the background through three stops. The readability veil is baked into that palette, so it costs nothing.

**Performance findings** (from the October 2026 pass):

- The CPU cost of line-heavy animations is Direct2D tessellating segments, about 0.5 µs each, whether drawn singly or batched. What helps is fewer segments and fewer frames: contour grids and dot spacings have pixel floors (`MIN_CELL`), so small scales don't multiply the work, and Contour and Fluid default to 20 fps.
- Hardware mode made no difference; the target already uses the GPU.
- `cargo test --release frame_cost -- --ignored --nocapture` times every animation at 3440×1440; `GLYPHDRIFT_SCALE=0.5` tests small scales.

At the default scale, the ASCII and loop animations use under 1% CPU on an 8-thread machine, contour and fluid ones 1–3%, and Dune ripples about 4.5%.

## Settings, tray and messages

The settings window and tray talk to the wallpaper window with `WM_APP` messages, so all state lives in `App`:

| Message | Purpose |
|---|---|
| `+2` | toggle pause |
| `+3`, `+4` | set animation, theme (WPARAM = index) |
| `+5` | get animation and theme indices |
| `+9`, `+10` | set / get a tweak (speed, fps, readability, scales, clock); value × 1000 |
| `+11` | to the tray window: open settings (from a second launch) |
| `+12` | to the settings window: refresh after a tray change |

A named mutex keeps one instance; a second launch opens the first one's settings.

## Configuration

- `%APPDATA%\Glyphdrift\config.toml`, edited with `toml_edit` so hand-written comments survive.
- Invalid values are clamped or defaulted with a logged warning, never a failed start.
- The file is created on first launch, which also marks the first-run notification as shown.
- Startup migrates the settings folder and Run entry from the earlier names (WallMotion, ascii-wallpaper).

## Security

- No network access, telemetry or accounts.
- Runs as the user; the only registry write is the optional Run entry.
- The exe isn't code-signed yet, so SmartScreen warns on first run; releases list SHA-256 checksums.

## macOS host

`src/macos/` mirrors the Windows modules: `mod.rs` (app state, frame timer, lifecycle), `desktop.rs`, `render.rs`, `menu.rs`, `settings.rs`, `power.rs`, `autostart.rs`. Everything runs on the main thread; AppKit callbacks reach the state through `with_app`, which skips re-entrant calls instead of panicking.

- **Desktop:** a borderless window per screen at `kCGDesktopWindowLevel`, above the desktop picture and below Finder's icons, ignoring the mouse, on all Spaces and stationary in Mission Control. Each screen has its own animation instance, focused on that screen. Screens are rebuilt on `applicationDidChangeScreenParameters:`.
- **Units:** points, not pixels; the font size is in points, so 16 px on Windows and 16 pt here look the same size.
- **Frame loop:** an `NSTimer` in the common run loop modes, so it keeps firing while a menu is open or a slider is dragged (Windows needs its fallback `WM_TIMER` for this). Once a second: config hot reload and pause checks.
- **Pausing:** per window via `occlusionState`, which covers fullscreen Spaces, windows over the whole desktop and the lock screen; Low Power Mode stands in for battery saver.
- **Rendering:** Metal with 4x MSAA. Core Graphics came first, but CPU stroking cost 40-140 ms a frame for the contour animations at Retina sizes. Now everything is triangles in one vertex format: grid cells sample a glyph atlas (Core Text rasterizes each character once), lines and flattened curves are a quad per segment with overlapping joints, circles and polygons are fans (polygons must be convex), and text shapes are cached textures. Lines thinner than a pixel fade instead of thinning, like Direct2D coverage. The CPU side is under 1.5 ms a frame for every animation; the MSAA buffer is memoryless on Apple GPUs.
- **Menu bar and settings:** an `NSStatusItem` whose menu is rebuilt in `menuNeedsUpdate:`; the settings window uses standard AppKit controls and saves slider values on mouse-up.
- **Single instance:** `try_lock` on a lock file next to the config; a second launch posts a distributed notification that opens the first one's settings. Reopening the app from Finder does the same through `applicationShouldHandleReopen:`.
- **Open at Login:** a per-user LaunchAgent pointing at this executable, compared exactly like the Windows Run entry.
- **Match menu bar to theme** (`picture.rs`): macOS backs the menu bar with a pale frosted strip wherever a window lies under it, desktop-level windows included, but leaves it clear over the desktop picture. The fix needs both halves: the wallpaper windows stop at `visibleFrame`'s top, and the picture becomes a solid PNG in the theme background (a new file name per color, or macOS keeps the cached one). The user's picture and scaling options are written to `desktop-pictures.txt` before anything changes, restored on turning off, in `applicationWillTerminate:`, or on the next launch after a crash. The check runs once a second, which also debounces slider drags and catches other Spaces, new screens and pictures the user picks meanwhile (they become the new originals).

At 3456x2234 on an M1 Pro, `ps` shows about 5% of one core for Matrix rain and 13-20% of one core for the contour and fluid animations, most of it the shared animation math: 0.5-2% of the whole CPU, in line with Windows.

## Testing

- Unit tests: config parsing, contours, noise, slider mapping, grouped lists, determinism and bounds for every animation (including a 4K run that once caught a never-ending loop).
- macOS: `cargo test --release snapshots -- --ignored --nocapture` draws every animation through the Metal renderer offscreen, times the CPU side and writes PNGs to `target/snapshots`.
- Website live previews: `web/` builds the portable core to WebAssembly with a plain C ABI (no bindings generator, no extra dependencies): JavaScript passes strings through `gd_alloc`, steps an animation and reads each frame back from linear memory (cells as `char | brightness << 21`, shapes as `f32` records, curves already as Bézier segments). `site/live.js` draws them on a canvas: grid rows as runs of same-colored text, then the shapes. Each card simulates its screenshot's scene (1200x750 px, same scale, theme and seed, 20 s warm-up), so the live frame picks up where the still left off; the clock shows the visitor's local time. Cards are drawn in the theme picked on the page (recolored in place when it changes) and move only while hovered or keyboard-focused, at half speed; the viewer moves while open. One animation runs at a time, at its own rate: with several running at once the contour ones were too heavy for a page. Reduced motion keeps the drawn frames still; without WebAssembly the screenshots stay. The header runs Topographic drift the same way, at the app's own speed and from a random seed per visit, flowing around the logo box through `Animation::set_keep_clear` (the box the clock normally reserves, without the clock). It replaced a separate JavaScript port, so the header can't drift from the app.
- Website images: `glyphdrift --screenshots <dir>` (`src/macos/screenshots.rs`) renders the gallery and the Settings window offscreen (`render::render_offscreen`, `settings::render_offscreen`), so no screen capture and no personal content. Gallery images use 150% Overall scale (130% Object scale for Loops) to read as small cards, except Ripples, Wind and Flow field, which add drops, gusts or particles per cell or area and would look sparse. A disabled `NSSlider` draws its knob at the far left offscreen, so the Settings image shows a Loops animation, where every slider is enabled.
- Manual: Windows 10 and 11, multi-monitor, DPI, Explorer restart, fullscreen apps, battery saver. macOS so far: macOS 26 on Apple silicon, one Retina screen.
