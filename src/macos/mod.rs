//! macOS host: wallpaper windows, frame loop, menu bar item, settings window and lifecycle.
//! Everything runs on the main thread; state lives in `APP`, reached from AppKit callbacks
//! through `with_app`.

mod autostart;
mod desktop;
mod menu;
mod picture;
mod power;
pub mod render;
mod saver;
mod screenshots;
mod settings;

use std::cell::{OnceCell, RefCell};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSMenu,
    NSMenuDelegate, NSMenuItem, NSPopUpButton, NSScreen, NSWindow, NSWindowDelegate,
    NSWindowOcclusionState,
};
use objc2_foundation::{
    ns_string, NSDate, NSDateFormatter, NSDateFormatterStyle, NSDistributedNotificationCenter,
    NSLocale, NSNotification, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer,
};

use crate::anim::{self, Animation};
use crate::config::{self, Config};
use crate::engine::{self, save};
use crate::grid::Grid;
use crate::shape::Shape;
use crate::theme::{self, THEMES};
use crate::ui;
use render::Renderer;

const PAUSED_INTERVAL: f64 = 1.0;
/// How often the lock screen frame is redrawn, to stay close to what's running.
const LOCK_SCREEN_REFRESH: std::time::Duration = std::time::Duration::from_secs(300);
/// Posted by a second launch to the running one: open settings.
const OPEN_SETTINGS: &str = "com.sectr99.glyphdrift.open-settings";

/// One screen: its window and its own copy of the animation, so centered objects center on
/// every screen.
struct Surface {
    window: Retained<NSWindow>,
    target: render::Target,
    /// Pixels per point.
    scale: f64,
    anim: Box<dyn Animation>,
    grid: Grid,
    shapes: Vec<Shape>,
    width: f64,
    height: f64,
    /// False while something covers the whole window.
    visible: bool,
}

pub(crate) struct App {
    mtm: MainThreadMarker,
    cfg: Config,
    renderer: Renderer,
    /// The highest screen scale, which glyphs are rasterized at.
    renderer_scale: f64,
    surfaces: Vec<Surface>,
    controller: Retained<Controller>,
    timer: Option<Retained<NSTimer>>,
    status: menu::Status,
    seed: u64,
    last_frame: Instant,
    last_check: Instant,
    /// Config file mtime at last load.
    config_mtime: Option<SystemTime>,
    user_paused: bool,
    /// Why the wallpaper paused itself, if it did.
    auto_paused: Option<&'static str>,
    /// Time and date last shown.
    clock: Option<[String; 2]>,
    /// The desktop picture, while Match menu bar to theme or Show on lock screen replaces it.
    picture: picture::DesktopPicture,
    /// Show on lock screen: the frames shown, when they were drawn, and whether something
    /// changed since (drawn at the next second's check, so slider drags settle first).
    lock_frames: Vec<std::path::PathBuf>,
    picture_drawn: Instant,
    picture_due: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Run `f` on the app state; None if it's missing or already borrowed (a re-entrant call).
pub(crate) fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok()?.as_mut().map(f))
}

impl App {
    fn paused(&self) -> bool {
        self.user_paused || self.auto_paused.is_some()
    }

    fn fps(&self) -> u32 {
        engine::frame_rate(
            &self.cfg,
            self.surfaces.first().map_or(24, |s| s.anim.fps()),
        )
    }

    fn interval(&self) -> f64 {
        if self.paused() {
            PAUSED_INTERVAL
        } else {
            1.0 / self.fps() as f64
        }
    }

    /// (Re)start the frame timer at the current interval. Common modes keep it firing while
    /// a menu is open or a slider is dragged.
    fn restart_timer(&mut self) {
        if let Some(t) = self.timer.take() {
            t.invalidate();
        }
        // SAFETY: `tick:` takes the timer and the target outlives it (the timer retains it).
        let timer = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                self.interval(),
                &self.controller,
                sel!(tick:),
                None,
                true,
            )
        };
        timer.setTolerance(self.interval() * 0.1);
        // SAFETY: a fresh timer on the main run loop.
        unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        self.timer = Some(timer);
    }

    fn theme(&self) -> &'static theme::Theme {
        &THEMES[theme::index_of(&self.cfg.theme.name)]
    }

    /// One window per screen, replacing any current ones.
    fn rebuild_surfaces(&mut self) {
        for s in self.surfaces.drain(..) {
            s.window.orderOut(None);
        }
        let bg = self.renderer.background();
        let screens = NSScreen::screens(self.mtm);
        let max_scale = screens
            .iter()
            .map(|s| s.backingScaleFactor())
            .fold(1.0, f64::max);
        if max_scale != self.renderer_scale {
            self.renderer_scale = max_scale;
            self.renderer.set_font(
                &self.cfg.font.family,
                engine::font_size(&self.cfg),
                max_scale,
            );
        }
        for screen in screens.iter() {
            let scale = screen.backingScaleFactor();
            let target = self.renderer.target(scale);
            let window = desktop::create(
                self.mtm,
                &screen,
                &target,
                bg,
                self.cfg.general.match_menu_bar,
            );
            self.surfaces.push(Surface {
                window,
                target,
                scale,
                anim: anim::by_name(&self.cfg.general.animation, &self.cfg.theme.ramp),
                grid: Grid::new(1, 1),
                shapes: Vec::new(),
                width: 0.0,
                height: 0.0,
                visible: true,
            });
        }
        self.layout();
        self.sync_picture(); // new screens get the picture too
    }

    /// The desktop picture: a still frame for Show on lock screen (redrawn when due), else
    /// the theme color for Match menu bar to theme, else the user's own. Runs every second,
    /// so other Spaces and new screens catch up; cheap when nothing changed.
    fn sync_picture(&mut self) {
        let bg = self.renderer.background();
        if self.cfg.general.lock_screen {
            if self.picture_due || self.picture_drawn.elapsed() >= LOCK_SCREEN_REFRESH {
                self.picture_due = false;
                self.picture_drawn = Instant::now();
                let frames = self.lock_screen_frames();
                if !frames.is_empty() {
                    self.lock_frames = frames;
                }
            }
            self.picture.show(self.mtm, &self.lock_frames, bg);
        } else if self.cfg.general.match_menu_bar {
            self.picture.solid(self.mtm, bg);
        } else if self.picture.active() {
            self.picture.restore(self.mtm);
        }
    }

    /// A still frame per screen, in screen order, written as PNGs: the current animation
    /// state without the clock (it would go stale), and a theme-colored strip behind the
    /// menu bar when the windows stop below it.
    fn lock_screen_frames(&mut self) -> Vec<std::path::PathBuf> {
        let bg = self.renderer.background();
        let screens = NSScreen::screens(self.mtm);
        let mut files = Vec::new();
        for (i, (s, screen)) in self.surfaces.iter_mut().zip(screens.iter()).enumerate() {
            let full = screen.frame().size;
            let (mut grid, mut shapes) = (Grid::new(s.grid.cols(), s.grid.rows()), Vec::new());
            s.anim.set_clock(None);
            s.anim.render(&mut grid);
            s.anim.draw(&mut shapes);
            s.anim.set_clock(self.clock.as_ref());
            let frame = match render::render_offscreen(
                &mut self.renderer,
                (s.width, s.height),
                s.scale,
                &grid,
                &shapes,
            ) {
                Ok(f) => f,
                Err(e) => {
                    log!("lock screen: {e}");
                    continue;
                }
            };
            let image = if s.height < full.height - 0.5 {
                picture::with_menu_bar_strip(&frame, (full.width, full.height), s.scale, bg)
            } else {
                Some(frame)
            };
            let (Some(image), Some(path)) = (image, picture::frame_path(i)) else {
                continue;
            };
            match render::png(&image).and_then(|png| {
                std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))
            }) {
                Ok(()) => files.push(path),
                Err(e) => log!("lock screen: {e}"),
            }
        }
        files
    }

    fn toggle_lock_screen(&mut self) {
        self.cfg.general.lock_screen = !self.cfg.general.lock_screen;
        save("general", "lock_screen", self.cfg.general.lock_screen);
        self.picture_due = true;
        self.sync_picture();
        settings::refresh(self);
    }

    fn toggle_match_menu_bar(&mut self) {
        self.cfg.general.match_menu_bar = !self.cfg.general.match_menu_bar;
        save("general", "match_menu_bar", self.cfg.general.match_menu_bar);
        self.rebuild_surfaces(); // the windows stop at the menu bar or cover it; syncs too
        settings::refresh(self);
    }

    /// Size grids to the windows and re-init the animations.
    fn layout(&mut self) {
        self.picture_due = true;
        let (cw, ch) = self.renderer.cell_size();
        let g = &self.cfg.general;
        for (i, s) in self.surfaces.iter_mut().enumerate() {
            let size = s.window.frame().size;
            s.width = size.width;
            s.height = size.height;
            let cols = (size.width / cw).max(1.0) as usize;
            let rows = (size.height / ch).max(1.0) as usize;
            s.grid.resize(cols, rows);
            let (w, h) = (size.width as f32, size.height as f32);
            s.anim.init(
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
                self.seed.wrapping_add(i as u64),
            );
        }
        self.render_frame();
    }

    /// Capture the current frame on every visible screen and show it.
    fn render_frame(&mut self) {
        self.clock = self.cfg.general.clock.then(local_clock);
        for s in self.surfaces.iter_mut().filter(|s| s.visible) {
            s.anim.render(&mut s.grid);
            s.anim.set_clock(self.clock.as_ref());
            s.shapes.clear();
            s.anim.draw(&mut s.shapes);
        }
        self.present();
    }

    /// Draw the captured frames on every visible screen.
    fn present(&mut self) {
        for s in self.surfaces.iter_mut().filter(|s| s.visible) {
            self.renderer.paint(
                &mut s.target,
                (s.width, s.height),
                s.scale,
                &s.grid,
                &s.shapes,
            );
        }
    }

    /// Push the theme's colors (warm or cool, per animation) to the renderer and windows.
    fn apply_colors(&mut self) {
        self.picture_due = true;
        let t = self.theme();
        let warm = self.surfaces.first().is_some_and(|s| s.anim.warm());
        let stops = if warm { &t.warm } else { &t.cool };
        self.renderer.set_colors(t.bg, stops, self.cfg.theme.glass);
        let bg = desktop::color(self.renderer.background());
        for s in &self.surfaces {
            s.window.setBackgroundColor(Some(&bg));
        }
        self.present();
    }

    fn set_animation(&mut self, index: usize) {
        let Some(&(name, ..)) = anim::ANIMATIONS.get(index) else {
            return;
        };
        self.cfg.general.animation = name.into();
        self.apply(engine::Changed {
            animation: true,
            frame_rate: true, // the animation's own rate may differ
            ..Default::default()
        });
        save("general", "animation", name);
        self.update_status();
        settings::refresh(self);
    }

    fn set_theme(&mut self, index: usize) {
        let Some(t) = THEMES.get(index) else {
            return;
        };
        self.cfg.theme.name = t.name.into();
        self.apply(engine::Changed {
            colors: true,
            ..Default::default()
        });
        save("theme", "name", t.name);
        self.update_status();
        settings::refresh(self);
    }

    /// Apply a setting live; with `commit`, save it too.
    fn tweak(&mut self, id: usize, value: f32, commit: bool) {
        let changed = engine::tweak(&mut self.cfg, id, value, commit);
        self.apply(changed);
    }

    /// Act on what a setting or config edit changed.
    fn apply(&mut self, c: engine::Changed) {
        if c.font {
            self.renderer.set_font(
                &self.cfg.font.family,
                engine::font_size(&self.cfg),
                self.renderer_scale,
            );
        }
        if c.animation {
            for s in &mut self.surfaces {
                s.anim = anim::by_name(&self.cfg.general.animation, &self.cfg.theme.ramp);
            }
        }
        if c.menu_bar {
            self.rebuild_surfaces(); // window height changes; lays out too
        } else if c.font || c.animation || c.layout {
            self.layout(); // new cell size, animation or object size
        } else if c.frame {
            self.render_frame();
        }
        if c.colors || c.animation {
            self.apply_colors(); // warm or cool gradient follows the animation
        }
        if c.frame_rate {
            self.restart_timer();
        }
        if c.lock_screen {
            self.picture_due = true;
        }
    }

    /// Reload the config if the file changed, applying only what differs. True if reloaded.
    fn reload_if_changed(&mut self) -> bool {
        let mtime = config::modified();
        if mtime == self.config_mtime {
            return false;
        }
        self.config_mtime = mtime;
        let (cfg, warnings) = config::load();
        for w in &warnings {
            log!("config: {w}");
        }
        let old = std::mem::replace(&mut self.cfg, cfg);
        self.apply(engine::reloaded(&old, &self.cfg));
        settings::refresh(self);
        true
    }

    /// Re-evaluate pausing; true if the timer interval must change.
    fn refresh_pause(&mut self) -> bool {
        let before = self.paused();
        let (on_cover, on_low_power) = (
            self.cfg.general.pause_on_fullscreen,
            self.cfg.general.pause_on_battery_saver,
        );
        let mut uncovered = false;
        for s in &mut self.surfaces {
            let visible = !on_cover
                || s.window
                    .occlusionState()
                    .contains(NSWindowOcclusionState::Visible);
            uncovered |= visible && !s.visible;
            s.visible = visible;
        }
        if uncovered {
            self.render_frame(); // catch up the screens that just came back
        }
        self.auto_paused = if on_cover && !self.surfaces.iter().any(|s| s.visible) {
            Some("the desktop is covered")
        } else if on_low_power && power::low_power_mode() {
            Some("Low Power Mode is on")
        } else {
            None
        };
        self.update_status();
        self.paused() != before
    }

    /// Menu bar hover text: animation and theme, or why it's paused.
    fn update_status(&self) {
        let tip = engine::status(&self.cfg, self.user_paused, self.auto_paused);
        self.status.set(&tip, self.paused());
    }

    fn toggle_pause(&mut self) {
        self.user_paused = !self.user_paused;
        self.update_status();
        self.last_frame = Instant::now();
        self.restart_timer();
    }

    /// One frame tick: config edits and auto-pause once a second, then animate.
    fn tick(&mut self) {
        if self.last_check.elapsed().as_secs() >= 1 {
            self.last_check = Instant::now();
            let changed = self.reload_if_changed() | self.refresh_pause();
            // Theme and readability changes, other Spaces, a picture the user just chose.
            self.sync_picture();
            if changed {
                self.last_frame = Instant::now();
                self.restart_timer();
            }
        }
        if self.paused() {
            // Keep the clock current while paused.
            if self.cfg.general.clock && self.clock != Some(local_clock()) {
                self.render_frame();
            }
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;
        let dt = dt * self.cfg.general.speed;
        for s in self.surfaces.iter_mut().filter(|s| s.visible) {
            s.anim.update(dt);
        }
        self.render_frame();
    }
}

define_class!(
    /// Target of the timer, menu, settings controls and app notifications.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    pub(crate) struct Controller;

    impl Controller {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            with_app(App::tick);
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            with_app(settings::open);
        }

        /// From an animation menu item; the tag is the index into `ANIMATIONS`.
        #[unsafe(method(chooseAnimation:))]
        fn choose_animation(&self, item: &NSMenuItem) {
            let i = item.tag() as usize;
            with_app(|app| app.set_animation(i));
        }

        /// From a theme menu item; the tag is the index into `THEMES`.
        #[unsafe(method(chooseTheme:))]
        fn choose_theme(&self, item: &NSMenuItem) {
            let i = item.tag() as usize;
            with_app(|app| app.set_theme(i));
        }

        /// From the settings popups: the selected item's tag.
        #[unsafe(method(popupAnimation:))]
        fn popup_animation(&self, popup: &NSPopUpButton) {
            let i = popup.selectedTag() as usize;
            with_app(|app| app.set_animation(i));
        }

        #[unsafe(method(popupTheme:))]
        fn popup_theme(&self, popup: &NSPopUpButton) {
            let i = popup.selectedTag() as usize;
            with_app(|app| app.set_theme(i));
        }

        #[unsafe(method(toggleClock:))]
        fn toggle_clock(&self, _sender: Option<&AnyObject>) {
            with_app(|app| {
                let on = app.cfg.general.clock;
                app.tweak(ui::CLOCK, if on { 0.0 } else { 1.0 }, true);
                settings::refresh(app);
            });
        }

        #[unsafe(method(toggleLockScreen:))]
        fn toggle_lock_screen(&self, _sender: Option<&AnyObject>) {
            with_app(App::toggle_lock_screen);
        }

        #[unsafe(method(installScreenSaver:))]
        fn install_screen_saver(&self, _sender: Option<&AnyObject>) {
            saver::install();
        }

        #[unsafe(method(toggleMatchMenuBar:))]
        fn toggle_match_menu_bar(&self, _sender: Option<&AnyObject>) {
            with_app(App::toggle_match_menu_bar);
        }

        #[unsafe(method(toggleLogin:))]
        fn toggle_login(&self, _sender: Option<&AnyObject>) {
            if let Err(e) = autostart::set(!autostart::enabled()) {
                log!("autostart: {e}");
            }
        }

        #[unsafe(method(togglePause:))]
        fn toggle_pause(&self, _sender: Option<&AnyObject>) {
            with_app(App::toggle_pause);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        /// A settings slider moved; its tag is the tweak id.
        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, slider: &objc2_app_kit::NSSlider) {
            settings::slider_changed(slider);
        }

        #[unsafe(method(frameRateChanged:))]
        fn frame_rate_changed(&self, popup: &NSPopUpButton) {
            let fps = ui::FRAME_RATES[popup.indexOfSelectedItem().max(0) as usize].0;
            with_app(|app| app.tweak(ui::FPS, fps, true));
        }

        #[unsafe(method(resetSettings:))]
        fn reset_settings(&self, _sender: Option<&AnyObject>) {
            with_app(|app| {
                let anim = anim::index_of(&app.cfg.general.animation);
                let (object_scale, _) = ui::applies(anim);
                // Applied one by one, then saved in one write.
                app.tweak(ui::FPS, 0.0, false);
                let mut reset = vec![ui::FPS];
                for (id, _, _, _, default) in ui::SLIDERS {
                    // Leave greyed-out sliders alone.
                    if id != ui::OBJECT_SCALE || object_scale {
                        app.tweak(id, default, false);
                        reset.push(id);
                    }
                }
                engine::save_tweaks(&app.cfg, &reset);
                settings::refresh(app);
            });
        }

        #[unsafe(method(openSettingsNotification:))]
        fn open_settings_notification(&self, _note: &NSNotification) {
            with_app(settings::open);
        }
    }

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationDidChangeScreenParameters:))]
        fn screens_changed(&self, _note: &NSNotification) {
            with_app(App::rebuild_surfaces);
        }

        /// Opening the app again (Finder, Spotlight, `open`) opens settings.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            with_app(settings::open);
            false
        }

        /// Give the user's desktop pictures back.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _note: &NSNotification) {
            with_app(|app| {
                if app.picture.active() {
                    app.picture.restore(app.mtm);
                }
            });
        }
    }

    unsafe impl NSMenuDelegate for Controller {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            with_app(|app| menu::fill(app, menu));
        }
    }

    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _note: &NSNotification) {
            settings::closed();
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

thread_local! {
    /// Time, long date and log timestamp formatters.
    static FORMATTERS: OnceCell<[Retained<NSDateFormatter>; 3]> = const { OnceCell::new() };
}

fn format_now(which: usize) -> String {
    FORMATTERS.with(|f| {
        let f = f.get_or_init(|| {
            let time = NSDateFormatter::new();
            time.setTimeStyle(NSDateFormatterStyle::ShortStyle);
            time.setDateStyle(NSDateFormatterStyle::NoStyle);
            let date = NSDateFormatter::new();
            date.setTimeStyle(NSDateFormatterStyle::NoStyle);
            date.setDateStyle(NSDateFormatterStyle::FullStyle);
            let log = NSDateFormatter::new();
            log.setLocale(Some(&NSLocale::localeWithLocaleIdentifier(ns_string!(
                "en_US_POSIX"
            ))));
            log.setDateFormat(Some(ns_string!("yyyy-MM-dd HH:mm:ss")));
            [time, date, log]
        });
        f[which].stringFromDate(&NSDate::now()).to_string()
    })
}

/// Local time (no seconds) and long date, in the user's format.
pub fn local_clock() -> [String; 2] {
    [format_now(0), format_now(1)]
}

/// Local time for log lines.
pub(crate) fn timestamp() -> String {
    format_now(2)
}

/// True for the first running copy; a second launch asks the first to open settings.
fn first_instance() -> bool {
    let Some(path) = config::default_path().map(|p| p.with_file_name(".lock")) else {
        return true;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(file) = std::fs::File::create(&path) else {
        return true;
    };
    if file.try_lock().is_ok() {
        // Held until exit: it marks the process running.
        std::mem::forget(file);
        return true;
    }
    let center = NSDistributedNotificationCenter::defaultCenter();
    // SAFETY: a plain notification with no object or user info.
    unsafe {
        center.postNotificationName_object_userInfo_deliverImmediately(
            &NSString::from_str(OPEN_SETTINGS),
            None,
            None,
            true,
        );
    }
    false
}

pub fn run() -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("must start on the main thread")?;
    if !first_instance() {
        return Ok(());
    }
    crate::log::init();
    saver::update();
    let first_run = config::create_if_missing();
    let (cfg, warnings) = config::load();
    for w in &warnings {
        log!("config: {w}");
    }

    let app = NSApplication::sharedApplication(mtm);
    // A menu bar app: no Dock icon or app menu.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let controller = Controller::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    // SAFETY: the selector takes one NSNotification; the observer lives for the process.
    unsafe {
        NSDistributedNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &controller,
            sel!(openSettingsNotification:),
            Some(&NSString::from_str(OPEN_SETTINGS)),
            None,
        );
    }

    let t = &THEMES[theme::index_of(&cfg.theme.name)];
    let renderer = Renderer::new(
        &cfg.font.family,
        engine::font_size(&cfg),
        1.0,
        t.bg,
        &t.cool,
        cfg.theme.glass,
    )?;
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let now = Instant::now();
    let status = menu::Status::new(mtm, &controller);
    let mut state = App {
        mtm,
        cfg,
        renderer,
        renderer_scale: 1.0,
        surfaces: Vec::new(),
        controller,
        timer: None,
        status,
        seed,
        last_frame: now,
        last_check: now,
        config_mtime: config::modified(),
        user_paused: false,
        auto_paused: None,
        clock: None,
        // Pictures a crashed run left recorded are restored or kept by `sync_picture`.
        picture: picture::DesktopPicture::load(),
        lock_frames: Vec::new(),
        picture_drawn: now,
        picture_due: true,
    };
    state.rebuild_surfaces();
    state.apply_colors();
    state.update_status();
    state.restart_timer();
    if first_run {
        // Show new users where the controls are.
        settings::open(&mut state);
    }
    APP.with(|a| *a.borrow_mut() = Some(state));

    app.run();
    Ok(())
}

/// Website images, drawn offscreen (`glyphdrift --screenshots <dir>`).
pub fn screenshots(dir: &std::path::Path) -> Result<(), String> {
    screenshots::run(dir)
}

/// Fatal error as an alert.
pub fn fatal(text: &str) {
    crate::log::write(&format!("fatal: {text}"));
    if let Some(mtm) = MainThreadMarker::new() {
        let alert = NSAlert::new(mtm);
        alert.setMessageText(ns_string!("Glyphdrift"));
        alert.setInformativeText(&NSString::from_str(text));
        alert.runModal();
    }
}
