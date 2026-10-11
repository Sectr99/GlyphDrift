//! The animations as a WebAssembly module for the website (`site/live.js`). A plain C ABI,
//! no bindings generator: JavaScript passes strings through `gd_alloc` memory, steps an
//! animation, and reads each frame back out of linear memory.
//!
//! A frame is the grid, one `u32` per cell (`char | brightness << 21`), and the shapes as
//! `f32` records (see `push_shape`); text shapes point into a byte buffer.

use glyphdrift::anim::{self, Animation, Rect, Size};
use glyphdrift::grid::Grid;
use glyphdrift::shape::{catmull_rom, Shape};
use glyphdrift::theme::{self, THEMES};

/// Palette levels, as in the native renderers.
const LEVELS: usize = 16;

pub struct Live {
    anim: Box<dyn Animation>,
    grid: Grid,
    shapes: Vec<Shape>,
    clock: Option<[String; 2]>,
    cells: Vec<u32>,
    ops: Vec<f32>,
    text: Vec<u8>,
}

/// # Safety
/// `ptr` must be `len` readable bytes (or `len` 0).
unsafe fn string(ptr: *const u8, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
}

/// `len` bytes for JavaScript to write a string into; free with `gd_free`.
#[no_mangle]
pub extern "C" fn gd_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` and `len` must come from one `gd_alloc` call.
#[no_mangle]
pub unsafe extern "C" fn gd_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

/// A new animation by config name (unknown names give Matrix rain), laid out like the
/// app: a `cols` x `rows` grid over `width` x `height` px, at the given Overall and Object
/// scale, centered on the leftmost `focus_width` px.
///
/// # Safety
/// `name` must be `name_len` readable bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn gd_new(
    name: *const u8,
    name_len: usize,
    cols: u32,
    rows: u32,
    width: f32,
    height: f32,
    zoom: f32,
    object_zoom: f32,
    seed: u32,
) -> *mut Live {
    let name = string(name, name_len);
    let mut anim = anim::by_name(&name, &glyphdrift::config::Config::default().theme.ramp);
    let (cols, rows) = (cols.max(1) as usize, rows.max(1) as usize);
    anim.init(
        Size {
            cols,
            rows,
            width,
            height,
            focus: Rect {
                zoom,
                object_zoom,
                ..Rect::new(0.0, 0.0, width, height)
            },
        },
        seed as u64,
    );
    Box::into_raw(Box::new(Live {
        anim,
        grid: Grid::new(cols, rows),
        shapes: Vec::new(),
        clock: None,
        cells: Vec::new(),
        ops: Vec::new(),
        text: Vec::new(),
    }))
}

/// # Safety
/// `live` must come from `gd_new` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn gd_drop(live: *mut Live) {
    drop(Box::from_raw(live));
}

/// # Safety
/// `live` must come from `gd_new`.
#[no_mangle]
pub unsafe extern "C" fn gd_update(live: *mut Live, dt: f32) {
    (*live).anim.update(dt);
}

/// The frame rate the app uses for this animation on Auto.
///
/// # Safety
/// `live` must come from `gd_new`.
#[no_mangle]
pub unsafe extern "C" fn gd_fps(live: *const Live) -> u32 {
    (*live).anim.fps()
}

/// 1 if the animation uses the theme's warm gradient.
///
/// # Safety
/// `live` must come from `gd_new`.
#[no_mangle]
pub unsafe extern "C" fn gd_warm(live: *const Live) -> u32 {
    u32::from((*live).anim.warm())
}

/// The clock, as "time\ndate"; empty for none.
///
/// # Safety
/// `live` must come from `gd_new`; `text` must be `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn gd_set_clock(live: *mut Live, text: *const u8, len: usize) {
    let text = string(text, len);
    (*live).clock = text
        .split_once('\n')
        .map(|(t, d)| [t.to_string(), d.to_string()]);
}

/// Render the current frame into the cell, shape and text buffers.
///
/// # Safety
/// `live` must come from `gd_new`.
#[no_mangle]
pub unsafe extern "C" fn gd_frame(live: *mut Live) {
    let l = &mut *live;
    l.anim.set_clock(l.clock.as_ref());
    l.anim.render(&mut l.grid);
    l.shapes.clear();
    l.anim.draw(&mut l.shapes);
    l.cells.clear();
    for y in 0..l.grid.rows() {
        l.cells.extend(
            l.grid
                .row(y)
                .iter()
                .map(|c| c.ch as u32 | (c.brightness as u32) << 21),
        );
    }
    l.ops.clear();
    l.text.clear();
    for s in &l.shapes {
        push_shape(&mut l.ops, &mut l.text, s);
    }
}

/// Shape records, each `kind, level, ...`:
/// - 0 line: ax, ay, bx, by, width
/// - 1 circle: cx, cy, r, fill (0 or 1)
/// - 2 polygon: n, then n points
/// - 3 curve: width, closed, n, start point, then n cubic segments (c1, c2, end)
/// - 4 text: cx, cy, size, weight, byte offset, byte length
fn push_shape(ops: &mut Vec<f32>, text: &mut Vec<u8>, s: &Shape) {
    match s {
        Shape::Line { a, b, width, level } => {
            ops.extend([0.0, *level as f32, a.0, a.1, b.0, b.1, *width]);
        }
        Shape::Circle { c, r, fill, level } => {
            ops.extend([1.0, *level as f32, c.0, c.1, *r, f32::from(u8::from(*fill))]);
        }
        Shape::Polygon { points, level } => {
            ops.extend([2.0, *level as f32, points.len() as f32]);
            ops.extend(points.iter().flat_map(|p| [p.0, p.1]));
        }
        Shape::Curve {
            points,
            width,
            level,
        } => {
            if points.len() < 2 {
                return;
            }
            let (segments, closed) = catmull_rom(points);
            ops.extend([
                3.0,
                *level as f32,
                *width,
                f32::from(u8::from(closed)),
                segments.len() as f32,
                points[0].0,
                points[0].1,
            ]);
            ops.extend(segments.iter().flatten().flat_map(|p| [p.0, p.1]));
        }
        Shape::Text {
            c,
            size,
            weight,
            text: t,
            level,
        } => {
            ops.extend([
                4.0,
                *level as f32,
                c.0,
                c.1,
                *size,
                *weight as f32,
                text.len() as f32,
                t.len() as f32,
            ]);
            text.extend_from_slice(t.as_bytes());
        }
    }
}

/// # Safety
/// `live` must come from `gd_new`.
#[no_mangle]
pub unsafe extern "C" fn gd_cells(live: *const Live) -> *const u32 {
    (*live).cells.as_ptr()
}

/// # Safety
/// `live` must come from `gd_new`.
#[no_mangle]
pub unsafe extern "C" fn gd_ops(live: *const Live) -> *const f32 {
    (*live).ops.as_ptr()
}

/// # Safety
/// `live` must come from `gd_new`.
#[no_mangle]
pub unsafe extern "C" fn gd_ops_len(live: *const Live) -> usize {
    (*live).ops.len()
}

/// # Safety
/// `live` must come from `gd_new`.
#[no_mangle]
pub unsafe extern "C" fn gd_text(live: *const Live) -> *const u8 {
    (*live).text.as_ptr()
}

/// The palette for a theme, as the native renderers build it: background, then `LEVELS`
/// brightness levels, 3 bytes each, into `out` (`(1 + LEVELS) * 3` bytes).
///
/// # Safety
/// `name` must be `name_len` readable bytes; `out` must be writable for 51 bytes.
#[no_mangle]
pub unsafe extern "C" fn gd_palette(
    name: *const u8,
    name_len: usize,
    warm: u32,
    glass: f32,
    out: *mut u8,
) {
    let t = &THEMES[theme::index_of(&string(name, name_len))];
    let stops = if warm != 0 { &t.warm } else { &t.cool };
    let out = std::slice::from_raw_parts_mut(out, (1 + LEVELS) * 3);
    let colors = std::iter::once(theme::frost(t.bg, t.bg, glass)).chain(
        theme::gradient(t.bg, stops, LEVELS)
            .into_iter()
            .map(|c| theme::frost(c, t.bg, glass)),
    );
    for (slot, c) in out.as_chunks_mut::<3>().0.iter_mut().zip(colors) {
        slot.copy_from_slice(&[c.0, c.1, c.2]);
    }
}
