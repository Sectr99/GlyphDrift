//! Desktop embedding: attach a window behind the desktop icons via Progman/WorkerW.
//!
//! The 0x052C Progman message is undocumented and Explorer's window layout has
//! changed between Windows builds, so every version-specific assumption lives here.

use windows::core::{w, Result};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetSystemMetrics, SendMessageTimeoutW, SetWindowPos,
    SMTO_NORMAL, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOZORDER,
};

/// Find (or ask Explorer to create) the window our wallpaper should be parented to.
pub fn find_wallpaper_parent() -> Result<HWND> {
    // SAFETY: plain Win32 calls with valid arguments; all out-pointers point at live locals.
    unsafe {
        let progman = FindWindowW(w!("Progman"), None)?;

        // Ask Explorer to spawn a WorkerW behind the icons.
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

        // Newer layout (Windows 11 24H2+): Progman itself hosts SHELLDLL_DefView and has
        // WS_EX_NOREDIRECTIONBITMAP, so only layered children render. We become a layered
        // child of Progman and `place` keeps us directly below the icons.
        if FindWindowExW(progman, None, w!("SHELLDLL_DefView"), None).is_ok() {
            return Ok(progman);
        }

        // Classic layout (Windows 10, early Windows 11): a top-level WorkerW sibling that
        // follows the WorkerW hosting SHELLDLL_DefView.
        let mut found = HWND::default();
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut found as *mut HWND as isize));
        if !found.is_invalid() {
            return Ok(found);
        }

        // Last resort: parent directly to Progman. Icons may draw over/under us oddly.
        Ok(progman)
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam is the address of the `HWND` local in `find_wallpaper_parent`,
    // which outlives the synchronous EnumWindows call.
    let out = &mut *(lparam.0 as *mut HWND);
    if FindWindowExW(hwnd, None, w!("SHELLDLL_DefView"), None).is_ok() {
        if let Ok(worker) = FindWindowExW(None, hwnd, w!("WorkerW"), None) {
            *out = worker;
            return BOOL(0); // stop enumerating
        }
    }
    BOOL(1)
}

/// Bounding rectangle of all monitors, in screen coordinates.
pub fn virtual_screen() -> RECT {
    // SAFETY: GetSystemMetrics has no preconditions.
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

/// The primary monitor as (x, y, w, h) in the wallpaper window's pixels. The window spans the
/// virtual screen, whose origin can be negative when a monitor sits left of or above the
/// primary; the primary itself always starts at screen (0, 0).
pub fn primary_in_window() -> (f32, f32, f32, f32) {
    let v = virtual_screen();
    // SAFETY: GetSystemMetrics has no preconditions.
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (-v.left as f32, -v.top as f32, w as f32, h as f32)
}

/// Size `hwnd` to the virtual screen within `parent`. When the icons share our parent
/// (24H2+ layout), also put `hwnd` directly below them in z-order.
pub fn place(hwnd: HWND, parent: HWND) {
    let (x, y, w, h) = rect_in_parent(parent);
    // SAFETY: plain Win32 calls on window handles; failures are harmless no-ops.
    unsafe {
        let _ = match FindWindowExW(parent, None, w!("SHELLDLL_DefView"), None) {
            Ok(icons) => SetWindowPos(hwnd, icons, x, y, w, h, SWP_NOACTIVATE),
            Err(_) => SetWindowPos(hwnd, None, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER),
        };
    }
}

/// Virtual-screen rectangle converted to `parent`'s client coordinates (x, y, w, h).
pub fn rect_in_parent(parent: HWND) -> (i32, i32, i32, i32) {
    let r = virtual_screen();
    let mut origin = POINT {
        x: r.left,
        y: r.top,
    };
    // SAFETY: `origin` is a valid POINT; a failure leaves it in screen coordinates.
    unsafe {
        let _ = ScreenToClient(parent, &mut origin);
    }
    (origin.x, origin.y, r.right - r.left, r.bottom - r.top)
}
