//! Wallpaper window, timer-driven frame loop, and application lifecycle.

use std::ffi::c_void;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use windows::core::{w, Error, Result};
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, IsWindow, MessageBoxW, PostQuitMessage, RegisterClassW,
    SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW, TranslateMessage,
    CREATESTRUCTW, GWLP_USERDATA, LWA_ALPHA, MB_ICONERROR, MB_OK, MSG, WM_CLOSE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_NCCREATE, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_VISIBLE,
};

use crate::anim::{self, Animation};
use crate::config::{self, Config};
use crate::desktop;
use crate::grid::Grid;
use crate::power;
use crate::render::Renderer;
use crate::theme::{self, THEMES};
use crate::tray;

const TIMER_ID: usize = 1;
const PAUSED_INTERVAL_MS: u32 = 1000;

struct App {
    cfg: Config,
    grid: Grid,
    anim: Box<dyn Animation>,
    renderer: Renderer,
    parent: HWND,
    width: i32,
    height: i32,
    seed: u64,
    last_frame: Instant,
    last_power_check: Instant,
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
            1000 / self.cfg.general.fps.max(1)
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
        self.anim.init(cols, rows, self.seed);
        self.anim.render(&mut self.grid);
    }

    /// Push the current theme's colors (warm or cool, per animation) to the renderer.
    fn apply_colors(&mut self) {
        let t = &THEMES[theme::index_of(&self.cfg.theme.name)];
        let stops = if self.anim.warm() { &t.warm } else { &t.cool };
        self.renderer.set_colors(t.bg, stops, self.cfg.theme.glass);
    }

    /// Re-evaluate pause state; returns true if the timer interval needs to change.
    fn refresh_pause(&mut self) -> bool {
        if self.last_power_check.elapsed().as_secs() < 1 {
            return false;
        }
        self.last_power_check = Instant::now();
        let before = self.paused();
        self.auto_paused = self.cfg.general.pause_on_fullscreen && power::fullscreen_app_active();
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
                let changed = app.refresh_pause();
                if changed {
                    app.last_frame = Instant::now();
                    SetTimer(hwnd, TIMER_ID, app.interval_ms(), None);
                }
                if !app.paused() {
                    let now = Instant::now();
                    let dt = now.duration_since(app.last_frame).as_secs_f32().min(0.25);
                    app.last_frame = now;
                    app.anim.update(dt * app.cfg.general.speed);
                    app.anim.render(&mut app.grid);
                    let _ = InvalidateRect(hwnd, None, BOOL(0));
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some(app) = app_of(hwnd) {
                app.renderer.paint(hdc, app.width, app.height, &app.grid);
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
            )),
            None => LRESULT(0),
        },
        tray::WM_SET_ANIMATION => {
            if let (Some(app), Some(&(name, _))) = (app_of(hwnd), anim::ANIMATIONS.get(wp.0)) {
                app.cfg.general.animation = name.into();
                app.anim = anim::by_name(name, &app.cfg.theme.ramp);
                app.resize(app.width, app.height); // re-inits and renders a first frame
                app.apply_colors();
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
                SetTimer(hwnd, TIMER_ID, app.interval_ms(), None);
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
            anim,
            renderer,
            parent,
            width: 0,
            height: 0,
            seed,
            last_frame: now,
            last_power_check: now,
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
            SetTimer(hwnd, TIMER_ID, (*app_ptr).interval_ms(), None);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
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
