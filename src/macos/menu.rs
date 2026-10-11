//! Menu bar item: the menu has Settings and quick actions. The icon dims while paused, and
//! its hover text says why.

use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBitmapImageRep, NSControlStateValueOff, NSControlStateValueOn, NSDeviceRGBColorSpace,
    NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{ns_string, NSSize, NSString};

use super::{autostart, App, Controller};
use crate::anim::ANIMATIONS;
use crate::theme::THEMES;
use crate::ui::{self, Row};

/// Icon size in points; drawn at 2x for Retina.
const ICON_PT: usize = 18;

pub struct Status {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
}

/// The app icon as a template image (macOS tints it for the menu bar).
fn icon() -> Option<Retained<NSImage>> {
    let n = ICON_PT * 2;
    // SAFETY: null planes make the rep allocate its own n x n RGBA buffer, which is then
    // filled within its bounds.
    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            n as isize,
            n as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (n * 4) as isize,
            32,
        )?;
        let data = std::slice::from_raw_parts_mut(rep.bitmapData(), n * n * 4);
        for y in 0..n {
            for x in 0..n {
                let a = (crate::icon::coverage(x, y, n) * 255.0).round() as u8;
                // Premultiplied black.
                data[(y * n + x) * 4..][..4].copy_from_slice(&[0, 0, 0, a]);
            }
        }
        rep.setSize(NSSize::new(ICON_PT as f64, ICON_PT as f64));
        let image = NSImage::initWithSize(
            NSImage::alloc(),
            NSSize::new(ICON_PT as f64, ICON_PT as f64),
        );
        image.addRepresentation(&rep);
        image.setTemplate(true);
        Some(image)
    }
}

impl Status {
    pub fn new(mtm: MainThreadMarker, controller: &Controller) -> Self {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(mtm) {
            match icon() {
                Some(image) => button.setImage(Some(&image)),
                None => button.setTitle(ns_string!("Glyphdrift")),
            }
        }
        let menu = NSMenu::new(mtm);
        menu.setDelegate(Some(ProtocolObject::from_ref(controller)));
        item.setMenu(Some(&menu));
        Self { mtm, item }
    }

    /// Update hover text and paused look.
    pub fn set(&self, tip: &str, paused: bool) {
        if let Some(button) = self.item.button(self.mtm) {
            button.setToolTip(Some(&NSString::from_str(tip)));
            button.setAppearsDisabled(paused);
        }
    }
}

fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    target: &Controller,
) -> Retained<NSMenuItem> {
    // SAFETY: the action takes one sender argument and the target outlives the menu.
    unsafe {
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            ns_string!(""),
        );
        item.setTarget(Some(target));
        item
    }
}

fn checked(item: &NSMenuItem, on: bool) {
    item.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

/// A submenu of `rows`: disabled headings, then items tagged with their index.
fn grouped(
    mtm: MainThreadMarker,
    title: &str,
    rows: &[Row],
    label: impl Fn(usize) -> &'static str,
    current: usize,
    action: Sel,
    target: &Controller,
) -> Retained<NSMenuItem> {
    let sub = NSMenu::new(mtm);
    sub.setAutoenablesItems(false);
    for (n, &row) in rows.iter().enumerate() {
        match row {
            Row::Heading(h) => {
                if n > 0 {
                    sub.addItem(&NSMenuItem::separatorItem(mtm));
                }
                let heading = item(mtm, h, None, target);
                heading.setEnabled(false);
                sub.addItem(&heading);
            }
            Row::Item(i) => {
                let it = item(mtm, label(i), Some(action), target);
                it.setTag(i as isize);
                checked(&it, i == current);
                sub.addItem(&it);
            }
        }
    }
    let parent = item(mtm, title, None, target);
    parent.setSubmenu(Some(&sub));
    parent
}

/// Rebuild the menu with the current state, just before it opens.
pub fn fill(app: &App, menu: &NSMenu) {
    let (mtm, target) = (app.mtm, &*app.controller);
    menu.removeAllItems();
    menu.addItem(&item(mtm, "Settings…", Some(sel!(openSettings:)), target));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let anim = crate::anim::index_of(&app.cfg.general.animation);
    menu.addItem(&grouped(
        mtm,
        "Animation",
        &ui::anim_rows(),
        |i| ANIMATIONS[i].1,
        anim,
        sel!(chooseAnimation:),
        target,
    ));
    menu.addItem(&grouped(
        mtm,
        "Theme",
        &ui::theme_rows(),
        |i| THEMES[i].label,
        crate::theme::index_of(&app.cfg.theme.name),
        sel!(chooseTheme:),
        target,
    ));
    let clock = item(mtm, "Show clock", Some(sel!(toggleClock:)), target);
    checked(&clock, app.cfg.general.clock);
    menu.addItem(&clock);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let login = item(mtm, "Open at Login", Some(sel!(toggleLogin:)), target);
    checked(&login, autostart::enabled());
    menu.addItem(&login);
    let pause = if app.user_paused { "Resume" } else { "Pause" };
    menu.addItem(&item(mtm, pause, Some(sel!(togglePause:)), target));
    menu.addItem(&item(mtm, "Quit Glyphdrift", Some(sel!(quit:)), target));
}
