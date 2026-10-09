# ASCII Live Wallpaper for Windows: Design Document

## 1. Overview

A lightweight live wallpaper application for Windows that renders ASCII-art animations (matrix rain, plasma, fire, etc.) behind the desktop icons.

**Primary goals**
- Minimal resource usage: low RAM, near-zero idle CPU, tiny GPU cost
- Single small executable with no runtime dependencies
- Easy to add new animations
- Safe to run permanently as a background process

**Non-goals (v1)**
- Video or GIF wallpapers
- Cross-platform support
- A heavy settings GUI

## 2. Technology Decisions

| Area | Decision | Rationale |
|---|---|---|
| Language | Rust | Small binary, memory safety for a long-running process, no runtime |
| Win32 bindings | `windows` crate | Official Microsoft bindings, full API coverage |
| Rendering (MVP) | Direct2D + DirectWrite | Simple text-grid drawing, low resource use |
| Rendering (later) | Direct3D 11 glyph atlas (or `wgpu`) | One texture of glyphs, one quad per cell, for high-res effects |
| Config | TOML (`serde` + `toml`) | Human-readable, easy to hot-reload |
| Prototyping | Python or Rust + `crossterm` | Iterate on animations in a terminal first |

**Alternatives considered:** C++/Win32 (same footprint, manual memory safety), C#/WPF (faster to build but needs .NET and uses 30-60 MB), Python/pygame (heaviest, awkward to package), Electron/Tauri (WebView overhead).

## 3. Architecture

```
+---------------------------------------------------+
|                    main.rs                        |
|   startup, message loop, lifecycle                |
+---------+------------+-------------+--------------+
          |            |             |
   +------v-----+ +----v------+ +----v--------+
   | desktop/   | | render/   | | anim/       |
   | WorkerW    | | D2D grid  | | animations  |
   | attach     | | renderer  | | (pure fns)  |
   +------+-----+ +----+------+ +----+--------+
          |            |             |
   +------v-----+ +----v------+ +----v--------+
   | tray/      | | config/   | | power/      |
   | tray icon  | | TOML load | | pause logic |
   +------------+ +-----------+ +-------------+
```

### Modules

- **desktop**: finds `Progman`, sends `0x052C`, locates the `WorkerW` window, creates a borderless window and calls `SetParent`. Handles multi-monitor geometry.
- **render**: owns the Direct2D/DirectWrite resources. Takes a `Grid` (cells of char + color/brightness) and draws it each frame.
- **anim**: the animation trait and built-in animations.
- **config**: loads and validates settings; watches the file for changes.
- **tray**: notification-area icon with pause/resume, animation picker, and quit.
- **power**: decides when to pause rendering (fullscreen app, battery saver, session locked).

## 4. Desktop Embedding (WorkerW Technique)

1. `FindWindow("Progman")`
2. `SendMessageTimeout(progman, 0x052C, 0xD, 0x1, ...)` so Explorer creates a `WorkerW` behind the icons
3. `EnumWindows` to find the `WorkerW` that follows the window containing `SHELLDLL_DefView`
4. Create a borderless, non-activating window sized to the virtual screen (or per-monitor)
5. `SetParent(my_hwnd, workerw)`
6. Handle `WM_DISPLAYCHANGE`, `WM_DPICHANGED`, and Explorer restarts (the `TaskbarCreated` message) by re-attaching

**Risk:** the `0x052C` message is undocumented and has changed behavior between Windows versions (notably Windows 11 24H2). Isolate it in the `desktop` module and test across versions.

## 5. Animation API

Animations are small, pure, and testable.

```rust
pub struct Cell {
    pub ch: char,
    pub brightness: u8,      // 0-255
    pub color: Option<Rgb>,  // None = use theme color
}

pub trait Animation {
    fn name(&self) -> &'static str;
    fn init(&mut self, cols: usize, rows: usize, seed: u64);
    fn resize(&mut self, cols: usize, rows: usize);
    fn update(&mut self, dt: f32);           // advance state
    fn render(&self, grid: &mut Grid);       // write cells
}
```

Stateless effects (plasma, waves) can be expressed as `(x, y, t) -> Cell`.

### Planned animations
| Animation | Type | Notes |
|---|---|---|
| Matrix rain | Stateful | Falling columns with fading trails |
| Plasma | Stateless | Sine-sum field mapped to a character ramp |
| Fire | Stateful | Cellular heat propagation |
| Starfield | Stateful | Parallax points mapped to `. * +` |
| Waves | Stateless | Scrolling sine field |
| Clock | Stateful | Big-digit ASCII time overlay (optional) |

Character ramp for brightness mapping: `" .:-=+*#%@"` (configurable).

## 6. Rendering

**MVP (Direct2D + DirectWrite)**
- Monospace font (Consolas or user-configured), fixed cell size derived from font metrics
- Grid dimensions = screen size / cell size
- Draw only changed rows when possible; otherwise full redraw per frame
- Cap at 15-30 FPS (configurable)

**Phase 2 (GPU glyph atlas)**
- Pre-render the glyph set into a texture
- One instanced quad per cell, with the shader picking the glyph and tint
- Enables full-resolution effects at negligible GPU cost

## 7. Performance and Power

- Fixed-timestep loop using a waitable timer, never a busy loop
- Pause rendering when:
  - A fullscreen app or game is foreground (`SHQueryUserNotificationState`)
  - The session is locked or the display is off
  - Battery saver is active (optional setting)
  - The desktop is fully covered (optional occlusion check)
- Drop to 1-2 FPS (or stop) while paused and resume instantly on change

**Targets**
| Metric | Target |
|---|---|
| Working set | < 15 MB |
| CPU at 30 FPS (1080p) | < 2% on a modern CPU |
| CPU when paused | ~0% |
| Binary size | < 2 MB (release, stripped) |

## 8. Configuration

`config.toml` in `%APPDATA%\ascii-wallpaper\`:

```toml
[general]
fps = 24
animation = "matrix"
pause_on_fullscreen = true
pause_on_battery_saver = true

[font]
family = "Consolas"
size = 16

[theme]
foreground = "#00ff66"
background = "#000000"
ramp = " .:-=+*#%@"

[monitors]
mode = "all"   # "all" | "primary" | per-monitor overrides

[animation.matrix]
density = 0.6
speed = 1.0
```

Hot-reload on file change.

## 9. Security Considerations

- No network access and no telemetry
- Config parsed with strict schema validation; unknown keys warn, invalid values fall back to defaults
- No `unsafe` outside the thin Win32 interop layer; each `unsafe` block gets a comment explaining its invariants
- Run at standard user privilege (no admin required)
- Dependency hygiene: `cargo audit` and `cargo deny` in CI, minimal dependency tree, pinned versions
- Any future user-script support (e.g. Lua/Rhai) must be sandboxed with execution time and memory limits, and treated as untrusted input
- Release binaries signed if distributed

## 10. Project Structure

```
ascii-wallpaper/
├── Cargo.toml
├── DESIGN.md
├── config.example.toml
├── src/
│   ├── main.rs
│   ├── desktop/        # WorkerW attach, monitor geometry
│   ├── render/         # Direct2D/DirectWrite grid renderer
│   ├── anim/           # Animation trait + built-ins
│   │   ├── mod.rs
│   │   ├── matrix.rs
│   │   ├── plasma.rs
│   │   └── fire.rs
│   ├── config/
│   ├── tray/
│   └── power/
├── tests/
└── tools/
    └── proto/          # terminal prototypes for animations
```

## 11. Milestones

1. **M0, Prototype:** terminal-based animations (matrix, plasma, fire) to validate the `Animation` API
2. **M1, Embed:** Rust app attaches a window to `WorkerW` and fills it with a solid color
3. **M2, Render:** Direct2D text grid with matrix rain running at a fixed FPS
4. **M3, Lifecycle:** tray icon, TOML config, multi-monitor, DPI, Explorer-restart recovery
5. **M4, Power:** fullscreen/lock/battery pausing and perf tuning against targets
6. **M5, Polish:** more animations, themes, installer or portable zip, optional autostart
7. **M6, Stretch:** GPU glyph atlas renderer, audio-reactive mode, sandboxed user scripts

## 12. Testing Strategy

- **Unit tests:** animation logic with fixed seeds, grid math, config parsing
- **Snapshot tests:** render an animation frame to a `Grid` and compare against a stored text snapshot
- **Manual matrix:** Windows 10 and 11, single and multi-monitor, mixed DPI, Explorer restart, lock/unlock, fullscreen game
- **Perf checks:** measure working set and CPU with Process Explorer / Task Manager at each milestone

## 13. Open Questions

- Per-monitor independent animations vs. one animation spanning the virtual screen?
- Autostart via registry `Run` key vs. Task Scheduler?
- Ship an installer (MSIX/Inno Setup) or a portable zip only?
- Do we want audio-reactive animations, and is the extra capture dependency worth it?
- Is Windows 11 24H2 `WorkerW` behavior still compatible with the `0x052C` approach? Verify early in M1.

## 14. References

- Raymond Chen / community writeups on the Progman `0x052C` WorkerW technique
- Microsoft Docs: Direct2D, DirectWrite, `SHQueryUserNotificationState`
- `windows` crate documentation (microsoft.github.io/windows-docs-rs)
