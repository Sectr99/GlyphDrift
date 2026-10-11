//! Wallpaper window, timer-driven frame loop, and application lifecycle.

use std::ffi::c_void;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use windows::core::{w, Error, Result};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, BOOL, COLORREF, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT,
    WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Globalization::{
    GetDateFormatEx, GetTimeFormatEx, DATE_LONGDATE, TIME_NOSECONDS,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateMutexW, CreateWaitableTimerExW, SetWaitableTimer, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
    INFINITE, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForSystem, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    FindWindowW, GetWindowLongPtrW, GetWindowThreadProcessId, IsDialogMessageW, IsWindow,
    MessageBoxW, MsgWaitForMultipleObjectsEx, PeekMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW,
    TranslateMessage, CREATESTRUCTW, GWLP_USERDATA, LWA_ALPHA, MB_ICONERROR, MB_OK, MSG,
    MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE,
    WM_ERASEBKGND, WM_NCCREATE, WM_PAINT, WM_QUIT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_VISIBLE,
};

use crate::anim::{self, Animation};
use crate::config::{self, Config};
use crate::desktop;
use crate::grid::Grid;
use crate::power;
use crate::render::Renderer;
use crate::settings;
use crate::shape::Shape;
use crate::theme::{self, THEMES};
use crate::tray;

const TIMER_ID: usize = 1;
const PAUSED_INTERVAL_MS: u32 = 1000;

/// Frame clock. WM_TIMER fires on the ~15.6 ms system tick, so asking for 33 ms gives
/// ~21 fps; a high-resolution waitable timer (Windows 10 1803+) gives the real rate without
/// raising the system-wide timer resolution. Older systems fall back to a normal timer.
struct FrameTimer(HANDLE);

impl FrameTimer {
    fn new() -> Result<Self> {
        let access = TIMER_ALL_ACCESS.0;
        // SAFETY: plain object creation; the handle is closed in Drop.
        unsafe {
            CreateWaitableTimerExW(None, None, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, access)
                .or_else(|_| CreateWaitableTimerExW(None, None, 0, access))
                .map(Self)
        }
    }

    /// Fire every `ms` milliseconds, starting `ms` from now.
    fn set(&self, ms: u32) {
        let due = -(ms as i64 * 10_000); // negative = relative, in 100 ns units
                                         // SAFETY: `due` outlives the call; no completion routine.
        unsafe {
            let _ = SetWaitableTimer(self.0, &due, ms as i32, None, None, false);
        }
    }
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateWaitableTimerExW and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct App {
    cfg: Config,
    grid: Grid,
    shapes: Vec<Shape>,
    anim: Box<dyn Animation>,
    renderer: Renderer,
    timer: FrameTimer,
    parent: HWND,
    width: i32,
    height: i32,
    seed: u64,
    last_frame: Instant,
    last_power_check: Instant,
    /// Config file mtime at last load; a change triggers a reload.
    config_mtime: Option<SystemTime>,
    user_paused: bool,
    auto_paused: bool,
    /// Time and date last shown, when the clock is on.
    clock: Option<[String; 2]>,
}

impl App {
    fn paused(&self) -> bool {
        self.user_paused || self.auto_paused
    }

    fn interval_ms(&self) -> u32 {
        if self.paused() {
            PAUSED_INTERVAL_MS
        } else {
            let fps = match self.cfg.general.fps {
                0 => self.anim.fps(),
                n => n,
            };
            1000 / fps.max(1)
        }
    }

    fn resize(&mut self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.width = width;
        self.height = height;
        let (cw, ch) = self.renderer.cell_size();
        let cols = (width / cw).max(1) as usize;
        let rows = (height / ch).max(1) as usize;
        self.grid.resize(cols, rows);
        let (fx, fy, fw, fh) = desktop::primary_in_window();
        let size = anim::Size {
            cols,
            rows,
            width: width as f32,
            height: height as f32,
            focus: anim::Rect {
                zoom: self.cfg.general.scale,
                object_zoom: self.cfg.general.object_scale,
                ..anim::Rect::new(fx, fy, fw, fh)
            },
        };
        self.anim.init(size, self.seed);
        self.render_frame();
    }

    /// Capture the animation's current frame: grid cells plus vector shapes.
    fn render_frame(&mut self) {
        self.anim.render(&mut self.grid);
        self.clock = self.cfg.general.clock.then(local_clock);
        self.anim.set_clock(self.clock.as_ref());
        self.shapes.clear();
        self.anim.draw(&mut self.shapes);
    }

    /// New renderer for the current font and overall scale, then re-lay out the frame.
    fn rebuild_renderer(&mut self) {
        let t = &THEMES[theme::index_of(&self.cfg.theme.name)];
        match Renderer::new(
            &self.cfg.font.family,
            font_size(&self.cfg),
            t.bg,
            &t.cool,
            0.0,
        ) {
            Ok(r) => self.renderer = r,
            Err(e) => log!("render: rebuild failed: {e}"),
        }
        self.apply_colors();
        self.resize(self.width, self.height);
    }

    /// A settings value as the settings window shows it (see `settings::WM_GET_TWEAK`).
    fn tweak_value(&self, id: usize) -> f32 {
        let g = &self.cfg.general;
        match id {
            settings::SPEED => g.speed,
            settings::FPS => g.fps as f32,
            settings::GLASS => self.cfg.theme.glass,
            settings::SCALE => g.scale,
            settings::OBJECT_SCALE => g.object_scale,
            settings::CLOCK => f32::from(u8::from(g.clock)),
            _ => 0.0,
        }
    }

    /// Apply a settings value live; with `commit`, also save it to the config.
    fn tweak(&mut self, id: usize, value: f32, commit: bool) {
        match id {
            settings::SPEED => {
                self.cfg.general.speed = value.clamp(0.1, 4.0);
                if commit {
                    save("general", "speed", decimal(self.cfg.general.speed));
                }
            }
            settings::FPS => {
                self.cfg.general.fps = value.round().clamp(0.0, 60.0) as u32;
                self.timer.set(self.interval_ms());
                if commit {
                    save("general", "fps", self.cfg.general.fps as i64);
                }
            }
            settings::GLASS => {
                self.cfg.theme.glass = value.clamp(0.0, 0.9);
                self.apply_colors();
                if commit {
                    save("theme", "glass", decimal(self.cfg.theme.glass));
                }
            }
            settings::SCALE => {
                let scale = value.clamp(0.5, 2.0);
                if scale != self.cfg.general.scale {
                    self.cfg.general.scale = scale;
                    self.rebuild_renderer(); // the font size follows the overall scale
                }
                if commit {
                    save("general", "scale", decimal(scale));
                }
            }
            settings::OBJECT_SCALE => {
                let scale = value.clamp(0.5, 2.0);
                if scale != self.cfg.general.object_scale {
                    self.cfg.general.object_scale = scale;
                    self.resize(self.width, self.height);
                }
                if commit {
                    save("general", "object_scale", decimal(scale));
                }
            }
            settings::CLOCK => {
                self.cfg.general.clock = value > 0.5;
                self.render_frame();
                if commit {
                    save("general", "clock", self.cfg.general.clock);
                }
            }
            _ => {}
        }
    }

    /// Push the current theme's colors (warm or cool, per animation) to the renderer.
    fn apply_colors(&mut self) {
        let t = &THEMES[theme::index_of(&self.cfg.theme.name)];
        let stops = if self.anim.warm() { &t.warm } else { &t.cool };
        self.renderer.set_colors(t.bg, stops, self.cfg.theme.glass);
    }

    /// Reload the config if the file changed (hand edits, or our own tray saves), applying
    /// only what differs so an unchanged animation keeps running. Returns true if reloaded.
    fn reload_if_changed(&mut self) -> bool {
        let mtime = config::modified();
        if mtime == self.config_mtime {
            return false;
        }
        self.config_mtime = mtime;
        let (cfg, warnings) = config::load();
        for w in &warnings {
            log!("config: {w}");
        }
        let old = std::mem::replace(&mut self.cfg, cfg);
        let (new_font, new_anim) = (
            (&old.font.family, old.font.size, old.general.scale)
                != (
                    &self.cfg.font.family,
                    self.cfg.font.size,
                    self.cfg.general.scale,
                ),
            (&old.general.animation, &old.theme.ramp)
                != (&self.cfg.general.animation, &self.cfg.theme.ramp),
        );
        if new_font {
            let t = &THEMES[theme::index_of(&self.cfg.theme.name)];
            match Renderer::new(
                &self.cfg.font.family,
                font_size(&self.cfg),
                t.bg,
                &t.cool,
                0.0,
            ) {
                Ok(r) => self.renderer = r,
                Err(e) => log!("config: font change failed: {e}"),
            }
        }
        if new_anim {
            self.anim = anim::by_name(&self.cfg.general.animation, &self.cfg.theme.ramp);
        }
        if new_font || new_anim || old.general.object_scale != self.cfg.general.object_scale {
            self.resize(self.width, self.height); // new cell size and/or fresh animation
        }
        self.apply_colors();
        true
    }

    /// Re-evaluate pause state; returns true if the timer interval needs to change.
    fn refresh_pause(&mut self) -> bool {
        if self.last_power_check.elapsed().as_secs() < 1 {
            return false;
        }
        self.last_power_check = Instant::now();
        let before = self.paused();
        let g = &self.cfg.general;
        self.auto_paused = (g.pause_on_fullscreen && power::fullscreen_app_active())
            || (g.pause_on_battery_saver && power::battery_saver_on());
        self.paused() != before
    }
}

/// SAFETY contract: the pointer in GWLP_USERDATA is the `Box<App>` leaked in `run`, and
/// is only touched from the window's own thread, one message at a time.
unsafe fn app_of<'a>(hwnd: HWND) -> Option<&'a mut App> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App).as_mut()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_SIZE => {
            if let Some(app) = app_of(hwnd) {
                app.resize((lp.0 & 0xFFFF) as i32, ((lp.0 >> 16) & 0xFFFF) as i32);
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_ID => {
            if let Some(app) = app_of(hwnd) {
                // Explorer restarted and our parent died: re-attach.
                if !IsWindow(app.parent).as_bool() {
                    if let Ok(parent) = desktop::find_wallpaper_parent() {
                        app.parent = parent;
                        let _ = SetParent(hwnd, parent);
                        on_display_change(hwnd, app);
                    }
                }
                // Once a second: pick up config edits, then re-check auto-pause.
                let reloaded =
                    app.last_power_check.elapsed().as_secs() >= 1 && app.reload_if_changed();
                if reloaded {
                    let _ = InvalidateRect(hwnd, None, BOOL(0));
                }
                let changed = app.refresh_pause() || reloaded;
                if changed {
                    app.last_frame = Instant::now();
                    app.timer.set(app.interval_ms());
                }
                if app.paused() {
                    // Keep the clock current while paused (ticks are 1 s apart here).
                    if app.cfg.general.clock && app.clock != Some(local_clock()) {
                        app.render_frame();
                        let _ = InvalidateRect(hwnd, None, BOOL(0));
                    }
                } else {
                    let now = Instant::now();
                    let dt = now.duration_since(app.last_frame).as_secs_f32().min(0.25);
                    app.last_frame = now;
                    app.anim.update(dt * app.cfg.general.speed);
                    app.render_frame();
                    let _ = InvalidateRect(hwnd, None, BOOL(0));
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some(app) = app_of(hwnd) {
                app.renderer
                    .paint(hdc, app.width, app.height, &app.grid, &app.shapes);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1), // we paint every pixel ourselves
        WM_DISPLAYCHANGE => {
            if let Some(app) = app_of(hwnd) {
                on_display_change(hwnd, app);
            }
            LRESULT(0)
        }
        tray::WM_GET_STATE => match app_of(hwnd) {
            Some(app) => LRESULT(tray::pack_state(
                anim::index_of(&app.cfg.general.animation),
                theme::index_of(&app.cfg.theme.name),
                anim::nearest(anim::SPEEDS, app.cfg.general.speed),
                anim::nearest(theme::GLASS, app.cfg.theme.glass),
                anim::nearest(anim::FRAME_RATES, app.cfg.general.fps as f32),
            )),
            None => LRESULT(0),
        },
        tray::WM_SET_ANIMATION => {
            if let (Some(app), Some(&(name, ..))) = (app_of(hwnd), anim::ANIMATIONS.get(wp.0)) {
                app.cfg.general.animation = name.into();
                app.anim = anim::by_name(name, &app.cfg.theme.ramp);
                app.resize(app.width, app.height); // re-inits and renders a first frame
                app.apply_colors();
                app.timer.set(app.interval_ms()); // frame rate may differ
                save("general", "animation", name);
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
            LRESULT(0)
        }
        tray::WM_SET_THEME => {
            if let (Some(app), Some(t)) = (app_of(hwnd), THEMES.get(wp.0)) {
                app.cfg.theme.name = t.name.into();
                app.apply_colors();
                save("theme", "name", t.name);
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
            LRESULT(0)
        }
        tray::WM_SET_FPS => {
            if let (Some(app), Some(&(fps, _))) = (app_of(hwnd), anim::FRAME_RATES.get(wp.0)) {
                app.cfg.general.fps = fps as u32;
                app.timer.set(app.interval_ms());
                save("general", "fps", fps as i64);
            }
            LRESULT(0)
        }
        tray::WM_SET_GLASS => {
            if let (Some(app), Some(&(glass, _))) = (app_of(hwnd), theme::GLASS.get(wp.0)) {
                app.cfg.theme.glass = glass;
                app.apply_colors();
                save("theme", "glass", decimal(glass));
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
            LRESULT(0)
        }
        settings::WM_TWEAK => {
            if let Some(app) = app_of(hwnd) {
                let (id, commit) = (wp.0 & 0xFF, (wp.0 >> 8) & 1 == 1);
                app.tweak(id, lp.0 as f32 / 1000.0, commit);
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
            LRESULT(0)
        }
        settings::WM_GET_TWEAK => match app_of(hwnd) {
            Some(app) => LRESULT((app.tweak_value(wp.0) * 1000.0).round() as isize),
            None => LRESULT(0),
        },
        tray::WM_SET_SPEED => {
            if let (Some(app), Some(&(speed, _))) = (app_of(hwnd), anim::SPEEDS.get(wp.0)) {
                app.cfg.general.speed = speed;
                save("general", "speed", decimal(speed));
            }
            LRESULT(0)
        }
        tray::WM_TOGGLE_PAUSE => {
            if let Some(app) = app_of(hwnd) {
                app.user_paused = !app.user_paused;
                app.last_frame = Instant::now();
                app.timer.set(app.interval_ms());
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// The configured font size is in pixels at 100% display scaling; scale it by the system DPI
/// so ASCII animations keep their proportions on high-DPI screens.
fn font_px(size: i32) -> i32 {
    // SAFETY: no preconditions; returns 96 at 100% scaling.
    let dpi = unsafe { GetDpiForSystem() }.max(96);
    (size as f32 * dpi as f32 / 96.0).round() as i32
}

/// The renderer's font size: DPI-scaled, times the overall scale setting.
fn font_size(cfg: &Config) -> i32 {
    (font_px(cfg.font.size) as f32 * cfg.general.scale)
        .round()
        .max(4.0) as i32
}

/// A setting as it should appear in the config: 3 decimals, not the f32-to-f64 artifact
/// (0.3 would otherwise be saved as 0.30000001192092896).
fn decimal(v: f32) -> f64 {
    (v as f64 * 1000.0).round() / 1000.0
}

/// Persist a tray choice; failure only costs the setting surviving a restart.
fn save(section: &str, key: &str, value: impl Into<toml_edit::Value>) {
    if let Err(e) = config::save(section, key, value) {
        log!("config: could not save {section}.{key}: {e}");
    }
}

unsafe fn on_display_change(hwnd: HWND, app: &App) {
    desktop::place(hwnd, app.parent);
}

pub fn run() -> Result<()> {
    // SAFETY: called once at startup before any window exists.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    config::migrate();
    if !first_instance() {
        return Ok(());
    }
    crate::log::init();
    crate::autostart::migrate();
    let (cfg, warnings) = config::load();
    for w in &warnings {
        log!("config: {w}");
    }

    let anim = anim::by_name(&cfg.general.animation, &cfg.theme.ramp);
    let t = &THEMES[theme::index_of(&cfg.theme.name)];
    let renderer = Renderer::new(
        &cfg.font.family,
        font_size(&cfg),
        t.bg,
        if anim.warm() { &t.warm } else { &t.cool },
        cfg.theme.glass,
    )?;
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);

    // SAFETY: Win32 window creation; `app_ptr` is reclaimed with Box::from_raw on every
    // exit path below, after the message loop (and thus all wndproc calls) has finished.
    unsafe {
        let parent = desktop::find_wallpaper_parent()?;
        let now = Instant::now();
        let app_ptr = Box::into_raw(Box::new(App {
            cfg,
            grid: Grid::new(1, 1),
            shapes: Vec::new(),
            anim,
            renderer,
            timer: FrameTimer::new()?,
            parent,
            width: 0,
            height: 0,
            seed,
            last_frame: now,
            last_power_check: now,
            config_mtime: config::modified(),
            user_paused: false,
            auto_paused: false,
            clock: None,
        }));

        let result = (|| -> Result<()> {
            let hinst = GetModuleHandleW(None)?;
            let class = w!("WallMotion");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(Error::from_win32());
            }

            let (x, y, w, h) = desktop::rect_in_parent(parent);
            let hwnd = CreateWindowExW(
                // Layered: required to render under a WS_EX_NOREDIRECTIONBITMAP Progman.
                WS_EX_NOACTIVATE | WS_EX_LAYERED,
                class,
                w!("WallMotion"),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                x,
                y,
                w,
                h,
                parent,
                None,
                hinst,
                Some(app_ptr as *const c_void),
            )?;
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;
            desktop::place(hwnd, parent);

            let tray = tray::create(hwnd)?;
            (*app_ptr).timer.set((*app_ptr).interval_ms());

            // Wait for either a frame tick or window messages. A tick is delivered as the
            // WM_TIMER the window procedure already handles.
            let timer = (*app_ptr).timer.0;
            let mut msg = MSG::default();
            'pump: loop {
                let wake = MsgWaitForMultipleObjectsEx(
                    Some(&[timer]),
                    INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                if wake == WAIT_OBJECT_0 {
                    SendMessageW(hwnd, WM_TIMER, WPARAM(TIMER_ID), LPARAM(0));
                }
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'pump;
                    }
                    // Tab / arrow / Esc navigation for the settings window's controls.
                    let dialog = settings::window();
                    if !dialog.is_invalid() && IsDialogMessageW(dialog, &msg).as_bool() {
                        continue;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            tray::destroy(tray);
            Ok(())
        })();

        drop(Box::from_raw(app_ptr));
        result
    }
}

/// True for the first running copy. A second launch asks the first to open its settings
/// window (so "run it again" does something visible) and should then exit.
fn first_instance() -> bool {
    // SAFETY: plain Win32 calls with constant strings. The mutex handle is deliberately
    // never closed: it marks this process as running until it exits.
    unsafe {
        let mutex = CreateMutexW(None, false, w!("Local\\WallMotion"));
        if mutex.is_ok() && GetLastError() != ERROR_ALREADY_EXISTS {
            return true;
        }
        if let Ok(tray) = FindWindowW(w!("WallMotionTray"), None) {
            let mut pid = 0;
            GetWindowThreadProcessId(tray, Some(&mut pid));
            let _ = AllowSetForegroundWindow(pid); // let it bring the settings window forward
            let _ = PostMessageW(tray, tray::WM_OPEN_SETTINGS, WPARAM(0), LPARAM(0));
        }
        false
    }
}

/// The local time (no seconds) and long date, in the user's regional format.
fn local_clock() -> [String; 2] {
    let (mut time, mut date) = ([0u16; 64], [0u16; 128]);
    // SAFETY: the buffers are live locals; null locale/time/format mean the user's
    // default locale, the current time and the locale's own format.
    let (t, d) = unsafe {
        (
            GetTimeFormatEx(None, TIME_NOSECONDS, None, None, Some(&mut time)),
            GetDateFormatEx(None, DATE_LONGDATE, None, None, Some(&mut date), None),
        )
    };
    // Lengths include the terminating NUL; 0 means failure.
    let text = |buf: &[u16], n: i32| String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize]);
    [text(&time, t), text(&date, d)]
}

/// Show a fatal error. Release builds have no console, so use a message box.
pub fn fatal(text: &str) {
    crate::log::write(&format!("fatal: {text}"));
    let body: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(body.as_ptr()),
            w!("WallMotion"),
            MB_OK | MB_ICONERROR,
        );
    }
}
