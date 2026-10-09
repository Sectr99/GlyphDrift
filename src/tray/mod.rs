//! Notification-area icon with a Pause/Resume and Quit menu.
//!
//! Uses its own hidden top-level window: the wallpaper window is a child of WorkerW and
//! cannot reliably take foreground focus, which popup menus require.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{w, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, GetWindowLongPtrW, LoadIconW, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, CREATESTRUCTW,
    GWLP_USERDATA, IDI_APPLICATION, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE,
    WM_APP, WM_CLOSE, WM_DESTROY, WM_NCCREATE, WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
};

/// Posted to the wallpaper window when the user picks Pause/Resume.
pub const WM_TOGGLE_PAUSE: u32 = WM_APP + 2;
const WM_TRAY: u32 = WM_APP + 1;
const ID_PAUSE: usize = 1;
const ID_QUIT: usize = 2;
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

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
        add_icon(hwnd);
        Ok(hwnd)
    }
}

pub fn destroy(tray: HWND) {
    // SAFETY: `tray` was created by `create` on this thread.
    unsafe {
        remove_icon(tray);
        let _ = DestroyWindow(tray);
    }
}

unsafe fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
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
                let _ = DestroyMenu(menu);
                match cmd.0 as usize {
                    ID_PAUSE => {
                        let _ = PostMessageW(target, WM_TOGGLE_PAUSE, WPARAM(0), LPARAM(0));
                    }
                    ID_QUIT => {
                        let _ = PostMessageW(target, WM_CLOSE, WPARAM(0), LPARAM(0));
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
