//! The settings window: animation, theme (with color swatches) and live sliders for speed,
//! frame rate, glass and scale. Standard Win32 controls, so keyboard navigation and screen
//! readers work for free. Changes apply as you drag and are saved when you let go; the window
//! talks to the wallpaper window with messages, like the tray menu.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    BOOL, COLORREF, ERROR_SUCCESS, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, CreateSolidBrush, DeleteObject, DrawTextW, FillRect, GetSysColor,
    GetSysColorBrush, InvalidateRect, SetBkColor, SetBkMode, SetTextColor, COLOR_BTNFACE,
    COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, DT_LEFT,
    DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC, HGDIOBJ, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, SetWindowTheme, DRAWITEMSTRUCT, ICC_BAR_CLASSES, INITCOMMONCONTROLSEX,
    MEASUREITEMSTRUCT, ODS_COMBOBOXEDIT, ODS_SELECTED, TBM_SETPOS, TBM_SETRANGE, TB_ENDTRACK,
    TRACKBAR_CLASSW,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    GetDlgCtrlID, GetDlgItem, IsWindow, LoadIconW, PostMessageW, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetWindowTextW, ShowWindow, SystemParametersInfoW, BM_GETCHECK,
    BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CBS_DROPDOWNLIST, CBS_HASSTRINGS,
    CBS_OWNERDRAWFIXED, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, HMENU, LBS_NOTIFY,
    LBS_OWNERDRAWFIXED, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL, NONCLIENTMETRICSW,
    SPI_GETNONCLIENTMETRICS, SPI_GETWORKAREA, SW_RESTORE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLORLISTBOX,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_ERASEBKGND, WM_HSCROLL, WM_MEASUREITEM,
    WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CONTROLPARENT, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::anim::ANIMATIONS;
use crate::theme::{gradient, THEMES};
use crate::tray::{unpack_state, WM_GET_STATE, WM_SET_ANIMATION, WM_SET_THEME};

/// Sent to the wallpaper window: WPARAM = tweak id | commit << 8, LPARAM = value * 1000.
/// `commit` (set when a slider is released) means "save to the config too".
pub const WM_TWEAK: u32 = WM_APP + 9;
/// Sent to the wallpaper window: WPARAM = tweak id; returns value * 1000.
pub const WM_GET_TWEAK: u32 = WM_APP + 10;
/// Posted to the settings window when the wallpaper's settings change elsewhere (the tray).
pub const WM_REFRESH: u32 = WM_APP + 12;
pub const SPEED: usize = 0;
pub const FPS: usize = 1;
pub const GLASS: usize = 2;
pub const SCALE: usize = 3;
pub const OBJECT_SCALE: usize = 4;
/// Clock on/off (value 1 or 0); a checkbox, not a slider.
pub const CLOCK: usize = 5;

/// (tweak id, label, slider min, slider max, default value)
const SLIDERS: [(usize, &str, i32, i32, f32); 5] = [
    (SPEED, "Speed", 10, 400, 1.0),
    (FPS, "Frame rate", 9, 60, 0.0), // position 9 means Auto
    (GLASS, "Glass", 0, 90, 0.0),
    (SCALE, "Overall scale", 50, 200, 1.0),
    (OBJECT_SCALE, "Object scale", 50, 200, 1.0),
];

const ID_ANIMATION: usize = 100;
const ID_THEME: usize = 101;
const ID_RESET: usize = 102;
const ID_CLOCK: usize = 103;
const ID_CLOCK_LABEL: usize = 104;
const SS_NOTIFY: u32 = 0x100;
const ID_CLOSE: usize = 2; // IDCANCEL, so Esc closes the window
const ID_SLIDER: usize = 200; // + tweak id
const ID_VALUE: usize = 300; // + tweak id
const ID_LABEL: usize = 400; // + tweak id
const TBM_GETPOS: u32 = 0x400; // WM_USER; not exported by the windows crate
const CBN_SELCHANGE: usize = 1;
const LBN_SELCHANGE: usize = 1;
const THEME_ROWS: i32 = 8;
const ROW_H: i32 = 30;
const CLIENT_W: i32 = 520;

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static FONT: AtomicIsize = AtomicIsize::new(0);
/// Last selected row of the animation dropdown, to step over headings in the same direction.
static LAST_ROW: AtomicIsize = AtomicIsize::new(0);
static DARK: AtomicBool = AtomicBool::new(false);
static BG_BRUSH: AtomicIsize = AtomicIsize::new(0);
static SURFACE_BRUSH: AtomicIsize = AtomicIsize::new(0);

/// Window colors: Windows' own in light mode, Windows 11's dark palette in dark mode.
struct Palette {
    /// Window background.
    bg: COLORREF,
    /// List and dropdown background.
    surface: COLORREF,
    text: COLORREF,
}

fn palette() -> Palette {
    if DARK.load(Ordering::Relaxed) {
        Palette {
            bg: COLORREF(0x202020),
            surface: COLORREF(0x2b2b2b),
            text: COLORREF(0xf0f0f0),
        }
    } else {
        // SAFETY: no preconditions.
        unsafe {
            Palette {
                bg: COLORREF(GetSysColor(COLOR_BTNFACE)),
                surface: COLORREF(GetSysColor(COLOR_WINDOW)),
                text: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
            }
        }
    }
}

/// True when Windows is set to dark mode for apps (Settings > Personalization > Colors).
fn apps_use_dark() -> bool {
    let (mut light, mut size) = (1u32, 4u32);
    // SAFETY: `light` and `size` are live locals; `size` holds the buffer size in bytes.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut light as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    r == ERROR_SUCCESS && light == 0
}

fn brush(slot: &AtomicIsize) -> HBRUSH {
    HBRUSH(slot.load(Ordering::Relaxed) as *mut _)
}

fn slider_pos(id: usize, value: f32) -> i32 {
    match id {
        FPS if value < 1.0 => 9,
        FPS => value.round() as i32,
        _ => (value * 100.0).round() as i32,
    }
}

fn slider_value(id: usize, pos: i32) -> f32 {
    match id {
        FPS if pos <= 9 => 0.0,
        FPS => pos as f32,
        _ => pos as f32 / 100.0,
    }
}

fn value_label(id: usize, value: f32) -> String {
    match id {
        SPEED => format!("{value:.2}×"),
        FPS if value < 1.0 => "Auto".into(),
        FPS => format!("{value:.0} fps"),
        _ => format!("{:.0}%", value * 100.0),
    }
}

/// One row of the animation dropdown.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    /// A group heading (not selectable).
    Heading(&'static str),
    /// An index into `ANIMATIONS`.
    Anim(usize),
}

/// The animation dropdown: a heading wherever the group changes, then its animations.
fn anim_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for (i, &(_, _, group)) in ANIMATIONS.iter().enumerate() {
        if i == 0 || ANIMATIONS[i - 1].2 != group {
            rows.push(Row::Heading(match group {
                "ascii" => "ASCII",
                "graphics" => "Graphics",
                "loops" => "Loops",
                "fluid" => "Fluid",
                "contour" => "Contour",
                other => other,
            }));
        }
        rows.push(Row::Anim(i));
    }
    rows
}

fn row_of_anim(anim: usize) -> usize {
    anim_rows()
        .iter()
        .position(|&r| r == Row::Anim(anim))
        .unwrap_or(1)
}

/// The nearest animation row to `row`, searching in the direction of travel first.
fn nearest_anim_row(rows: &[Row], row: usize, up: bool) -> usize {
    let is_anim = |i: &usize| matches!(rows.get(*i), Some(Row::Anim(_)));
    let below = || (row..rows.len()).find(is_anim);
    let above = || (0..=row.min(rows.len() - 1)).rev().find(is_anim);
    let found = if up {
        above().or_else(below)
    } else {
        below().or_else(above)
    };
    found.unwrap_or(row)
}

/// Which optional controls apply to animation `anim`: (object scale, clock).
fn applies(anim: usize) -> (bool, bool) {
    let (name, _, group) = ANIMATIONS.get(anim).copied().unwrap_or(ANIMATIONS[0]);
    (group == "loops", name == "topo")
}

fn dpi_scale() -> f32 {
    // SAFETY: no preconditions.
    unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0
}

/// The settings window, if open (for keyboard navigation in the message loop).
pub fn window() -> HWND {
    HWND(WINDOW.load(Ordering::Relaxed) as *mut _)
}

fn target() -> HWND {
    HWND(TARGET.load(Ordering::Relaxed) as *mut _)
}

/// Open the settings window for the wallpaper window `wallpaper`, or bring it forward.
pub fn open(wallpaper: HWND) {
    // SAFETY: standard window creation on the UI thread; the class is registered once and
    // re-registering just fails harmlessly.
    unsafe {
        let existing = window();
        if !existing.is_invalid() && IsWindow(existing).as_bool() {
            let _ = ShowWindow(existing, SW_RESTORE);
            let _ = SetForegroundWindow(existing);
            return;
        }
        TARGET.store(wallpaper.0 as isize, Ordering::Relaxed);
        // ponytail: read once per open; a theme switch applies the next time it opens.
        DARK.store(apps_use_dark(), Ordering::Relaxed);
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);
        let Ok(hinst) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("WallMotionSettings");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: hinst.into(),
            lpszClassName: class,
            // MAKEINTRESOURCE(1): the exe icon (resource id 1), an integer passed as a pointer.
            hIcon: LoadIconW(hinst, PCWSTR(std::ptr::without_provenance(1))).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as isize as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let s = dpi_scale();
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: px(CLIENT_W, s),
            bottom: px(client_height(), s),
        };
        let _ = AdjustWindowRectEx(&mut rect, style, false, WS_EX_CONTROLPARENT);
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        // Bottom-right, by the tray, so the wallpaper's centre stays visible as a preview.
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut RECT as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let gap = px(12, s);
        let (x, y) = (
            (work.right - w - gap).max(work.left),
            (work.bottom - h - gap).max(work.top),
        );
        let _ = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class,
            w!("WallMotion Settings"),
            style | WS_VISIBLE,
            x,
            y,
            w,
            h,
            None,
            None,
            hinst,
            None,
        );
    }
}

fn px(v: i32, s: f32) -> i32 {
    (v as f32 * s).round() as i32
}

const M: i32 = 16; // margin
const LABEL_H: i32 = 20;

/// Client height at 96 DPI, matching the layout in `build`.
fn client_height() -> i32 {
    M + (LABEL_H + 4)
        + 34
        + (LABEL_H + 4)
        + THEME_ROWS * ROW_H
        + 4
        + 14
        + SLIDERS.len() as i32 * 36
        + 28
        + 8
        + 28
        + M
}

/// Create the controls (96-DPI layout scaled by `s`) and load the current settings.
unsafe fn build(hwnd: HWND) {
    let s = dpi_scale();
    let Ok(hinst) = GetModuleHandleW(None) else {
        return;
    };
    let font = {
        let mut ncm = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        let _ = SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        CreateFontIndirectW(&ncm.lfMessageFont)
    };
    FONT.store(font.0 as isize, Ordering::Relaxed);
    let colors = palette();
    BG_BRUSH.store(CreateSolidBrush(colors.bg).0 as isize, Ordering::Relaxed);
    SURFACE_BRUSH.store(
        CreateSolidBrush(colors.surface).0 as isize,
        Ordering::Relaxed,
    );
    let dark = DARK.load(Ordering::Relaxed);
    if dark {
        let on = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &on as *const BOOL as *const _,
            std::mem::size_of::<BOOL>() as u32,
        );
    }
    let control = |class: PCWSTR,
                   text: &str,
                   style: WINDOW_STYLE,
                   (x, y, w, h): (i32, i32, i32, i32),
                   id: usize| {
        let child = CreateWindowExW(
            Default::default(),
            class,
            &HSTRING::from(text),
            WS_CHILD | WS_VISIBLE | style,
            px(x, s),
            px(y, s),
            px(w, s),
            px(h, s),
            hwnd,
            HMENU(id as *mut _),
            hinst,
            None,
        )
        .unwrap_or_default();
        SendMessageW(child, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        if dark {
            // Windows' own dark control styles (as in Explorer and the file dialogs).
            match class.to_string().unwrap_or_default().as_str() {
                "STATIC" => {}
                "COMBOBOX" => {
                    let _ = SetWindowTheme(child, w!("DarkMode_CFD"), PCWSTR::null());
                }
                _ => {
                    let _ = SetWindowTheme(child, w!("DarkMode_Explorer"), PCWSTR::null());
                }
            }
        }
        child
    };
    let full = CLIENT_W - 2 * M;
    let mut y = M;
    control(
        w!("STATIC"),
        "Animation",
        WINDOW_STYLE(0),
        (M, y, full, LABEL_H),
        0,
    );
    y += LABEL_H + 4;
    let combo = control(
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32)
            | WS_TABSTOP
            | WS_VSCROLL,
        (M, y, full, 480),
        ID_ANIMATION,
    );
    for row in anim_rows() {
        // Strings too, so screen readers can read the owner-drawn rows.
        let text = match row {
            Row::Heading(name) => name,
            Row::Anim(i) => ANIMATIONS[i].1,
        };
        SendMessageW(
            combo,
            CB_ADDSTRING,
            WPARAM(0),
            LPARAM(HSTRING::from(text).as_ptr() as isize),
        );
    }
    y += 34;
    control(
        w!("STATIC"),
        "Theme",
        WINDOW_STYLE(0),
        (M, y, full, LABEL_H),
        0,
    );
    y += LABEL_H + 4;
    let list = control(
        w!("LISTBOX"),
        "",
        WINDOW_STYLE((LBS_OWNERDRAWFIXED | LBS_NOTIFY) as u32) | WS_TABSTOP | WS_VSCROLL,
        (M, y, full, THEME_ROWS * ROW_H + 4),
        ID_THEME,
    );
    for i in 0..THEMES.len() {
        SendMessageW(list, LB_ADDSTRING, WPARAM(0), LPARAM(i as isize)); // owner-drawn: data only
    }
    y += THEME_ROWS * ROW_H + 4 + 14;
    for (id, label, min, max, _) in SLIDERS {
        control(
            w!("STATIC"),
            label,
            WINDOW_STYLE(0),
            (M, y + 6, 130, LABEL_H),
            ID_LABEL + id,
        );
        let slider = control(
            TRACKBAR_CLASSW,
            "",
            WS_TABSTOP,
            (M + 130, y, full - 130 - 74, 30),
            ID_SLIDER + id,
        );
        SendMessageW(
            slider,
            TBM_SETRANGE,
            WPARAM(1),
            LPARAM(((max << 16) | min) as isize),
        );
        control(
            w!("STATIC"),
            "",
            WINDOW_STYLE(2),
            (CLIENT_W - M - 70, y + 6, 70, LABEL_H),
            ID_VALUE + id,
        ); // SS_RIGHT
        y += 36;
    }
    // The checkbox is only as wide as its box (its text stays, for screen readers) and a
    // clickable label sits beside it: themed checkboxes ignore the dark text color.
    control(
        w!("BUTTON"),
        "Show clock (Topographic drift)",
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        (M, y, 14, 24),
        ID_CLOCK,
    );
    control(
        w!("STATIC"),
        "Show clock (Topographic drift)",
        WINDOW_STYLE(SS_NOTIFY),
        (M + 20, y + 3, full - 20, LABEL_H),
        ID_CLOCK_LABEL,
    );
    y += 28 + 8;
    control(
        w!("BUTTON"),
        "Reset to defaults",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        (M, y, 150, 28),
        ID_RESET,
    );
    control(
        w!("BUTTON"),
        "Close",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        (CLIENT_W - M - 100, y, 100, 28),
        ID_CLOSE,
    );
    load(hwnd);
}

/// Reflect the wallpaper's current settings in the controls.
unsafe fn load(hwnd: HWND) {
    let (anim, theme) = unpack_state(SendMessageW(target(), WM_GET_STATE, WPARAM(0), LPARAM(0)).0);
    let row = row_of_anim(anim);
    LAST_ROW.store(row as isize, Ordering::Relaxed);
    SendMessageW(
        GetDlgItem(hwnd, ID_ANIMATION as i32).unwrap_or_default(),
        CB_SETCURSEL,
        WPARAM(row),
        LPARAM(0),
    );
    SendMessageW(
        GetDlgItem(hwnd, ID_THEME as i32).unwrap_or_default(),
        LB_SETCURSEL,
        WPARAM(theme),
        LPARAM(0),
    );
    // Enable first: `show` labels a disabled slider "Loops only" instead of its value.
    set_applicable(hwnd, anim);
    for (id, ..) in SLIDERS {
        show(hwnd, id, get_tweak(id));
    }
    let clock = get_tweak(CLOCK) != 0.0;
    SendMessageW(
        GetDlgItem(hwnd, ID_CLOCK as i32).unwrap_or_default(),
        BM_SETCHECK,
        WPARAM(usize::from(clock)),
        LPARAM(0),
    );
}

fn get_tweak(id: usize) -> f32 {
    // SAFETY: plain message send to the wallpaper window.
    unsafe { SendMessageW(target(), WM_GET_TWEAK, WPARAM(id), LPARAM(0)).0 as f32 / 1000.0 }
}

/// Grey out the controls that do nothing for animation `anim`.
unsafe fn set_applicable(hwnd: HWND, anim: usize) {
    let (object_scale, clock) = applies(anim);
    for (id, on) in [(ID_SLIDER + OBJECT_SCALE, object_scale), (ID_CLOCK, clock)] {
        if let Ok(control) = GetDlgItem(hwnd, id as i32) {
            let _ = EnableWindow(control, on);
        }
    }
    // Their labels stay enabled (disabled statics draw embossed, which looks broken on
    // dark backgrounds) and are muted instead; see WM_CTLCOLORSTATIC.
    for id in [
        ID_LABEL + OBJECT_SCALE,
        ID_VALUE + OBJECT_SCALE,
        ID_CLOCK_LABEL,
    ] {
        if let Ok(label) = GetDlgItem(hwnd, id as i32) {
            let _ = InvalidateRect(label, None, true);
        }
    }
}

/// True if the label `id` belongs to a control that is greyed out.
unsafe fn label_inactive(hwnd: HWND, id: usize) -> bool {
    let owner = match id {
        _ if id == ID_LABEL + OBJECT_SCALE || id == ID_VALUE + OBJECT_SCALE => {
            ID_SLIDER + OBJECT_SCALE
        }
        ID_CLOCK_LABEL => ID_CLOCK,
        _ => return false,
    };
    GetDlgItem(hwnd, owner as i32).is_ok_and(|c| !IsWindowEnabled(c).as_bool())
}

/// Move slider `id` to `value` and update its readout.
unsafe fn show(hwnd: HWND, id: usize, value: f32) {
    if let Ok(slider) = GetDlgItem(hwnd, (ID_SLIDER + id) as i32) {
        SendMessageW(
            slider,
            TBM_SETPOS,
            WPARAM(1),
            LPARAM(slider_pos(id, value) as isize),
        );
    }
    if let Ok(label) = GetDlgItem(hwnd, (ID_VALUE + id) as i32) {
        let text = if label_inactive(hwnd, ID_VALUE + id) {
            "Loops only".into()
        } else {
            value_label(id, value)
        };
        let _ = SetWindowTextW(label, &HSTRING::from(text));
    }
}

/// One row of the animation dropdown: headings in muted text, animations indented below.
unsafe fn draw_anim_row(d: &DRAWITEMSTRUCT) {
    let rows = anim_rows();
    let Some(&row) = rows.get(d.itemID as usize) else {
        FillRect(d.hDC, &d.rcItem, brush(&SURFACE_BRUSH)); // empty selection
        return;
    };
    let in_field = d.itemState.0 & ODS_COMBOBOXEDIT.0 != 0;
    let selected = d.itemState.0 & ODS_SELECTED.0 != 0 && matches!(row, Row::Anim(_));
    if selected {
        FillRect(d.hDC, &d.rcItem, GetSysColorBrush(COLOR_HIGHLIGHT));
    } else {
        FillRect(d.hDC, &d.rcItem, brush(&SURFACE_BRUSH));
    }
    SetBkMode(d.hDC, TRANSPARENT);
    let s = dpi_scale();
    let (text, color, indent) = match row {
        Row::Heading(name) => (name, muted_text(), 6),
        Row::Anim(i) => (
            ANIMATIONS[i].1,
            if selected {
                COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT))
            } else {
                palette().text
            },
            if in_field { 6 } else { 18 },
        ),
    };
    SetTextColor(d.hDC, color);
    let mut rc = RECT {
        left: d.rcItem.left + px(indent, s),
        ..d.rcItem
    };
    let mut text: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(
        d.hDC,
        &mut text,
        &mut rc,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
    );
}

/// Secondary text: group headings.
fn muted_text() -> COLORREF {
    if DARK.load(Ordering::Relaxed) {
        COLORREF(0x9a9a9a)
    } else {
        // SAFETY: no preconditions.
        COLORREF(unsafe { GetSysColor(COLOR_GRAYTEXT) })
    }
}

fn tweak(id: usize, value: f32, commit: bool) {
    let wp = id | usize::from(commit) << 8;
    // SAFETY: plain message post to the wallpaper window.
    unsafe {
        let _ = PostMessageW(
            target(),
            WM_TWEAK,
            WPARAM(wp),
            LPARAM((value * 1000.0).round() as isize),
        );
    }
}

/// One theme row: its name, then swatches of its background, cool and warm gradients.
unsafe fn draw_theme_row(d: &DRAWITEMSTRUCT) {
    let Some(theme) = THEMES.get(d.itemID as usize) else {
        return;
    };
    let selected = d.itemState.0 & ODS_SELECTED.0 != 0;
    let rc = d.rcItem;
    if selected {
        FillRect(d.hDC, &rc, GetSysColorBrush(COLOR_HIGHLIGHT));
    } else {
        FillRect(d.hDC, &rc, brush(&SURFACE_BRUSH));
    }
    SetBkMode(d.hDC, TRANSPARENT);
    SetTextColor(
        d.hDC,
        if selected {
            COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT))
        } else {
            palette().text
        },
    );
    let s = dpi_scale();
    let name_w = px(150, s);
    let mut text_rc = RECT {
        left: rc.left + px(8, s),
        right: rc.left + name_w,
        ..rc
    };
    let mut name: Vec<u16> = theme.label.encode_utf16().collect();
    DrawTextW(
        d.hDC,
        &mut name,
        &mut text_rc,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
    );

    let swatch = |x0: i32, x1: i32, c: crate::config::Rgb| {
        let r = RECT {
            left: x0,
            top: rc.top + px(5, s),
            right: x1,
            bottom: rc.bottom - px(5, s),
        };
        let brush = CreateSolidBrush(COLORREF(
            c.0 as u32 | (c.1 as u32) << 8 | (c.2 as u32) << 16,
        ));
        FillRect(d.hDC, &r, brush);
        let _ = DeleteObject(brush);
    };
    let (left, right) = (rc.left + name_w, rc.right - px(8, s));
    let bg_w = px(26, s);
    let gap = px(6, s);
    let cell = (right - left - bg_w - 2 * gap) as f32 / 32.0;
    swatch(left, left + bg_w, theme.bg);
    for (row, stops) in [(0, &theme.cool), (1, &theme.warm)] {
        let x0 = left + bg_w + gap + row * (16.0 * cell) as i32 + row * gap;
        for (i, c) in gradient(theme.bg, stops, 16).into_iter().enumerate() {
            swatch(
                x0 + (i as f32 * cell) as i32,
                x0 + ((i + 1) as f32 * cell) as i32,
                c,
            );
        }
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
            build(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            FillRect(HDC(wp.0 as *mut _), &rc, brush(&BG_BRUSH));
            LRESULT(1)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLORLISTBOX => {
            let colors = palette();
            let (bg, slot) = if msg == WM_CTLCOLORLISTBOX {
                (colors.surface, &SURFACE_BRUSH)
            } else {
                (colors.bg, &BG_BRUSH)
            };
            let hdc = HDC(wp.0 as *mut _);
            let id = GetDlgCtrlID(HWND(lp.0 as *mut _)) as usize;
            SetTextColor(
                hdc,
                if label_inactive(hwnd, id) {
                    muted_text()
                } else {
                    colors.text
                },
            );
            SetBkColor(hdc, bg);
            LRESULT(brush(slot).0 as isize)
        }
        WM_MEASUREITEM => {
            let m = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
            let h = if m.CtlID as usize == ID_ANIMATION {
                24
            } else {
                ROW_H
            };
            m.itemHeight = px(h, dpi_scale()) as u32;
            LRESULT(1)
        }
        WM_DRAWITEM => {
            let d = &*(lp.0 as *const DRAWITEMSTRUCT);
            if d.CtlID as usize == ID_ANIMATION {
                draw_anim_row(d);
            } else {
                draw_theme_row(d);
            }
            LRESULT(1)
        }
        WM_REFRESH => {
            load(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let (id, code) = (wp.0 & 0xFFFF, (wp.0 >> 16) & 0xFFFF);
            let source = HWND(lp.0 as *mut _);
            match id {
                ID_ANIMATION if code == CBN_SELCHANGE => {
                    let sel = SendMessageW(source, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                    if sel >= 0 {
                        // Headings can't be chosen: move on to the nearest animation.
                        let rows = anim_rows();
                        let up = sel < LAST_ROW.load(Ordering::Relaxed);
                        let row = nearest_anim_row(&rows, sel as usize, up);
                        if row != sel as usize {
                            SendMessageW(source, CB_SETCURSEL, WPARAM(row), LPARAM(0));
                        }
                        LAST_ROW.store(row as isize, Ordering::Relaxed);
                        if let Some(Row::Anim(anim)) = rows.get(row) {
                            let _ =
                                PostMessageW(target(), WM_SET_ANIMATION, WPARAM(*anim), LPARAM(0));
                        }
                    }
                }
                ID_THEME if code == LBN_SELCHANGE => {
                    let sel = SendMessageW(source, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                    if sel >= 0 {
                        let _ =
                            PostMessageW(target(), WM_SET_THEME, WPARAM(sel as usize), LPARAM(0));
                    }
                }
                ID_CLOCK | ID_CLOCK_LABEL if code == 0 && !label_inactive(hwnd, ID_CLOCK_LABEL) => {
                    // BN_CLICKED / STN_CLICKED; a label click toggles the checkbox itself.
                    let check = GetDlgItem(hwnd, ID_CLOCK as i32).unwrap_or_default();
                    let mut on = SendMessageW(check, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 != 0;
                    if id == ID_CLOCK_LABEL {
                        on = !on;
                        SendMessageW(check, BM_SETCHECK, WPARAM(usize::from(on)), LPARAM(0));
                    }
                    tweak(CLOCK, f32::from(u8::from(on)), true);
                }
                ID_RESET => {
                    for (sid, _, _, _, default) in SLIDERS {
                        if let Ok(slider) = GetDlgItem(hwnd, (ID_SLIDER + sid) as i32) {
                            // A greyed-out slider's value is invisible; leave it alone.
                            if !IsWindowEnabled(slider).as_bool() {
                                continue;
                            }
                        }
                        show(hwnd, sid, default);
                        tweak(sid, default, true);
                    }
                }
                ID_CLOSE => {
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_HSCROLL => {
            let slider = HWND(lp.0 as *mut _);
            let id = (GetDlgCtrlID(slider) as usize).wrapping_sub(ID_SLIDER);
            if id < SLIDERS.len() {
                let pos = SendMessageW(slider, TBM_GETPOS, WPARAM(0), LPARAM(0)).0 as i32;
                let value = slider_value(id, pos);
                if let Ok(label) = GetDlgItem(hwnd, (ID_VALUE + id) as i32) {
                    let _ = SetWindowTextW(label, &HSTRING::from(value_label(id, value)));
                }
                // Live while dragging; saved when released (or after a keyboard step).
                tweak(id, value, (wp.0 & 0xFFFF) as u32 == TB_ENDTRACK);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            WINDOW.store(0, Ordering::Relaxed);
            for slot in [&FONT, &BG_BRUSH, &SURFACE_BRUSH] {
                let object = slot.swap(0, Ordering::Relaxed);
                if object != 0 {
                    let _ = DeleteObject(HGDIOBJ(object as *mut _));
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_positions_round_trip() {
        for (id, _, min, max, default) in SLIDERS {
            assert!(
                (min..=max).contains(&slider_pos(id, default)),
                "default out of range for {id}"
            );
            for pos in min..=max {
                assert_eq!(
                    slider_pos(id, slider_value(id, pos)),
                    pos,
                    "slider {id} position {pos}"
                );
            }
        }
        assert_eq!(value_label(FPS, 0.0), "Auto");
    }

    #[test]
    fn dropdown_headings_are_skipped() {
        let rows = anim_rows();
        assert_eq!(rows[0], Row::Heading("ASCII"));
        let anims = rows.iter().filter(|r| matches!(r, Row::Anim(_))).count();
        assert_eq!(anims, ANIMATIONS.len());
        for i in 0..ANIMATIONS.len() {
            assert_eq!(rows[row_of_anim(i)], Row::Anim(i));
        }
        let second = rows
            .iter()
            .skip(1)
            .position(|r| matches!(r, Row::Heading(_)))
            .unwrap()
            + 1;
        // Down onto a heading goes to the first animation below it; up goes above it.
        assert_eq!(nearest_anim_row(&rows, second, false), second + 1);
        assert_eq!(nearest_anim_row(&rows, second, true), second - 1);
        // Up onto the very first heading has nowhere to go but down.
        assert_eq!(nearest_anim_row(&rows, 0, true), 1);
        assert_eq!(value_label(SCALE, 1.25), "125%");
    }
}
