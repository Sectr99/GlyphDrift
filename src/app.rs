//! Wallpaper window, frame loop and application lifecycle; the screen saver runs the same
//! loop in its own window.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use windows::core::{w, Error, Result};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, COLORREF, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT,
    POINT, RECT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Globalization::{
    GetDateFormatEx, GetTimeFormatEx, DATE_LONGDATE, TIME_NOSECONDS,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
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
    FindWindowW, GetClientRect, GetCursorPos, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsDialogMessageW, IsWindow, MessageBoxW, MsgWaitForMultipleObjectsEx,
    PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SetCursor,
    SetForegroundWindow, SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW,
    TranslateMessage, CREATESTRUCTW, GWLP_USERDATA, LWA_ALPHA, MB_ICONERROR, MB_ICONINFORMATION,
    MB_OK, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SM_CXSCREEN, WINDOW_EX_STYLE,
    WM_ACTIVATEAPP, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT, WM_QUIT, WM_RBUTTONDOWN,
    WM_SETCURSOR, WM_SIZE, WM_SYSKEYDOWN, WM_TIMER, WM_XBUTTONDOWN, WNDCLASSW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use crate::anim::{self, Animation};
use crate::config::{self, Config};
use crate::desktop;
use crate::engine::{self, save};
use crate::grid::Grid;
use crate::power;
use crate::render::Renderer;
use crate::saver::Mode;
use crate::settings;
use crate::shape::Shape;
use crate::theme::{self, THEMES};
use crate::tray;
use crate::ui;

const TIMER_ID: usize = 1;
/// A plain WM_TIMER at the frame interval: modal loops (scrollbar drags, window moves, the
/// tray menu) skip our pump and the waitable timer, but still dispatch WM_TIMER.
const FALLBACK_TIMER_ID: usize = 2;
const PAUSED_INTERVAL_MS: u32 = 1000;
/// How often the lock screen frame is redrawn, to stay close to what's running.
const LOCK_SCREEN_REFRESH: std::time::Duration = std::time::Duration::from_secs(300);

/// Frame clock. WM_TIMER rounds to the 15.6 ms system tick; a high-resolution waitable
/// timer (Windows 10 1803+) gives the real rate. Older systems get a normal one.
struct FrameTimer {
    handle: HANDLE,
    /// For the fallback WM_TIMER; set once the window exists.
    window: HWND,
}

impl FrameTimer {
    fn new() -> Result<Self> {
        let access = TIMER_ALL_ACCESS.0;
        // SAFETY: plain object creation; the handle is closed in Drop.
        unsafe {
            CreateWaitableTimerExW(None, None, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, access)
                .or_else(|_| CreateWaitableTimerExW(None, None, 0, access))
                .map(|handle| Self {
                    handle,
                    window: HWND::default(),
                })
        }
    }

    /// Fire every `ms` ms.
    fn set(&self, ms: u32) {
        let due = -(ms as i64 * 10_000); // relative, 100 ns units
                                         // SAFETY: `due` outlives the call; no completion routine.
        unsafe {
            let _ = SetWaitableTimer(self.handle, &due, ms as i32, None, None, false);
            if !self.window.is_invalid() {
                SetTimer(Some(self.window), FALLBACK_TIMER_ID, ms, None);
            }
        }
    }
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateWaitableTimerExW and is closed once.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

struct App {
    mode: Mode,
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
    /// When our message loop last ran; stale means a modal loop has the thread.
    last_pump: Instant,
    /// Config file mtime at last load.
    config_mtime: Option<SystemTime>,
    user_paused: bool,
    /// Why the wallpaper paused itself, if it did.
    auto_paused: Option<&'static str>,
    /// Tray window for status updates; invalid until created.
    tray: HWND,
    /// Time and date last shown.
    clock: Option<[String; 2]>,
    /// Show on lock screen: a redraw is wanted (something visible changed), when the last
    /// frame was drawn, and the next frame's number (file names alternate).
    lock_due: bool,
    lock_drawn: Option<Instant>,
    lock_n: u32,
    /// Monitor layout the window was last fitted to (`desktop::layout`).
    layout: [i32; 6],
    /// Screen saver: where the mouse was first seen; moving away from it ends the show.
    cursor_start: Option<POINT>,
}

impl App {
    fn paused(&self) -> bool {
        self.user_paused || self.auto_paused.is_some()
    }

    /// True while a modal loop owns the thread.
    fn pump_stalled(&self) -> bool {
        self.last_pump.elapsed().as_millis() > 2 * self.interval_ms() as u128 + 50
    }

    fn interval_ms(&self) -> u32 {
        if self.paused() {
            PAUSED_INTERVAL_MS
        } else {
            1000 / engine::frame_rate(&self.cfg, self.anim.fps())
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
        let (fx, fy, fw, fh) = match self.mode {
            Mode::Preview(_) => (0.0, 0.0, width as f32, height as f32),
            _ => desktop::primary_in_window(),
        };
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

    /// Capture the current frame: grid cells and shapes.
    fn render_frame(&mut self) {
        self.anim.render(&mut self.grid);
        self.clock = self.cfg.general.clock.then(local_clock);
        self.anim.set_clock(self.clock.as_ref());
        self.shapes.clear();
        self.anim.draw(&mut self.shapes);
    }

    /// Apply a setting live; with `commit`, save it too.
    fn tweak(&mut self, id: usize, value: f32, commit: bool) {
        let changed = engine::tweak(&mut self.cfg, id, value, commit);
        self.apply(changed);
    }

    /// Act on what a setting or config edit changed.
    fn apply(&mut self, c: engine::Changed) {
        if c.font {
            let t = &THEMES[theme::index_of(&self.cfg.theme.name)];
            match Renderer::new(
                &self.cfg.font.family,
                font_size(&self.cfg, self.mode),
                t.bg,
                &t.cool,
                0.0,
            ) {
                Ok(r) => self.renderer = r,
                Err(e) => log!("render: rebuild failed: {e}"),
            }
        }
        if c.animation {
            self.anim = anim::by_name(&self.cfg.general.animation, &self.cfg.theme.ramp);
        }
        // Colors before the resize below, which renders: a new renderer starts uncolored.
        if c.colors || c.font || c.animation {
            self.apply_colors();
        }
        if c.font || c.animation || c.layout {
            self.resize(self.width, self.height); // new cell size, animation or object size
        } else if c.frame {
            self.render_frame();
        }
        if c.frame_rate {
            self.timer.set(self.interval_ms());
        }
        if c.settings_look {
            settings::refresh(); // restyles an open settings window
        }
        if c.lock_screen && !self.cfg.general.lock_screen && self.mode == Mode::Wallpaper {
            crate::lockscreen::restore();
        }
        self.lock_due |= c.lock_screen || c.animation || c.colors || c.font || c.layout;
    }

    /// Show on lock screen: draw a fresh frame when one is due. Called once a second.
    fn sync_lock_screen(&mut self) {
        // Not while locked: the animation is paused, and a new picture would flicker.
        if !self.cfg.general.lock_screen
            || self.mode != Mode::Wallpaper
            || power::hidden().is_some()
        {
            return;
        }
        let stale = self
            .lock_drawn
            .is_none_or(|t| t.elapsed() >= LOCK_SCREEN_REFRESH);
        if !(self.lock_due || stale) {
            return;
        }
        self.lock_due = false;
        self.lock_drawn = Some(Instant::now());
        let Some(path) = crate::lockscreen::frame_path(self.lock_n) else {
            return;
        };
        self.lock_n = self.lock_n.wrapping_add(1);
        // Without the clock: the lock screen shows its own, and ours would go stale.
        let mut grid = Grid::new(self.grid.cols(), self.grid.rows());
        let mut shapes = Vec::new();
        self.anim.set_clock(None);
        self.anim.render(&mut grid);
        self.anim.draw(&mut shapes);
        self.anim.set_clock(self.clock.as_ref());
        // The lock screen is the primary monitor's size; crop the window to it.
        let (x, y, w, h) = desktop::primary_in_window();
        let size = (w as u32, h as u32);
        match self
            .renderer
            .render_png(&path, size, (x, y), &grid, &shapes)
        {
            Ok(()) => crate::lockscreen::show(path),
            Err(e) => log!("lock screen: {e}"),
        }
    }

    /// Push the theme's colors (warm or cool, per animation) to the renderer.
    fn apply_colors(&mut self) {
        let t = &THEMES[theme::index_of(&self.cfg.theme.name)];
        let stops = if self.anim.warm() { &t.warm } else { &t.cool };
        self.renderer.set_colors(t.bg, stops, self.cfg.theme.glass);
    }

    /// Reload the config if the file changed, applying only what differs. True if reloaded.
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
        self.apply(engine::reloaded(&old, &self.cfg));
        true
    }

    /// Re-evaluate pausing; true if the timer interval must change.
    fn refresh_pause(&mut self) -> bool {
        if self.last_power_check.elapsed().as_secs() < 1 {
            return false;
        }
        self.last_power_check = Instant::now();
        let before = self.paused();
        let g = &self.cfg.general;
        self.auto_paused = power::hidden().or_else(|| {
            if g.pause_on_fullscreen && power::fullscreen_app_active() {
                Some("a fullscreen app is open")
            } else if g.pause_on_battery_saver && power::battery_saver_on() {
                Some("battery saver is on")
            } else {
                None
            }
        });
        self.update_tray();
        self.paused() != before
    }

    /// Tray hover text: animation and theme, or why it's paused.
    fn update_tray(&self) {
        let tip = engine::status(&self.cfg, self.user_paused, self.auto_paused);
        tray::set_status(self.tray, &tip, self.paused(), self.user_paused);
    }
}

/// SAFETY contract: GWLP_USERDATA holds the `Box<App>` leaked in `run`, used only on
/// the window's thread.
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
        WM_TIMER
            if wp.0 == TIMER_ID
                || (wp.0 == FALLBACK_TIMER_ID
                    && app_of(hwnd).is_some_and(|a| a.pump_stalled())) =>
        {
            if let Some(app) = app_of(hwnd) {
                let wallpaper = app.mode == Mode::Wallpaper;
                // Explorer restarted: re-attach.
                if wallpaper && !IsWindow(Some(app.parent)).as_bool() {
                    if let Ok(parent) = desktop::find_wallpaper_parent() {
                        app.parent = parent;
                        let _ = SetParent(hwnd, Some(parent));
                        on_display_change(hwnd, app);
                    }
                }
                // Once a second: config edits, then auto-pause. The screen saver skips
                // these: it is the fullscreen app, and it only runs for a while.
                let second = wallpaper && app.last_power_check.elapsed().as_secs() >= 1;
                let reloaded = second && app.reload_if_changed();
                if reloaded {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                let changed = (second && app.refresh_pause()) || reloaded;
                if second {
                    // Monitors changed (resolution, a split screen, one added or removed).
                    // WM_DISPLAYCHANGE goes to top-level windows only, and Explorer resizes
                    // our parent around then, so this check catches it reliably.
                    if app.layout != desktop::layout() {
                        on_display_change(hwnd, app);
                    }
                    app.sync_lock_screen();
                }
                if changed {
                    app.last_frame = Instant::now();
                    app.timer.set(app.interval_ms());
                }
                if app.paused() {
                    // Keep the clock current while paused.
                    if app.cfg.general.clock && app.clock != Some(local_clock()) {
                        app.render_frame();
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                } else {
                    let now = Instant::now();
                    let dt = now.duration_since(app.last_frame).as_secs_f32().min(0.25);
                    app.last_frame = now;
                    app.anim.update(dt * app.cfg.general.speed);
                    app.render_frame();
                    let _ = InvalidateRect(Some(hwnd), None, false);
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
        WM_ERASEBKGND => LRESULT(1), // we paint every pixel
        // Screen saver: no cursor, and any input ends it.
        WM_SETCURSOR if app_of(hwnd).is_some_and(|a| a.mode == Mode::Saver) => {
            SetCursor(None);
            LRESULT(1)
        }
        WM_MOUSEMOVE if app_of(hwnd).is_some_and(|a| a.mode == Mode::Saver) => {
            let app = app_of(hwnd).unwrap();
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // Windows sends a move when the window appears; a nudge of a few px is ignored.
            let start = *app.cursor_start.get_or_insert(pt);
            if (pt.x - start.x).abs() > 4 || (pt.y - start.y).abs() > 4 {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
        | WM_XBUTTONDOWN | WM_MOUSEWHEEL
            if app_of(hwnd).is_some_and(|a| a.mode == Mode::Saver) =>
        {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_ACTIVATEAPP if wp.0 == 0 && app_of(hwnd).is_some_and(|a| a.mode == Mode::Saver) => {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        tray::WM_GET_STATE => match app_of(hwnd) {
            Some(app) => LRESULT(tray::pack_state(
                anim::index_of(&app.cfg.general.animation),
                theme::index_of(&app.cfg.theme.name),
            )),
            None => LRESULT(0),
        },
        tray::WM_SET_ANIMATION => {
            if let (Some(app), Some(&(name, ..))) = (app_of(hwnd), anim::ANIMATIONS.get(wp.0)) {
                app.cfg.general.animation = name.into();
                app.apply(engine::Changed {
                    animation: true,
                    frame_rate: true, // the animation's own rate may differ
                    ..Default::default()
                });
                save("general", "animation", name);
                app.update_tray();
                let _ = InvalidateRect(Some(hwnd), None, false);
                settings::refresh();
            }
            LRESULT(0)
        }
        tray::WM_SET_THEME => {
            if let (Some(app), Some(t)) = (app_of(hwnd), THEMES.get(wp.0)) {
                app.cfg.theme.name = t.name.into();
                app.apply(engine::Changed {
                    colors: true,
                    ..Default::default()
                });
                save("theme", "name", t.name);
                app.update_tray();
                let _ = InvalidateRect(Some(hwnd), None, false);
                settings::refresh(); // picked from the tray
            }
            LRESULT(0)
        }
        settings::WM_TWEAK => {
            if let Some(app) = app_of(hwnd) {
                let (id, commit) = (wp.0 & 0xFF, (wp.0 >> 8) & 1 == 1);
                app.tweak(id, lp.0 as f32 / 1000.0, commit);
                let _ = InvalidateRect(Some(hwnd), None, false);
                if id == ui::CLOCK || id == ui::LOCK_SCREEN {
                    settings::refresh(); // toggled from the tray
                }
            }
            LRESULT(0)
        }
        settings::WM_SAVE_TWEAKS => {
            if let Some(app) = app_of(hwnd) {
                let ids: Vec<usize> = (0..usize::BITS as usize)
                    .filter(|i| wp.0 >> i & 1 == 1)
                    .collect();
                engine::save_tweaks(&app.cfg, &ids);
            }
            LRESULT(0)
        }
        settings::WM_GET_TWEAK => match app_of(hwnd) {
            Some(app) => LRESULT((engine::tweak_value(&app.cfg, wp.0) * 1000.0).round() as isize),
            None => LRESULT(0),
        },
        tray::WM_TOGGLE_PAUSE => {
            if let Some(app) = app_of(hwnd) {
                app.user_paused = !app.user_paused;
                app.update_tray();
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

/// Font size in px at 100% scaling, scaled by system DPI.
fn font_px(size: i32) -> i32 {
    // SAFETY: no preconditions.
    let dpi = unsafe { GetDpiForSystem() }.max(96);
    (size as f32 * dpi as f32 / 96.0).round() as i32
}

/// The renderer's font size: DPI-scaled, times the overall scale. The preview is a
/// miniature of the screen.
fn font_size(cfg: &Config, mode: Mode) -> i32 {
    let mut px = font_px(cfg.font.size) as f32 * cfg.general.scale;
    if let Mode::Preview(window) = mode {
        let mut r = RECT::default();
        // SAFETY: plain queries; a bad handle leaves `r` empty.
        let screen = unsafe {
            let _ = GetClientRect(window, &mut r);
            GetSystemMetrics(SM_CXSCREEN).max(1)
        };
        px *= r.right as f32 / screen as f32;
    }
    px.round().max(4.0) as i32
}

/// Fit the window to the monitors again, and restart the animation for the new layout
/// (WM_SIZE does it too, but not when only the primary monitor changed).
unsafe fn on_display_change(hwnd: HWND, app: &mut App) {
    app.layout = desktop::layout();
    log!("display: refit to {:?}", app.layout);
    desktop::place(hwnd, app.parent);
    app.resize(app.width, app.height);
    app.lock_due = true;
}

pub fn run(mode: Mode) -> Result<()> {
    // SAFETY: once, before any window exists. COM is for WIC (lock screen frames).
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    if mode == Mode::Configure {
        configure();
        return Ok(());
    }
    // The screen saver only reads the settings, and may run beside the wallpaper.
    let first_run = mode == Mode::Wallpaper && {
        if !first_instance() {
            return Ok(());
        }
        crate::log::init();
        crate::update::clean_up();
        crate::saver::refresh_installed();
        config::create_if_missing()
    };
    let (cfg, warnings) = config::load();
    for w in &warnings {
        log!("config: {w}");
    }

    let anim = anim::by_name(&cfg.general.animation, &cfg.theme.ramp);
    let t = &THEMES[theme::index_of(&cfg.theme.name)];
    let renderer = Renderer::new(
        &cfg.font.family,
        font_size(&cfg, mode),
        t.bg,
        if anim.warm() { &t.warm } else { &t.cool },
        cfg.theme.glass,
    )?;
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);

    // SAFETY: Win32 window creation; `app_ptr` is reclaimed after the message loop, when no
    // more wndproc calls can happen.
    unsafe {
        let parent = match mode {
            Mode::Preview(window) => window,
            Mode::Wallpaper => desktop::find_wallpaper_parent()?,
            _ => HWND::default(),
        };
        let now = Instant::now();
        let app_ptr = Box::into_raw(Box::new(App {
            mode,
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
            last_pump: now,
            config_mtime: config::modified(),
            user_paused: false,
            auto_paused: None,
            tray: HWND::default(),
            clock: None,
            lock_due: false,
            lock_drawn: None,
            lock_n: 0,
            layout: desktop::layout(),
            cursor_start: None,
        }));

        let result = (|| -> Result<()> {
            let hinst = GetModuleHandleW(None)?;
            let class = w!("Glyphdrift");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(Error::from_thread());
            }

            let (ex_style, style, (x, y, w, h)) = match mode {
                Mode::Saver => {
                    let r = desktop::virtual_screen();
                    let rect = (r.left, r.top, r.right - r.left, r.bottom - r.top);
                    (WS_EX_TOPMOST, WS_POPUP | WS_VISIBLE, rect)
                }
                Mode::Preview(_) => {
                    let mut r = RECT::default();
                    GetClientRect(parent, &mut r)?;
                    (
                        WINDOW_EX_STYLE(0),
                        WS_CHILD | WS_VISIBLE,
                        (0, 0, r.right, r.bottom),
                    )
                }
                // Layered: required under a WS_EX_NOREDIRECTIONBITMAP Progman.
                _ => (
                    WS_EX_NOACTIVATE | WS_EX_LAYERED,
                    WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                    desktop::rect_in_parent(parent),
                ),
            };
            let hwnd = CreateWindowExW(
                ex_style,
                class,
                w!("Glyphdrift"),
                style,
                x,
                y,
                w,
                h,
                (!parent.is_invalid()).then_some(parent),
                None,
                Some(hinst.into()),
                Some(app_ptr as *const c_void),
            )?;
            if mode == Mode::Wallpaper {
                SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;
                desktop::place(hwnd, parent);
                let tray = tray::create(hwnd)?;
                (*app_ptr).tray = tray;
                (*app_ptr).update_tray();
            } else if mode == Mode::Saver {
                let _ = SetForegroundWindow(hwnd); // for the keyboard
            }
            let tray = (*app_ptr).tray;
            if first_run {
                // Show new users where the controls are.
                tray::notify(
                    tray,
                    "Glyphdrift is running",
                    "Click the Glyphdrift icon in the taskbar tray for settings, or right-click it \
                     for quick options like Start with Windows. Can't see it? Click ^ next to \
                     the clock.",
                );
            }
            (*app_ptr).timer.window = hwnd;
            (*app_ptr).timer.set((*app_ptr).interval_ms());

            // Wait for a frame tick or messages; ticks arrive as WM_TIMER.
            let timer = (*app_ptr).timer.handle;
            let mut msg = MSG::default();
            'pump: loop {
                let wake = MsgWaitForMultipleObjectsEx(
                    Some(&[timer]),
                    INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                (*app_ptr).last_pump = Instant::now();
                if wake == WAIT_OBJECT_0 {
                    SendMessageW(hwnd, WM_TIMER, Some(WPARAM(TIMER_ID)), Some(LPARAM(0)));
                }
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'pump;
                    }
                    // Keyboard navigation in the settings window.
                    let dialog = settings::window();
                    if !dialog.is_invalid() && IsDialogMessageW(dialog, &msg).as_bool() {
                        continue;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if !tray.is_invalid() {
                tray::destroy(tray);
            }
            Ok(())
        })();

        drop(Box::from_raw(app_ptr));
        // An installed update: hand over now that this copy has stopped, so the new exe
        // starts as the only one.
        if let Some(path) = crate::update::take_restart() {
            let _ = CloseHandle(HANDLE(INSTANCE.load(Ordering::Relaxed) as *mut _));
            match crate::update::restart(&path) {
                Ok(()) => log!("update: started {}", path.display()),
                Err(e) => log!("update: {e}"),
            }
        }
        result
    }
}

/// The single-instance mutex, once this is the first copy.
static INSTANCE: AtomicIsize = AtomicIsize::new(0);

/// The screen saver's Settings button: the screen saver shows the wallpaper's animation and
/// theme, so open the wallpaper's settings, or say where they are.
fn configure() {
    if first_instance() {
        // SAFETY: constant strings.
        unsafe {
            MessageBoxW(
                None,
                w!("The screen saver shows the animation and theme you pick in Glyphdrift. Start Glyphdrift and click its tray icon to change them."),
                w!("Glyphdrift"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }
}

/// True for the first running copy; a second launch opens the first one's settings.
fn first_instance() -> bool {
    // SAFETY: constant strings. The mutex stays open while the process runs, marking it
    // running; only a handover to an update closes it early.
    unsafe {
        let mutex = CreateMutexW(None, false, w!("Local\\Glyphdrift"));
        if let (Ok(mutex), false) = (mutex, GetLastError() == ERROR_ALREADY_EXISTS) {
            INSTANCE.store(mutex.0 as isize, Ordering::Relaxed);
            return true;
        }
        if let Ok(tray) = FindWindowW(w!("GlyphdriftTray"), None) {
            let mut pid = 0;
            GetWindowThreadProcessId(tray, Some(&mut pid));
            let _ = AllowSetForegroundWindow(pid); // so it can come to the front
            let _ = PostMessageW(Some(tray), tray::WM_OPEN_SETTINGS, WPARAM(0), LPARAM(0));
        }
        false
    }
}

/// Local time (no seconds) and long date, in the user's format.
fn local_clock() -> [String; 2] {
    let (mut time, mut date) = ([0u16; 64], [0u16; 128]);
    // SAFETY: live local buffers; nulls mean user locale, current time, locale format.
    let (t, d) = unsafe {
        (
            GetTimeFormatEx(None, TIME_NOSECONDS, None, None, Some(&mut time)),
            GetDateFormatEx(None, DATE_LONGDATE, None, None, Some(&mut date), None),
        )
    };
    // Lengths include the NUL; 0 means failure.
    let text = |buf: &[u16], n: i32| String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize]);
    [text(&time, t), text(&date, d)]
}

/// Fatal error as a message box (release builds have no console).
pub fn fatal(text: &str) {
    crate::log::write(&format!("fatal: {text}"));
    let body: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(body.as_ptr()),
            w!("Glyphdrift"),
            MB_OK | MB_ICONERROR,
        );
    }
}
