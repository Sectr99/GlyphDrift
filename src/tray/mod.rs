//! Notification-area icon with Animation, Theme, Speed and Glass pickers, Pause/Resume
//! and Quit.
//!
//! Uses its own hidden top-level window: the wallpaper window is a child of WorkerW and
//! cannot reliably take foreground focus, which popup menus require.

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, GetCursorPos, GetSystemMetrics, GetWindowLongPtrW,
    LoadIconW, PostMessageW, RegisterClassW, RegisterWindowMessageW, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, CREATESTRUCTW, GWLP_USERDATA, HICON,
    HMENU, IDI_APPLICATION, MF_BYCOMMAND, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    SM_CXSMICON, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_NCCREATE, WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
};

/// Posted to the wallpaper window when the user picks Pause/Resume.
pub const WM_TOGGLE_PAUSE: u32 = WM_APP + 2;
const WM_TRAY: u32 = WM_APP + 1;
const ID_PAUSE: usize = 1;
/// Posted to the wallpaper window with WPARAM = index into `anim::ANIMATIONS`.
pub const WM_SET_ANIMATION: u32 = WM_APP + 3;
/// Posted to the wallpaper window with WPARAM = index into `theme::THEMES`.
pub const WM_SET_THEME: u32 = WM_APP + 4;
/// Sent to the wallpaper window; returns `pack_state` of the current selections.
pub const WM_GET_STATE: u32 = WM_APP + 5;
/// Posted to the wallpaper window with WPARAM = index into `anim::SPEEDS`.
pub const WM_SET_SPEED: u32 = WM_APP + 6;
/// Posted to the wallpaper window with WPARAM = index into `theme::GLASS`.
pub const WM_SET_GLASS: u32 = WM_APP + 7;
const ID_QUIT: usize = 2;
const ID_AUTOSTART: usize = 3;
const ID_ANIM_BASE: usize = 100;
const ID_THEME_BASE: usize = 200;
const ID_SPEED_BASE: usize = 300;
const ID_GLASS_BASE: usize = 400;

/// Current menu selections, one byte each, as the WM_GET_STATE result.
pub fn pack_state(anim: usize, theme: usize, speed: usize, glass: usize) -> isize {
    (anim | theme << 8 | speed << 16 | glass << 24) as isize
}

fn unpack_state(v: isize) -> [usize; 4] {
    let v = v as usize;
    [
        v & 0xFF,
        (v >> 8) & 0xFF,
        (v >> 16) & 0xFF,
        (v >> 24) & 0xFF,
    ]
}

/// Submenu of radio items `base + i` labelled `labels`, with `checked` marked.
unsafe fn radio_submenu(
    labels: impl Iterator<Item = &'static str>,
    base: usize,
    checked: usize,
) -> Option<HMENU> {
    let sub = CreatePopupMenu().ok()?;
    let mut n = 0;
    for label in labels {
        let _ = AppendMenuW(sub, MF_STRING, base + n, &HSTRING::from(label));
        n += 1;
    }
    let last = (base + n).saturating_sub(1) as u32;
    let _ = CheckMenuRadioItem(
        sub,
        base as u32,
        last,
        (base + checked) as u32,
        MF_BYCOMMAND.0,
    );
    Some(sub)
}
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// The tray HICON, created in `create` and destroyed in `destroy`; 0 = stock icon.
static ICON: AtomicIsize = AtomicIsize::new(0);

/// White, like the stock notification-area icons.
const ICON_RGB: (u8, u8, u8) = (0xff, 0xff, 0xff);

/// Rasterize the icon at the small-icon size (DPI-scaled) into a 32-bit alpha HICON.
unsafe fn make_icon() -> HICON {
    let n = GetSystemMetrics(SM_CXSMICON).clamp(16, 256) as usize;
    let bgra = crate::icon::bgra(n, ICON_RGB);
    // 1-bpp AND mask, rows padded to 16 bits; all zero so the alpha channel decides.
    let mask = vec![0u8; n.div_ceil(16) * 2 * n];
    // SAFETY: `bgra` holds n*n 32-bit pixels and `mask` n rows of WORD-aligned 1-bpp bits,
    // the sizes CreateIcon reads for an n x n, 1-plane, 32-bpp icon.
    CreateIcon(
        None,
        n as i32,
        n as i32,
        1,
        32,
        mask.as_ptr(),
        bgra.as_ptr(),
    )
    .unwrap_or_default()
}

/// Create the tray window + icon. `wallpaper` receives WM_TOGGLE_PAUSE / WM_CLOSE.
pub fn create(wallpaper: HWND) -> Result<HWND> {
    // SAFETY: standard window-class registration and creation; `wallpaper` is stored as
    // an integer and only used as a PostMessage target.
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let class = w!("AsciiWallpaperTray");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(tray_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc); // already-registered on a second call is fine
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("ascii-wallpaper tray"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            hinst,
            Some(wallpaper.0 as *const _),
        )?;
        ICON.store(make_icon().0 as isize, Ordering::Relaxed);
        add_icon(hwnd);
        Ok(hwnd)
    }
}

pub fn destroy(tray: HWND) {
    // SAFETY: `tray` was created by `create` on this thread.
    unsafe {
        remove_icon(tray);
        let _ = DestroyWindow(tray);
        let icon = ICON.swap(0, Ordering::Relaxed);
        if icon != 0 {
            let _ = DestroyIcon(HICON(icon as *mut _));
        }
    }
}

unsafe fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: match ICON.load(Ordering::Relaxed) {
            0 => LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            icon => HICON(icon as *mut _),
        },
        ..Default::default()
    };
    for (dst, src) in nid.szTip.iter_mut().zip("ASCII Wallpaper".encode_utf16()) {
        *dst = src;
    }
    nid
}

unsafe fn add_icon(hwnd: HWND) {
    let _ = Shell_NotifyIconW(NIM_ADD, &icon_data(hwnd));
}

unsafe fn remove_icon(hwnd: HWND) {
    let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
}

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_TRAY if lp.0 as u32 == WM_RBUTTONUP => {
            let target = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
            if let Ok(menu) = CreatePopupMenu() {
                let [anim_i, theme_i, speed_i, glass_i] =
                    unpack_state(SendMessageW(target, WM_GET_STATE, WPARAM(0), LPARAM(0)).0);
                let anims = crate::anim::ANIMATIONS.iter().map(|a| a.1);
                if let Some(sub) = radio_submenu(anims, ID_ANIM_BASE, anim_i) {
                    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Animation"));
                }
                let themes = crate::theme::THEMES.iter().map(|t| t.label);
                if let Some(sub) = radio_submenu(themes, ID_THEME_BASE, theme_i) {
                    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Theme"));
                }
                let speeds = crate::anim::SPEEDS.iter().map(|s| s.1);
                if let Some(sub) = radio_submenu(speeds, ID_SPEED_BASE, speed_i) {
                    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Speed"));
                }
                let glass = crate::theme::GLASS.iter().map(|g| g.1);
                if let Some(sub) = radio_submenu(glass, ID_GLASS_BASE, glass_i) {
                    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Glass"));
                }
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                let autostart = crate::autostart::enabled();
                let flags = if autostart {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                };
                let _ = AppendMenuW(menu, flags, ID_AUTOSTART, w!("Start with Windows"));
                let _ = AppendMenuW(menu, MF_STRING, ID_PAUSE, w!("Pause / Resume"));
                let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit"));
                let mut pt = Default::default();
                let _ = GetCursorPos(&mut pt);
                let _ = SetForegroundWindow(hwnd); // required or the menu never dismisses
                let cmd = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    pt.x,
                    pt.y,
                    0,
                    hwnd,
                    None,
                );
                let _ = DestroyMenu(menu); // also destroys the submenus
                match cmd.0 as usize {
                    ID_PAUSE => {
                        let _ = PostMessageW(target, WM_TOGGLE_PAUSE, WPARAM(0), LPARAM(0));
                    }
                    ID_AUTOSTART => {
                        if let Err(e) = crate::autostart::set(!autostart) {
                            eprintln!("autostart: {e}");
                        }
                    }
                    ID_QUIT => {
                        let _ = PostMessageW(target, WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    id if id >= ID_GLASS_BASE => {
                        let _ = PostMessageW(
                            target,
                            WM_SET_GLASS,
                            WPARAM(id - ID_GLASS_BASE),
                            LPARAM(0),
                        );
                    }
                    id if id >= ID_SPEED_BASE => {
                        let _ = PostMessageW(
                            target,
                            WM_SET_SPEED,
                            WPARAM(id - ID_SPEED_BASE),
                            LPARAM(0),
                        );
                    }
                    id if id >= ID_THEME_BASE => {
                        let _ = PostMessageW(
                            target,
                            WM_SET_THEME,
                            WPARAM(id - ID_THEME_BASE),
                            LPARAM(0),
                        );
                    }
                    id if id >= ID_ANIM_BASE => {
                        let _ = PostMessageW(
                            target,
                            WM_SET_ANIMATION,
                            WPARAM(id - ID_ANIM_BASE),
                            LPARAM(0),
                        );
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            // Explorer restarted; our icon vanished with it.
            add_icon(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
