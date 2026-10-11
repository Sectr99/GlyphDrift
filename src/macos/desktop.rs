//! Wallpaper windows: one borderless window per screen at the desktop window level, which is
//! above the desktop picture and below Finder's desktop icons. They ignore the mouse, show
//! on every Space and stay put in Mission Control. Each hosts a Metal layer.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSScreen, NSView, NSWindow, NSWindowAnimationBehavior,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::NSRect;

use super::render::Target;
use crate::config::Rgb;

pub fn color(c: Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        c.0 as f64 / 255.0,
        c.1 as f64 / 255.0,
        c.2 as f64 / 255.0,
        1.0,
    )
}

/// A wallpaper window covering `screen`, showing `target`'s layer behind the desktop icons.
/// With `below_menu_bar`, it stops at the menu bar: macOS backs the menu bar with a pale
/// frosted strip wherever a window lies under it, but not over the desktop picture.
pub fn create(
    mtm: MainThreadMarker,
    screen: &NSScreen,
    target: &Target,
    bg: Rgb,
    below_menu_bar: bool,
) -> Retained<NSWindow> {
    let mut frame = screen.frame();
    if below_menu_bar {
        // The menu bar's height; 0 when it hides automatically.
        let top = screen.visibleFrame();
        frame.size.height = (top.origin.y + top.size.height - frame.origin.y).max(1.0);
    }
    // SAFETY: standard initializer; the window is kept alive by its `Retained`, so AppKit
    // must not release it on close.
    let window = unsafe {
        let w = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        );
        w.setReleasedWhenClosed(false);
        w
    };
    window.setLevel(CGWindowLevelForKey(CGWindowLevelKey::DesktopWindowLevelKey) as isize);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window.setIgnoresMouseEvents(true);
    window.setOpaque(true);
    window.setHasShadow(false);
    window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    window.setBackgroundColor(Some(&color(bg)));
    // A layer-hosting view: the layer is set before turning layers on.
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(Default::default(), frame.size),
    );
    view.setLayer(Some(target.layer()));
    view.setWantsLayer(true);
    window.setContentView(Some(&view));
    window.setFrame_display(frame, false);
    window.orderFrontRegardless();
    window
}
