//! Renderer: Metal, one pass per screen with 4x MSAA. Everything becomes triangles in one
//! vertex format: grid cells are quads textured from a glyph atlas (Core Text rasterizes
//! each character once), lines and flattened curves are a quad per segment, circles and
//! polygons are fans. Text shapes are rasterized to small cached textures. Positions are in
//! points; `scale` is the screen's pixels per point.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontAttributeName, NSFontWeightLight,
    NSFontWeightRegular, NSForegroundColorAttributeName, NSGraphicsContext,
};
use objc2_core_foundation::{CFRetained, CFString, CGFloat, CGPoint, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGGlyph, CGImageAlphaInfo,
};
use objc2_core_text::{CTFont, CTFontOrientation};
use objc2_foundation::{NSAttributedString, NSDictionary, NSString};
use objc2_metal::{
    MTLBlendFactor, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLGPUFamily, MTLLibrary, MTLLoadAction, MTLOrigin,
    MTLPixelFormat, MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLResourceOptions, MTLSize,
    MTLStorageMode, MTLStoreAction, MTLTexture, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};

use crate::config::{Rgb, DEFAULT_FONT};
use crate::grid::Grid;
use crate::shape::Shape;
use crate::theme;

const LEVELS: usize = 16;
const SAMPLES: usize = 4;
const PIXEL_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;
const ATLAS: usize = 2048;
/// Text textures kept; the clock needs two at a time.
const TEXT_CACHE: usize = 8;

const SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Vertex { float2 pos; float2 uv; float4 color; };
struct Out { float4 pos [[position]]; float2 uv; float4 color; };
vertex Out vs(const device Vertex* v [[buffer(0)]], constant float2& size [[buffer(1)]],
              uint i [[vertex_id]]) {
    float2 p = v[i].pos / size * 2.0 - 1.0;
    Out o;
    o.pos = float4(p.x, -p.y, 0.0, 1.0);
    o.uv = v[i].uv;
    o.color = v[i].color;
    return o;
}
// uv.x < 0: a solid shape; else coverage from the bound texture.
fragment float4 fs(Out in [[stage_in]], texture2d<float> tex [[texture(0)]]) {
    constexpr sampler s(filter::linear);
    float a = in.color.a * (in.uv.x < 0.0 ? 1.0 : tex.sample(s, in.uv).r);
    return float4(in.color.rgb * a, a);
}
"#;

type Device = ProtocolObject<dyn MTLDevice>;
type Texture = Retained<ProtocolObject<dyn MTLTexture>>;
type P = (f32, f32);

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

/// A run of vertices drawn with one texture: the atlas, or a text texture.
struct Draw {
    start: usize,
    count: usize,
    text: Option<usize>,
}

/// One screen's layer and multisample buffer.
pub struct Target {
    layer: Retained<CAMetalLayer>,
    msaa: Option<Texture>,
}

impl Target {
    pub fn layer(&self) -> &CAMetalLayer {
        &self.layer
    }
}

struct TextTexture {
    key: (String, u32, u16),
    texture: Texture,
    /// Size in points.
    size: (f32, f32),
}

pub struct Renderer {
    device: Retained<Device>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pipeline: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    memoryless: bool,
    /// Pixels per point the glyphs are rasterized at.
    scale: f64,
    font: CFRetained<CTFont>,
    baseline: f64,
    cell_w: f64,
    cell_h: f64,
    /// Space around each glyph's cell in its atlas slot, for glyphs that overhang.
    pad: f64,
    atlas: Texture,
    /// Atlas slot per character; None if it didn't fit.
    slots: HashMap<char, Option<(f32, f32)>>,
    next_slot: usize,
    bg: Rgb,
    /// Foreground color per brightness level.
    palette: [[f32; 4]; LEVELS],
    texts: Vec<TextTexture>,
    /// Text fonts by (size bits, weight).
    // ponytail: never evicted; sizes only change on resize.
    text_fonts: Vec<((u32, u16), Retained<NSFont>)>,
    vertices: Vec<Vertex>,
    draws: Vec<Draw>,
}

/// Palette index for a 0..=255 brightness.
fn level_index(brightness: u8) -> usize {
    (brightness as usize * LEVELS / 256).min(LEVELS - 1)
}

fn rgba(c: Rgb) -> [f32; 4] {
    [
        c.0 as f32 / 255.0,
        c.1 as f32 / 255.0,
        c.2 as f32 / 255.0,
        1.0,
    ]
}

/// The named font, or the default if Core Text substituted another.
fn find_font(family: &str, size: f64) -> CFRetained<CTFont> {
    // SAFETY: valid names; a null matrix means identity.
    let make = |name: &str| unsafe {
        CTFont::with_name(&CFString::from_str(name), size, std::ptr::null())
    };
    let font = make(family);
    // SAFETY: `font` is live.
    if unsafe { font.family_name() }
        .to_string()
        .eq_ignore_ascii_case(family)
    {
        return font;
    }
    log!("font: '{family}' not found");
    make(DEFAULT_FONT)
}

/// An 8-bit alpha-only bitmap context, `w` x `h` px, cleared.
fn alpha_context(w: usize, h: usize) -> Option<CFRetained<CGContext>> {
    // SAFETY: a null buffer makes the context allocate its own.
    let ctx = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            w,
            h,
            8,
            0,
            None,
            CGImageAlphaInfo::Only.0,
        )
    }?;
    CGContext::set_should_smooth_fonts(Some(&ctx), false);
    Some(ctx)
}

/// Copy an alpha-only context into an R8 texture region at (x, y).
fn upload(
    ctx: &CGContext,
    texture: &ProtocolObject<dyn MTLTexture>,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) {
    let data = objc2_core_graphics::CGBitmapContextGetData(Some(ctx));
    let row = objc2_core_graphics::CGBitmapContextGetBytesPerRow(Some(ctx));
    let Some(data) = NonNull::new(data) else {
        return;
    };
    let region = MTLRegion {
        origin: MTLOrigin { x, y, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    // SAFETY: the context's buffer holds `h` rows of `row` bytes, covering the region.
    unsafe { texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(region, 0, data, row) };
}

fn r8_texture(device: &Device, w: usize, h: usize) -> Result<Texture, String> {
    // SAFETY: valid sizes and format.
    let desc = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::R8Unorm,
            w.max(1),
            h.max(1),
            false,
        )
    };
    desc.setUsage(MTLTextureUsage::ShaderRead);
    device
        .newTextureWithDescriptor(&desc)
        .ok_or_else(|| "cannot create texture".into())
}

impl Renderer {
    /// `size` is the font's em height in points; `scale` the screens' highest pixels per point.
    pub fn new(
        family: &str,
        size: f64,
        scale: f64,
        bg: Rgb,
        stops: &[Rgb; 3],
        glass: f32,
    ) -> Result<Self, String> {
        let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        let queue = device.newCommandQueue().ok_or("no command queue")?;
        let library = device
            .newLibraryWithSource_options_error(&NSString::from_str(SHADER), None)
            .map_err(|e| format!("shader: {}", e.localizedDescription()))?;
        let desc = MTLRenderPipelineDescriptor::new();
        desc.setVertexFunction(
            library
                .newFunctionWithName(&NSString::from_str("vs"))
                .as_deref(),
        );
        desc.setFragmentFunction(
            library
                .newFunctionWithName(&NSString::from_str("fs"))
                .as_deref(),
        );
        // SAFETY: a supported sample count; attachment 0 exists.
        unsafe {
            desc.setRasterSampleCount(SAMPLES);
            let color = desc.colorAttachments().objectAtIndexedSubscript(0);
            color.setPixelFormat(PIXEL_FORMAT);
            // Premultiplied alpha, for lines thinner than a pixel.
            color.setBlendingEnabled(true);
            color.setSourceRGBBlendFactor(MTLBlendFactor::One);
            color.setSourceAlphaBlendFactor(MTLBlendFactor::One);
            color.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            color.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        }
        let pipeline = device
            .newRenderPipelineStateWithDescriptor_error(&desc)
            .map_err(|e| format!("pipeline: {}", e.localizedDescription()))?;
        let memoryless = device.supportsFamily(MTLGPUFamily::Apple1);
        let atlas = r8_texture(&device, ATLAS, ATLAS)?;
        let font = find_font(family, size);
        let mut r = Self {
            device,
            queue,
            pipeline,
            memoryless,
            scale,
            font,
            baseline: 0.0,
            cell_w: 1.0,
            cell_h: 1.0,
            pad: 0.0,
            atlas,
            slots: HashMap::new(),
            next_slot: 0,
            bg,
            palette: [rgba(bg); LEVELS],
            texts: Vec::new(),
            text_fonts: Vec::new(),
            vertices: Vec::new(),
            draws: Vec::new(),
        };
        r.set_font(family, size, scale);
        r.set_colors(bg, stops, glass);
        Ok(r)
    }

    /// New grid font, size or screen scale: re-measure and start a fresh atlas.
    pub fn set_font(&mut self, family: &str, size: f64, scale: f64) {
        self.font = find_font(family, size);
        self.scale = scale.max(1.0);
        // SAFETY: `font` is live; out-pointers are live locals of the right length.
        let (ascent, descent, advance) = unsafe {
            let mut ch = 'M' as u16;
            let mut glyph: CGGlyph = 0;
            self.font
                .glyphs_for_characters(NonNull::from(&mut ch), NonNull::from(&mut glyph), 1);
            let advance = self.font.advances_for_glyphs(
                CTFontOrientation::Horizontal,
                NonNull::from(&mut glyph),
                std::ptr::null_mut(),
                1,
            );
            (self.font.ascent(), self.font.descent(), advance)
        };
        self.baseline = ascent;
        self.cell_w = advance.max(1.0);
        self.cell_h = (ascent + descent).ceil().max(1.0);
        self.pad = (self.cell_h * 0.25).ceil();
        self.slots.clear();
        self.next_slot = 0;
        self.text_fonts.clear();
        self.texts.clear();
    }

    /// Background and three stops (dim to bright) under a veil of opacity `glass`.
    pub fn set_colors(&mut self, bg: Rgb, stops: &[Rgb; 3], glass: f32) {
        self.bg = theme::frost(bg, bg, glass);
        for (slot, c) in self
            .palette
            .iter_mut()
            .zip(theme::gradient(bg, stops, LEVELS))
        {
            *slot = rgba(theme::frost(c, bg, glass));
        }
    }

    pub fn background(&self) -> Rgb {
        self.bg
    }

    /// Cell size in points.
    pub fn cell_size(&self) -> (f64, f64) {
        (self.cell_w, self.cell_h)
    }

    /// A layer for one screen at `scale` pixels per point.
    pub fn target(&self, scale: f64) -> Target {
        let layer = CAMetalLayer::new();
        layer.setDevice(Some(&self.device));
        layer.setPixelFormat(PIXEL_FORMAT);
        layer.setFramebufferOnly(true);
        layer.setOpaque(true);
        layer.setContentsScale(scale);
        // SAFETY: a constant color space name.
        let srgb = CGColorSpace::with_name(Some(unsafe { objc2_core_graphics::kCGColorSpaceSRGB }));
        layer.setColorspace(srgb.as_deref());
        Target { layer, msaa: None }
    }

    /// Slot size in pixels.
    fn slot_px(&self) -> (usize, usize) {
        (
            ((self.cell_w + 2.0 * self.pad) * self.scale).ceil() as usize,
            ((self.cell_h + 2.0 * self.pad) * self.scale).ceil() as usize,
        )
    }

    /// Atlas position (normalized) of `ch`, rasterizing it on first use.
    fn slot(&mut self, ch: char) -> Option<(f32, f32)> {
        if let Some(&s) = self.slots.get(&ch) {
            return s;
        }
        let (sw, sh) = self.slot_px();
        let per_row = (ATLAS / sw.max(1)).max(1);
        let n = self.next_slot;
        let (x, y) = ((n % per_row) * sw, (n / per_row) * sh);
        let slot = if y + sh <= ATLAS {
            self.rasterize(ch, x, y, sw, sh)
        } else {
            log!("render: glyph atlas full");
            None
        };
        if slot.is_some() {
            self.next_slot += 1;
        }
        self.slots.insert(ch, slot);
        slot
    }

    fn rasterize(&self, ch: char, x: usize, y: usize, sw: usize, sh: usize) -> Option<(f32, f32)> {
        let ctx = alpha_context(sw, sh)?;
        let c = Some(&*ctx);
        let mut units = [0u16; 2];
        let units = ch.encode_utf16(&mut units);
        let mut glyphs = [0 as CGGlyph; 2];
        // SAFETY: both buffers hold `units.len()` (1 or 2) entries.
        unsafe {
            self.font.glyphs_for_characters(
                NonNull::new_unchecked(units.as_mut_ptr()),
                NonNull::new_unchecked(glyphs.as_mut_ptr()),
                units.len() as isize,
            );
        }
        CGContext::scale_ctm(c, self.scale, self.scale);
        CGContext::set_gray_fill_color(c, 1.0, 1.0);
        // Bitmap contexts are y-up: the baseline sits `pad + ascent` below the slot's top.
        let mut pos = CGPoint::new(self.pad, sh as f64 / self.scale - self.pad - self.baseline);
        // SAFETY: one glyph and one position.
        unsafe {
            self.font.draw_glyphs(
                NonNull::from(&mut glyphs[0]),
                NonNull::from(&mut pos),
                1,
                &ctx,
            )
        };
        upload(&ctx, &self.atlas, x, y, sw, sh);
        Some((x as f32 / ATLAS as f32, y as f32 / ATLAS as f32))
    }

    /// Index into `texts` for this text, rasterizing it if new.
    fn text(&mut self, text: &str, size: f32, weight: u16) -> Option<usize> {
        let key = (text.to_string(), size.to_bits(), weight);
        if let Some(i) = self.texts.iter().position(|t| t.key == key) {
            return Some(i);
        }
        let fkey = (size.to_bits(), weight);
        let font = match self.text_fonts.iter().find(|f| f.0 == fkey) {
            Some((_, f)) => f.clone(),
            None => {
                // SAFETY: plain constants.
                let w = unsafe {
                    if weight < 400 {
                        NSFontWeightLight
                    } else {
                        NSFontWeightRegular
                    }
                };
                let f = NSFont::systemFontOfSize_weight(size.max(1.0) as CGFloat, w);
                self.text_fonts.push((fkey, f.clone()));
                f
            }
        };
        let white = NSColor::whiteColor();
        // SAFETY: AppKit's attribute keys, with values of the types they expect.
        let attrs = unsafe {
            NSDictionary::<NSString, objc2::runtime::AnyObject>::from_slices(
                &[NSFontAttributeName, NSForegroundColorAttributeName],
                &[
                    &*font as &objc2::runtime::AnyObject,
                    &*white as &objc2::runtime::AnyObject,
                ],
            )
        };
        // SAFETY: `attrs` holds valid string attributes.
        let s = unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str(text),
                Some(&attrs),
            )
        };
        let extent = s.size();
        let (w, h) = (
            ((extent.width * self.scale).ceil() as usize).max(1),
            ((extent.height * self.scale).ceil() as usize).max(1),
        );
        let ctx = alpha_context(w, h)?;
        CGContext::scale_ctm(Some(&ctx), self.scale, self.scale);
        let ns = NSGraphicsContext::graphicsContextWithCGContext_flipped(&ctx, false);
        let previous = NSGraphicsContext::currentContext();
        NSGraphicsContext::setCurrentContext(Some(&ns));
        s.drawAtPoint(CGPoint::ZERO);
        NSGraphicsContext::setCurrentContext(previous.as_deref());
        let texture = r8_texture(&self.device, w, h).ok()?;
        upload(&ctx, &texture, 0, 0, w, h);
        if self.texts.len() >= TEXT_CACHE {
            self.texts.remove(0);
        }
        self.texts.push(TextTexture {
            key,
            texture,
            size: (extent.width as f32, extent.height as f32),
        });
        Some(self.texts.len() - 1)
    }

    fn quad(&mut self, corners: [P; 4], uv: [P; 4], color: [f32; 4]) {
        for i in [0, 1, 2, 2, 1, 3] {
            self.vertices.push(Vertex {
                pos: [corners[i].0, corners[i].1],
                uv: [uv[i].0, uv[i].1],
                color,
            });
        }
    }

    fn triangle(&mut self, p: [P; 3], color: [f32; 4]) {
        for q in p {
            self.vertices.push(Vertex {
                pos: [q.0, q.1],
                uv: [-1.0, -1.0],
                color,
            });
        }
    }

    /// A segment `width` points wide; `extend` adds half the width at each end, closing
    /// the gap at a joint. Lines thinner than a pixel fade instead of thinning.
    fn segment(
        &mut self,
        a: P,
        b: P,
        width: f32,
        color: [f32; 4],
        extend: (bool, bool),
        scale: f32,
    ) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        if len < 1e-4 {
            return;
        }
        let mut color = color;
        color[3] *= (width * scale).min(1.0);
        let half = width.max(1.0 / scale) / 2.0;
        let (ux, uy) = (dx / len, dy / len);
        let (nx, ny) = (-uy * half, ux * half);
        let ea = if extend.0 { half } else { 0.0 };
        let eb = if extend.1 { half } else { 0.0 };
        let a = (a.0 - ux * ea, a.1 - uy * ea);
        let b = (b.0 + ux * eb, b.1 + uy * eb);
        self.quad(
            [
                (a.0 + nx, a.1 + ny),
                (a.0 - nx, a.1 - ny),
                (b.0 + nx, b.1 + ny),
                (b.0 - nx, b.1 - ny),
            ],
            [(-1.0, -1.0); 4],
            color,
        );
    }

    fn polyline(&mut self, points: &[P], closed: bool, width: f32, color: [f32; 4], scale: f32) {
        let n = points.len();
        for i in 1..n {
            let joined = (i > 1 || closed, i < n - 1 || closed);
            self.segment(points[i - 1], points[i], width, color, joined, scale);
        }
    }

    /// Points around a circle, about one per 2 px of circumference; the first repeats.
    fn circle_points(c: P, r: f32, scale: f32) -> Vec<P> {
        let n = (r * scale * std::f32::consts::PI).clamp(8.0, 96.0) as usize;
        (0..=n)
            .map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect()
    }

    /// Fill `vertices` and `draws` for one frame.
    fn build(&mut self, grid: &Grid, shapes: &[Shape], scale: f64) {
        self.vertices.clear();
        self.draws.clear();
        let (sw, sh) = self.slot_px();
        let (uw, uh) = (sw as f32 / ATLAS as f32, sh as f32 / ATLAS as f32);
        let (qw, qh) = (
            (sw as f64 / self.scale) as f32,
            (sh as f64 / self.scale) as f32,
        );
        let snap = |v: f64| ((v * scale).round() / scale) as f32;
        for y in 0..grid.rows() {
            for (x, cell) in grid.row(y).iter().enumerate() {
                if cell.brightness == 0 || cell.ch == ' ' {
                    continue;
                }
                let Some((u, v)) = self.slot(cell.ch) else {
                    continue;
                };
                // Pixel-aligned, so glyphs sample their atlas texels exactly.
                let x0 = snap(x as f64 * self.cell_w - self.pad);
                let y0 = snap(y as f64 * self.cell_h - self.pad);
                let color = self.palette[level_index(cell.brightness)];
                self.quad(
                    [(x0, y0), (x0, y0 + qh), (x0 + qw, y0), (x0 + qw, y0 + qh)],
                    [(u, v), (u, v + uh), (u + uw, v), (u + uw, v + uh)],
                    color,
                );
            }
        }
        let s = scale as f32;
        let mut start = 0;
        for (i, shape) in shapes.iter().enumerate() {
            match *shape {
                Shape::Line { a, b, width, level } if level > 0 => {
                    // Joints with the neighboring lines get no gap.
                    let prev = i.checked_sub(1).and_then(|j| shapes.get(j));
                    let joined_a = matches!(prev, Some(Shape::Line { b: pb, .. }) if *pb == a);
                    let joined_b =
                        matches!(shapes.get(i + 1), Some(Shape::Line { a: na, .. }) if *na == b);
                    let color = self.palette[level_index(level)];
                    self.segment(a, b, width, color, (joined_a, joined_b), s);
                }
                Shape::Curve {
                    ref points,
                    width,
                    level,
                } if level > 0 && points.len() >= 2 => {
                    let (segments, closed) = crate::shape::catmull_rom(points);
                    let mut flat = vec![points[0]];
                    for seg in segments {
                        let p0 = *flat.last().unwrap();
                        let d = |a: P, b: P| (b.0 - a.0).hypot(b.1 - a.1);
                        let length = d(p0, seg[0]) + d(seg[0], seg[1]) + d(seg[1], seg[2]);
                        // About one segment per 6 px.
                        let steps = ((length * s / 6.0).ceil() as usize).clamp(1, 32);
                        for k in 1..=steps {
                            let t = k as f32 / steps as f32;
                            let m = 1.0 - t;
                            let w = [m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t];
                            flat.push((
                                w[0] * p0.0 + w[1] * seg[0].0 + w[2] * seg[1].0 + w[3] * seg[2].0,
                                w[0] * p0.1 + w[1] * seg[0].1 + w[2] * seg[1].1 + w[3] * seg[2].1,
                            ));
                        }
                    }
                    let color = self.palette[level_index(level)];
                    self.polyline(&flat, closed, width, color, s);
                }
                Shape::Circle { c, r, fill, level } if level > 0 => {
                    let color = self.palette[level_index(level)];
                    let ring = Self::circle_points(c, r, s);
                    if fill {
                        for w in ring.windows(2) {
                            self.triangle([c, w[0], w[1]], color);
                        }
                    } else {
                        self.polyline(&ring, true, 1.0, color, s);
                    }
                }
                Shape::Polygon { ref points, level } if level > 0 && points.len() >= 3 => {
                    // ponytail: a fan, so convex polygons only (all the animations draw).
                    let color = self.palette[level_index(level)];
                    for w in points[1..].windows(2) {
                        self.triangle([points[0], w[0], w[1]], color);
                    }
                }
                Shape::Text {
                    c,
                    size,
                    weight,
                    ref text,
                    level,
                } if level > 0 => {
                    let Some(t) = self.text(text, size, weight) else {
                        continue;
                    };
                    // Close the run so far; the text gets its own draw.
                    self.draws.push(Draw {
                        start,
                        count: self.vertices.len() - start,
                        text: None,
                    });
                    let (w, h) = self.texts[t].size;
                    let (x0, y0) = (snap((c.0 - w / 2.0) as f64), snap((c.1 - h / 2.0) as f64));
                    let color = self.palette[level_index(level)];
                    let begin = self.vertices.len();
                    self.quad(
                        [(x0, y0), (x0, y0 + h), (x0 + w, y0), (x0 + w, y0 + h)],
                        [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)],
                        color,
                    );
                    self.draws.push(Draw {
                        start: begin,
                        count: 6,
                        text: Some(t),
                    });
                    start = self.vertices.len();
                }
                _ => {} // level 0 is invisible
            }
        }
        self.draws.push(Draw {
            start,
            count: self.vertices.len() - start,
            text: None,
        });
    }

    /// Encode one frame into `resolve` (the drawable's texture, or an offscreen one).
    fn encode(
        &mut self,
        cmd: &ProtocolObject<dyn MTLCommandBuffer>,
        resolve: &ProtocolObject<dyn MTLTexture>,
        msaa: &mut Option<Texture>,
        size: (f64, f64),
    ) -> Result<(), String> {
        let (w, h) = (resolve.width(), resolve.height());
        if msaa
            .as_ref()
            .is_none_or(|m| (m.width(), m.height()) != (w, h))
        {
            // SAFETY: valid sizes and format.
            let desc = unsafe {
                MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                    PIXEL_FORMAT,
                    w,
                    h,
                    false,
                )
            };
            desc.setTextureType(MTLTextureType::Type2DMultisample);
            // SAFETY: a supported sample count.
            unsafe { desc.setSampleCount(SAMPLES) };
            desc.setUsage(MTLTextureUsage::RenderTarget);
            // Tile memory only on Apple GPUs: no 4x framebuffer in RAM.
            desc.setStorageMode(if self.memoryless {
                MTLStorageMode::Memoryless
            } else {
                MTLStorageMode::Private
            });
            *msaa = Some(
                self.device
                    .newTextureWithDescriptor(&desc)
                    .ok_or("cannot create MSAA texture")?,
            );
        }
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: attachment 0 exists.
        let color = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        color.setTexture(msaa.as_deref());
        color.setResolveTexture(Some(resolve));
        color.setLoadAction(MTLLoadAction::Clear);
        color.setStoreAction(MTLStoreAction::MultisampleResolve);
        let bg = rgba(self.bg);
        color.setClearColor(MTLClearColor {
            red: bg[0] as f64,
            green: bg[1] as f64,
            blue: bg[2] as f64,
            alpha: 1.0,
        });
        let enc = cmd
            .renderCommandEncoderWithDescriptor(&pass)
            .ok_or("cannot encode")?;
        if !self.vertices.is_empty() {
            let bytes = std::mem::size_of_val(self.vertices.as_slice());
            // SAFETY: the pointer covers `bytes` bytes; Metal copies them.
            let buffer = unsafe {
                self.device.newBufferWithBytes_length_options(
                    NonNull::new_unchecked(self.vertices.as_mut_ptr().cast::<c_void>()),
                    bytes,
                    MTLResourceOptions::StorageModeShared,
                )
            }
            .ok_or("cannot create vertex buffer")?;
            let mut viewport = [size.0 as f32, size.1 as f32];
            enc.setRenderPipelineState(&self.pipeline);
            // SAFETY: buffer indices and the uniform's size match the shader; each draw's
            // range lies within the buffer.
            unsafe {
                enc.setVertexBuffer_offset_atIndex(Some(&buffer), 0, 0);
                enc.setVertexBytes_length_atIndex(NonNull::from(&mut viewport).cast(), 8, 1);
                for d in &self.draws {
                    if d.count == 0 {
                        continue;
                    }
                    let texture = match d.text {
                        Some(t) => &self.texts[t].texture,
                        None => &self.atlas,
                    };
                    enc.setFragmentTexture_atIndex(Some(texture), 0);
                    enc.drawPrimitives_vertexStart_vertexCount(
                        MTLPrimitiveType::Triangle,
                        d.start,
                        d.count,
                    );
                }
            }
        }
        enc.endEncoding();
        Ok(())
    }

    /// Draw `grid` then `shapes` to `target`, `size` points at `scale` px per point.
    pub fn paint(
        &mut self,
        target: &mut Target,
        size: (f64, f64),
        scale: f64,
        grid: &Grid,
        shapes: &[Shape],
    ) {
        if size.0 < 1.0 || size.1 < 1.0 {
            return;
        }
        let px = CGSize::new((size.0 * scale).round(), (size.1 * scale).round());
        if target.layer.drawableSize() != px {
            target.layer.setDrawableSize(px);
        }
        self.build(grid, shapes, scale);
        let Some(drawable) = target.layer.nextDrawable() else {
            return; // none free; skip the frame
        };
        let Some(cmd) = self.queue.commandBuffer() else {
            return;
        };
        if let Err(e) = self.encode(&cmd, &drawable.texture(), &mut target.msaa, size) {
            log!("render: {e}");
            return;
        }
        cmd.presentDrawable(ProtocolObject::from_ref(&*drawable));
        cmd.commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{self, ANIMATIONS};
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace};
    use objc2_metal::MTLBlitCommandEncoder;

    /// Draw every animation offscreen at 1728x1117 points (2x), timing the CPU side of a
    /// frame, and save PNGs to `target/snapshots`.
    /// `cargo test --release snapshots -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn snapshots() {
        let (w, h, scale) = (1728.0, 1117.0, 2.0);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/snapshots");
        std::fs::create_dir_all(&dir).unwrap();
        let t = &crate::theme::THEMES[0];
        let mut r = Renderer::new("Menlo", 16.0, scale, t.bg, &t.cool, 0.0).unwrap();
        let (cw, ch) = r.cell_size();
        let (pw, ph) = ((w * scale) as usize, (h * scale) as usize);
        // SAFETY: valid sizes and format.
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                PIXEL_FORMAT,
                pw,
                ph,
                false,
            )
        };
        desc.setUsage(MTLTextureUsage::RenderTarget);
        desc.setStorageMode(MTLStorageMode::Managed);
        let out = r.device.newTextureWithDescriptor(&desc).unwrap();
        let mut msaa = None;
        for &(name, ..) in ANIMATIONS {
            let mut a = anim::by_name(name, " .:-=+*#%@");
            let (cols, rows) = ((w / cw) as usize, (h / ch) as usize);
            a.init(
                anim::Size {
                    cols,
                    rows,
                    width: w as f32,
                    height: h as f32,
                    focus: anim::Rect::new(0.0, 0.0, w as f32, h as f32),
                },
                42,
            );
            let (mut grid, mut shapes) = (Grid::new(cols, rows), Vec::new());
            for _ in 0..60 {
                a.update(1.0 / 30.0);
            }
            a.set_clock(Some(&[
                "12:18 AM".into(),
                "Sunday, October 11, 2026".into(),
            ]));
            a.render(&mut grid);
            a.draw(&mut shapes);
            r.set_colors(t.bg, if a.warm() { &t.warm } else { &t.cool }, 0.0);
            const N: u32 = 20;
            let start = std::time::Instant::now();
            for _ in 0..N {
                r.build(&grid, &shapes, scale);
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0 / N as f64;
            let cmd = r.queue.commandBuffer().unwrap();
            r.encode(&cmd, &out, &mut msaa, (w, h)).unwrap();
            let blit = cmd.blitCommandEncoder().unwrap();
            blit.synchronizeResource(ProtocolObject::from_ref(&*out));
            blit.endEncoding();
            cmd.commit();
            cmd.waitUntilCompleted();
            println!(
                "{name:>14}: {ms:6.2} ms/frame CPU, {} vertices",
                r.vertices.len()
            );

            let mut bgra = vec![0u8; pw * ph * 4];
            // SAFETY: `bgra` holds the whole texture.
            unsafe {
                out.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                    NonNull::new_unchecked(bgra.as_mut_ptr().cast()),
                    pw * 4,
                    MTLRegion {
                        origin: MTLOrigin { x: 0, y: 0, z: 0 },
                        size: MTLSize {
                            width: pw,
                            height: ph,
                            depth: 1,
                        },
                    },
                    0,
                );
            }
            // SAFETY: null planes make the rep allocate its own pw x ph RGBA buffer.
            let rep = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(), std::ptr::null_mut(), pw as isize, ph as isize,
                    8, 4, true, false, NSDeviceRGBColorSpace, (pw * 4) as isize, 32,
                ).unwrap()
            };
            // SAFETY: the rep's buffer is pw x ph x 4 bytes.
            let dst = unsafe { std::slice::from_raw_parts_mut(rep.bitmapData(), pw * ph * 4) };
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(bgra.as_chunks::<4>().0)
            {
                d.copy_from_slice(&[s[2], s[1], s[0], 255]);
            }
            // SAFETY: an empty properties dictionary.
            let png = unsafe {
                rep.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .unwrap();
            std::fs::write(dir.join(format!("{name}.png")), png.to_vec()).unwrap();
        }
    }
}
