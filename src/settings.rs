//! Settings window in the Bench design system (`bench`): two columns, the picks (animation,
//! theme) on the left and the tuning on the right, in the Native or Instrument style, light or
//! dark. The controls are standard Win32 ones, so keyboard use and screen readers work as
//! usual; only their painting is ours (owner-draw and custom draw), with Direct2D for shapes and
//! GDI for text. Changes apply live and save on release, sent as messages like the tray.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, BOOL, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    COLORREF, ERROR_SUCCESS, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    GetDC, GetTextExtentPoint32W, GetTextFaceW, InvalidateRect, RedrawWindow, ReleaseDC,
    SelectObject, SetBkColor, SetBkMode, SetTextCharacterExtra, SetTextColor, DRAW_TEXT_FORMAT,
    DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, FW_SEMIBOLD, HBRUSH, HDC,
    HFONT, LOGFONTW, PAINTSTRUCT, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, SetWindowTheme, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDIS_FOCUS, CDIS_HOT,
    CDIS_SELECTED, CDIS_SHOWKEYBOARDCUES, CDRF_DODEFAULT, CDRF_NOTIFYITEMDRAW, CDRF_SKIPDEFAULT,
    DRAWITEMSTRUCT, ICC_BAR_CLASSES, INITCOMMONCONTROLSEX, MEASUREITEMSTRUCT, NMCUSTOMDRAW, NMHDR,
    NM_CUSTOMDRAW, ODS_COMBOBOXEDIT, ODS_SELECTED, ODT_STATIC, TBCD_CHANNEL, TBM_GETTHUMBRECT,
    TBM_SETPOS, TBM_SETRANGE, TBS_NOTICKS, TB_ENDTRACK, TOOLTIPS_CLASSW, TRACKBAR_CLASSW,
    TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH, TTS_ALWAYSTIP, TTTOOLINFOW,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, SetFocus,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetClientRect,
    GetDlgCtrlID, GetDlgItem, GetWindow, IsWindow, LoadIconW, MessageBoxW, PostMessageW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow,
    SystemParametersInfoW, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON,
    BS_PUSHBUTTON, CBS_DROPDOWNLIST, CBS_HASSTRINGS, CBS_OWNERDRAWFIXED, CB_ADDSTRING,
    CB_GETCURSEL, CB_GETDROPPEDSTATE, CB_GETLBTEXT, CB_GETLBTEXTLEN, CB_SETCURSEL,
    CB_SETITEMHEIGHT, CW_USEDEFAULT, GW_CHILD, HMENU, LBS_HASSTRINGS, LBS_NOTIFY,
    LBS_OWNERDRAWVARIABLE, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL, MB_ICONWARNING, MB_OK,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SW_RESTORE, SW_SHOWNORMAL,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND,
    WM_CREATE, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_ERASEBKGND,
    WM_HSCROLL, WM_MEASUREITEM, WM_NOTIFY, WM_PAINT, WM_SETFONT, WM_SETTINGCHANGE, WNDCLASSW,
    WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_EX_TOPMOST, WS_GROUP,
    WS_MINIMIZEBOX, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::anim::ANIMATIONS;
use crate::bench::{self, Look};
use crate::config::Rgb;
use crate::theme::{mix, THEMES};
use crate::tray::{unpack_state, WM_GET_STATE, WM_SET_ANIMATION, WM_SET_THEME};
use crate::ui::*;
use windows_numerics::Vector2;

/// To the wallpaper: WPARAM = tweak id | commit << 8 (save too), LPARAM = value * 1000.
pub const WM_TWEAK: u32 = WM_APP + 9;
/// To the wallpaper: WPARAM = tweak id; returns value * 1000.
pub const WM_GET_TWEAK: u32 = WM_APP + 10;
/// To this window after a change from the tray or the config file.
const WM_REFRESH: u32 = WM_APP + 12;
/// To the wallpaper: save the settings whose tweak ids are set bits of WPARAM, in one write.
pub const WM_SAVE_TWEAKS: u32 = WM_APP + 13;

const ID_ANIMATION: usize = 100;
const ID_THEME: usize = 101;
const ID_RESET: usize = 102;
const ID_CLOCK: usize = 103;
const ID_FPS: usize = 105;
const ID_LOCK: usize = 106;
const ID_PAUSE_FULLSCREEN: usize = 108;
const ID_PAUSE_BATTERY: usize = 110;
const ID_AUTOSTART: usize = 112;
const ID_UPDATES: usize = 114;
const ID_VERSION: usize = 115;
/// Section labels: + section number.
const ID_SECTION: usize = 120;
/// Style radios: + index into `STYLES`.
const ID_STYLE: usize = 130;
/// Appearance radios: + index into `APPEARANCES`.
const ID_APPEARANCE: usize = 140;
const ID_SLIDER: usize = 200; // + tweak id
const ID_VALUE: usize = 300; // + tweak id
const ID_LABEL: usize = 400; // + tweak id
/// What IsDialogMessage sends for Esc.
const IDCANCEL: usize = 2;
const SS_RIGHT: u32 = 0x2;
const SS_OWNERDRAW: u32 = 0xD;
const SS_NOTIFY: u32 = 0x100;
/// One line, centered vertically.
const SS_CENTERIMAGE: u32 = 0x200;
const TBM_GETPOS: u32 = 0x400; // WM_USER; not in the windows crate
const CBN_SELCHANGE: usize = 1;
const LBN_SELCHANGE: usize = 1;
const LBN_SETFOCUS: usize = 4;
const LBN_KILLFOCUS: usize = 5;

/// Start with Windows: a switch, but not a tweak; set here, not by the wallpaper.
const AUTOSTART: usize = 0xFF;

/// Switches: (control id, setting, label, muted suffix).
const TOGGLES: [(usize, usize, &str, &str); 5] = [
    (ID_CLOCK, CLOCK, "Show clock", "Topographic drift"),
    (ID_LOCK, LOCK_SCREEN, "Show on lock screen", ""),
    (
        ID_PAUSE_FULLSCREEN,
        PAUSE_FULLSCREEN,
        "Pause during fullscreen apps",
        "",
    ),
    (
        ID_PAUSE_BATTERY,
        PAUSE_BATTERY,
        "Pause on battery saver",
        "",
    ),
    (ID_AUTOSTART, AUTOSTART, "Start with Windows", ""),
];

/// The switch's setting for control `id`.
fn toggle_of(id: usize) -> Option<usize> {
    TOGGLES.iter().find(|t| t.0 == id).map(|t| t.1)
}

/// Hover help, with Start with Windows's own.
fn help(setting: usize) -> &'static str {
    match setting {
        AUTOSTART => "Starts Glyphdrift when you sign in to Windows.",
        _ => hint(setting),
    }
}

// Layout at 96 DPI, in Bench's spacing steps.
const CLIENT_W: f32 = 800.0;
/// Window padding.
const PAD: f32 = bench::SPACE[4];
/// Between the columns, and between sections.
const GAP: f32 = bench::SPACE[5];
const COLUMN_W: f32 = (CLIENT_W - 2.0 * PAD - GAP) / 2.0;
/// Row label width, and slider readout width (right column).
const LABEL_W: f32 = 112.0;
const VALUE_W: f32 = 72.0;
/// Room around a drawn control for its focus ring (2px, offset 2px).
const RING: f32 = 4.0;

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
/// Last selected rows, to step over headings in the direction of travel.
static LAST_ANIM_ROW: AtomicIsize = AtomicIsize::new(0);
static LAST_THEME_ROW: AtomicIsize = AtomicIsize::new(0);

/// Everything the open window draws with; rebuilt when the look changes.
struct Ui {
    look: Look,
    /// Pixels per 96-DPI pixel.
    s: f32,
    body: HFONT,
    /// Semibold body: buttons, the selected theme.
    strong: HFONT,
    /// Uppercase section labels and list headings.
    label: HFONT,
    /// Numbers: the body face in Native, monospaced in Instrument.
    num: HFONT,
    window: HBRUSH,
    field: HBRUSH,
    chrome: HBRUSH,
    tooltip: HWND,
    /// Client size in px.
    size: (i32, i32),
    /// Theme list frame and footer band, in client px.
    list_frame: RECT,
    footer_top: i32,
    canvas: Option<ID2D1DCRenderTarget>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static D2D: Option<ID2D1Factory> =
        // SAFETY: plain factory creation.
        unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok() };
}

impl Ui {
    fn px(&self, v: f32) -> i32 {
        (v * self.s).round() as i32
    }
}

/// Run `f` with the open window's `Ui`, if there is one. Shared, so drawing that sends
/// messages back to this window can't trip over its own borrow.
fn with_ui<R>(f: impl FnOnce(&Ui) -> R) -> Option<R> {
    UI.with(|ui| ui.borrow().as_ref().map(f))
}

fn colorref(c: Rgb) -> COLORREF {
    COLORREF(c.0 as u32 | (c.1 as u32) << 8 | (c.2 as u32) << 16)
}

fn d2d(c: Rgb) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.0 as f32 / 255.0,
        g: c.1 as f32 / 255.0,
        b: c.2 as f32 / 255.0,
        a: 1.0,
    }
}

/// `c` at Bench's disabled opacity (45%) over `bg`.
fn faded(c: Rgb, bg: Rgb) -> Rgb {
    mix(c, bg, 0.55)
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

/// The look the wallpaper's settings ask for.
fn wanted_look() -> Look {
    let pick = |names: &[&'static str], id| names[(get_tweak(id) as usize).min(names.len() - 1)];
    Look::new(
        pick(&bench::STYLE_NAMES, STYLE),
        pick(&bench::APPEARANCE_NAMES, APPEARANCE),
        apps_use_dark(),
    )
}

fn dpi_scale() -> f32 {
    // SAFETY: no preconditions.
    unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0
}

/// The settings window, if open.
pub fn window() -> HWND {
    HWND(WINDOW.load(Ordering::Relaxed) as *mut _)
}

/// Ask an open settings window to re-read the settings.
pub fn refresh() {
    let open = window();
    if !open.is_invalid() {
        // SAFETY: plain message post; a stale handle just fails.
        let _ = unsafe { PostMessageW(Some(open), WM_REFRESH, WPARAM(0), LPARAM(0)) };
    }
}

fn target() -> HWND {
    HWND(TARGET.load(Ordering::Relaxed) as *mut _)
}

/// Open the settings window, or bring it forward.
pub fn open(wallpaper: HWND) {
    // SAFETY: window creation on the UI thread; re-registering the class fails harmlessly.
    unsafe {
        let existing = window();
        if !existing.is_invalid() && IsWindow(Some(existing)).as_bool() {
            let _ = ShowWindow(existing, SW_RESTORE);
            let _ = SetForegroundWindow(existing);
            return;
        }
        TARGET.store(wallpaper.0 as isize, Ordering::Relaxed);
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
            hIcon: LoadIconW(Some(hinst.into()), PCWSTR(std::ptr::without_provenance(1)))
                .unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        // Built hidden in WM_CREATE, then sized to its content and placed by the tray.
        if let Ok(hwnd) = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class,
            w!("Glyphdrift settings"),
            frame_style() | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            100,
            100,
            None,
            None,
            Some(hinst.into()),
            None,
        ) {
            place(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn frame_style() -> WINDOW_STYLE {
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX
}

/// Size the window to its content, bottom-right by the tray, so the wallpaper stays visible
/// as a preview.
unsafe fn place(hwnd: HWND) {
    let Some((cw, ch, s)) = with_ui(|ui| (ui.size.0, ui.size.1, ui.s)) else {
        return;
    };
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: cw,
        bottom: ch,
    };
    let _ = AdjustWindowRectEx(&mut rect, frame_style(), false, WS_EX_CONTROLPARENT);
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    let mut work = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work as *mut RECT as *mut _),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    let gap = (12.0 * s).round() as i32;
    let (x, y) = (
        (work.right - w - gap).max(work.left),
        (work.bottom - h - gap).max(work.top),
    );
    let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
}

/// A GDI font: the first of `faces` that's installed (Windows substitutes silently otherwise).
unsafe fn font(hwnd: HWND, faces: &[&str], px: i32, weight: i32) -> HFONT {
    let make = |face: &str| {
        let mut lf = LOGFONTW {
            lfHeight: -px,
            lfWeight: weight,
            ..Default::default()
        };
        for (d, c) in lf.lfFaceName[..31].iter_mut().zip(face.encode_utf16()) {
            *d = c;
        }
        CreateFontIndirectW(&lf)
    };
    let hdc = GetDC(Some(hwnd));
    for face in faces {
        let f = make(face);
        let old = SelectObject(hdc, f.into());
        let mut got = [0u16; 32];
        let n = GetTextFaceW(hdc, Some(&mut got)).max(1) as usize;
        SelectObject(hdc, old);
        if String::from_utf16_lossy(&got[..n - 1]).eq_ignore_ascii_case(face) {
            ReleaseDC(Some(hwnd), hdc);
            return f;
        }
        let _ = DeleteObject(f.into());
    }
    ReleaseDC(Some(hwnd), hdc);
    make("Segoe UI")
}

/// The fonts, brushes and title bar colors for `look`.
unsafe fn make_ui(hwnd: HWND, look: Look) -> Ui {
    let s = dpi_scale();
    let px = |v: f32| (v * s).round() as i32;
    let size = look.size;
    // Instrument is set in IBM Plex where installed; Windows doesn't ship it.
    let faces: &[&str] = match look.style {
        bench::Style::Native => &["Segoe UI Variable Text", "Segoe UI"],
        bench::Style::Instrument => &["IBM Plex Sans", "Segoe UI"],
    };
    let mono: &[&str] = &["IBM Plex Mono", "Cascadia Mono", "Consolas"];
    let c = look.colors;
    // Windows 11 colors the title bar; older versions ignore these.
    let dark = BOOL::from(look.dark);
    let attr = |a, v: &u32| {
        let _ = DwmSetWindowAttribute(hwnd, a, v as *const u32 as *const _, 4);
    };
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &dark as *const BOOL as *const _,
        4,
    );
    attr(DWMWA_CAPTION_COLOR, &colorref(c.chrome).0);
    attr(DWMWA_TEXT_COLOR, &colorref(c.ink_muted).0);
    attr(DWMWA_BORDER_COLOR, &colorref(c.border).0);
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_IGNORE,
        },
        // One DIP per pixel: the layout is in physical pixels.
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    let canvas = D2D.with(|f| f.as_ref().and_then(|f| f.CreateDCRenderTarget(&props).ok()));
    Ui {
        look,
        s,
        body: font(hwnd, faces, px(size.body), FW_NORMAL.0 as i32),
        strong: font(hwnd, faces, px(size.body), FW_SEMIBOLD.0 as i32),
        label: font(hwnd, faces, px(size.label), FW_SEMIBOLD.0 as i32),
        num: if size.mono_numbers {
            font(hwnd, mono, px(size.body), 500)
        } else {
            font(hwnd, faces, px(size.body), FW_NORMAL.0 as i32)
        },
        window: CreateSolidBrush(colorref(c.window)),
        field: CreateSolidBrush(colorref(c.field)),
        chrome: CreateSolidBrush(colorref(c.chrome)),
        tooltip: HWND::default(),
        size: (0, 0),
        list_frame: RECT::default(),
        footer_top: 0,
        canvas,
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        // SAFETY: GDI objects this `Ui` created; the controls using them are gone or going.
        unsafe {
            for f in [self.body, self.strong, self.label, self.num] {
                let _ = DeleteObject(f.into());
            }
            for b in [self.window, self.field, self.chrome] {
                let _ = DeleteObject(b.into());
            }
            if !self.tooltip.is_invalid() {
                let _ = DestroyWindow(self.tooltip);
            }
        }
    }
}

/// Shapes over `rc` of `hdc` with Direct2D (anti-aliased, unlike GDI): `bg` first, then `f`
/// with coordinates relative to `rc`.
unsafe fn shapes(ui: &Ui, hdc: HDC, rc: &RECT, bg: Rgb, f: impl FnOnce(&Pen)) {
    let Some(rt) = &ui.canvas else {
        return;
    };
    if rt.BindDC(hdc, rc).is_err() {
        return;
    }
    rt.BeginDraw();
    rt.Clear(Some(&d2d(bg)));
    f(&Pen { rt });
    let _ = rt.EndDraw(None, None);
}

struct Pen<'a> {
    rt: &'a ID2D1DCRenderTarget,
}

impl Pen<'_> {
    fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
        D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        }
    }

    /// A filled rounded rectangle, with an optional `(width, color)` outline inside its edge.
    fn round(&self, r: D2D_RECT_F, radius: f32, fill: Option<Rgb>, line: Option<(f32, Rgb)>) {
        // SAFETY: drawing between BeginDraw and EndDraw on this thread's target.
        unsafe {
            if let Some(c) = fill {
                if let Ok(b) = self.rt.CreateSolidColorBrush(&d2d(c), None) {
                    let rr = D2D1_ROUNDED_RECT {
                        rect: r,
                        radiusX: radius,
                        radiusY: radius,
                    };
                    self.rt.FillRoundedRectangle(&rr, &b);
                }
            }
            if let Some((w, c)) = line {
                if let Ok(b) = self.rt.CreateSolidColorBrush(&d2d(c), None) {
                    let h = w / 2.0;
                    let rr = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: r.left + h,
                            top: r.top + h,
                            right: r.right - h,
                            bottom: r.bottom - h,
                        },
                        radiusX: (radius - h).max(0.0),
                        radiusY: (radius - h).max(0.0),
                    };
                    self.rt.DrawRoundedRectangle(&rr, &b, w, None);
                }
            }
        }
    }

    fn line(&self, a: (f32, f32), b: (f32, f32), w: f32, c: Rgb) {
        // SAFETY: as in `round`.
        unsafe {
            if let Ok(brush) = self.rt.CreateSolidColorBrush(&d2d(c), None) {
                let p = |(x, y)| Vector2 { X: x, Y: y };
                self.rt.DrawLine(p(a), p(b), &brush, w, None);
            }
        }
    }

    /// Bench's focus ring: 2px of accent, 2px outside `r`.
    fn ring(&self, r: D2D_RECT_F, radius: f32, accent: Rgb) {
        let o = 4.0;
        let outer = D2D_RECT_F {
            left: r.left - o,
            top: r.top - o,
            right: r.right + o,
            bottom: r.bottom + o,
        };
        self.round(outer, radius + o, None, Some((2.0, accent)));
    }
}

/// GDI text in `rc`, vertically centered.
unsafe fn text(hdc: HDC, font: HFONT, color: Rgb, rc: RECT, s: &str, format: DRAW_TEXT_FORMAT) {
    let old = SelectObject(hdc, font.into());
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, colorref(color));
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    let mut rc = rc;
    DrawTextW(hdc, &mut wide, &mut rc, format | DT_VCENTER | DT_SINGLELINE);
    SelectObject(hdc, old);
}

/// Width of `s` in `font`, in px.
unsafe fn text_width(hdc: HDC, font: HFONT, s: &str) -> i32 {
    let old = SelectObject(hdc, font.into());
    let wide: Vec<u16> = s.encode_utf16().collect();
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);
    SelectObject(hdc, old);
    size.cx
}

/// A Bench label: uppercase, semibold, tracked, muted.
unsafe fn draw_label(ui: &Ui, hdc: HDC, rc: RECT, s: &str) {
    FillRect(hdc, &rc, ui.window);
    let track = (ui.look.size.label_track * ui.look.size.label * ui.s).round() as i32;
    SetTextCharacterExtra(hdc, track);
    text(
        hdc,
        ui.label,
        ui.look.colors.ink_muted,
        rc,
        &bench::label(s),
        DT_LEFT,
    );
    SetTextCharacterExtra(hdc, 0);
}

/// Create the controls for the current look (96-DPI layout scaled up), then load the settings.
unsafe fn build(hwnd: HWND) {
    let look = wanted_look();
    let ui = make_ui(hwnd, look);
    let Ok(hinst) = GetModuleHandleW(None) else {
        return;
    };
    let (s, m) = (ui.s, look.size);
    let p = |v: f32| (v * s).round() as i32;
    let fonts = (ui.body, ui.num);
    let strong = ui.strong;
    // Stored first: some controls paint while they're being created.
    UI.with(|slot| *slot.borrow_mut() = Some(ui));
    let control = |class: PCWSTR,
                   text: &str,
                   style: WINDOW_STYLE,
                   (x, y, w, h): (f32, f32, f32, f32),
                   id: usize,
                   font: HFONT| {
        let child = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            &HSTRING::from(text),
            WS_CHILD | WS_VISIBLE | style,
            p(x),
            p(y),
            p(w),
            p(h),
            Some(hwnd),
            Some(HMENU(id as *mut _)),
            Some(hinst.into()),
            None,
        )
        .unwrap_or_default();
        SendMessageW(
            child,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        if look.dark {
            // Dark scroll bars and dropdown lists.
            let _ = SetWindowTheme(child, w!("DarkMode_Explorer"), PCWSTR::null());
        }
        child
    };
    let mut sections = 0;
    let mut section = |x: f32, y: &mut f32, name: &str| {
        control(
            w!("STATIC"),
            name,
            WINDOW_STYLE(SS_OWNERDRAW),
            (x, *y, COLUMN_W, m.label + 4.0),
            ID_SECTION + sections,
            fonts.0,
        );
        sections += 1;
        *y += m.label + 4.0 + bench::SPACE[2];
    };
    // A dropdown: owner-drawn items, the closed field painted by `combo_proc`.
    let combo = |(x, y, w): (f32, f32, f32), id: usize, items: &mut dyn Iterator<Item = &str>| {
        let c = control(
            w!("COMBOBOX"),
            "",
            WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32)
                | WS_TABSTOP
                | WS_VSCROLL,
            (x, y, w, 400.0),
            id,
            fonts.0,
        );
        for item in items {
            // Strings too, for screen readers.
            SendMessageW(
                c,
                CB_ADDSTRING,
                Some(WPARAM(0)),
                Some(LPARAM(HSTRING::from(item).as_ptr() as isize)),
            );
        }
        // The closed field's height: the control's height is the field plus 6px of frame.
        SendMessageW(
            c,
            CB_SETITEMHEIGHT,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(p(m.ctl) as isize - 6)),
        );
        let _ = SetWindowSubclass(c, Some(combo_proc), 1, 0);
        c
    };

    // Left column: animation, theme.
    let lx = PAD;
    let mut ly = PAD;
    section(lx, &mut ly, "Animation");
    let anim_rows = anim_rows();
    let mut names = anim_rows.iter().map(|r| match *r {
        Row::Heading(name) => name,
        Row::Item(i) => ANIMATIONS[i].1,
    });
    combo((lx, ly, COLUMN_W), ID_ANIMATION, &mut names);
    ly += m.ctl + GAP;
    section(lx, &mut ly, "Theme");
    let list_top = ly;
    // Created here for the tab order; sized once the right column's height is known.
    let list = control(
        w!("LISTBOX"),
        "Theme",
        WINDOW_STYLE((LBS_OWNERDRAWVARIABLE | LBS_HASSTRINGS | LBS_NOTIFY) as u32)
            | WS_TABSTOP
            | WS_VSCROLL,
        (lx + 1.0, list_top + 1.0, COLUMN_W - 2.0, 100.0),
        ID_THEME,
        fonts.0,
    );
    for row in theme_rows() {
        let text = match row {
            Row::Heading(name) => name,
            Row::Item(i) => THEMES[i].label,
        };
        SendMessageW(
            list,
            LB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(HSTRING::from(text).as_ptr() as isize)),
        );
    }

    // Right column: motion, size, behaviour, appearance.
    let rx = PAD + COLUMN_W + GAP;
    let mut ry = PAD;
    let slider = |y: f32, id: usize, label: &str, min: i32, max: i32| {
        control(
            w!("STATIC"),
            label,
            WINDOW_STYLE(SS_NOTIFY | SS_CENTERIMAGE),
            (rx, y, LABEL_W, m.ctl),
            ID_LABEL + id,
            fonts.0,
        );
        let t = control(
            TRACKBAR_CLASSW,
            label,
            WS_TABSTOP | WINDOW_STYLE(TBS_NOTICKS),
            (rx + LABEL_W, y, COLUMN_W - LABEL_W - VALUE_W, m.ctl),
            ID_SLIDER + id,
            fonts.0,
        );
        SendMessageW(
            t,
            TBM_SETRANGE,
            Some(WPARAM(1)),
            Some(LPARAM(((max << 16) | min) as isize)),
        );
        let _ = SetWindowSubclass(t, Some(slider_proc), 1, 0);
        // WM_UPDATEUISTATE, which `slider_proc` turns into hiding its focus box.
        SendMessageW(t, 0x0128, None, None);
        control(
            w!("STATIC"),
            "",
            WINDOW_STYLE(SS_RIGHT | SS_NOTIFY | SS_CENTERIMAGE),
            (rx + COLUMN_W - VALUE_W, y, VALUE_W, m.ctl),
            ID_VALUE + id,
            fonts.1,
        );
    };
    let row_gap = bench::SPACE[3];
    section(rx, &mut ry, "Motion");
    let speed = SLIDERS[0];
    slider(ry, speed.0, speed.1, speed.2, speed.3);
    ry += m.ctl + row_gap;
    control(
        w!("STATIC"),
        "Frame rate",
        WINDOW_STYLE(SS_NOTIFY | SS_CENTERIMAGE),
        (rx, ry, LABEL_W, m.ctl),
        ID_LABEL + FPS,
        fonts.0,
    );
    let mut rates = FRAME_RATES.iter().map(|r| r.1);
    combo((rx + LABEL_W, ry, COLUMN_W - LABEL_W), ID_FPS, &mut rates);
    ry += m.ctl + GAP;
    section(rx, &mut ry, "Size and readability");
    for (id, label, min, max, _) in [SLIDERS[2], SLIDERS[3], SLIDERS[1]] {
        slider(ry, id, label, min, max);
        ry += m.ctl + row_gap;
    }
    ry += GAP - row_gap;
    section(rx, &mut ry, "Behaviour");
    for (id, _, label, _) in TOGGLES {
        // The whole row is the switch: a checkbox, drawn by `draw_button`.
        control(
            w!("BUTTON"),
            label,
            WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
            (rx, ry, COLUMN_W, m.row),
            id,
            fonts.0,
        );
        ry += m.row;
    }
    ry += GAP;
    section(rx, &mut ry, "Appearance");
    let radios = |y: f32, setting: usize, base: usize, label: &str, choices: &[&str]| {
        control(
            w!("STATIC"),
            label,
            WINDOW_STYLE(SS_NOTIFY | SS_CENTERIMAGE),
            (rx, y, LABEL_W, m.ctl),
            ID_LABEL + setting,
            fonts.0,
        );
        let hdc = GetDC(Some(hwnd));
        let mut x = rx + LABEL_W - RING;
        for (i, choice) in choices.iter().enumerate() {
            let w = text_width(hdc, fonts.0, choice) as f32 / s + 16.0 + 8.0 + 2.0 * RING;
            // WS_GROUP on the first: arrow keys move within the group.
            let group = if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) };
            control(
                w!("BUTTON"),
                choice,
                WINDOW_STYLE(BS_AUTORADIOBUTTON as u32) | WS_TABSTOP | group,
                (x, y, w, m.ctl),
                base + i,
                fonts.0,
            );
            x += w + bench::SPACE[3] - 2.0 * RING;
        }
        ReleaseDC(Some(hwnd), hdc);
    };
    radios(ry, STYLE, ID_STYLE, "Style", &STYLES);
    ry += m.ctl + row_gap;
    radios(ry, APPEARANCE, ID_APPEARANCE, "Appearance", &APPEARANCES);
    ry += m.ctl;

    // The theme list fills the left column down to the right column's last row.
    let list_frame = (lx, list_top, COLUMN_W, ry - list_top);
    let _ = SetWindowPos(
        list,
        None,
        p(list_frame.0 + 1.0),
        p(list_frame.1 + 1.0),
        p(list_frame.2 - 2.0),
        p(list_frame.3 - 2.0),
        SWP_NOZORDER | SWP_NOACTIVATE,
    );

    // Footer on chrome: version, Check for updates, Reset to defaults.
    let footer_top = ry + PAD;
    let fy = footer_top + bench::SPACE[3];
    let version = format!("Glyphdrift {}", env!("CARGO_PKG_VERSION"));
    control(
        w!("STATIC"),
        &version,
        WINDOW_STYLE(SS_OWNERDRAW),
        (PAD, fy, 140.0, m.ctl),
        ID_VERSION,
        fonts.0,
    );
    let button = |x: f32, w: f32, label: &str, id: usize| {
        control(
            w!("BUTTON"),
            label,
            WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
            (x - RING, fy - RING, w + 2.0 * RING, m.ctl + 2.0 * RING),
            id,
            strong,
        );
    };
    button(PAD + 140.0, 150.0, "Check for updates", ID_UPDATES);
    button(CLIENT_W - PAD - 150.0, 150.0, "Reset to defaults", ID_RESET);
    let height = fy + m.ctl + bench::SPACE[3];

    let tooltip = add_tooltips(hwnd, look.dark, s);
    UI.with(|slot| {
        if let Some(ui) = slot.borrow_mut().as_mut() {
            ui.size = (p(CLIENT_W), p(height));
            ui.list_frame = RECT {
                left: p(list_frame.0),
                top: p(list_frame.1),
                right: p(list_frame.0 + list_frame.2),
                bottom: p(list_frame.1 + list_frame.3),
            };
            ui.footer_top = p(footer_top);
            ui.tooltip = tooltip;
        }
    });
    load(hwnd);
}

/// Throw the controls away and build them again in the look the settings now ask for,
/// keeping keyboard focus on the same control.
unsafe fn rebuild(hwnd: HWND) {
    let focused = GetFocus();
    let focus = (!focused.is_invalid()).then(|| GetDlgCtrlID(focused));
    while let Ok(child) = GetWindow(hwnd, GW_CHILD) {
        let _ = DestroyWindow(child);
    }
    UI.with(|slot| slot.borrow_mut().take());
    build(hwnd);
    place(hwnd);
    let _ = RedrawWindow(
        Some(hwnd),
        None,
        None,
        RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
    );
    if let Some(Ok(control)) = focus.map(|id| GetDlgItem(Some(hwnd), id)) {
        let _ = SetFocus(Some(control));
    }
}

/// Hover help on each label and control.
unsafe fn add_tooltips(hwnd: HWND, dark: bool, s: f32) -> HWND {
    let Ok(tip) = CreateWindowExW(
        WS_EX_TOPMOST,
        TOOLTIPS_CLASSW,
        None,
        WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP),
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        Some(hwnd),
        None,
        None,
        None,
    ) else {
        return HWND::default();
    };
    if dark {
        let _ = SetWindowTheme(tip, w!("DarkMode_Explorer"), PCWSTR::null());
    }
    SendMessageW(
        tip,
        TTM_SETMAXTIPWIDTH,
        Some(WPARAM(0)),
        Some(LPARAM((320.0 * s).round() as isize)),
    );
    let mut tools: Vec<(usize, usize)> = Vec::new(); // (control id, setting id)
    for (id, ..) in SLIDERS {
        tools.extend([
            (ID_LABEL + id, id),
            (ID_SLIDER + id, id),
            (ID_VALUE + id, id),
        ]);
    }
    tools.extend([(ID_LABEL + FPS, FPS), (ID_FPS, FPS)]);
    for (id, setting, ..) in TOGGLES {
        tools.push((id, setting));
    }
    tools.extend([
        (ID_LABEL + STYLE, STYLE),
        (ID_LABEL + APPEARANCE, APPEARANCE),
    ]);
    tools.extend((0..STYLES.len()).map(|i| (ID_STYLE + i, STYLE)));
    tools.extend((0..APPEARANCES.len()).map(|i| (ID_APPEARANCE + i, APPEARANCE)));
    for (control, setting) in tools {
        let Ok(child) = GetDlgItem(Some(hwnd), control as i32) else {
            continue;
        };
        // The tooltip copies the text.
        let mut text: Vec<u16> = help(setting).encode_utf16().chain(Some(0)).collect();
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
            Some(WPARAM(0)),
            Some(LPARAM(&info as *const TTTOOLINFOW as isize)),
        );
    }
    tip
}

/// Show the wallpaper's current settings; rebuild first if the look has changed.
unsafe fn load(hwnd: HWND) {
    if with_ui(|ui| ui.look) != Some(wanted_look()) {
        rebuild(hwnd); // which loads
        return;
    }
    let (anim, theme) =
        unpack_state(SendMessageW(target(), WM_GET_STATE, Some(WPARAM(0)), Some(LPARAM(0))).0);
    let row = row_of(&anim_rows(), anim);
    LAST_ANIM_ROW.store(row as isize, Ordering::Relaxed);
    SendMessageW(
        GetDlgItem(Some(hwnd), ID_ANIMATION as i32).unwrap_or_default(),
        CB_SETCURSEL,
        Some(WPARAM(row)),
        Some(LPARAM(0)),
    );
    let theme_row = row_of(&theme_rows(), theme);
    LAST_THEME_ROW.store(theme_row as isize, Ordering::Relaxed);
    SendMessageW(
        GetDlgItem(Some(hwnd), ID_THEME as i32).unwrap_or_default(),
        LB_SETCURSEL,
        Some(WPARAM(theme_row)),
        Some(LPARAM(0)),
    );
    // Before `show`, which labels disabled sliders "Loops only".
    set_applicable(hwnd, anim);
    for (id, ..) in SLIDERS {
        show(hwnd, id, get_tweak(id));
    }
    SendMessageW(
        GetDlgItem(Some(hwnd), ID_FPS as i32).unwrap_or_default(),
        CB_SETCURSEL,
        Some(WPARAM(frame_rate_index(get_tweak(FPS)))),
        Some(LPARAM(0)),
    );
    for (id, setting, ..) in TOGGLES {
        let on = match setting {
            AUTOSTART => crate::autostart::enabled(),
            _ => get_tweak(setting) != 0.0,
        };
        set_check(hwnd, id, on);
    }
    for (base, setting, n) in [
        (ID_STYLE, STYLE, STYLES.len()),
        (ID_APPEARANCE, APPEARANCE, APPEARANCES.len()),
    ] {
        let picked = get_tweak(setting) as usize;
        for i in 0..n {
            set_check(hwnd, base + i, i == picked);
        }
    }
}

unsafe fn set_check(hwnd: HWND, id: usize, on: bool) {
    SendMessageW(
        GetDlgItem(Some(hwnd), id as i32).unwrap_or_default(),
        BM_SETCHECK,
        Some(WPARAM(usize::from(on))),
        Some(LPARAM(0)),
    );
}

fn get_tweak(id: usize) -> f32 {
    // SAFETY: plain message send.
    unsafe {
        SendMessageW(target(), WM_GET_TWEAK, Some(WPARAM(id)), Some(LPARAM(0))).0 as f32 / 1000.0
    }
}

/// Grey out the controls that do nothing for `anim`.
unsafe fn set_applicable(hwnd: HWND, anim: usize) {
    let (object_scale, clock) = applies(anim);
    for (id, on) in [(ID_SLIDER + OBJECT_SCALE, object_scale), (ID_CLOCK, clock)] {
        if let Ok(control) = GetDlgItem(Some(hwnd), id as i32) {
            let _ = EnableWindow(control, on);
        }
    }
    // Labels stay enabled (so they keep their hover help) and are muted instead.
    for id in [ID_LABEL + OBJECT_SCALE, ID_VALUE + OBJECT_SCALE] {
        if let Ok(label) = GetDlgItem(Some(hwnd), id as i32) {
            let _ = InvalidateRect(Some(label), None, true);
        }
    }
}

/// True if label `id` belongs to a greyed-out control.
unsafe fn label_inactive(hwnd: HWND, id: usize) -> bool {
    let owner = match id {
        _ if id == ID_LABEL + OBJECT_SCALE || id == ID_VALUE + OBJECT_SCALE => {
            ID_SLIDER + OBJECT_SCALE
        }
        _ => return false,
    };
    GetDlgItem(Some(hwnd), owner as i32).is_ok_and(|c| !IsWindowEnabled(c).as_bool())
}

/// Move slider `id` to `value` and update its readout.
unsafe fn show(hwnd: HWND, id: usize, value: f32) {
    if let Ok(slider) = GetDlgItem(Some(hwnd), (ID_SLIDER + id) as i32) {
        SendMessageW(
            slider,
            TBM_SETPOS,
            Some(WPARAM(1)),
            Some(LPARAM(slider_pos(value) as isize)),
        );
    }
    if let Ok(label) = GetDlgItem(Some(hwnd), (ID_VALUE + id) as i32) {
        let inactive = label_inactive(hwnd, ID_VALUE + id);
        // Words in the body face, numbers in the number face (mono in Instrument).
        if let Some(font) = with_ui(|ui| if inactive { ui.body } else { ui.num }) {
            SendMessageW(
                label,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(0)),
            );
        }
        let text = if inactive {
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
    let sel = SendMessageW(list, get, Some(WPARAM(0)), Some(LPARAM(0))).0;
    if sel < 0 {
        return None;
    }
    let row = nearest_item_row(rows, sel as usize, sel < last.load(Ordering::Relaxed));
    if row != sel as usize {
        SendMessageW(list, set, Some(WPARAM(row)), Some(LPARAM(0)));
    }
    last.store(row as isize, Ordering::Relaxed);
    match rows.get(row) {
        Some(&Row::Item(i)) => Some(i),
        _ => None,
    }
}

fn tweak(id: usize, value: f32, commit: bool) {
    let wp = id | usize::from(commit) << 8;
    // SAFETY: plain message post.
    unsafe {
        let _ = PostMessageW(
            Some(target()),
            WM_TWEAK,
            WPARAM(wp),
            LPARAM((value * 1000.0).round() as isize),
        );
    }
}

/// A dropdown list row: muted headings, items filled with the accent when highlighted
/// (Bench's menu rows). The closed field is `combo_proc`'s.
unsafe fn draw_dropdown_row(ui: &Ui, d: &DRAWITEMSTRUCT) {
    if d.itemState.0 & ODS_COMBOBOXEDIT.0 != 0 {
        return;
    }
    let c = ui.look.colors;
    let heading = match d.CtlID as usize {
        ID_ANIMATION => match anim_rows().get(d.itemID as usize) {
            Some(&Row::Heading(name)) => Some(name),
            Some(_) => None,
            None => return,
        },
        _ => None,
    };
    let selected = heading.is_none() && d.itemState.0 & ODS_SELECTED.0 != 0;
    let bg = if selected { c.accent } else { c.field };
    let brush = CreateSolidBrush(colorref(bg));
    FillRect(d.hDC, &d.rcItem, brush);
    let _ = DeleteObject(brush.into());
    let pad = ui.px(10.0);
    if let Some(name) = heading {
        let rc = RECT {
            left: d.rcItem.left + pad,
            ..d.rcItem
        };
        let track = (ui.look.size.label_track * ui.look.size.label * ui.s).round() as i32;
        SetTextCharacterExtra(d.hDC, track);
        text(
            d.hDC,
            ui.label,
            c.ink_muted,
            rc,
            &bench::label(name),
            DT_LEFT,
        );
        SetTextCharacterExtra(d.hDC, 0);
        return;
    }
    let label = combo_text(d.hwndItem, d.itemID as usize);
    let indent = if d.CtlID as usize == ID_ANIMATION {
        2
    } else {
        1
    };
    let rc = RECT {
        left: d.rcItem.left + pad * indent,
        ..d.rcItem
    };
    let ink = if selected { c.on_accent } else { c.ink };
    text(d.hDC, ui.body, ink, rc, &label, DT_LEFT | DT_END_ELLIPSIS);
}

/// The text of a dropdown's item `i`.
unsafe fn combo_text(combo: HWND, i: usize) -> String {
    let n = SendMessageW(combo, CB_GETLBTEXTLEN, Some(WPARAM(i)), Some(LPARAM(0))).0;
    if n <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; n as usize + 1];
    SendMessageW(
        combo,
        CB_GETLBTEXT,
        Some(WPARAM(i)),
        Some(LPARAM(buf.as_mut_ptr() as isize)),
    );
    String::from_utf16_lossy(&buf[..n as usize])
}

/// Paints a dropdown's closed field as a Bench select: a field well with a control border
/// (accent when focused), the choice and a chevron.
unsafe extern "system" fn combo_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            with_ui(|ui| {
                let c = ui.look.colors;
                let enabled = IsWindowEnabled(hwnd).as_bool();
                let focused =
                    GetFocus() == hwnd || SendMessageW(hwnd, CB_GETDROPPEDSTATE, None, None).0 != 0;
                let (w, h) = ((rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32);
                let radius = ui.look.size.radius * ui.s;
                let chevron = ui.s * 4.0;
                let (cx, cy) = (w - ui.s * 16.0, h / 2.0);
                shapes(ui, hdc, &rc, c.window, |pen| {
                    let border = if focused {
                        (2.0, c.accent)
                    } else {
                        (1.0, c.border_control)
                    };
                    pen.round(
                        Pen::rect(0.0, 0.0, w, h),
                        radius,
                        Some(c.field),
                        Some(border),
                    );
                    let ink = if enabled {
                        c.ink_muted
                    } else {
                        faded(c.ink_muted, c.field)
                    };
                    let lw = 1.5 * ui.s;
                    pen.line(
                        (cx - chevron, cy - chevron / 2.0),
                        (cx, cy + chevron / 2.0),
                        lw,
                        ink,
                    );
                    pen.line(
                        (cx, cy + chevron / 2.0),
                        (cx + chevron, cy - chevron / 2.0),
                        lw,
                        ink,
                    );
                });
                let sel = SendMessageW(hwnd, CB_GETCURSEL, None, None).0;
                if sel >= 0 {
                    let label = combo_text(hwnd, sel as usize);
                    let text_rc = RECT {
                        left: rc.left + ui.px(10.0),
                        right: rc.right - ui.px(30.0),
                        ..rc
                    };
                    let ink = if enabled {
                        c.ink
                    } else {
                        faded(c.ink, c.field)
                    };
                    text(
                        hdc,
                        ui.body,
                        ink,
                        text_rc,
                        &label,
                        DT_LEFT | DT_END_ELLIPSIS,
                    );
                }
            });
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        // Repaint the border as focus comes and goes.
        0x0007 | 0x0008 => {
            // WM_SETFOCUS, WM_KILLFOCUS
            let r = DefSubclassProc(hwnd, msg, wp, lp);
            let _ = InvalidateRect(Some(hwnd), None, false);
            r
        }
        _ => DefSubclassProc(hwnd, msg, wp, lp),
    }
}

/// Theme row: the selected one in surface-selected with a 3px accent edge, then the name and
/// swatches of the background and the three cool stops; or a group label.
unsafe fn draw_theme_row(ui: &Ui, d: &DRAWITEMSTRUCT) {
    let c = ui.look.colors;
    let rc = d.rcItem;
    let theme = match theme_rows().get(d.itemID as usize) {
        Some(&Row::Item(i)) => &THEMES[i],
        Some(&Row::Heading(name)) => {
            FillRect(d.hDC, &rc, ui.field);
            let rc = RECT {
                left: rc.left + ui.px(12.0),
                top: rc.top + ui.px(4.0),
                ..rc
            };
            let track = (ui.look.size.label_track * ui.look.size.label * ui.s).round() as i32;
            SetTextCharacterExtra(d.hDC, track);
            text(
                d.hDC,
                ui.label,
                c.ink_muted,
                rc,
                &bench::label(name),
                DT_LEFT,
            );
            SetTextCharacterExtra(d.hDC, 0);
            return;
        }
        None => return,
    };
    let selected = d.itemState.0 & ODS_SELECTED.0 != 0;
    let bg = if selected { c.selected } else { c.field };
    let (w, h) = ((rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32);
    let sw = 14.0 * ui.s;
    let gap = 3.0 * ui.s;
    let swatches_w = 22.0 * ui.s + 3.0 * gap + 3.0 * (sw + gap);
    shapes(ui, d.hDC, &rc, bg, |pen| {
        if selected {
            pen.round(
                Pen::rect(0.0, 0.0, 3.0 * ui.s, h),
                0.0,
                Some(c.accent),
                None,
            );
        }
        let r = (ui.look.size.radius - 2.0).max(0.0) * ui.s;
        let mut x = w - 12.0 * ui.s - swatches_w;
        let y = (h - sw) / 2.0;
        // A hairline keeps a swatch that matches the row visible.
        let edge = Some((1.0, mix(bg, c.ink, 0.15)));
        pen.round(Pen::rect(x, y, 22.0 * ui.s, sw), r, Some(theme.bg), edge);
        x += 22.0 * ui.s + 3.0 * gap;
        for stop in theme.cool {
            pen.round(Pen::rect(x, y, sw, sw), r, Some(stop), edge);
            x += sw + gap;
        }
    });
    let text_rc = RECT {
        left: rc.left + ui.px(15.0),
        right: rc.right - swatches_w as i32 - ui.px(16.0),
        ..rc
    };
    let font = if selected { ui.strong } else { ui.body };
    text(
        d.hDC,
        font,
        c.ink,
        text_rc,
        theme.label,
        DT_LEFT | DT_END_ELLIPSIS,
    );
}

/// Buttons are drawn as Bench controls: switches (checkboxes) with the label on the left,
/// radios, and buttons (quiet for Check for updates).
unsafe fn draw_button(ui: &Ui, cd: &NMCUSTOMDRAW) {
    let hwnd = cd.hdr.hwndFrom;
    let id = cd.hdr.idFrom;
    let c = ui.look.colors;
    let m = ui.look.size;
    let s = ui.s;
    let rc = cd.rc;
    let (w, h) = ((rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32);
    let state = cd.uItemState.0;
    let enabled = IsWindowEnabled(hwnd).as_bool();
    let focus = state & CDIS_FOCUS.0 != 0 && state & CDIS_SHOWKEYBOARDCUES.0 != 0;
    let checked = SendMessageW(hwnd, BM_GETCHECK, None, None).0 != 0;
    let fade = |x: Rgb, bg: Rgb| if enabled { x } else { faded(x, bg) };
    let mut label = [0u16; 64];
    let n = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut label) as usize;
    let label = String::from_utf16_lossy(&label[..n]);

    if let Some(setting) = toggle_of(id) {
        // Switch row: label left, 34x20 switch right, a soft rule below.
        let (tw, th) = (34.0 * s, 20.0 * s);
        let (tx, ty) = (w - tw - RING * s, (h - th) / 2.0);
        let last = setting == AUTOSTART;
        shapes(ui, cd.hdc, &rc, c.window, |pen| {
            if !last {
                pen.line((0.0, h - 0.5), (w, h - 0.5), 1.0, c.border_soft);
            }
            let track = Pen::rect(tx, ty, tw, th);
            let round = if m.round { th / 2.0 } else { 2.0 * s };
            let (fill, line, thumb, tx0) = if checked {
                (c.accent, c.accent, c.on_accent, tx + 17.0 * s)
            } else {
                (c.field, c.border_control, c.ink_muted, tx + 3.0 * s)
            };
            pen.round(
                track,
                round,
                Some(fade(fill, c.window)),
                Some((1.0, fade(line, c.window))),
            );
            let knob = Pen::rect(tx0 + s, ty + 4.0 * s, 12.0 * s, 12.0 * s);
            let kr = if m.round { 6.0 * s } else { 2.0 * s };
            pen.round(knob, kr, Some(fade(thumb, fill)), None);
            if focus {
                pen.ring(track, round, c.accent);
            }
        });
        let suffix = TOGGLES.iter().find(|t| t.0 == id).map_or("", |t| t.3);
        let ink = fade(c.ink, c.window);
        let mut text_rc = RECT {
            right: rc.right - (tw + 2.0 * RING * s) as i32,
            ..rc
        };
        text(cd.hdc, ui.body, ink, text_rc, &label, DT_LEFT);
        if !suffix.is_empty() {
            text_rc.left += text_width(cd.hdc, ui.body, &label) + ui.px(6.0);
            let muted = fade(c.ink_muted, c.window);
            text(
                cd.hdc,
                ui.body,
                muted,
                text_rc,
                &format!("({suffix})"),
                DT_LEFT,
            );
        }
    } else if (ID_STYLE..ID_APPEARANCE + APPEARANCES.len()).contains(&id) {
        // Radio: 16px, round in Native and square in Instrument; accent with a dot when on.
        let d = 16.0 * s;
        let (x, y) = (RING * s, (h - d) / 2.0);
        let r = if m.round { d / 2.0 } else { 2.0 * s };
        let bg = c.window;
        shapes(ui, cd.hdc, &rc, bg, |pen| {
            let b = Pen::rect(x, y, d, d);
            if checked {
                pen.round(b, r, Some(c.accent), None);
                let dot = Pen::rect(x + 5.0 * s, y + 5.0 * s, 6.0 * s, 6.0 * s);
                let dr = if m.round { 3.0 * s } else { 1.0 * s };
                pen.round(dot, dr, Some(c.on_accent), None);
            } else {
                pen.round(b, r, Some(c.field), Some((1.0, c.border_control)));
            }
            if focus {
                pen.ring(b, r, c.accent);
            }
        });
        let text_rc = RECT {
            left: rc.left + ((RING + 16.0 + 8.0) * s) as i32,
            ..rc
        };
        text(cd.hdc, ui.body, c.ink, text_rc, &label, DT_LEFT);
    } else {
        // Button, inside a margin left for the focus ring: secondary, or quiet on chrome.
        let quiet = id == ID_UPDATES;
        let hot = state & CDIS_HOT.0 != 0;
        let pressed = state & CDIS_SELECTED.0 != 0;
        let bg = c.chrome;
        let b = Pen::rect(RING * s, RING * s, w - 2.0 * RING * s, h - 2.0 * RING * s);
        let radius = m.radius * s;
        shapes(ui, cd.hdc, &rc, bg, |pen| {
            let (fill, line) = match (quiet, hot || pressed) {
                (true, false) => (None, None),
                (true, true) => (Some(c.panel), None),
                (false, hot) => (
                    Some(if pressed { c.selected } else { c.panel }),
                    Some((1.0, if hot { c.border_control } else { c.border })),
                ),
            };
            pen.round(b, radius, fill, line);
            if focus {
                pen.ring(b, radius, c.accent);
            }
        });
        text(
            cd.hdc,
            ui.strong,
            c.ink,
            rc,
            &label,
            windows::Win32::Graphics::Gdi::DT_CENTER,
        );
    }
}

/// True when focus should show: the keyboard has been used in `window` (Windows hides focus
/// cues after mouse use, as Bench's `:focus-visible` does).
unsafe fn keyboard_cues(window: HWND) -> bool {
    const WM_QUERYUISTATE: u32 = 0x0129;
    const UISF_HIDEFOCUS: isize = 0x1;
    SendMessageW(window, WM_QUERYUISTATE, None, None).0 & UISF_HIDEFOCUS == 0
}

/// Keeps a trackbar's own dotted focus box off, whatever the window's cue state: our ring
/// replaces it. Repaints as focus comes and goes.
unsafe extern "system" fn slider_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    const WM_CHANGEUISTATE: u32 = 0x0127;
    const WM_UPDATEUISTATE: u32 = 0x0128;
    const HIDE_FOCUS: usize = 1 | 1 << 16; // UIS_SET, UISF_HIDEFOCUS
    match msg {
        WM_CHANGEUISTATE | WM_UPDATEUISTATE => {
            DefSubclassProc(hwnd, WM_UPDATEUISTATE, WPARAM(HIDE_FOCUS), lp)
        }
        0x0007 | 0x0008 => {
            // WM_SETFOCUS, WM_KILLFOCUS
            let r = DefSubclassProc(hwnd, msg, wp, lp);
            let _ = InvalidateRect(Some(hwnd), None, false);
            r
        }
        _ => DefSubclassProc(hwnd, msg, wp, lp),
    }
}

/// Slider: a 4px track in border-control, accent up to the thumb, and a field-colored thumb
/// ringed in accent (round in Native, square in Instrument).
unsafe fn draw_slider(ui: &Ui, cd: &NMCUSTOMDRAW) -> u32 {
    if cd.dwDrawStage == CDDS_PREPAINT {
        return CDRF_NOTIFYITEMDRAW;
    }
    if cd.dwDrawStage != CDDS_ITEMPREPAINT {
        return CDRF_DODEFAULT;
    }
    if cd.dwItemSpec as u32 != TBCD_CHANNEL {
        return CDRF_SKIPDEFAULT; // the thumb and tics are drawn with the channel
    }
    let hwnd = cd.hdr.hwndFrom;
    let c = ui.look.colors;
    let s = ui.s;
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let mut thumb = RECT::default();
    SendMessageW(
        hwnd,
        TBM_GETTHUMBRECT,
        None,
        Some(LPARAM(&mut thumb as *mut RECT as isize)),
    );
    let enabled = IsWindowEnabled(hwnd).as_bool();
    let focus = GetFocus() == hwnd && keyboard_cues(window());
    let fade = |x: Rgb| if enabled { x } else { faded(x, c.window) };
    let (w, h) = (
        (client.right - client.left) as f32,
        (client.bottom - client.top) as f32,
    );
    let cx = (thumb.left + thumb.right) as f32 / 2.0;
    let cy = h / 2.0;
    let d = 16.0 * s;
    let edge = d / 2.0;
    let cx = cx.clamp(edge, w - edge);
    let round = ui.look.size.round;
    shapes(ui, cd.hdc, &client, c.window, |pen| {
        let track_r = if round { 2.0 * s } else { 0.0 };
        let track = Pen::rect(edge, cy - 2.0 * s, w - 2.0 * edge, 4.0 * s);
        pen.round(track, track_r, Some(fade(c.border_control)), None);
        if enabled {
            let fill = Pen::rect(edge, cy - 2.0 * s, cx - edge, 4.0 * s);
            pen.round(fill, track_r, Some(c.accent), None);
        }
        let knob = Pen::rect(cx - d / 2.0, cy - d / 2.0, d, d);
        let kr = if round { d / 2.0 } else { 2.0 * s };
        pen.round(knob, kr, Some(c.field), Some((2.0 * s, fade(c.accent))));
        if focus {
            pen.ring(knob, kr, c.accent);
        }
    });
    CDRF_SKIPDEFAULT
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
            build(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            with_ui(|ui| {
                let c = ui.look.colors;
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let (w, h) = ((rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32);
                let list = ui.list_frame;
                let list_focus = GetDlgItem(Some(hwnd), ID_THEME as i32)
                    .is_ok_and(|l| GetFocus() == l)
                    && keyboard_cues(hwnd);
                shapes(ui, hdc, &rc, c.window, |pen| {
                    // Footer band on chrome, ruled off from the content.
                    let top = ui.footer_top as f32;
                    pen.round(Pen::rect(0.0, top, w, h - top), 0.0, Some(c.chrome), None);
                    pen.line((0.0, top + 0.5), (w, top + 0.5), 1.0, c.border);
                    // Theme list frame.
                    let r = Pen::rect(
                        list.left as f32,
                        list.top as f32,
                        (list.right - list.left) as f32,
                        (list.bottom - list.top) as f32,
                    );
                    let radius = ui.look.size.radius * ui.s;
                    pen.round(r, radius, Some(c.field), Some((1.0, c.border)));
                    if list_focus {
                        pen.ring(r, radius, c.accent);
                    }
                });
            });
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            let hdc = HDC(wp.0 as *mut _);
            let control = HWND(lp.0 as *mut _);
            let id = GetDlgCtrlID(control) as usize;
            with_ui(|ui| {
                let c = ui.look.colors;
                if msg == WM_CTLCOLORLISTBOX {
                    SetTextColor(hdc, colorref(c.ink));
                    SetBkColor(hdc, colorref(c.field));
                    return LRESULT(ui.field.0 as isize);
                }
                let value = (ID_VALUE..ID_VALUE + 100).contains(&id);
                let ink = if label_inactive(hwnd, id) {
                    faded(c.ink_muted, c.window)
                } else if value {
                    c.ink_muted
                } else {
                    c.ink
                };
                SetTextColor(hdc, colorref(ink));
                SetBkColor(hdc, colorref(c.window));
                LRESULT(ui.window.0 as isize)
            })
            .unwrap_or(LRESULT(0))
        }
        WM_MEASUREITEM => {
            let mi = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
            let heading = mi.CtlID as usize == ID_THEME
                && matches!(theme_rows().get(mi.itemID as usize), Some(Row::Heading(_)));
            let h = with_ui(|ui| {
                let row = ui.look.size.row;
                ui.px(if heading { row * 0.85 } else { row })
            });
            mi.itemHeight = h.unwrap_or(30) as u32;
            LRESULT(1)
        }
        WM_DRAWITEM => {
            let d = &*(lp.0 as *const DRAWITEMSTRUCT);
            with_ui(|ui| match d.CtlID as usize {
                ID_THEME => draw_theme_row(ui, d),
                ID_ANIMATION | ID_FPS => draw_dropdown_row(ui, d),
                ID_VERSION if d.CtlType == ODT_STATIC => {
                    FillRect(d.hDC, &d.rcItem, ui.chrome);
                    let c = ui.look.colors;
                    let name = "Glyphdrift ";
                    text(d.hDC, ui.body, c.ink_muted, d.rcItem, name, DT_LEFT);
                    let rc = RECT {
                        left: d.rcItem.left + text_width(d.hDC, ui.body, name),
                        ..d.rcItem
                    };
                    let version = env!("CARGO_PKG_VERSION");
                    text(d.hDC, ui.num, c.ink_muted, rc, version, DT_LEFT);
                }
                id if (ID_SECTION..ID_SECTION + 10).contains(&id) => {
                    let mut name = [0u16; 64];
                    let n = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(
                        d.hwndItem, &mut name,
                    ) as usize;
                    let name = String::from_utf16_lossy(&name[..n]);
                    draw_label(ui, d.hDC, d.rcItem, &name);
                }
                _ => {}
            });
            LRESULT(1)
        }
        WM_NOTIFY => {
            let hdr = &*(lp.0 as *const NMHDR);
            if hdr.code != NM_CUSTOMDRAW {
                return LRESULT(0);
            }
            let cd = &*(lp.0 as *const NMCUSTOMDRAW);
            let id = hdr.idFrom;
            let r = with_ui(|ui| {
                if (ID_SLIDER..ID_SLIDER + 100).contains(&id) {
                    draw_slider(ui, cd)
                } else if cd.dwDrawStage == CDDS_PREPAINT {
                    draw_button(ui, cd);
                    CDRF_SKIPDEFAULT
                } else {
                    CDRF_DODEFAULT
                }
            });
            LRESULT(r.unwrap_or(CDRF_DODEFAULT) as isize)
        }
        WM_SETTINGCHANGE => {
            // The system went light or dark: follow it if Appearance says Match system.
            let area = lp.0 as *const u16;
            if !area.is_null()
                && PCWSTR(area)
                    .to_string()
                    .is_ok_and(|a| a == "ImmersiveColorSet")
            {
                load(hwnd);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
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
                        let _ =
                            PostMessageW(Some(target()), WM_SET_ANIMATION, WPARAM(anim), LPARAM(0));
                    }
                }
                ID_FPS if code == CBN_SELCHANGE => {
                    let sel = SendMessageW(source, CB_GETCURSEL, None, None).0;
                    if let Some(&(fps, _)) = FRAME_RATES.get(sel as usize) {
                        tweak(FPS, fps, true);
                    }
                }
                ID_THEME if code == LBN_SELCHANGE => {
                    let rows = theme_rows();
                    let picked = pick(source, (LB_GETCURSEL, LB_SETCURSEL), &rows, &LAST_THEME_ROW);
                    if let Some(theme) = picked {
                        let _ =
                            PostMessageW(Some(target()), WM_SET_THEME, WPARAM(theme), LPARAM(0));
                    }
                }
                ID_THEME if code == LBN_SETFOCUS || code == LBN_KILLFOCUS => {
                    // The focus ring around the list frame.
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                _ if code == 0 && toggle_of(id).is_some() => {
                    let setting = toggle_of(id).unwrap_or(AUTOSTART);
                    let on = SendMessageW(source, BM_GETCHECK, None, None).0 != 0;
                    if setting != AUTOSTART {
                        tweak(setting, f32::from(u8::from(on)), true);
                    } else if let Err(e) = crate::autostart::set(on) {
                        log!("autostart: {e}");
                        set_check(hwnd, id, !on);
                        let text = format!("Couldn't change Start with Windows: {e}");
                        MessageBoxW(
                            Some(hwnd),
                            &HSTRING::from(text),
                            w!("Glyphdrift"),
                            MB_OK | MB_ICONWARNING,
                        );
                    }
                }
                _ if code == 0 && (ID_STYLE..ID_STYLE + STYLES.len()).contains(&id) => {
                    tweak(STYLE, (id - ID_STYLE) as f32, true);
                    refresh(); // after the wallpaper has it: rebuild in the new look
                }
                _ if code == 0
                    && (ID_APPEARANCE..ID_APPEARANCE + APPEARANCES.len()).contains(&id) =>
                {
                    tweak(APPEARANCE, (id - ID_APPEARANCE) as f32, true);
                    refresh();
                }
                ID_UPDATES => {
                    // The tray runs the check and shows the result.
                    if let Ok(tray) = FindWindowW(w!("GlyphdriftTray"), None) {
                        let _ = PostMessageW(
                            Some(tray),
                            WM_COMMAND,
                            WPARAM(crate::tray::CHECK_FOR_UPDATES),
                            LPARAM(0),
                        );
                    }
                }
                ID_RESET => {
                    SendMessageW(
                        GetDlgItem(Some(hwnd), ID_FPS as i32).unwrap_or_default(),
                        CB_SETCURSEL,
                        Some(WPARAM(0)),
                        Some(LPARAM(0)),
                    );
                    let _ = InvalidateRect(GetDlgItem(Some(hwnd), ID_FPS as i32).ok(), None, false);
                    // Applied one by one, then saved in one write.
                    tweak(FPS, 0.0, false);
                    let mut reset = 1 << FPS;
                    for (sid, _, _, _, default) in SLIDERS {
                        if let Ok(slider) = GetDlgItem(Some(hwnd), (ID_SLIDER + sid) as i32) {
                            // Leave greyed-out sliders alone.
                            if !IsWindowEnabled(slider).as_bool() {
                                continue;
                            }
                        }
                        show(hwnd, sid, default);
                        tweak(sid, default, false);
                        reset |= 1 << sid;
                    }
                    let _ = PostMessageW(Some(target()), WM_SAVE_TWEAKS, WPARAM(reset), LPARAM(0));
                }
                IDCANCEL => {
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
                let pos = SendMessageW(slider, TBM_GETPOS, None, None).0 as i32;
                let value = slider_value(pos);
                if let Ok(label) = GetDlgItem(Some(hwnd), (ID_VALUE + id) as i32) {
                    let _ = SetWindowTextW(label, &HSTRING::from(value_label(id, value)));
                }
                // Live while dragging; saved on release.
                tweak(id, value, (wp.0 & 0xFFFF) as u32 == TB_ENDTRACK);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            WINDOW.store(0, Ordering::Relaxed);
            UI.with(|slot| slot.borrow_mut().take());
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
