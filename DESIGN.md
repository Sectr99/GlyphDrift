# Glyphdrift: Design Notes

How the app works and why. For what it does and how to use it, see the [README](README.md).

## Goals

- A live wallpaper that can run all day: low CPU, pauses itself, no surprises
- One small portable exe: no installer, no admin rights, no network access
- Boring, auditable code: few dependencies (`windows`, `serde`, `toml`, `toml_edit`), minimal `unsafe`, each block with a `// SAFETY:` note

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
├── log.rs            %APPDATA%\Glyphdrift\glyphdrift.log
├── grid.rs, shape.rs what animations draw into
└── icon.rs           the icon shape, shared with build.rs
build.rs              embeds the manifest, icon and version info (no rc.exe)
site/                 glyphdrift.com, deployed by Cloudflare Pages
```

The core (`anim`, `grid`, `config`, `theme`, `icon`, `shape`) is portable and unit-tested on any platform; Windows host code is `cfg`-gated.

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

## Testing

- Unit tests: config parsing, contours, noise, slider mapping, grouped lists, determinism and bounds for every animation (including a 4K run that once caught a never-ending loop).
- Manual: Windows 10 and 11, multi-monitor, DPI, Explorer restart, fullscreen apps, battery saver.
