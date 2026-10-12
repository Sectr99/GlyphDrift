//! Desktop embedding behind the icons via Progman/WorkerW. The 0x052C message is
//! undocumented and Explorer's layout varies by build, so those assumptions live here.

use windows::core::{w, Result, BOOL};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetSystemMetrics, SendMessageTimeoutW, SetWindowPos,
    SMTO_NORMAL, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOZORDER,
};

/// The window to parent the wallpaper to.
pub fn find_wallpaper_parent() -> Result<HWND> {
    // SAFETY: valid arguments; out-pointers are live locals.
    unsafe {
        let progman = FindWindowW(w!("Progman"), None)?;

        // Ask Explorer for a WorkerW behind the icons.
        let mut result = 0usize;
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0xD),
            LPARAM(0x1),
            SMTO_NORMAL,
            1000,
            Some(&mut result),
        );

        // Windows 11 24H2+: Progman hosts the icons and only layered children render, so we
        // become one, kept directly below the icons by `place`.
        if FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), None).is_ok() {
            return Ok(progman);
        }

        // Windows 10 and early 11: the WorkerW after the one hosting the icons.
        let mut found = HWND::default();
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut found as *mut HWND as isize));
        if !found.is_invalid() {
            return Ok(found);
        }

        // Last resort: Progman itself.
        Ok(progman)
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam points at a local that outlives the synchronous EnumWindows call.
    let out = &mut *(lparam.0 as *mut HWND);
    if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None).is_ok() {
        if let Ok(worker) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), None) {
            *out = worker;
            return BOOL(0); // stop
        }
    }
    BOOL(1)
}

/// Bounds of all monitors, screen coordinates.
pub fn virtual_screen() -> RECT {
    // SAFETY: no preconditions.
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        RECT {
            left: x,
            top: y,
            right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// The primary monitor (x, y, w, h) in window pixels. The window spans the virtual
/// screen, whose origin may be negative; the primary starts at screen (0, 0).
pub fn primary_in_window() -> (f32, f32, f32, f32) {
    let v = virtual_screen();
    // SAFETY: no preconditions.
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (-v.left as f32, -v.top as f32, w as f32, h as f32)
}

/// Size `hwnd` to the virtual screen; on 24H2+ also put it directly below the icons.
pub fn place(hwnd: HWND, parent: HWND) {
    let (x, y, w, h) = rect_in_parent(parent);
    // SAFETY: window calls; failures are harmless.
    unsafe {
        let _ = match FindWindowExW(Some(parent), None, w!("SHELLDLL_DefView"), None) {
            Ok(icons) => SetWindowPos(hwnd, Some(icons), x, y, w, h, SWP_NOACTIVATE),
            Err(_) => SetWindowPos(hwnd, None, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER),
        };
    }
}

/// Virtual screen in `parent`'s client coordinates (x, y, w, h).
pub fn rect_in_parent(parent: HWND) -> (i32, i32, i32, i32) {
    let r = virtual_screen();
    let mut origin = POINT {
        x: r.left,
        y: r.top,
    };
    // SAFETY: `origin` is a valid POINT.
    unsafe {
        let _ = ScreenToClient(parent, &mut origin);
    }
    (origin.x, origin.y, r.right - r.left, r.bottom - r.top)
}
