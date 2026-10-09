//! Grid renderer.
//!
//! M1/M2 scaffold uses GDI with a back buffer so it has no extra dependencies and is
//! easy to verify. The design calls for Direct2D/DirectWrite (then a GPU glyph atlas);
//! keep the public surface (`new`, `cell_size`, `paint`) stable so the swap is local.

use crate::config::Rgb;
use crate::grid::Grid;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush, DeleteDC,
    DeleteObject, FillRect, GetDC, GetTextMetricsW, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, TextOutW, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, FF_MODERN,
    FIXED_PITCH, FW_NORMAL, HDC, HFONT, OUT_DEFAULT_PRECIS, SRCCOPY, TEXTMETRICW, TRANSPARENT,
};

const LEVELS: usize = 16;

pub struct GdiRenderer {
    font: HFONT,
    cell_w: i32,
    cell_h: i32,
    bg: COLORREF,
    /// Foreground color pre-scaled for each brightness level.
    palette: [COLORREF; LEVELS],
}

fn colorref(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

impl GdiRenderer {
    pub fn new(family: &str, pixel_height: i32, fg: Rgb, bg: Rgb) -> Self {
        let face: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `face` is NUL-terminated and outlives the call; the screen DC is released.
        unsafe {
            let font = CreateFontW(
                -pixel_height,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                (FIXED_PITCH.0 | FF_MODERN.0) as u32,
                PCWSTR(face.as_ptr()),
            );
            let dc = GetDC(None);
            let old = SelectObject(dc, font);
            let mut tm = TEXTMETRICW::default();
            let _ = GetTextMetricsW(dc, &mut tm);
            SelectObject(dc, old);
            ReleaseDC(None, dc);

            let mut palette = [COLORREF(0); LEVELS];
            for (i, slot) in palette.iter_mut().enumerate() {
                let k = (i + 1) as f32 / LEVELS as f32;
                let mix = |f: u8, b: u8| (b as f32 + (f as f32 - b as f32) * k) as u8;
                *slot = colorref(mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2));
            }

            Self {
                font,
                cell_w: tm.tmAveCharWidth.max(1),
                cell_h: tm.tmHeight.max(1),
                bg: colorref(bg.0, bg.1, bg.2),
                palette,
            }
        }
    }

    pub fn cell_size(&self) -> (i32, i32) {
        (self.cell_w, self.cell_h)
    }

    /// Draw `grid` into `hdc` (a `width` x `height` client area) through a back buffer.
    pub fn paint(&self, hdc: HDC, width: i32, height: i32, grid: &Grid) {
        if width <= 0 || height <= 0 {
            return;
        }
        // SAFETY: every GDI object created here is deselected and deleted before return.
        unsafe {
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, width, height);
            let old_bmp = SelectObject(mem, bmp);
            let old_font = SelectObject(mem, self.font);

            let brush = CreateSolidBrush(self.bg);
            FillRect(
                mem,
                &RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                brush,
            );
            let _ = DeleteObject(brush);
            SetBkMode(mem, TRANSPARENT);

            let mut last_level = usize::MAX;
            for y in 0..grid.rows() {
                for (x, cell) in grid.row(y).iter().enumerate() {
                    if cell.brightness == 0 || cell.ch == ' ' {
                        continue;
                    }
                    let level = (cell.brightness as usize * LEVELS / 256).min(LEVELS - 1);
                    if level != last_level {
                        SetTextColor(mem, self.palette[level]);
                        last_level = level;
                    }
                    let mut buf = [0u16; 2];
                    let s = cell.ch.encode_utf16(&mut buf);
                    let _ = TextOutW(mem, x as i32 * self.cell_w, y as i32 * self.cell_h, s);
                }
            }

            let _ = BitBlt(hdc, 0, 0, width, height, mem, 0, 0, SRCCOPY);

            SelectObject(mem, old_font);
            SelectObject(mem, old_bmp);
            let _ = DeleteObject(bmp);
            let _ = DeleteDC(mem);
        }
    }
}

impl Drop for GdiRenderer {
    fn drop(&mut self) {
        // SAFETY: the font was created in `new` and is not selected into any live DC.
        unsafe {
            let _ = DeleteObject(self.font);
        }
    }
}
