//! Glyphdrift as a macOS screen saver: a `ScreenSaverView` that draws the app's animation
//! and theme with the app's Metal renderer. macOS loads it from `Glyphdrift.saver` (built
//! by `macos/bundle.sh`) into its screen saver host, one view per screen plus a small one
//! for the preview in System Settings.
//!
//! Settings come from the app's config file, read each time the saver starts. The host is
//! sandboxed with its own home folder, but may read any file, so the path is built from the
//! account's real home.
//!
//! The host (legacyScreenSaver) has been unreliable since macOS 14: it may never call
//! `stopAnimation`, keeps old views running after the screen saver ends, and its
//! `animateOneFrame` timing varies. So the view drives its own timer, and when the system
//! says the screen saver is stopping, it stops drawing and ends the host process, which
//! macOS starts again next time.

#![cfg(target_os = "macos")]

use std::cell::RefCell;
use std::ffi::{c_char, CStr};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use glyphdrift::anim::{self, Animation};
use glyphdrift::config::Config;
use glyphdrift::engine;
use glyphdrift::grid::Grid;
use glyphdrift::macos::render::{Renderer, Target};
use glyphdrift::shape::Shape;
use glyphdrift::theme::{self, THEMES};
use objc2::rc::{Allocated, Retained};
use objc2::{define_class, msg_send, sel, ClassType, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSScreen, NSView};
use objc2_foundation::{
    ns_string, NSDistributedNotificationCenter, NSNotification, NSRect, NSRunLoop,
    NSRunLoopCommonModes, NSTimer,
};
use objc2_screen_saver::ScreenSaverView;

/// Posted to every app when the screen saver is about to end.
const WILL_STOP: &str = "com.apple.screensaver.willstop";
/// Seconds between that and ending the host process: time for the screen to fade back.
const EXIT_DELAY: f64 = 2.0;

/// Everything a running view draws with, made fresh at each start.
struct State {
    cfg: Config,
    renderer: Renderer,
    target: Target,
    /// The layer-hosting subview that shows `target`.
    canvas: Retained<NSView>,
    anim: Box<dyn Animation>,
    grid: Grid,
    shapes: Vec<Shape>,
    /// Size in points and pixels per point, at the last layout.
    size: (f64, f64),
    scale: f64,
    timer: Retained<NSTimer>,
    last_frame: Instant,
}

#[derive(Default)]
struct Ivars {
    state: RefCell<Option<State>>,
}

define_class!(
    /// The screen saver's principal class (`NSPrincipalClass` in the bundle's Info.plist).
    #[unsafe(super(ScreenSaverView, NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "GlyphdriftSaverView"]
    #[ivars = Ivars]
    struct SaverView;

    impl SaverView {
        #[unsafe(method_id(initWithFrame:isPreview:))]
        fn init_with_frame(
            this: Allocated<Self>,
            frame: NSRect,
            is_preview: bool,
        ) -> Option<Retained<Self>> {
            let this = this.set_ivars(Ivars::default());
            // SAFETY: ScreenSaverView's designated initializer.
            unsafe { msg_send![super(this), initWithFrame: frame, isPreview: is_preview] }
        }

        #[unsafe(method(startAnimation))]
        fn start_animation(&self) {
            // SAFETY: the superclass method, with no arguments.
            let () = unsafe { msg_send![super(self), startAnimation] };
            self.start();
        }

        #[unsafe(method(stopAnimation))]
        fn stop_animation(&self) {
            // SAFETY: as above.
            let () = unsafe { msg_send![super(self), stopAnimation] };
            self.stop();
        }

        /// Frames come from the view's own timer instead.
        #[unsafe(method(animateOneFrame))]
        fn animate_one_frame(&self) {}

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            self.frame();
        }

        #[unsafe(method(willStop:))]
        fn will_stop(&self, _note: &NSNotification) {
            self.stop();
            // SAFETY: plain property read.
            if unsafe { self.isPreview() } {
                return;
            }
            // SAFETY: `exitNow:` takes the timer; the timer retains the target.
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    EXIT_DELAY,
                    self,
                    sel!(exitNow:),
                    None,
                    false,
                );
            }
        }

        #[unsafe(method(exitNow:))]
        fn exit_now(&self, _timer: &NSTimer) {
            std::process::exit(0);
        }
    }
);

impl SaverView {
    /// Read the settings, set up the renderer and animation, and start the frame timer.
    fn start(&self) {
        self.stop();
        let cfg = load_config();
        let mtm = self.mtm();
        let scale = self.window().map_or(2.0, |w| w.backingScaleFactor());
        // The preview is a miniature of the full screen, so its font shrinks to match.
        // SAFETY: plain property read.
        let shrink = if unsafe { self.isPreview() } {
            NSScreen::mainScreen(mtm).map_or(1.0, |s| {
                (self.bounds().size.width / s.frame().size.width).min(1.0)
            })
        } else {
            1.0
        };
        let font_size = (engine::font_size(&cfg) * shrink).max(2.0);
        let anim = anim::by_name(&cfg.general.animation, &cfg.theme.ramp);
        let t = &THEMES[theme::index_of(&cfg.theme.name)];
        let stops = if anim.warm() { &t.warm } else { &t.cool };
        let renderer = match Renderer::new(
            &cfg.font.family,
            font_size,
            scale,
            t.bg,
            stops,
            cfg.theme.glass,
        ) {
            Ok(r) => r,
            Err(e) => {
                glyphdrift::log::write(&format!("screen saver: {e}"));
                return;
            }
        };
        let target = renderer.target(scale);
        // A layer-hosting subview that follows the saver view's size: the layer is set
        // before turning layers on.
        let canvas = NSView::initWithFrame(NSView::alloc(mtm), self.bounds());
        canvas.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        canvas.setLayer(Some(target.layer()));
        canvas.setWantsLayer(true);
        self.addSubview(&canvas);

        let interval = 1.0 / engine::frame_rate(&cfg, anim.fps()) as f64;
        // SAFETY: `tick:` takes the timer; the timer retains the target until invalidated.
        let timer = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                interval,
                self,
                sel!(tick:),
                None,
                true,
            )
        };
        timer.setTolerance(interval * 0.1);
        // SAFETY: a fresh timer on the main run loop.
        unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        // SAFETY: `willStop:` takes the notification; removed again in `stop`.
        unsafe {
            NSDistributedNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(willStop:),
                Some(ns_string!(WILL_STOP)),
                None,
            );
        }

        let mut st = State {
            cfg,
            renderer,
            target,
            canvas,
            anim,
            grid: Grid::new(1, 1),
            shapes: Vec::new(),
            size: (0.0, 0.0),
            scale,
            timer,
            last_frame: Instant::now(),
        };
        layout(&mut st, self.points());
        draw(&mut st);
        *self.ivars().state.borrow_mut() = Some(st);
    }

    /// Stop the timer and let go of the renderer.
    fn stop(&self) {
        let Some(st) = self.ivars().state.borrow_mut().take() else {
            return;
        };
        st.timer.invalidate();
        st.canvas.removeFromSuperview();
        // SAFETY: removes this view's own observation.
        unsafe { NSDistributedNotificationCenter::defaultCenter().removeObserver(self) };
    }

    /// The view's size in points.
    fn points(&self) -> (f64, f64) {
        let size = self.bounds().size;
        (size.width, size.height)
    }

    /// Advance the animation and draw a frame.
    fn frame(&self) {
        let mut state = self.ivars().state.borrow_mut();
        let Some(st) = state.as_mut() else {
            return;
        };
        let now = Instant::now();
        let dt = now.duration_since(st.last_frame).as_secs_f32().min(0.25);
        st.last_frame = now;
        let size = self.points();
        if size != st.size {
            layout(st, size);
        }
        st.anim.update(dt * st.cfg.general.speed);
        draw(st);
    }
}

/// Size the grid to `size` points and start the animation afresh.
fn layout(st: &mut State, size: (f64, f64)) {
    st.size = size;
    let (cw, ch) = st.renderer.cell_size();
    let cols = (size.0 / cw).max(1.0) as usize;
    let rows = (size.1 / ch).max(1.0) as usize;
    st.grid.resize(cols, rows);
    let (w, h) = (size.0 as f32, size.1 as f32);
    let g = &st.cfg.general;
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    st.anim.init(
        anim::Size {
            cols,
            rows,
            width: w,
            height: h,
            focus: anim::Rect {
                zoom: g.scale,
                object_zoom: g.object_scale,
                ..anim::Rect::new(0.0, 0.0, w, h)
            },
        },
        seed,
    );
}

fn draw(st: &mut State) {
    let clock = st.cfg.general.clock.then(glyphdrift::macos::local_clock);
    st.anim.render(&mut st.grid);
    st.anim.set_clock(clock.as_ref());
    st.shapes.clear();
    st.anim.draw(&mut st.shapes);
    st.renderer
        .paint(&mut st.target, st.size, st.scale, &st.grid, &st.shapes);
}

/// The app's settings; defaults if there are none or they can't be read.
fn load_config() -> Config {
    let path = real_home().map(|h| h.join("Library/Application Support/Glyphdrift/config.toml"));
    match path.and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => Config::from_toml(&text).0,
        None => Config::default(),
    }
}

/// The account's home folder. `HOME` points into the host's sandbox container instead.
fn real_home() -> Option<PathBuf> {
    #[repr(C)]
    struct Passwd {
        name: *const c_char,
        passwd: *const c_char,
        uid: u32,
        gid: u32,
        change: i64,
        class: *const c_char,
        gecos: *const c_char,
        dir: *const c_char,
        shell: *const c_char,
        expire: i64,
    }
    extern "C" {
        fn getuid() -> u32;
        fn getpwuid(uid: u32) -> *const Passwd;
    }
    // SAFETY: getpwuid returns null or a valid entry whose `dir` is a C string; it's copied
    // out at once, before any other call could reuse the buffer.
    unsafe {
        let pw = getpwuid(getuid());
        if pw.is_null() || (*pw).dir.is_null() {
            return None;
        }
        let dir = CStr::from_ptr((*pw).dir).to_str().ok()?;
        Some(PathBuf::from(dir))
    }
}

/// Register the class as soon as macOS loads the bundle, so it can find the principal
/// class by name.
#[used]
#[link_section = "__DATA,__mod_init_func"]
static REGISTER: extern "C" fn() = register;

extern "C" fn register() {
    let _ = SaverView::class();
}
