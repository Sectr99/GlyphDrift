//! Grid renderer: Direct2D + DirectWrite.
//!
//! All cells of one brightness level go out in a single `DrawGlyphRun`, each glyph placed
//! with a glyph offset, so a frame is at most `LEVELS` draw calls. A DC render target
//! draws into the WM_PAINT DC, which keeps the public surface (`new`, `cell_size`,
//! `paint`) independent of the window. The next step up is a GPU glyph atlas.

use std::mem::ManuallyDrop;

use crate::config::Rgb;
use crate::grid::Grid;
use crate::shape::Shape;
use crate::theme;
use windows::core::{w, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{BOOL, D2DERR_RECREATE_TARGET, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontFace, IDWriteTextFormat,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_METRICS, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_GLYPH_METRICS,
    DWRITE_GLYPH_OFFSET, DWRITE_GLYPH_RUN, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::HDC;

const LEVELS: usize = 16;
const FALLBACK_FONT: &str = "Consolas";
/// Typeface for `Shape::Text` (Windows 11; DirectWrite falls back to Segoe UI elsewhere).
const TEXT_FONT: PCWSTR = w!("Segoe UI Variable Display");

pub struct Renderer {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    face: IDWriteFontFace,
    /// Text formats for `Shape::Text`, by (size bits, weight).
    // ponytail: never evicted; sizes only change on resize or scale changes.
    formats: Vec<((u32, u16), IDWriteTextFormat)>,
    em: f32,
    baseline: f32,
    cell_w: i32,
    cell_h: i32,
    bg: D2D1_COLOR_F,
    /// Foreground color pre-scaled for each brightness level.
    palette: [D2D1_COLOR_F; LEVELS],
    /// Device-dependent resources; dropped and rebuilt on D2DERR_RECREATE_TARGET.
    target: Option<Target>,
    /// Per-level glyph batches, reused across frames to avoid allocation.
    batches: [Batch; LEVELS],
}

struct Target {
    rt: ID2D1DCRenderTarget,
    brushes: Vec<ID2D1SolidColorBrush>,
}

/// All glyphs of one brightness level, drawn with a single `DrawGlyphRun`.
#[derive(Default)]
struct Batch {
    codepoints: Vec<u32>,
    glyphs: Vec<u16>,
    advances: Vec<f32>,
    /// Absolute cell positions; DirectWrite's ascender offset points up, hence `-y`.
    offsets: Vec<DWRITE_GLYPH_OFFSET>,
}

impl Batch {
    fn clear(&mut self) {
        self.codepoints.clear();
        self.offsets.clear();
    }

    fn push(&mut self, x: f32, y: f32, ch: char) {
        self.codepoints.push(ch as u32);
        self.offsets.push(DWRITE_GLYPH_OFFSET {
            advanceOffset: x,
            ascenderOffset: -y,
        });
    }
}

/// Palette index for a 0..=255 brightness.
fn level_index(brightness: u8) -> usize {
    (brightness as usize * LEVELS / 256).min(LEVELS - 1)
}

fn color(c: Rgb) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.0 as f32 / 255.0,
        g: c.1 as f32 / 255.0,
        b: c.2 as f32 / 255.0,
        a: 1.0,
    }
}

impl Renderer {
    pub fn new(
        family: &str,
        pixel_height: i32,
        bg: Rgb,
        stops: &[Rgb; 3],
        glass: f32,
    ) -> Result<Self> {
        // SAFETY: factory/font calls with valid arguments; every out-pointer is a live local.
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let mut fonts = None;
            dwrite.GetSystemFontCollection(&mut fonts, false)?;
            let fonts = fonts.ok_or_else(windows::core::Error::empty)?;
            // Unknown family: fall back to Consolas, then to whatever font comes first.
            let mut index = 0;
            for name in [family, FALLBACK_FONT] {
                let (mut i, mut exists) = (0u32, BOOL(0));
                fonts.FindFamilyName(&HSTRING::from(name), &mut i, &mut exists)?;
                if exists.as_bool() {
                    index = i;
                    break;
                }
                eprintln!("font: '{name}' not found");
            }
            let face = fonts
                .GetFontFamily(index)?
                .GetFirstMatchingFont(
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )?
                .CreateFontFace()?;

            // Same sizing as the GDI renderer: em height = pixel_height, cell height =
            // ascent + descent, cell width = advance of 'M'.
            let em = pixel_height as f32;
            let mut m = DWRITE_FONT_METRICS::default();
            face.GetMetrics(&mut m);
            let scale = em / m.designUnitsPerEm.max(1) as f32;
            let mut glyph = 0u16;
            face.GetGlyphIndices(&('M' as u32), 1, &mut glyph)?;
            let mut gm = DWRITE_GLYPH_METRICS::default();
            face.GetDesignGlyphMetrics(&glyph, 1, &mut gm, false)?;

            let mut r = Self {
                factory,
                dwrite,
                face,
                formats: Vec::new(),
                em,
                baseline: m.ascent as f32 * scale,
                cell_w: ((gm.advanceWidth as f32 * scale).round() as i32).max(1),
                cell_h: (((m.ascent + m.descent) as f32 * scale).ceil() as i32).max(1),
                bg: color(bg),
                palette: [color(bg); LEVELS],
                target: None,
                batches: Default::default(),
            };
            r.set_colors(bg, stops, glass);
            Ok(r)
        }
    }

    /// Background plus three gradient stops (dim to bright), e.g. from `theme::THEMES`,
    /// seen through a frosted veil of opacity `glass` (0 = none).
    pub fn set_colors(&mut self, bg: Rgb, stops: &[Rgb; 3], glass: f32) {
        self.bg = color(theme::frost(bg, bg, glass));
        for (slot, c) in self
            .palette
            .iter_mut()
            .zip(theme::gradient(bg, stops, LEVELS))
        {
            *slot = color(theme::frost(c, bg, glass));
        }
        self.target = None; // brushes are rebuilt with the new palette
    }

    pub fn cell_size(&self) -> (i32, i32) {
        (self.cell_w, self.cell_h)
    }

    /// Draw `grid`, then `shapes` on top, into `hdc` (a `width` x `height` client area).
    pub fn paint(&mut self, hdc: HDC, width: i32, height: i32, grid: &Grid, shapes: &[Shape]) {
        if width <= 0 || height <= 0 {
            return;
        }
        if let Err(e) = self.try_paint(hdc, width, height, grid, shapes) {
            if e.code() == D2DERR_RECREATE_TARGET {
                self.target = None; // rebuilt on the next frame
            } else {
                eprintln!("render: {e}");
            }
        }
    }

    fn try_paint(
        &mut self,
        hdc: HDC,
        width: i32,
        height: i32,
        grid: &Grid,
        shapes: &[Shape],
    ) -> Result<()> {
        for b in &mut self.batches {
            b.clear();
        }
        for y in 0..grid.rows() {
            for (x, cell) in grid.row(y).iter().enumerate() {
                if cell.brightness == 0 || cell.ch == ' ' {
                    continue;
                }
                let level = level_index(cell.brightness);
                self.batches[level].push(
                    (x as i32 * self.cell_w) as f32,
                    (y as i32 * self.cell_h) as f32,
                    cell.ch,
                );
            }
        }

        for s in shapes {
            if let Shape::Text { size, weight, .. } = *s {
                let key = (size.to_bits(), weight);
                if !self.formats.iter().any(|f| f.0 == key) {
                    // SAFETY: COM calls on a live factory with valid constant arguments.
                    let format = unsafe {
                        let f = self.dwrite.CreateTextFormat(
                            TEXT_FONT,
                            None,
                            DWRITE_FONT_WEIGHT(weight as i32),
                            DWRITE_FONT_STYLE_NORMAL,
                            DWRITE_FONT_STRETCH_NORMAL,
                            size.max(1.0),
                            w!(""),
                        )?;
                        f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                        f
                    };
                    self.formats.push((key, format));
                }
            }
        }

        // SAFETY: COM calls on live interfaces. Every batch has equally long codepoints,
        // glyphs, advances and offsets, and those buffers outlive the DrawGlyphRun calls.
        unsafe {
            for b in &mut self.batches {
                let n = b.codepoints.len();
                b.glyphs.resize(n, 0);
                b.advances.resize(n, 0.0);
                if n > 0 {
                    self.face.GetGlyphIndices(
                        b.codepoints.as_ptr(),
                        n as u32,
                        b.glyphs.as_mut_ptr(),
                    )?;
                }
            }

            if self.target.is_none() {
                self.target = Some(self.create_target()?);
            }
            let t = self.target.as_ref().unwrap();
            let rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            t.rt.BindDC(hdc, &rect)?;
            t.rt.BeginDraw();
            t.rt.Clear(Some(&self.bg));
            let origin = D2D_POINT_2F {
                x: 0.0,
                y: self.baseline,
            };
            for (b, brush) in self.batches.iter().zip(&t.brushes) {
                if b.glyphs.is_empty() {
                    continue;
                }
                let run = DWRITE_GLYPH_RUN {
                    fontFace: ManuallyDrop::new(Some(self.face.clone())),
                    fontEmSize: self.em,
                    glyphCount: b.glyphs.len() as u32,
                    glyphIndices: b.glyphs.as_ptr(),
                    glyphAdvances: b.advances.as_ptr(),
                    glyphOffsets: b.offsets.as_ptr(),
                    isSideways: BOOL(0),
                    bidiLevel: 0,
                };
                t.rt.DrawGlyphRun(origin, &run, brush, DWRITE_MEASURING_MODE_NATURAL);
                // Release the reference taken by `clone` above.
                drop(ManuallyDrop::into_inner(run.fontFace));
            }
            let pt = |p: (f32, f32)| D2D_POINT_2F { x: p.0, y: p.1 };
            for s in shapes {
                match *s {
                    Shape::Polygon { ref points, level } if level > 0 && points.len() >= 3 => {
                        // ponytail: one path geometry per polygon per frame; cache if a
                        // loop ever needs hundreds of fills.
                        let geom = self.factory.CreatePathGeometry()?;
                        let sink = geom.Open()?;
                        sink.BeginFigure(pt(points[0]), D2D1_FIGURE_BEGIN_FILLED);
                        let rest: Vec<_> = points[1..].iter().map(|&p| pt(p)).collect();
                        sink.AddLines(&rest);
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        sink.Close()?;
                        t.rt.FillGeometry(&geom, &t.brushes[level_index(level)], None);
                    }
                    Shape::Line { a, b, width, level } if level > 0 => {
                        let brush = &t.brushes[level_index(level)];
                        t.rt.DrawLine(pt(a), pt(b), brush, width, None);
                    }
                    Shape::Circle { c, r, fill, level } if level > 0 => {
                        let brush = &t.brushes[level_index(level)];
                        let e = D2D1_ELLIPSE {
                            point: pt(c),
                            radiusX: r,
                            radiusY: r,
                        };
                        if fill {
                            t.rt.FillEllipse(&e, brush);
                        } else {
                            t.rt.DrawEllipse(&e, brush, 1.0, None);
                        }
                    }
                    Shape::Text {
                        c,
                        size,
                        weight,
                        ref text,
                        level,
                    } if level > 0 => {
                        let key = (size.to_bits(), weight);
                        if let Some((_, format)) = self.formats.iter().find(|f| f.0 == key) {
                            let text: Vec<u16> = text.encode_utf16().collect();
                            // Centered in a box far wider than any line of text.
                            let rect = D2D_RECT_F {
                                left: c.0 - size * 40.0,
                                top: c.1 - size,
                                right: c.0 + size * 40.0,
                                bottom: c.1 + size,
                            };
                            t.rt.DrawText(
                                &text,
                                format,
                                &rect,
                                &t.brushes[level_index(level)],
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                        }
                    }
                    _ => {} // level 0 is invisible
                }
            }
            t.rt.EndDraw(None, None)
        }
    }

    fn create_target(&self) -> Result<Target> {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            // 96 DPI: one DIP per pixel. The process is per-monitor DPI aware and the
            // grid is laid out in physical pixels.
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        // SAFETY: `props` and each palette color are valid for the duration of the calls.
        unsafe {
            let rt = self.factory.CreateDCRenderTarget(&props)?;
            let brushes = self
                .palette
                .iter()
                .map(|c| rt.CreateSolidColorBrush(c, None))
                .collect::<Result<_>>()?;
            Ok(Target { rt, brushes })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Batch;

    #[test]
    fn batch_offsets_are_absolute_with_y_up() {
        let mut b = Batch::default();
        b.push(10.0, 20.0, 'a');
        b.push(30.0, 50.0, 'b');
        assert_eq!(b.codepoints, ['a' as u32, 'b' as u32]);
        let o: Vec<_> = b
            .offsets
            .iter()
            .map(|o| (o.advanceOffset, o.ascenderOffset))
            .collect();
        assert_eq!(o, [(10.0, -20.0), (30.0, -50.0)]);
    }
}
