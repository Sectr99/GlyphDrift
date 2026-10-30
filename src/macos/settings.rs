//! Settings window in the Bench design system (`bench`), laid out like the Windows one: two
//! columns, the picks (animation, theme) on the left and the tuning on the right, in the
//! Native or Instrument style, light or dark. Standard AppKit controls, colored from Bench's
//! tokens where AppKit allows (switches and radios keep the system accent); changes apply live
//! and save on release. It opens at the top right, by the menu bar item, so the wallpaper
//! stays visible as a preview.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSBezierPath, NSBitmapImageRep,
    NSBorderType, NSBox, NSBoxType, NSButton, NSColor, NSDeviceRGBColorSpace, NSEventType, NSFont,
    NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSImage, NSImageView,
    NSMenuItem, NSPopUpButton, NSScreen, NSScrollView, NSSlider, NSSwitch, NSTableColumn,
    NSTableRowView, NSTableView, NSTableViewStyle, NSTextAlignment, NSTextField, NSTitlePosition,
    NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{ns_string, NSIndexSet, NSPoint, NSRect, NSSize, NSString, NSUserDefaults};

use super::{autostart, with_app, App, Controller};
use crate::anim::{self, ANIMATIONS};
use crate::bench::{self, Look};
use crate::config::{Config, Rgb};
use crate::engine::tweak_value;
use crate::theme::{self, THEMES};
use crate::ui::{self, Row, FRAME_RATES, SLIDERS};

// Layout in points, in Bench's spacing steps; the same as on Windows.
const W: f64 = 800.0;
const PAD: f64 = bench::SPACE[4] as f64;
const GAP: f64 = bench::SPACE[5] as f64;
const COLUMN_W: f64 = (W - 2.0 * PAD - GAP) / 2.0;
const LABEL_W: f64 = 112.0;
const VALUE_W: f64 = 72.0;

/// Switches, as (tweak id or `LOGIN`, label, muted suffix, action).
const LOGIN: usize = usize::MAX;
const TOGGLES: [(usize, &str, &str); 6] = [
    (ui::CLOCK, "Show clock", "Topographic drift"),
    (ui::LOCK_SCREEN, "Show on lock screen", ""),
    (MENU_BAR, "Match menu bar to theme", ""),
    (
        ui::PAUSE_FULLSCREEN,
        "Pause while the desktop is covered",
        "",
    ),
    (ui::PAUSE_BATTERY, "Pause in Low Power Mode", ""),
    (LOGIN, "Open at login", ""),
];
/// Match menu bar to theme: a setting, but not a tweak.
const MENU_BAR: usize = usize::MAX - 1;

/// (tweak id, slider, value label, name label)
type SliderRow = (
    usize,
    Retained<NSSlider>,
    Retained<NSTextField>,
    Retained<NSTextField>,
);

struct Settings {
    window: Retained<NSWindow>,
    look: Look,
    animation: Retained<NSPopUpButton>,
    themes: Retained<NSTableView>,
    sliders: Vec<SliderRow>,
    fps: Retained<NSPopUpButton>,
    /// (setting, switch, label)
    switches: Vec<(usize, Retained<NSSwitch>, Retained<NSTextField>)>,
    styles: Vec<Retained<NSButton>>,
    appearances: Vec<Retained<NSButton>>,
    fonts: Fonts,
}

struct Fonts {
    body: Retained<NSFont>,
    label: Retained<NSFont>,
    /// Numbers: tabular in Native, monospaced in Instrument.
    num: Retained<NSFont>,
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

fn color(c: Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        c.0 as f64 / 255.0,
        c.1 as f64 / 255.0,
        c.2 as f64 / 255.0,
        1.0,
    )
}

/// macOS's own appearance, for Match system.
fn system_dark() -> bool {
    NSUserDefaults::standardUserDefaults()
        .stringForKey(ns_string!("AppleInterfaceStyle"))
        .is_some_and(|s| s.to_string() == "Dark")
}

fn look_of(cfg: &Config) -> Look {
    Look::new(&cfg.settings.style, &cfg.settings.appearance, system_dark())
}

#[allow(deprecated)] // `activate` needs macOS 14
fn bring_to_front(window: &NSWindow) {
    NSApplication::sharedApplication(window.mtm()).activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
}

/// Open the window, or bring it to the front.
pub fn open(app: &mut App) {
    let shown = SETTINGS.with(|s| {
        let s = s.borrow();
        s.as_ref().inspect(|s| bring_to_front(&s.window)).is_some()
    });
    if shown {
        return;
    }
    let settings = build(app.mtm, &app.controller, look_of(&app.cfg));
    bring_to_front(&settings.window);
    store(settings);
    refresh(app);
}

/// The window closed.
pub fn closed() {
    // Taken out first: dropping the controls may re-enter.
    let old = SETTINGS.with(|s| s.borrow_mut().take());
    drop(old);
}

/// The style or appearance changed: build the window again, where it was, in the new look.
pub fn rebuild(app: &mut App) {
    let Some(old) = SETTINGS.with(|s| s.borrow_mut().take()) else {
        return;
    };
    let top_right = {
        let f = old.window.frame();
        NSPoint::new(f.origin.x + f.size.width, f.origin.y + f.size.height)
    };
    old.window.close();
    drop(old);
    let settings = build(app.mtm, &app.controller, look_of(&app.cfg));
    let size = settings.window.frame().size;
    settings.window.setFrameOrigin(NSPoint::new(
        top_right.x - size.width,
        top_right.y - size.height,
    ));
    bring_to_front(&settings.window);
    store(settings);
    refresh(app);
}

/// Show the current settings, if open.
pub fn refresh(app: &App) {
    SETTINGS.with(|s| {
        if let Ok(s) = s.try_borrow() {
            if let Some(s) = s.as_ref() {
                show(s, &app.cfg);
            }
        }
    });
}

/// Set the controls to `cfg`.
fn show(s: &Settings, cfg: &Config) {
    let anim = anim::index_of(&cfg.general.animation);
    s.animation.selectItemWithTag(anim as isize);
    let row = ui::row_of(&ui::theme_rows(), theme::index_of(&cfg.theme.name));
    s.themes
        .selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
    s.themes.scrollRowToVisible(row as isize);
    let (object_scale, clock) = ui::applies(anim);
    let c = s.look.colors;
    for (id, slider, value, label) in &s.sliders {
        let v = tweak_value(cfg, *id);
        slider.setIntegerValue(ui::slider_pos(v) as isize);
        // Grey out the controls that do nothing for this animation.
        let on = *id != ui::OBJECT_SCALE || object_scale;
        slider.setEnabled(on);
        let (text, font) = if on {
            (ui::value_label(*id, v), &s.fonts.num)
        } else {
            ("Loops only".into(), &s.fonts.body)
        };
        value.setStringValue(&NSString::from_str(&text));
        value.setFont(Some(font));
        let faded = crate::theme::mix(c.ink_muted, c.window, 0.55);
        value.setTextColor(Some(&color(if on { c.ink_muted } else { faded })));
        label.setTextColor(Some(&color(if on {
            c.ink
        } else {
            crate::theme::mix(c.ink, c.window, 0.55)
        })));
    }
    s.fps
        .selectItemAtIndex(ui::frame_rate_index(tweak_value(cfg, ui::FPS)) as isize);
    for (setting, switch, label) in &s.switches {
        let on = match *setting {
            LOGIN => autostart::enabled(),
            MENU_BAR => cfg.general.match_menu_bar,
            id => tweak_value(cfg, id) != 0.0,
        };
        switch.setState(isize::from(on));
        let enabled = *setting != ui::CLOCK || clock;
        switch.setEnabled(enabled);
        let ink = if enabled {
            c.ink
        } else {
            crate::theme::mix(c.ink, c.window, 0.55)
        };
        label.setTextColor(Some(&color(ink)));
    }
    let style = tweak_value(cfg, ui::STYLE) as usize;
    for (i, b) in s.styles.iter().enumerate() {
        b.setState(isize::from(i == style));
    }
    let appearance = tweak_value(cfg, ui::APPEARANCE) as usize;
    for (i, b) in s.appearances.iter().enumerate() {
        b.setState(isize::from(i == appearance));
    }
}

/// The window showing `cfg`, drawn to a bitmap without ever going on screen (screenshots).
pub fn render_offscreen(
    mtm: MainThreadMarker,
    target: &Controller,
    cfg: &Config,
) -> Option<Retained<NSBitmapImageRep>> {
    // Dark, like the wallpaper behind it, whatever this Mac uses.
    let look = Look::new(&cfg.settings.style, "dark", true);
    // Stored while drawing: the theme list's rows read it.
    store(build(mtm, target, look));
    let rep = SETTINGS.with(|s| {
        let s = s.try_borrow().ok()?;
        let s = s.as_ref()?;
        show(s, cfg);
        // The content view's superview is the frame view: title bar and controls.
        // SAFETY: read on the main thread while the window is alive.
        let view = unsafe { s.window.contentView()?.superview() }?;
        view.layoutSubtreeIfNeeded();
        let bounds = view.bounds();
        let rep = view.bitmapImageRepForCachingDisplayInRect(bounds)?;
        // The first pass draws disabled sliders with their knob at the far left; the second
        // draws them right.
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        let rep = view.bitmapImageRepForCachingDisplayInRect(bounds)?;
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        Some(rep)
    });
    // Then taken back out, so nothing else sees it as the open window.
    drop(SETTINGS.with(|s| s.borrow_mut().take()));
    rep
}

/// A slider moved: apply live, save once released.
pub fn slider_changed(slider: &NSSlider) {
    let id = slider.tag() as usize;
    let value = ui::slider_value(slider.integerValue() as i32);
    let dragging = NSApplication::sharedApplication(slider.mtm())
        .currentEvent()
        .is_some_and(|e| {
            matches!(
                e.r#type(),
                NSEventType::LeftMouseDragged | NSEventType::LeftMouseDown
            )
        });
    with_app(|app| app.tweak(id, value, !dragging));
    SETTINGS.with(|s| {
        if let Some(s) = s.borrow().as_ref() {
            if let Some((_, _, label, _)) = s.sliders.iter().find(|x| x.0 == id) {
                label.setStringValue(&NSString::from_str(&ui::value_label(id, value)));
            }
        }
    });
}

/// A switch flipped; its tag is the setting.
pub fn switch_changed(app: &mut App, switch: &NSSwitch) {
    let on = switch.state() != 0;
    match switch.tag() as usize {
        LOGIN => {
            if let Err(e) = autostart::set(on) {
                log!("autostart: {e}");
            }
        }
        MENU_BAR => app.toggle_match_menu_bar(),
        ui::LOCK_SCREEN => app.toggle_lock_screen(),
        id => app.tweak(id, f32::from(u8::from(on)), true),
    }
    refresh(app);
}

/// A Style or Appearance radio; the tag is the setting * 100 + the choice.
pub fn choice_changed(app: &mut App, button: &NSButton) {
    let tag = button.tag() as usize;
    app.tweak(tag / 100, (tag % 100) as f32, true);
}

/// Rows of the theme list.
pub fn theme_row_count() -> isize {
    ui::theme_rows().len() as isize
}

/// A theme list row picked.
pub fn theme_selected(app: &mut App, row: isize) {
    if let Some(&Row::Item(i)) = ui::theme_rows().get(row as usize) {
        if theme::index_of(&app.cfg.theme.name) != i {
            app.set_theme(i);
        }
    }
}

pub fn theme_row_selectable(row: isize) -> bool {
    matches!(ui::theme_rows().get(row as usize), Some(Row::Item(_)))
}

pub fn theme_row_height(row: isize) -> f64 {
    let Some(row_h) = SETTINGS.with(|s| s.try_borrow().ok()?.as_ref().map(|s| s.look.size.row))
    else {
        return 30.0;
    };
    let row_h = row_h as f64;
    if theme_row_selectable(row) {
        row_h
    } else {
        (row_h * 0.85).round()
    }
}

/// The view for theme list row `row`: a group label, or a name and its swatches.
pub fn theme_row_view(mtm: MainThreadMarker, row: isize) -> Option<Retained<NSView>> {
    SETTINGS.with(|s| {
        let s = s.try_borrow().ok()?;
        let s = s.as_ref()?;
        let c = s.look.colors;
        let h = theme_row_height(row);
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::ZERO, NSSize::new(COLUMN_W - 2.0, h)),
        );
        match ui::theme_rows().get(row as usize)? {
            Row::Heading(name) => {
                let l = section_label(mtm, name, &s.fonts, c);
                l.setFrame(NSRect::new(
                    NSPoint::new(12.0, 2.0),
                    NSSize::new(200.0, 14.0),
                ));
                view.addSubview(&l);
            }
            Row::Item(i) => {
                let t = &THEMES[*i];
                let l = text(mtm, t.label, &s.fonts.body, c.ink);
                let lh = l.fittingSize().height;
                l.setFrame(NSRect::new(
                    NSPoint::new(15.0, (h - lh) / 2.0),
                    NSSize::new(200.0, lh),
                ));
                view.addSubview(&l);
                if let Some(image) = swatch(t.bg, &t.cool) {
                    let size = image.size();
                    let iv = NSImageView::imageViewWithImage(&image, mtm);
                    iv.setFrame(NSRect::new(
                        NSPoint::new(COLUMN_W - 2.0 - 12.0 - size.width, (h - size.height) / 2.0),
                        size,
                    ));
                    view.addSubview(&iv);
                }
            }
        }
        Some(view)
    })
}

define_class!(
    /// Theme list row: Bench's selected row, surface-selected with a 3px accent edge, in
    /// place of the system's highlight.
    #[unsafe(super(NSTableRowView, NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    pub(crate) struct BenchRowView;

    impl BenchRowView {
        #[unsafe(method(drawSelectionInRect:))]
        fn draw_selection(&self, _dirty: NSRect) {
            let Some(c) = SETTINGS.with(|s| s.try_borrow().ok()?.as_ref().map(|s| s.look.colors))
            else {
                return;
            };
            let b = self.bounds();
            color(c.selected).setFill();
            NSBezierPath::fillRect(b);
            color(c.accent).setFill();
            NSBezierPath::fillRect(NSRect::new(b.origin, NSSize::new(3.0, b.size.height)));
        }
    }
);

/// A row view for the theme list.
pub fn theme_row_container(mtm: MainThreadMarker) -> Retained<NSTableRowView> {
    // SAFETY: NSView's designated initializer.
    let view: Retained<BenchRowView> =
        unsafe { msg_send![BenchRowView::alloc(mtm), initWithFrame: NSRect::ZERO] };
    Retained::into_super(view)
}

/// A theme's background and three stops, as a small swatch.
fn swatch(bg: Rgb, stops: &[Rgb; 3]) -> Option<Retained<NSImage>> {
    let (w, h) = (48usize, 24usize); // px, drawn at 2x
                                     // SAFETY: null planes make the rep allocate its own w x h RGBA buffer, which is then
                                     // filled within its bounds.
    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            w as isize,
            h as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (w * 4) as isize,
            32,
        )?;
        let data = std::slice::from_raw_parts_mut(rep.bitmapData(), w * h * 4);
        let block = (w - 8) / 3;
        for y in 0..h {
            for x in 0..w {
                // Three blocks inset by 2 px of background.
                let inner = (2..h - 2).contains(&y) && (2..w - 2).contains(&x);
                let c = if inner {
                    stops[((x - 2) / (block + 1)).min(2)]
                } else {
                    bg
                };
                data[(y * w + x) * 4..][..4].copy_from_slice(&[c.0, c.1, c.2, 255]);
            }
        }
        let size = NSSize::new(w as f64 / 2.0, h as f64 / 2.0);
        rep.setSize(size);
        let image = NSImage::initWithSize(NSImage::alloc(), size);
        image.addRepresentation(&rep);
        Some(image)
    }
}

/// A popup of grouped rows: headings are disabled, items are tagged with their index.
fn grouped_popup(
    mtm: MainThreadMarker,
    rows: &[Row],
    label: impl Fn(usize) -> &'static str,
    action: Sel,
    target: &Controller,
) -> Retained<NSPopUpButton> {
    let popup =
        NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, false);
    popup.setAutoenablesItems(false);
    let Some(menu) = popup.menu() else {
        return popup;
    };
    for &row in rows {
        // SAFETY: the action takes one sender argument and the target outlives the popup.
        let item = unsafe {
            let (title, act) = match row {
                Row::Heading(h) => (h, None),
                Row::Item(i) => (label(i), Some(action)),
            };
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                act,
                ns_string!(""),
            );
            item.setTarget(Some(target));
            item
        };
        match row {
            Row::Heading(_) => {
                item.setEnabled(false);
                item.setTag(-1);
            }
            Row::Item(i) => item.setTag(i as isize),
        }
        menu.addItem(&item);
    }
    popup
}

fn text(mtm: MainThreadMarker, s: &str, font: &NSFont, ink: Rgb) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(s), mtm);
    l.setFont(Some(font));
    l.setTextColor(Some(&color(ink)));
    l
}

/// A Bench label: uppercase, semibold, small, muted.
fn section_label(
    mtm: MainThreadMarker,
    s: &str,
    fonts: &Fonts,
    c: bench::Palette,
) -> Retained<NSTextField> {
    text(mtm, &bench::label(s), &fonts.label, c.ink_muted)
}

/// A filled panel: the content area, the footer band, a rule.
fn panel(mtm: MainThreadMarker, frame: NSRect, fill: Rgb) -> Retained<NSBox> {
    let b = NSBox::initWithFrame(NSBox::alloc(mtm), frame);
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(NSTitlePosition::NoTitle);
    b.setBorderWidth(0.0);
    b.setFillColor(&color(fill));
    b
}

fn fonts(look: Look) -> Fonts {
    let m = look.size;
    let (body, label) = (m.body as f64, m.label as f64);
    // SAFETY: AppKit's weight constants.
    let (regular, medium, semibold) = unsafe {
        (
            NSFontWeightRegular,
            NSFontWeightMedium,
            NSFontWeightSemibold,
        )
    };
    let plex = |name: &str, size| NSFont::fontWithName_size(&NSString::from_str(name), size);
    match look.style {
        bench::Style::Native => Fonts {
            body: NSFont::systemFontOfSize_weight(body, regular),
            label: NSFont::systemFontOfSize_weight(label, semibold),
            num: NSFont::monospacedDigitSystemFontOfSize_weight(body, regular),
        },
        // IBM Plex where installed; macOS doesn't ship it.
        bench::Style::Instrument => Fonts {
            body: plex("IBMPlexSans", body)
                .unwrap_or_else(|| NSFont::systemFontOfSize_weight(body, regular)),
            label: plex("IBMPlexSans-SemiBold", label)
                .unwrap_or_else(|| NSFont::systemFontOfSize_weight(label, semibold)),
            num: plex("IBMPlexMono-Medium", body)
                .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(body, medium)),
        },
    }
}

fn build(mtm: MainThreadMarker, target: &Controller, look: Look) -> Settings {
    let c = look.colors;
    let m = look.size;
    let (ctl, row_h) = (m.ctl as f64, m.row as f64);
    let fonts = fonts(look);
    let space = |i: usize| bench::SPACE[i] as f64;
    let label_h = m.label as f64 + 4.0;

    // Heights first: the right column sets the window's.
    let right_h = (label_h + space(2)) * 4.0 + ctl * 2.0 + space(3) + (ctl + space(3)) * 3.0
        - space(3)
        + row_h * TOGGLES.len() as f64
        + ctl * 2.0
        + space(3)
        + GAP * 3.0;
    let footer_h = ctl + 2.0 * space(3);
    let h = PAD + right_h + PAD + footer_h;

    // SAFETY: standard initializer; kept alive by `Settings`, so AppKit must not release
    // it on close.
    let window = unsafe {
        let w = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::ZERO, NSSize::new(W, h)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable,
            NSBackingStoreType::Buffered,
            false,
        );
        w.setReleasedWhenClosed(false);
        w
    };
    window.setTitle(ns_string!("Glyphdrift Settings"));
    window.setDelegate(Some(ProtocolObject::from_ref(target)));
    // The title bar on surface-chrome; controls follow light or dark.
    window.setTitlebarAppearsTransparent(true);
    window.setBackgroundColor(Some(&color(c.chrome)));
    // SAFETY: AppKit's constant names.
    let name = unsafe {
        if look.dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    window.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
    let content = window.contentView().expect("a window has a content view");
    content.addSubview(&panel(
        mtm,
        NSRect::new(NSPoint::new(0.0, footer_h), NSSize::new(W, h - footer_h)),
        c.window,
    ));
    content.addSubview(&panel(
        mtm,
        NSRect::new(NSPoint::new(0.0, footer_h), NSSize::new(W, 1.0)),
        c.border,
    ));

    // From the top; AppKit's y axis points up.
    let put = |view: &NSView, x: f64, top: f64, w: f64, height: f64| {
        view.setFrame(NSRect::new(
            NSPoint::new(x, h - top - height),
            NSSize::new(w, height),
        ));
        content.addSubview(view);
    };
    // A control `ctl` tall, with a label `ctl` tall beside it, both centered on the row.
    let put_centered = |view: &NSView, x: f64, top: f64, w: f64| {
        let fit = view.fittingSize().height.max(17.0);
        put(view, x, top + (ctl - fit) / 2.0, w, fit);
    };
    let section = |x: f64, top: &mut f64, name: &str| {
        put(
            &section_label(mtm, name, &fonts, c),
            x,
            *top,
            COLUMN_W,
            label_h,
        );
        *top += label_h + space(2);
    };
    let tip = |view: &NSView, id: usize| view.setToolTip(Some(&NSString::from_str(ui::hint(id))));

    // Left column: animation, theme.
    let lx = PAD;
    let mut ly = PAD;
    section(lx, &mut ly, "Animation");
    let animation = grouped_popup(
        mtm,
        &ui::anim_rows(),
        |i| ANIMATIONS[i].1,
        sel!(popupAnimation:),
        target,
    );
    put_centered(&animation, lx, ly, COLUMN_W);
    ly += ctl + GAP;
    section(lx, &mut ly, "Theme");
    let list_top = ly;

    // Right column: motion, size, behaviour, appearance.
    let rx = PAD + COLUMN_W + GAP;
    let mut ry = PAD;
    let mut sliders = Vec::new();
    let mut slider = |top: f64, (id, name, min, max, _): (usize, &str, i32, i32, f32)| {
        let l = text(mtm, name, &fonts.body, c.ink);
        put_centered(&l, rx, top, LABEL_W);
        // SAFETY: the action takes the slider and the target outlives it.
        let s = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(
                min as f64,
                min as f64,
                max as f64,
                Some(target),
                Some(sel!(sliderChanged:)),
                mtm,
            )
        };
        s.setTag(id as isize);
        s.setContinuous(true);
        s.setTrackFillColor(Some(&color(c.accent)));
        put_centered(&s, rx + LABEL_W, top, COLUMN_W - LABEL_W - VALUE_W);
        let value = text(mtm, "", &fonts.num, c.ink_muted);
        value.setAlignment(NSTextAlignment::Right);
        put_centered(&value, rx + COLUMN_W - VALUE_W, top, VALUE_W);
        for v in [&*l as &NSView, &s, &value] {
            tip(v, id);
        }
        sliders.push((id, s, value, l));
    };
    section(rx, &mut ry, "Motion");
    slider(ry, SLIDERS[0]);
    ry += ctl + space(3);
    let fps_label = text(mtm, "Frame rate", &fonts.body, c.ink);
    put_centered(&fps_label, rx, ry, LABEL_W);
    let fps =
        NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, false);
    for (_, name) in FRAME_RATES {
        fps.addItemWithTitle(&NSString::from_str(name));
    }
    // SAFETY: the action takes the popup and the target outlives it.
    unsafe {
        fps.setTarget(Some(target));
        fps.setAction(Some(sel!(frameRateChanged:)));
    }
    put_centered(&fps, rx + LABEL_W, ry, COLUMN_W - LABEL_W);
    tip(&fps_label, ui::FPS);
    tip(&fps, ui::FPS);
    ry += ctl + GAP;
    section(rx, &mut ry, "Size and readability");
    for s in [SLIDERS[2], SLIDERS[3], SLIDERS[1]] {
        slider(ry, s);
        ry += ctl + space(3);
    }
    ry += GAP - space(3);
    section(rx, &mut ry, "Behaviour");
    let mut switches = Vec::new();
    for (n, (setting, name, suffix)) in TOGGLES.into_iter().enumerate() {
        let title = if suffix.is_empty() {
            name.to_string()
        } else {
            format!("{name} ({suffix})")
        };
        let l = text(mtm, &title, &fonts.body, c.ink);
        put_centered_row(&content, &l, rx, ry, COLUMN_W - 60.0, row_h, h);
        let sw = NSSwitch::initWithFrame(NSSwitch::alloc(mtm), NSRect::ZERO);
        sw.setTag(setting as isize);
        // SAFETY: the action takes the switch and the target outlives it.
        unsafe {
            sw.setTarget(Some(target));
            sw.setAction(Some(sel!(switchChanged:)));
        }
        sw.setAccessibilityLabel(Some(&NSString::from_str(&title)));
        let fit = sw.fittingSize();
        put_centered_row(
            &content,
            &sw,
            rx + COLUMN_W - fit.width,
            ry,
            fit.width,
            row_h,
            h,
        );
        let help = match setting {
            LOGIN => "Starts Glyphdrift when you log in.",
            MENU_BAR => {
                "Removes the pale strip behind the menu bar: the wallpaper stops below it, and \
                 the desktop picture is set to the theme's background color. Your own picture \
                 comes back when you turn this off or quit."
            }
            ui::LOCK_SCREEN => {
                "Sets the desktop picture to a still frame of the animation, which the lock \
                 screen shows, redrawn every few minutes. Your own picture comes back when you \
                 turn this off or quit."
            }
            id => ui::hint(id),
        };
        for v in [&*l as &NSView, &sw] {
            v.setToolTip(Some(&NSString::from_str(help)));
        }
        if n + 1 < TOGGLES.len() {
            content.addSubview(&panel(
                mtm,
                NSRect::new(NSPoint::new(rx, h - ry - row_h), NSSize::new(COLUMN_W, 1.0)),
                c.border_soft,
            ));
        }
        switches.push((setting, sw, l));
        ry += row_h;
    }
    ry += GAP;
    section(rx, &mut ry, "Appearance");
    let radios = |top: f64, setting: usize, name: &str, choices: &[&str], action: Sel| {
        let l = text(mtm, name, &fonts.body, c.ink);
        put_centered(&l, rx, top, LABEL_W);
        tip(&l, setting);
        let mut x = rx + LABEL_W;
        let mut buttons = Vec::new();
        for (i, choice) in choices.iter().enumerate() {
            // SAFETY: the action takes the button and the target outlives it. Radios that
            // share an action and a superview act as one group.
            let b = unsafe {
                NSButton::radioButtonWithTitle_target_action(
                    &NSString::from_str(choice),
                    Some(target),
                    Some(action),
                    mtm,
                )
            };
            b.setTag((setting * 100 + i) as isize);
            b.setFont(Some(&fonts.body));
            tip(&b, setting);
            let w = b.fittingSize().width;
            put_centered(&b, x, top, w);
            x += w + space(3);
            buttons.push(b);
        }
        buttons
    };
    let styles = radios(ry, ui::STYLE, "Style", &ui::STYLES, sel!(chooseStyle:));
    ry += ctl + space(3);
    let appearances = radios(
        ry,
        ui::APPEARANCE,
        "Appearance",
        &ui::APPEARANCES,
        sel!(chooseAppearance:),
    );
    ry += ctl;
    debug_assert!(
        (ry - PAD - right_h).abs() < 0.5,
        "window height doesn't fit the rows"
    );

    // The theme list fills the left column down to the right column's last row.
    let list_h = ry - list_top;
    let frame = panel(
        mtm,
        NSRect::new(NSPoint::new(lx, h - ry), NSSize::new(COLUMN_W, list_h)),
        c.field,
    );
    frame.setBorderWidth(1.0);
    frame.setBorderColor(&color(c.border));
    frame.setCornerRadius(m.radius as f64);
    content.addSubview(&frame);
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(lx + 1.0, h - ry + 1.0),
            NSSize::new(COLUMN_W - 2.0, list_h - 2.0),
        ),
    );
    scroll.setHasVerticalScroller(true);
    scroll.setBorderType(NSBorderType::NoBorder);
    scroll.setDrawsBackground(false);
    let themes = NSTableView::initWithFrame(NSTableView::alloc(mtm), NSRect::ZERO);
    let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("theme"));
    column.setWidth(COLUMN_W - 2.0);
    themes.addTableColumn(&column);
    themes.setHeaderView(None);
    themes.setStyle(NSTableViewStyle::Plain);
    themes.setBackgroundColor(&color(c.field));
    themes.setIntercellSpacing(NSSize::new(0.0, 0.0));
    themes.setAccessibilityLabel(Some(ns_string!("Theme")));
    // SAFETY: the controller outlives the table.
    unsafe {
        themes.setDataSource(Some(ProtocolObject::from_ref(target)));
        themes.setDelegate(Some(ProtocolObject::from_ref(target)));
    }
    scroll.setDocumentView(Some(&themes));
    content.addSubview(&scroll);

    // Footer on chrome: the version, Reset to defaults.
    let fy = space(3);
    let version = text(
        mtm,
        &format!("Glyphdrift {}", env!("CARGO_PKG_VERSION")),
        &fonts.body,
        c.ink_muted,
    );
    let fit = version.fittingSize().height;
    version.setFrame(NSRect::new(
        NSPoint::new(PAD, fy + (ctl - fit) / 2.0),
        NSSize::new(200.0, fit),
    ));
    content.addSubview(&version);
    // SAFETY: the action takes one sender argument and the target outlives the button.
    let reset = unsafe {
        NSButton::buttonWithTitle_target_action(
            ns_string!("Reset to Defaults"),
            Some(target),
            Some(sel!(resetSettings:)),
            mtm,
        )
    };
    reset.setToolTip(Some(ns_string!(
        "Speed, icon readability, scales and frame rate back to defaults."
    )));
    let rw = reset.fittingSize().width;
    reset.setFrame(NSRect::new(
        NSPoint::new(W - PAD - rw, fy + (ctl - reset.fittingSize().height) / 2.0),
        NSSize::new(rw, reset.fittingSize().height),
    ));
    content.addSubview(&reset);
    // Esc closes, as before; there's no Close button now that changes apply live.
    // SAFETY: the window outlives the button.
    let close = unsafe {
        NSButton::buttonWithTitle_target_action(
            ns_string!(""),
            Some(&window),
            Some(sel!(performClose:)),
            mtm,
        )
    };
    close.setKeyEquivalent(ns_string!("\u{1b}"));
    close.setFrame(NSRect::new(
        NSPoint::new(-100.0, -100.0),
        NSSize::new(1.0, 1.0),
    ));
    content.addSubview(&close);

    // Top right of the main screen, by the menu bar item.
    if let Some(screen) = NSScreen::mainScreen(mtm) {
        let area = screen.visibleFrame();
        let frame = window.frame();
        window.setFrameOrigin(NSPoint::new(
            area.origin.x + area.size.width - frame.size.width - 12.0,
            area.origin.y + area.size.height - frame.size.height - 12.0,
        ));
    }
    // The theme list's rows ask `SETTINGS` for the look: `store` reloads them.
    Settings {
        window,
        look,
        animation,
        themes,
        sliders,
        fps,
        switches,
        styles,
        appearances,
        fonts,
    }
}

/// Make `settings` the open window's, and fill its theme list now that rows can see it.
fn store(settings: Settings) {
    let themes = settings.themes.clone();
    SETTINGS.with(|s| *s.borrow_mut() = Some(settings));
    themes.reloadData();
}

/// Put `view` centered vertically in a row `row_h` tall whose top is `top`.
fn put_centered_row(content: &NSView, view: &NSView, x: f64, top: f64, w: f64, row_h: f64, h: f64) {
    let fit = view.fittingSize().height.max(17.0);
    view.setFrame(NSRect::new(
        NSPoint::new(x, h - top - row_h + (row_h - fit) / 2.0),
        NSSize::new(w, fit),
    ));
    content.addSubview(view);
}
