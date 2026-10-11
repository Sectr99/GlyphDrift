//! Settings window: animation, theme with swatches, live sliders, frame rate and the clock
//! toggle. Standard AppKit controls; changes apply live and save on release. It opens at
//! the top right, by the menu bar item, so the wallpaper stays visible as a preview.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBitmapImageRep, NSButton, NSColor, NSControl,
    NSDeviceRGBColorSpace, NSEventType, NSImage, NSMenuItem, NSPopUpButton, NSScreen, NSSlider,
    NSTextAlignment, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{ns_string, NSPoint, NSRect, NSSize, NSString};

use super::{with_app, App, Controller};
use crate::anim::{self, ANIMATIONS};
use crate::config::Rgb;
use crate::theme::{self, THEMES};
use crate::ui::{self, Row, FRAME_RATES, SLIDERS};

const W: f64 = 480.0;
const M: f64 = 20.0;
const ROW_H: f64 = 34.0;
const LABEL_W: f64 = 130.0;
const VALUE_W: f64 = 52.0;
/// Rows: animation, theme, the sliders, frame rate, clock; then the buttons.
const ROWS: usize = 5 + SLIDERS.len();

/// (tweak id, slider, value label, name label)
type SliderRow = (
    usize,
    Retained<NSSlider>,
    Retained<NSTextField>,
    Retained<NSTextField>,
);

struct Settings {
    window: Retained<NSWindow>,
    animation: Retained<NSPopUpButton>,
    theme: Retained<NSPopUpButton>,
    sliders: Vec<SliderRow>,
    fps: Retained<NSPopUpButton>,
    clock: Retained<NSButton>,
    menu_bar: Retained<NSButton>,
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
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
    let settings = build(app.mtm, &app.controller);
    bring_to_front(&settings.window);
    SETTINGS.with(|s| *s.borrow_mut() = Some(settings));
    refresh(app);
}

/// The window closed.
pub fn closed() {
    // Taken out first: dropping the controls may re-enter.
    let old = SETTINGS.with(|s| s.borrow_mut().take());
    drop(old);
}

/// Show the current settings, if open.
pub fn refresh(app: &App) {
    SETTINGS.with(|s| {
        let Ok(s) = s.try_borrow() else {
            return;
        };
        let Some(s) = s.as_ref() else {
            return;
        };
        let anim = anim::index_of(&app.cfg.general.animation);
        s.animation.selectItemWithTag(anim as isize);
        s.theme
            .selectItemWithTag(theme::index_of(&app.cfg.theme.name) as isize);
        for (id, slider, value, _) in &s.sliders {
            let v = app.tweak_value(*id);
            slider.setIntegerValue(ui::slider_pos(v) as isize);
            value.setStringValue(&NSString::from_str(&ui::value_label(*id, v)));
        }
        s.fps
            .selectItemAtIndex(ui::frame_rate_index(app.tweak_value(ui::FPS)) as isize);
        s.clock.setState(isize::from(app.cfg.general.clock));
        // Grey out the controls that do nothing for this animation.
        let (object_scale, clock) = ui::applies(anim);
        for (id, slider, value, label) in &s.sliders {
            if *id == ui::OBJECT_SCALE {
                slider.setEnabled(object_scale);
                let color = if object_scale {
                    NSColor::labelColor()
                } else {
                    NSColor::disabledControlTextColor()
                };
                value.setTextColor(Some(&color));
                label.setTextColor(Some(&color));
            }
        }
        s.clock.setEnabled(clock);
        s.menu_bar
            .setState(isize::from(app.cfg.general.match_menu_bar));
    });
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
    image: impl Fn(usize) -> Option<Retained<NSImage>>,
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
            Row::Item(i) => {
                item.setTag(i as isize);
                item.setImage(image(i).as_deref());
            }
        }
        menu.addItem(&item);
    }
    popup
}

fn label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

fn build(mtm: MainThreadMarker, target: &Controller) -> Settings {
    let h = M * 2.0 + ROWS as f64 * ROW_H + 36.0;
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
    let content = window.contentView().expect("a window has a content view");

    // Rows from the top; AppKit's y axis points up.
    let row_y = |row: usize, height: f64| h - M - row as f64 * ROW_H - (ROW_H + height) / 2.0;
    let control_x = M + LABEL_W + 10.0;
    let control_w = W - control_x - M - VALUE_W - 8.0;
    let place = |view: &NSView, row: usize, x: f64, width: f64| {
        let height = view.fittingSize().height.max(17.0);
        view.setFrame(NSRect::new(
            NSPoint::new(x, row_y(row, height)),
            NSSize::new(width, height),
        ));
        content.addSubview(view);
    };
    let add_row = |row: usize,
                   name: &str,
                   control: &NSControl,
                   hint: Option<&str>|
     -> Retained<NSTextField> {
        let l = label(mtm, name);
        l.setAlignment(NSTextAlignment::Right);
        place(&l, row, M, LABEL_W);
        place(control, row, control_x, control_w);
        if let Some(hint) = hint {
            let hint = NSString::from_str(hint);
            l.setToolTip(Some(&hint));
            control.setToolTip(Some(&hint));
        }
        l
    };

    let animation = grouped_popup(
        mtm,
        &ui::anim_rows(),
        |i| ANIMATIONS[i].1,
        |_| None,
        sel!(popupAnimation:),
        target,
    );
    add_row(0, "Animation", &animation, None);
    let theme = grouped_popup(
        mtm,
        &ui::theme_rows(),
        |i| THEMES[i].label,
        |i| swatch(THEMES[i].bg, &THEMES[i].cool),
        sel!(popupTheme:),
        target,
    );
    add_row(1, "Theme", &theme, None);

    let mut sliders = Vec::new();
    for (n, (id, name, min, max, _)) in SLIDERS.into_iter().enumerate() {
        let row = 2 + n;
        // SAFETY: the action takes the slider and the target outlives it.
        let slider = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(
                min as f64,
                min as f64,
                max as f64,
                Some(target),
                Some(sel!(sliderChanged:)),
                mtm,
            )
        };
        slider.setTag(id as isize);
        slider.setContinuous(true);
        let l = add_row(row, name, &slider, Some(ui::hint(id)));
        let value = label(mtm, "");
        place(&value, row, W - M - VALUE_W, VALUE_W);
        sliders.push((id, slider, value, l));
    }

    let row = 2 + SLIDERS.len();
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
    add_row(row, "Frame rate", &fps, Some(ui::hint(ui::FPS)));

    // SAFETY: the action takes one sender argument and the target outlives the button.
    let clock = unsafe {
        NSButton::checkboxWithTitle_target_action(
            ns_string!("Show clock"),
            Some(target),
            Some(sel!(toggleClock:)),
            mtm,
        )
    };
    clock.setToolTip(Some(&NSString::from_str(ui::hint(ui::CLOCK))));
    place(&clock, row + 1, control_x, control_w);

    // SAFETY: as above.
    let menu_bar = unsafe {
        NSButton::checkboxWithTitle_target_action(
            ns_string!("Match menu bar to theme"),
            Some(target),
            Some(sel!(toggleMatchMenuBar:)),
            mtm,
        )
    };
    menu_bar.setToolTip(Some(ns_string!(
        "Removes the pale strip behind the menu bar: the wallpaper stops below it, and the \
         desktop picture is set to the theme's background color. Your own picture comes back \
         when you turn this off or quit."
    )));
    place(&menu_bar, row + 2, control_x, control_w);

    // SAFETY: as above.
    let (reset, close) = unsafe {
        (
            NSButton::buttonWithTitle_target_action(
                ns_string!("Reset"),
                Some(target),
                Some(sel!(resetSettings:)),
                mtm,
            ),
            NSButton::buttonWithTitle_target_action(
                ns_string!("Close"),
                Some(&window),
                Some(sel!(performClose:)),
                mtm,
            ),
        )
    };
    reset.setToolTip(Some(ns_string!(
        "Speed, icon readability, scales and frame rate back to defaults."
    )));
    close.setKeyEquivalent(ns_string!("\u{1b}")); // Esc closes
    let bottom = M;
    reset.setFrame(NSRect::new(
        NSPoint::new(M, bottom),
        NSSize::new(90.0, 28.0),
    ));
    close.setFrame(NSRect::new(
        NSPoint::new(W - M - 90.0, bottom),
        NSSize::new(90.0, 28.0),
    ));
    content.addSubview(&reset);
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
    Settings {
        window,
        animation,
        theme,
        sliders,
        fps,
        clock,
        menu_bar,
    }
}
