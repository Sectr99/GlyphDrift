//! `glyphdrift --screenshots <dir>`: images for the website, drawn offscreen. Nothing is
//! read from the screen, the user's config or the desktop, so no personal content can end
//! up in them: animations come from the Metal renderer, the settings window is drawn
//! without being shown, and every setting and the clock text are fixed here.

use std::path::Path;

use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBezierPath, NSBitmapImageRep, NSColor,
    NSDeviceRGBColorSpace, NSGraphicsContext, NSShadow,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::render::{self, Renderer};
use super::{settings, Controller};
use crate::anim;
use crate::config::{Config, DEFAULT_FONT};
use crate::grid::Grid;
use crate::shape::Shape;
use crate::theme::{self, THEMES};

/// Pixels per point: Retina.
const SCALE: f64 = 2.0;
/// Gallery images, in points (16:10).
const GALLERY: (f64, f64) = (1200.0, 750.0);
/// The "in use" image: 16:10, small enough that the settings window stays readable when
/// the image is shown at page width.
const IN_USE: (f64, f64) = (1000.0, 625.0);
/// Space between the settings window and the image's top and right edges, in points.
const MARGIN: f64 = 40.0;
/// The "in use" image's Object scale setting.
const IN_USE_OBJECT_ZOOM: f32 = 1.4;
/// The "in use" image's animation and theme.
const IN_USE_ANIMATION: (&str, &str) = ("cubes", "mono");
/// Fixed clock text, so no real time or date appears.
const CLOCK: [&str; 2] = ["9:41 AM", "Saturday, October 10, 2026"];
/// Seconds of animation before the picture, so each has filled the frame.
const WARM_UP: f32 = 20.0;

/// A theme per animation: varied, and bright enough to read as a small card.
const THEME_FOR: &[(&str, &str)] = &[
    ("matrix", "matrix"),
    ("plasma", "synthwave-84"),
    ("fire", "amber"),
    ("waves", "night-owl"),
    ("ripples", "catppuccin-mocha"),
    ("wind", "synthwave-84"),
    ("constellation", "night-owl"),
    ("flow", "poimandres"),
    ("helix", "synthwave-84"),
    ("globe", "matrix"),
    ("dotwave", "kanagawa"),
    ("cubes", "gruvbox"),
    ("sunburst", "claude"),
    ("moon", "mono"),
    ("lava", "synthwave-84"),
    ("ink", "poimandres"),
    ("topo", "mono"),
    ("ridges", "everforest"),
    ("isobars", "github-dark"),
    ("wood", "gruvbox"),
    ("interference", "oxocarbon"),
    ("hillshade", "solarized-dark"),
    ("dunes", "ayu-dark"),
];
/// Gallery sizes, as the user's Overall scale and Object scale (Loops only) settings:
/// larger features read better as a small card.
const GALLERY_ZOOM: f32 = 1.5;
/// Animations that add drops, gusts or particles per cell or per area: fewer cells at a
/// larger scale would leave them sparse, so they keep the default scale.
const DEFAULT_SCALE: &[&str] = &["ripples", "wind", "flow"];
const GALLERY_OBJECT_ZOOM: f32 = 1.3;

/// One frame of `name` in `theme_name`, `size` points, at the given Overall scale and
/// Object scale. Centered objects center on the leftmost `focus_width` points, as the app
/// centers them on the main screen within a wider window.
fn frame(
    r: &mut Renderer,
    name: &str,
    theme_name: &str,
    size: (f64, f64),
    zoom: (f32, f32),
    focus_width: f64,
) -> Result<Retained<NSBitmapImageRep>, String> {
    let cfg = Config::default();
    let mut a = anim::by_name(name, &cfg.theme.ramp);
    // The grid font follows the overall scale, as in the app.
    r.set_font(
        DEFAULT_FONT,
        (cfg.font.size as f32 * zoom.0).round() as f64,
        SCALE,
    );
    let (cw, ch) = r.cell_size();
    let (cols, rows) = ((size.0 / cw) as usize, (size.1 / ch) as usize);
    let (w, h) = (size.0 as f32, size.1 as f32);
    a.init(
        anim::Size {
            cols,
            rows,
            width: w,
            height: h,
            focus: anim::Rect {
                zoom: zoom.0,
                object_zoom: zoom.1,
                ..anim::Rect::new(0.0, 0.0, focus_width as f32, h)
            },
        },
        42,
    );
    let steps = (WARM_UP * 30.0) as usize;
    for _ in 0..steps {
        a.update(1.0 / 30.0);
    }
    let clock = CLOCK.map(String::from);
    a.set_clock(Some(&clock));
    let (mut grid, mut shapes) = (Grid::new(cols, rows), Vec::<Shape>::new());
    a.render(&mut grid);
    a.draw(&mut shapes);
    let t = &THEMES[theme::index_of(theme_name)];
    r.set_colors(t.bg, if a.warm() { &t.warm } else { &t.cool }, 0.0);
    render::render_offscreen(r, size, SCALE, &grid, &shapes)
}

/// `window` over `background` at the top right, with rounded corners and a shadow.
fn composite(
    background: &NSBitmapImageRep,
    window: &NSBitmapImageRep,
) -> Result<Retained<NSBitmapImageRep>, String> {
    let (w, h) = IN_USE;
    // SAFETY: null planes make the rep allocate its own buffer.
    let out = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            (w * SCALE) as isize,
            (h * SCALE) as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }
    .ok_or("cannot create bitmap")?;
    out.setSize(NSSize::new(w, h));
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&out).ok_or("no context")?;
    let previous = NSGraphicsContext::currentContext();
    NSGraphicsContext::setCurrentContext(Some(&ctx));

    background.drawInRect(NSRect::new(NSPoint::ZERO, NSSize::new(w, h)));
    let size = window.size();
    let rect = NSRect::new(
        NSPoint::new(w - size.width - MARGIN, h - size.height - MARGIN),
        size,
    );
    let radius = 16.0;
    let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius);
    // The shadow, drawn by a filled copy of the window's shape.
    NSGraphicsContext::saveGraphicsState_class();
    let shadow = NSShadow::new();
    shadow.setShadowBlurRadius(40.0);
    shadow.setShadowOffset(NSSize::new(0.0, -12.0));
    shadow.setShadowColor(Some(&NSColor::colorWithWhite_alpha(0.0, 0.6)));
    shadow.set();
    NSColor::blackColor().setFill();
    path.fill();
    NSGraphicsContext::restoreGraphicsState_class();
    // The window, clipped to its rounded corners.
    NSGraphicsContext::saveGraphicsState_class();
    path.addClip();
    window.drawInRect(rect);
    NSGraphicsContext::restoreGraphicsState_class();

    ctx.flushGraphics();
    NSGraphicsContext::setCurrentContext(previous.as_deref());
    Ok(out)
}

fn save(dir: &Path, name: &str, rep: &NSBitmapImageRep) -> Result<(), String> {
    let path = dir.join(name);
    std::fs::write(&path, render::png(rep)?).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{}", path.display());
    Ok(())
}

pub fn run(dir: &Path) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("must start on the main thread")?;
    // AppKit drawing needs the shared application; no Dock icon, windows or menu bar item.
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    let t = &THEMES[0];
    let mut r = Renderer::new(DEFAULT_FONT, 16.0, SCALE, t.bg, &t.cool, 0.0)?;
    for &(name, theme_name) in THEME_FOR {
        let loops = anim::ANIMATIONS[anim::index_of(name)].2 == "loops";
        let object = if loops { GALLERY_OBJECT_ZOOM } else { 1.0 };
        let zoom = if DEFAULT_SCALE.contains(&name) {
            1.0
        } else {
            GALLERY_ZOOM
        };
        let image = frame(&mut r, name, theme_name, GALLERY, (zoom, object), GALLERY.0)?;
        save(dir, &format!("anim-{name}.png"), &image)?;
    }

    // The settings window with default settings, showing Cubes: a Loops animation, so
    // every slider is live (a disabled slider doesn't draw offscreen correctly).
    let mut cfg = Config::default();
    cfg.general.animation = IN_USE_ANIMATION.0.into();
    cfg.theme.name = IN_USE_ANIMATION.1.into();
    // Larger cubes in the narrower space beside the window; the window shows the setting.
    cfg.general.object_scale = IN_USE_OBJECT_ZOOM;
    let controller = Controller::new(mtm);
    let window = settings::render_offscreen(mtm, &controller, &cfg)
        .ok_or("cannot draw the settings window")?;
    save(dir, "settings.png", &window)?;

    // The animation centers in the space left of the window.
    let beside = IN_USE.0 - window.size().width - MARGIN;
    let background = frame(
        &mut r,
        IN_USE_ANIMATION.0,
        IN_USE_ANIMATION.1,
        IN_USE,
        (1.0, IN_USE_OBJECT_ZOOM),
        beside,
    )?;
    let image = composite(&background, &window)?;
    save(dir, "in-use.png", &image)?;
    Ok(())
}
