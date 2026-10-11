//! Settings window: animation, theme swatches, live sliders and the clock toggle. Standard
//! Win32 controls; changes apply live and save on release, sent as messages like the tray.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::core::{w, HSTRING, PCWSTR, PWSTR};
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
    TOOLTIPS_CLASSW, TRACKBAR_CLASSW, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH,
    TTS_ALWAYSTIP, TTTOOLINFOW,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    GetDlgCtrlID, GetDlgItem, IsWindow, LoadIconW, PostMessageW, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetWindowTextW, ShowWindow, SystemParametersInfoW, BM_GETCHECK,
    BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CBS_DROPDOWNLIST, CBS_HASSTRINGS,
    CBS_OWNERDRAWFIXED, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CW_USEDEFAULT, HMENU,
    LBS_HASSTRINGS, LBS_NOTIFY, LBS_OWNERDRAWVARIABLE, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL,
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SPI_GETWORKAREA, SW_RESTORE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CREATE,
    WM_CTLCOLORBTN, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_ERASEBKGND,
    WM_HSCROLL, WM_MEASUREITEM, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CONTROLPARENT,
    WS_EX_TOPMOST, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

use crate::anim::ANIMATIONS;
use crate::theme::{gradient, THEMES};
use crate::tray::{unpack_state, WM_GET_STATE, WM_SET_ANIMATION, WM_SET_THEME};

/// To the wallpaper: WPARAM = tweak id | commit << 8 (save too), LPARAM = value * 1000.
pub const WM_TWEAK: u32 = WM_APP + 9;
/// To the wallpaper: WPARAM = tweak id; returns value * 1000.
pub const WM_GET_TWEAK: u32 = WM_APP + 10;
/// To this window after a change from the tray.
pub const WM_REFRESH: u32 = WM_APP + 12;
pub const SPEED: usize = 0;
pub const FPS: usize = 1;
pub const GLASS: usize = 2;
pub const SCALE: usize = 3;
pub const OBJECT_SCALE: usize = 4;
/// Clock on/off (1 or 0).
pub const CLOCK: usize = 5;

/// (tweak id, label, slider min, slider max, default value)
const SLIDERS: [(usize, &str, i32, i32, f32); 4] = [
    (SPEED, "Speed", 10, 400, 1.0),
    (GLASS, "Icon readability", 0, 90, 0.0),
    (SCALE, "Overall scale", 50, 200, 1.0),
    (OBJECT_SCALE, "Object scale", 50, 200, 1.0),
];

/// (fps, label); 0 = each animation's own rate.
const FRAME_RATES: [(f32, &str); 5] = [
    (0.0, "Auto (recommended)"),
    (15.0, "15 fps (lightest)"),
    (24.0, "24 fps"),
    (30.0, "30 fps"),
    (60.0, "60 fps (smoothest)"),
];

/// Hover help per setting.
fn hint(id: usize) -> &'static str {
    match id {
        SPEED => "How fast the animation moves.",
        GLASS => {
            "Fades the animation toward the background color so desktop icon labels stay \
             easy to read."
        }
        SCALE => "Size of everything: ASCII characters, line weights and terrain features.",
        OBJECT_SCALE => {
            "Size of the centered shape in the Loops animations, on top of the overall scale."
        }
        FPS => {
            "Auto uses each animation's own rate (20 to 30 fps). Higher is smoother but uses \
             more CPU."
        }
        CLOCK => {
            "Time and date at the center of Topographic drift; the contour lines flow \
             around it."
        }
        _ => "",
    }
}

/// Index of the choice nearest `fps`.
fn frame_rate_index(fps: f32) -> usize {
    (0..FRAME_RATES.len())
        .min_by(|&a, &b| {
            let d = |i: usize| (FRAME_RATES[i].0 - fps).abs();
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(0)
}

const ID_ANIMATION: usize = 100;
const ID_THEME: usize = 101;
const ID_RESET: usize = 102;
const ID_CLOCK: usize = 103;
const ID_CLOCK_LABEL: usize = 104;
const ID_FPS: usize = 105;
const SS_NOTIFY: u32 = 0x100;
const ID_CLOSE: usize = 2; // IDCANCEL, so Esc closes
const ID_SLIDER: usize = 200; // + tweak id
const ID_VALUE: usize = 300; // + tweak id
const ID_LABEL: usize = 400; // + tweak id
const TBM_GETPOS: u32 = 0x400; // WM_USER; not in the windows crate
const CBN_SELCHANGE: usize = 1;
const LBN_SELCHANGE: usize = 1;
const THEME_ROWS: i32 = 8;
const ROW_H: i32 = 30;
const CLIENT_W: i32 = 520;

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static FONT: AtomicIsize = AtomicIsize::new(0);
/// Last selected rows, to step over headings in the direction of travel.
static LAST_ANIM_ROW: AtomicIsize = AtomicIsize::new(0);
static LAST_THEME_ROW: AtomicIsize = AtomicIsize::new(0);
static DARK: AtomicBool = AtomicBool::new(false);
static BG_BRUSH: AtomicIsize = AtomicIsize::new(0);
static SURFACE_BRUSH: AtomicIsize = AtomicIsize::new(0);

/// Windows' colors in light mode, Windows 11's dark palette in dark mode.
struct Palette {
    /// Window.
    bg: COLORREF,
    /// Lists and dropdowns.
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

/// Windows' apps dark mode setting.
fn apps_use_dark() -> bool {
    let (mut light, mut size) = (1u32, 4u32);
    // SAFETY: `light` and `size` are live locals; `size` is the buffer size in bytes.
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

/// Sliders work in hundredths.
fn slider_pos(value: f32) -> i32 {
    (value * 100.0).round() as i32
}

fn slider_value(pos: i32) -> f32 {
    pos as f32 / 100.0
}

fn value_label(id: usize, value: f32) -> String {
    match id {
        SPEED => format!("{value:.2}×"),
        _ => format!("{:.0}%", value * 100.0),
    }
}

/// A row of a grouped list (the animation dropdown, the theme list).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    /// A group heading, not selectable.
    Heading(&'static str),
    /// An index into `ANIMATIONS` or `THEMES`.
    Item(usize),
}

/// A heading wherever the group changes, then the items.
fn grouped_rows(groups: impl Iterator<Item = &'static str>) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut last = None;
    for (i, group) in groups.enumerate() {
        if last != Some(group) {
            rows.push(Row::Heading(match group {
                "ascii" => "ASCII",
                "graphics" => "Graphics",
                "loops" => "Loops",
                "fluid" => "Fluid",
                "contour" => "Contour",
                "terminal" => "Terminal",
                "editor" => "Editor",
                "neon" => "Neon",
                other => other,
            }));
            last = Some(group);
        }
        rows.push(Row::Item(i));
    }
    rows
}

fn anim_rows() -> Vec<Row> {
    grouped_rows(ANIMATIONS.iter().map(|a| a.2))
}

fn theme_rows() -> Vec<Row> {
    grouped_rows(THEMES.iter().map(|t| t.group))
}

fn row_of(rows: &[Row], item: usize) -> usize {
    rows.iter().position(|&r| r == Row::Item(item)).unwrap_or(1)
}

/// Nearest item row, searching in the direction of travel first.
fn nearest_item_row(rows: &[Row], row: usize, up: bool) -> usize {
    let is_item = |i: &usize| matches!(rows.get(*i), Some(Row::Item(_)));
    let below = || (row..rows.len()).find(is_item);
    let above = || (0..=row.min(rows.len() - 1)).rev().find(is_item);
    let found = if up {
        above().or_else(below)
    } else {
        below().or_else(above)
    };
    found.unwrap_or(row)
}

/// Which optional controls apply: (object scale, clock).
fn applies(anim: usize) -> (bool, bool) {
    let (name, _, group) = ANIMATIONS.get(anim).copied().unwrap_or(ANIMATIONS[0]);
    (group == "loops", name == "topo")
}

fn dpi_scale() -> f32 {
    // SAFETY: no preconditions.
    unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0
}

/// The settings window, if open.
pub fn window() -> HWND {
    HWND(WINDOW.load(Ordering::Relaxed) as *mut _)
}

fn target() -> HWND {
    HWND(TARGET.load(Ordering::Relaxed) as *mut _)
}

/// Open the settings window, or bring it forward.
pub fn open(wallpaper: HWND) {
    // SAFETY: window creation on the UI thread; re-registering the class fails harmlessly.
    unsafe {
        let existing = window();
        if !existing.is_invalid() && IsWindow(existing).as_bool() {
            let _ = ShowWindow(existing, SW_RESTORE);
            let _ = SetForegroundWindow(existing);
            return;
        }
        TARGET.store(wallpaper.0 as isize, Ordering::Relaxed);
        // ponytail: read once per open; a theme switch applies on the next open.
        DARK.store(apps_use_dark(), Ordering::Relaxed);
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);
        let Ok(hinst) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("GlyphdriftSettings");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: hinst.into(),
            lpszClassName: class,
            // MAKEINTRESOURCE(1): the exe icon.
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
        // Bottom-right by the tray, so the wallpaper stays visible as a preview.
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
            w!("Glyphdrift Settings"),
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

const M: i32 = 16;
const LABEL_H: i32 = 20;

/// Client height at 96 DPI, matching `build`.
fn client_height() -> i32 {
    M + (LABEL_H + 4)
        + 34
        + (LABEL_H + 4)
        + THEME_ROWS * ROW_H
        + 4
        + 14
        + SLIDERS.len() as i32 * 36
        + 36
        + 28
        + 8
        + 28
        + M
}

/// Create the controls (96-DPI layout scaled by `s`) and load the settings.
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
            // Windows' own dark control styles.
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
        // Strings too, for screen readers.
        let text = match row {
            Row::Heading(name) => name,
            Row::Item(i) => ANIMATIONS[i].1,
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
        WINDOW_STYLE((LBS_OWNERDRAWVARIABLE | LBS_HASSTRINGS | LBS_NOTIFY) as u32)
            | WS_TABSTOP
            | WS_VSCROLL,
        (M, y, full, THEME_ROWS * ROW_H + 4),
        ID_THEME,
    );
    for row in theme_rows() {
        let text = match row {
            Row::Heading(name) => name,
            Row::Item(i) => THEMES[i].label,
        };
        SendMessageW(
            list,
            LB_ADDSTRING,
            WPARAM(0),
            LPARAM(HSTRING::from(text).as_ptr() as isize),
        );
    }
    y += THEME_ROWS * ROW_H + 4 + 14;
    for (id, label, min, max, _) in SLIDERS {
        control(
            w!("STATIC"),
            label,
            WINDOW_STYLE(SS_NOTIFY),
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
    control(
        w!("STATIC"),
        "Frame rate",
        WINDOW_STYLE(SS_NOTIFY),
        (M, y + 6, 130, LABEL_H),
        ID_LABEL + FPS,
    );
    let rates = control(
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_TABSTOP | WS_VSCROLL,
        (M + 130, y + 2, 200, 200),
        ID_FPS,
    );
    for (_, label) in FRAME_RATES {
        SendMessageW(
            rates,
            CB_ADDSTRING,
            WPARAM(0),
            LPARAM(HSTRING::from(label).as_ptr() as isize),
        );
    }
    y += 36;
    // Themed checkboxes ignore the dark text color: the box is sized to hide its own text
    // (kept for screen readers) and a clickable label sits beside it.
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
    add_tooltips(hwnd, s, dark);
    load(hwnd);
}

/// Hover help on each label and control; labels keep it when the control is greyed out.
unsafe fn add_tooltips(hwnd: HWND, s: f32, dark: bool) {
    let Ok(tip) = CreateWindowExW(
        WS_EX_TOPMOST,
        TOOLTIPS_CLASSW,
        None,
        WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP),
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        hwnd,
        None,
        None,
        None,
    ) else {
        return;
    };
    if dark {
        let _ = SetWindowTheme(tip, w!("DarkMode_Explorer"), PCWSTR::null());
    }
    SendMessageW(
        tip,
        TTM_SETMAXTIPWIDTH,
        WPARAM(0),
        LPARAM(px(320, s) as isize),
    );
    let mut tools: Vec<(usize, usize)> = Vec::new(); // (control id, setting id)
    for (id, ..) in SLIDERS {
        tools.extend([
            (ID_LABEL + id, id),
            (ID_SLIDER + id, id),
            (ID_VALUE + id, id),
        ]);
    }
    tools.extend([
        (ID_LABEL + FPS, FPS),
        (ID_FPS, FPS),
        (ID_CLOCK, CLOCK),
        (ID_CLOCK_LABEL, CLOCK),
    ]);
    for (control, setting) in tools {
        let Ok(child) = GetDlgItem(hwnd, control as i32) else {
            continue;
        };
        // The tooltip copies the text.
        let mut text: Vec<u16> = hint(setting).encode_utf16().chain(Some(0)).collect();
        let info = TTTOOLINFOW {
            cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            hwnd,
            uId: child.0 as usize,
            lpszText: PWSTR(text.as_mut_ptr()),
            ..Default::default()
        };
        SendMessageW(
            tip,
            TTM_ADDTOOLW,
            WPARAM(0),
            LPARAM(&info as *const TTTOOLINFOW as isize),
        );
    }
}

/// Show the wallpaper's current settings.
unsafe fn load(hwnd: HWND) {
    let (anim, theme) = unpack_state(SendMessageW(target(), WM_GET_STATE, WPARAM(0), LPARAM(0)).0);
    let row = row_of(&anim_rows(), anim);
    LAST_ANIM_ROW.store(row as isize, Ordering::Relaxed);
    SendMessageW(
        GetDlgItem(hwnd, ID_ANIMATION as i32).unwrap_or_default(),
        CB_SETCURSEL,
        WPARAM(row),
        LPARAM(0),
    );
    let theme_row = row_of(&theme_rows(), theme);
    LAST_THEME_ROW.store(theme_row as isize, Ordering::Relaxed);
    SendMessageW(
        GetDlgItem(hwnd, ID_THEME as i32).unwrap_or_default(),
        LB_SETCURSEL,
        WPARAM(theme_row),
        LPARAM(0),
    );
    // Before `show`, which labels disabled sliders "Loops only".
    set_applicable(hwnd, anim);
    for (id, ..) in SLIDERS {
        show(hwnd, id, get_tweak(id));
    }
    SendMessageW(
        GetDlgItem(hwnd, ID_FPS as i32).unwrap_or_default(),
        CB_SETCURSEL,
        WPARAM(frame_rate_index(get_tweak(FPS))),
        LPARAM(0),
    );
    let clock = get_tweak(CLOCK) != 0.0;
    SendMessageW(
        GetDlgItem(hwnd, ID_CLOCK as i32).unwrap_or_default(),
        BM_SETCHECK,
        WPARAM(usize::from(clock)),
        LPARAM(0),
    );
}

fn get_tweak(id: usize) -> f32 {
    // SAFETY: plain message send.
    unsafe { SendMessageW(target(), WM_GET_TWEAK, WPARAM(id), LPARAM(0)).0 as f32 / 1000.0 }
}

/// Grey out the controls that do nothing for `anim`.
unsafe fn set_applicable(hwnd: HWND, anim: usize) {
    let (object_scale, clock) = applies(anim);
    for (id, on) in [(ID_SLIDER + OBJECT_SCALE, object_scale), (ID_CLOCK, clock)] {
        if let Ok(control) = GetDlgItem(hwnd, id as i32) {
            let _ = EnableWindow(control, on);
        }
    }
    // Labels stay enabled (disabled statics look broken on dark) and are muted instead.
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

/// True if label `id` belongs to a greyed-out control.
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
            LPARAM(slider_pos(value) as isize),
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

/// The item picked in a grouped list (`get`/`set` are its cursel messages). Headings can't
/// be chosen, so the selection moves on to the nearest item in the direction of travel.
unsafe fn pick(
    list: HWND,
    (get, set): (u32, u32),
    rows: &[Row],
    last: &AtomicIsize,
) -> Option<usize> {
    let sel = SendMessageW(list, get, WPARAM(0), LPARAM(0)).0;
    if sel < 0 {
        return None;
    }
    let row = nearest_item_row(rows, sel as usize, sel < last.load(Ordering::Relaxed));
    if row != sel as usize {
        SendMessageW(list, set, WPARAM(row), LPARAM(0));
    }
    last.store(row as isize, Ordering::Relaxed);
    match rows.get(row) {
        Some(&Row::Item(i)) => Some(i),
        _ => None,
    }
}

/// Animation dropdown row: muted headings, indented animations.
unsafe fn draw_anim_row(d: &DRAWITEMSTRUCT) {
    let rows = anim_rows();
    let Some(&row) = rows.get(d.itemID as usize) else {
        FillRect(d.hDC, &d.rcItem, brush(&SURFACE_BRUSH)); // empty selection
        return;
    };
    let in_field = d.itemState.0 & ODS_COMBOBOXEDIT.0 != 0;
    let selected = d.itemState.0 & ODS_SELECTED.0 != 0 && matches!(row, Row::Item(_));
    if selected {
        FillRect(d.hDC, &d.rcItem, GetSysColorBrush(COLOR_HIGHLIGHT));
    } else {
        FillRect(d.hDC, &d.rcItem, brush(&SURFACE_BRUSH));
    }
    SetBkMode(d.hDC, TRANSPARENT);
    let s = dpi_scale();
    let (text, color, indent) = match row {
        Row::Heading(name) => (name, muted_text(), 6),
        Row::Item(i) => (
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

/// Group headings.
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
    // SAFETY: plain message post.
    unsafe {
        let _ = PostMessageW(
            target(),
            WM_TWEAK,
            WPARAM(wp),
            LPARAM((value * 1000.0).round() as isize),
        );
    }
}

/// Theme row: name, then background, cool and warm swatches; or a muted group heading.
unsafe fn draw_theme_row(d: &DRAWITEMSTRUCT) {
    let rc = d.rcItem;
    let theme = match theme_rows().get(d.itemID as usize) {
        Some(&Row::Item(i)) => &THEMES[i],
        Some(&Row::Heading(name)) => {
            FillRect(d.hDC, &rc, brush(&SURFACE_BRUSH));
            SetBkMode(d.hDC, TRANSPARENT);
            SetTextColor(d.hDC, muted_text());
            let mut text_rc = RECT {
                left: rc.left + px(8, dpi_scale()),
                ..rc
            };
            let mut text: Vec<u16> = name.encode_utf16().collect();
            DrawTextW(
                d.hDC,
                &mut text,
                &mut text_rc,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            return;
        }
        None => return,
    };
    let selected = d.itemState.0 & ODS_SELECTED.0 != 0;
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
            let heading = m.CtlID as usize == ID_THEME
                && matches!(theme_rows().get(m.itemID as usize), Some(Row::Heading(_)));
            let h = if m.CtlID as usize == ID_ANIMATION || heading {
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
                    let rows = anim_rows();
                    let picked = pick(source, (CB_GETCURSEL, CB_SETCURSEL), &rows, &LAST_ANIM_ROW);
                    if let Some(anim) = picked {
                        let _ = PostMessageW(target(), WM_SET_ANIMATION, WPARAM(anim), LPARAM(0));
                    }
                }
                ID_FPS if code == CBN_SELCHANGE => {
                    let sel = SendMessageW(source, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                    if let Some(&(fps, _)) = FRAME_RATES.get(sel as usize) {
                        tweak(FPS, fps, true);
                    }
                }
                ID_THEME if code == LBN_SELCHANGE => {
                    let rows = theme_rows();
                    let picked = pick(source, (LB_GETCURSEL, LB_SETCURSEL), &rows, &LAST_THEME_ROW);
                    if let Some(theme) = picked {
                        let _ = PostMessageW(target(), WM_SET_THEME, WPARAM(theme), LPARAM(0));
                    }
                }
                ID_CLOCK | ID_CLOCK_LABEL if code == 0 && !label_inactive(hwnd, ID_CLOCK_LABEL) => {
                    // BN_CLICKED / STN_CLICKED; a label click toggles the box.
                    let check = GetDlgItem(hwnd, ID_CLOCK as i32).unwrap_or_default();
                    let mut on = SendMessageW(check, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 != 0;
                    if id == ID_CLOCK_LABEL {
                        on = !on;
                        SendMessageW(check, BM_SETCHECK, WPARAM(usize::from(on)), LPARAM(0));
                    }
                    tweak(CLOCK, f32::from(u8::from(on)), true);
                }
                ID_RESET => {
                    SendMessageW(
                        GetDlgItem(hwnd, ID_FPS as i32).unwrap_or_default(),
                        CB_SETCURSEL,
                        WPARAM(0),
                        LPARAM(0),
                    );
                    tweak(FPS, 0.0, true);
                    for (sid, _, _, _, default) in SLIDERS {
                        if let Ok(slider) = GetDlgItem(hwnd, (ID_SLIDER + sid) as i32) {
                            // Leave greyed-out sliders alone.
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
            if SLIDERS.iter().any(|s| s.0 == id) {
                let pos = SendMessageW(slider, TBM_GETPOS, WPARAM(0), LPARAM(0)).0 as i32;
                let value = slider_value(pos);
                if let Ok(label) = GetDlgItem(hwnd, (ID_VALUE + id) as i32) {
                    let _ = SetWindowTextW(label, &HSTRING::from(value_label(id, value)));
                }
                // Live while dragging; saved on release.
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
                (min..=max).contains(&slider_pos(default)),
                "default out of range for {id}"
            );
            for pos in min..=max {
                assert_eq!(
                    slider_pos(slider_value(pos)),
                    pos,
                    "slider {id} position {pos}"
                );
            }
        }
        assert_eq!(FRAME_RATES[frame_rate_index(0.0)].0, 0.0);
        assert_eq!(FRAME_RATES[frame_rate_index(45.0)].0, 30.0); // ties go to the lower
        assert_eq!(FRAME_RATES[frame_rate_index(100.0)].0, 60.0);
    }

    #[test]
    fn grouped_lists_have_every_item_once() {
        for (rows, n, first) in [
            (anim_rows(), ANIMATIONS.len(), "ASCII"),
            (theme_rows(), THEMES.len(), "Terminal"),
        ] {
            assert_eq!(rows[0], Row::Heading(first));
            assert_eq!(rows.iter().filter(|r| matches!(r, Row::Item(_))).count(), n);
            for i in 0..n {
                assert_eq!(rows[row_of(&rows, i)], Row::Item(i));
            }
        }
    }

    #[test]
    fn headings_are_skipped() {
        let rows = anim_rows();
        let second = rows
            .iter()
            .skip(1)
            .position(|r| matches!(r, Row::Heading(_)))
            .unwrap()
            + 1;
        // Onto a heading: down goes below it, up goes above.
        assert_eq!(nearest_item_row(&rows, second, false), second + 1);
        assert_eq!(nearest_item_row(&rows, second, true), second - 1);
        // Nowhere to go above the first heading.
        assert_eq!(nearest_item_row(&rows, 0, true), 1);
        assert_eq!(value_label(SCALE, 1.25), "125%");
    }
}
