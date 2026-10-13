//! Tray icon: left-click opens settings, right-click shows quick actions. The icon turns
//! grey while paused, and its hover text says why. It has its own hidden window because
//! the wallpaper window can't reliably take the focus popup menus need.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIN_BALLOONUSERCLICK, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, GetCursorPos, GetSystemMetrics, GetWindowLongPtrW,
    LoadIconW, PostMessageW, RegisterClassW, RegisterWindowMessageW, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, CREATESTRUCTW, GWLP_USERDATA, HICON,
    HMENU, IDI_APPLICATION, MF_BYCOMMAND, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    SM_CXSMICON, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_LBUTTONUP, WM_NCCREATE, WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
};

/// To the wallpaper: Pause/Resume.
pub const WM_TOGGLE_PAUSE: u32 = WM_APP + 2;
const WM_TRAY: u32 = WM_APP + 1;
/// To the tray window, from a second launch: open settings.
pub const WM_OPEN_SETTINGS: u32 = WM_APP + 11;
const ID_PAUSE: usize = 1;
/// To the wallpaper: WPARAM = index into `anim::ANIMATIONS`.
pub const WM_SET_ANIMATION: u32 = WM_APP + 3;
/// To the wallpaper: WPARAM = index into `theme::THEMES`.
pub const WM_SET_THEME: u32 = WM_APP + 4;
/// To the wallpaper; returns `pack_state`.
pub const WM_GET_STATE: u32 = WM_APP + 5;
const ID_QUIT: usize = 2;
const ID_AUTOSTART: usize = 3;
const ID_SETTINGS: usize = 4;
const ID_CLOCK: usize = 5;
const ID_LOCK: usize = 6;
const ID_SAVER: usize = 7;
const ID_ANIM_BASE: usize = 100;
/// Animation index in the low byte, theme index in the next.
pub fn pack_state(anim: usize, theme: usize) -> isize {
    ((anim & 0xFF) | (theme & 0xFF) << 8) as isize
}

/// Inverse of `pack_state`.
pub fn unpack_state(v: isize) -> (usize, usize) {
    (v as usize & 0xFF, (v as usize >> 8) & 0xFF)
}

/// Radio submenu of items `base + i`, `checked` marked; separators between groups.
unsafe fn radio_submenu<'a>(
    items: impl Iterator<Item = (&'a str, &'a str)>,
    base: usize,
    checked: usize,
) -> Option<HMENU> {
    let sub = CreatePopupMenu().ok()?;
    let mut n = 0;
    let mut group = None;
    for (label, g) in items {
        if group.is_some_and(|prev| prev != g) {
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, None);
        }
        group = Some(g);
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
/// Tray icons, normal and paused; 0 = stock icon.
static ICON: AtomicIsize = AtomicIsize::new(0);
static ICON_PAUSED: AtomicIsize = AtomicIsize::new(0);
/// Hover text and paused state, for re-adding the icon after Explorer restarts.
static TIP: Mutex<String> = Mutex::new(String::new());
static PAUSED: AtomicBool = AtomicBool::new(false);
/// Paused from the menu (it then offers Resume), not automatically.
static USER_PAUSED: AtomicBool = AtomicBool::new(false);

/// White like the stock icons; grey while paused.
const ICON_RGB: (u8, u8, u8) = (0xff, 0xff, 0xff);
const ICON_PAUSED_RGB: (u8, u8, u8) = (0x80, 0x80, 0x80);

/// The icon at the small-icon size, as a 32-bit alpha HICON.
unsafe fn make_icon(rgb: (u8, u8, u8)) -> HICON {
    let n = GetSystemMetrics(SM_CXSMICON).clamp(16, 256) as usize;
    let bgra = crate::icon::bgra(n, rgb);
    // All-zero AND mask (rows padded to 16 bits): alpha decides.
    let mask = vec![0u8; n.div_ceil(16) * 2 * n];
    // SAFETY: `bgra` and `mask` have the sizes CreateIcon reads for an n x n 32-bpp icon.
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

/// Create the tray window and icon; `wallpaper` receives its messages.
pub fn create(wallpaper: HWND) -> Result<HWND> {
    // SAFETY: window creation; `wallpaper` is only used as a PostMessage target.
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let class = w!("GlyphdriftTray");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(tray_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly if already registered
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Glyphdrift tray"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            Some(wallpaper.0 as *const _),
        )?;
        ICON.store(make_icon(ICON_RGB).0 as isize, Ordering::Relaxed);
        ICON_PAUSED.store(make_icon(ICON_PAUSED_RGB).0 as isize, Ordering::Relaxed);
        add_icon(hwnd);
        Ok(hwnd)
    }
}

pub fn destroy(tray: HWND) {
    // SAFETY: `tray` was created by `create` on this thread.
    unsafe {
        remove_icon(tray);
        let _ = DestroyWindow(tray);
        for slot in [&ICON, &ICON_PAUSED] {
            let icon = slot.swap(0, Ordering::Relaxed);
            if icon != 0 {
                let _ = DestroyIcon(HICON(icon as *mut _));
            }
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
        hIcon: match (if PAUSED.load(Ordering::Relaxed) {
            &ICON_PAUSED
        } else {
            &ICON
        })
        .load(Ordering::Relaxed)
        {
            0 => LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            icon => HICON(icon as *mut _),
        },
        ..Default::default()
    };
    let tip = TIP.lock().map(|t| t.clone()).unwrap_or_default();
    let tip = if tip.is_empty() {
        "Glyphdrift".into()
    } else {
        tip
    };
    // Keep the terminating NUL.
    let room = nid.szTip.len() - 1;
    for (dst, src) in nid.szTip[..room].iter_mut().zip(tip.encode_utf16()) {
        *dst = src;
    }
    nid
}

/// One-off notification (a toast); clicking it opens settings.
pub fn notify(tray: HWND, title: &str, text: &str) {
    // SAFETY: `tray` is the window from `create`; the struct outlives the call.
    unsafe {
        let mut nid = icon_data(tray);
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        let fill = |dst: &mut [u16], s: &str| {
            let room = dst.len() - 1;
            for (d, c) in dst[..room].iter_mut().zip(s.encode_utf16()) {
                *d = c;
            }
        };
        fill(&mut nid.szInfoTitle, title);
        fill(&mut nid.szInfo, text);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// Update hover text and paused look; no-op if unchanged.
pub fn set_status(tray: HWND, tip: &str, paused: bool, user_paused: bool) {
    USER_PAUSED.store(user_paused, Ordering::Relaxed);
    let Ok(mut current) = TIP.lock() else {
        return;
    };
    if *current == tip && PAUSED.load(Ordering::Relaxed) == paused {
        return;
    }
    *current = tip.into();
    drop(current);
    PAUSED.store(paused, Ordering::Relaxed);
    if !tray.is_invalid() {
        // SAFETY: `tray` is the window from `create`; the struct outlives the call.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &icon_data(tray));
        }
    }
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
        WM_OPEN_SETTINGS => {
            crate::settings::open(HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _));
            LRESULT(0)
        }
        WM_TRAY if lp.0 as u32 == WM_LBUTTONUP || lp.0 as u32 == NIN_BALLOONUSERCLICK => {
            crate::settings::open(HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _));
            LRESULT(0)
        }
        WM_TRAY if lp.0 as u32 == WM_RBUTTONUP => {
            let target = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
            if let Ok(menu) = CreatePopupMenu() {
                let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, w!("Settings…"));
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                let (anim_i, _) = unpack_state(
                    SendMessageW(target, WM_GET_STATE, Some(WPARAM(0)), Some(LPARAM(0))).0,
                );
                let anims = crate::anim::ANIMATIONS.iter().map(|a| (a.1, a.2));
                if let Some(sub) = radio_submenu(anims, ID_ANIM_BASE, anim_i) {
                    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Animation"));
                }
                let on = |setting: usize| {
                    SendMessageW(
                        target,
                        crate::settings::WM_GET_TWEAK,
                        Some(WPARAM(setting)),
                        Some(LPARAM(0)),
                    )
                    .0 != 0
                };
                let clock = on(crate::settings::CLOCK);
                let lock = on(crate::settings::LOCK_SCREEN);
                for (id, checked, label) in [
                    (ID_CLOCK, clock, w!("Show clock")),
                    (ID_LOCK, lock, w!("Show on lock screen")),
                ] {
                    let flags = if checked {
                        MF_STRING | MF_CHECKED
                    } else {
                        MF_STRING
                    };
                    let _ = AppendMenuW(menu, flags, id, label);
                }
                let _ = AppendMenuW(menu, MF_STRING, ID_SAVER, w!("Install screen saver…"));
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                let autostart = crate::autostart::enabled();
                let flags = if autostart {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                };
                let _ = AppendMenuW(menu, flags, ID_AUTOSTART, w!("Start with Windows"));
                let pause = if USER_PAUSED.load(Ordering::Relaxed) {
                    w!("Resume")
                } else {
                    w!("Pause")
                };
                let _ = AppendMenuW(menu, MF_STRING, ID_PAUSE, pause);
                let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit"));
                let mut pt = Default::default();
                let _ = GetCursorPos(&mut pt);
                let _ = SetForegroundWindow(hwnd); // or the menu never dismisses
                let cmd = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    pt.x,
                    pt.y,
                    Some(0),
                    hwnd,
                    None,
                );
                let _ = DestroyMenu(menu); // and its submenus
                match cmd.0 as usize {
                    ID_PAUSE => {
                        let _ = PostMessageW(Some(target), WM_TOGGLE_PAUSE, WPARAM(0), LPARAM(0));
                    }
                    ID_SETTINGS => crate::settings::open(target),
                    ID_CLOCK | ID_LOCK => {
                        // Toggle and save (value 0 or 1000).
                        let (setting, on) = if cmd.0 as usize == ID_CLOCK {
                            (crate::settings::CLOCK, clock)
                        } else {
                            (crate::settings::LOCK_SCREEN, lock)
                        };
                        let _ = PostMessageW(
                            Some(target),
                            crate::settings::WM_TWEAK,
                            WPARAM(setting | 1 << 8),
                            LPARAM(if on { 0 } else { 1000 }),
                        );
                    }
                    ID_SAVER => {
                        if let Err(e) = crate::saver::install() {
                            log!("screen saver: {e}");
                        }
                    }
                    ID_AUTOSTART => {
                        if let Err(e) = crate::autostart::set(!autostart) {
                            log!("autostart: {e}");
                        }
                    }
                    ID_QUIT => {
                        let _ = PostMessageW(Some(target), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    id if id >= ID_ANIM_BASE => {
                        let _ = PostMessageW(
                            Some(target),
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
            // Explorer restarted; re-add the icon.
            add_icon(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
