//! Wallpaper window, timer-driven frame loop, and application lifecycle.

use std::ffi::c_void;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use windows::core::{w, Error, Result};
use windows::Win32::Foundation::{
    CloseHandle, BOOL, COLORREF, HANDLE, HWND, LPARAM, LRESULT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, SetWaitableTimer, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE,
    TIMER_ALL_ACCESS,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowLongPtrW, IsWindow,
    MessageBoxW, MsgWaitForMultipleObjectsEx, PeekMessageW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW, TranslateMessage,
    CREATESTRUCTW, GWLP_USERDATA, LWA_ALPHA, MB_ICONERROR, MB_OK, MSG, MWMO_INPUTAVAILABLE,
    PM_REMOVE, QS_ALLINPUT, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_NCCREATE,
    WM_PAINT, WM_QUIT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_VISIBLE,
};

use crate::anim::{self, Animation};
use crate::config::{self, Config};
use crate::desktop;
use crate::grid::Grid;
use crate::power;
use crate::render::Renderer;
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
        let size = anim::Size {
            cols,
            rows,
            width: width as f32,
            height: height as f32,
        };
        self.anim.init(size, self.seed);
        self.render_frame();
    }

    /// Capture the animation's current frame: grid cells plus vector shapes.
    fn render_frame(&mut self) {
        self.anim.render(&mut self.grid);
        self.shapes.clear();
        self.anim.draw(&mut self.shapes);
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
            eprintln!("config: {w}");
        }
        let old = std::mem::replace(&mut self.cfg, cfg);
        let (new_font, new_anim) = (
            (&old.font.family, old.font.size) != (&self.cfg.font.family, self.cfg.font.size),
            (&old.general.animation, &old.theme.ramp)
                != (&self.cfg.general.animation, &self.cfg.theme.ramp),
        );
        if new_font {
            let t = &THEMES[theme::index_of(&self.cfg.theme.name)];
            match Renderer::new(
                &self.cfg.font.family,
                self.cfg.font.size,
                t.bg,
                &t.cool,
                0.0,
            ) {
                Ok(r) => self.renderer = r,
                Err(e) => eprintln!("config: font change failed: {e}"),
            }
        }
        if new_anim {
            self.anim = anim::by_name(&self.cfg.general.animation, &self.cfg.theme.ramp);
        }
        if new_font || new_anim {
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
                if !app.paused() {
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
                save("theme", "glass", glass as f64);
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
            LRESULT(0)
        }
        tray::WM_SET_SPEED => {
            if let (Some(app), Some(&(speed, _))) = (app_of(hwnd), anim::SPEEDS.get(wp.0)) {
                app.cfg.general.speed = speed;
                save("general", "speed", speed as f64);
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

/// Persist a tray choice; failure only costs the setting surviving a restart.
fn save(section: &str, key: &str, value: impl Into<toml_edit::Value>) {
    if let Err(e) = config::save(section, key, value) {
        eprintln!("config: could not save {section}.{key}: {e}");
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

    let (cfg, warnings) = config::load();
    for w in &warnings {
        eprintln!("config: {w}");
    }

    let anim = anim::by_name(&cfg.general.animation, &cfg.theme.ramp);
    let t = &THEMES[theme::index_of(&cfg.theme.name)];
    let renderer = Renderer::new(
        &cfg.font.family,
        cfg.font.size,
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
        }));

        let result = (|| -> Result<()> {
            let hinst = GetModuleHandleW(None)?;
            let class = w!("AsciiWallpaper");
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
                w!("ascii-wallpaper"),
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

/// Show a fatal error. Release builds have no console, so use a message box.
pub fn fatal(text: &str) {
    let body: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(body.as_ptr()),
            w!("ascii-wallpaper"),
            MB_OK | MB_ICONERROR,
        );
    }
}
