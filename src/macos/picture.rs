//! The desktop picture, for two settings:
//! - Match menu bar to theme. macOS backs the menu bar with a pale frosted strip wherever a
//!   window lies under it, wallpaper windows included; over the desktop picture it stays
//!   clear. While on, the wallpaper windows stop at the menu bar (see `desktop::create`)
//!   and each screen's picture is a solid image in the theme's background color.
//! - Show on lock screen. The lock screen shows the desktop picture, not app windows, so
//!   the picture becomes a still frame of the animation (drawn by the app, see `mod.rs`).
//!
//! The user's pictures are recorded first, in a file next to the config, and restored when
//! both settings are off or the app quits, or on the next launch after a crash.
//!
//! macOS keeps a picture per Space and only the current one can be set: other Spaces catch
//! up within a second of being visited, and a quit restores the current Space's screens.

use std::path::PathBuf;

use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImageScaling, NSScreen,
    NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageFillColorKey,
    NSWorkspaceDesktopImageScalingKey,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

use crate::config::{self, Rgb};

/// Recorded pictures, one line per screen: name, scaling, clipping, path (tab-separated).
const STATE: &str = "desktop-pictures.txt";
/// Our pictures: solid `menu-bar-RRGGBB.png` and lock screen frames `lock-screen-*.png`. A
/// new name per change makes macOS reload; the prefixes tell ours from the user's.
const PREFIXES: [&str; 2] = ["menu-bar-", "lock-screen-"];

/// True for a picture this app wrote: in its folder, with one of its prefixes.
fn is_ours(path: &str) -> bool {
    let path = std::path::Path::new(path);
    path.parent().is_some_and(|p| Some(p) == dir().as_deref())
        && path
            .file_name()
            .is_some_and(|n| PREFIXES.iter().any(|p| n.to_string_lossy().starts_with(p)))
}

/// A new lock screen frame's path for screen `index`.
pub fn frame_path(index: usize) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Some(dir()?.join(format!("{}{index}-{stamp}.png", PREFIXES[1])))
}

/// A screen's own picture, to restore.
struct Original {
    screen: String,
    path: String,
    scaling: isize,
    clipping: bool,
}

pub struct DesktopPicture {
    originals: Vec<Original>,
}

fn dir() -> Option<PathBuf> {
    Some(config::default_path()?.parent()?.to_path_buf())
}

fn current(screen: &NSScreen) -> Option<String> {
    let url = NSWorkspace::sharedWorkspace().desktopImageURLForScreen(screen)?;
    Some(url.path()?.to_string())
}

fn set(
    screen: &NSScreen,
    path: &str,
    scaling: isize,
    clipping: bool,
    fill: Option<Rgb>,
) -> Result<(), String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let scaling = NSNumber::new_isize(scaling);
    let clipping = NSNumber::new_bool(clipping);
    let mut keys = vec![
        // SAFETY: AppKit's constant keys.
        unsafe { NSWorkspaceDesktopImageScalingKey },
        unsafe { NSWorkspaceDesktopImageAllowClippingKey },
    ];
    let mut values: Vec<&AnyObject> = vec![&scaling, &clipping];
    let color = fill.map(super::desktop::color);
    if let Some(color) = &color {
        // SAFETY: as above.
        keys.push(unsafe { NSWorkspaceDesktopImageFillColorKey });
        values.push(color);
    }
    let options = NSDictionary::from_slices(&keys, &values);
    // SAFETY: the options hold the value types AppKit documents for each key.
    unsafe {
        NSWorkspace::sharedWorkspace()
            .setDesktopImageURL_forScreen_options_error(&url, screen, &options)
    }
    .map_err(|e| e.localizedDescription().to_string())
}

/// A small solid PNG in `c`, stretched to fill the screen.
fn write_solid(path: &PathBuf, c: Rgb) -> Result<(), String> {
    const N: usize = 16;
    // SAFETY: null planes make the rep allocate its own N x N RGBA buffer, filled within its
    // bounds.
    let png = unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            N as isize,
            N as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (N * 4) as isize,
            32,
        )
        .ok_or("cannot create image")?;
        let data = std::slice::from_raw_parts_mut(rep.bitmapData(), N * N * 4);
        for px in data.chunks_mut(4) {
            px.copy_from_slice(&[c.0, c.1, c.2, 255]);
        }
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
            .ok_or("cannot encode PNG")?
    };
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{e}"))?;
    }
    std::fs::write(path, png.to_vec()).map_err(|e| format!("{e}"))
}

/// `frame` (`size` points minus the menu bar, at `scale`) on a `size` image, under a strip of
/// `bg` where the menu bar is: for when the wallpaper windows stop below the menu bar.
pub fn with_menu_bar_strip(
    frame: &NSBitmapImageRep,
    size: (f64, f64),
    scale: f64,
    bg: Rgb,
) -> Option<objc2::rc::Retained<NSBitmapImageRep>> {
    // SAFETY: null planes make the rep allocate its own buffer.
    let out = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            (size.0 * scale).round() as isize,
            (size.1 * scale).round() as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    out.setSize(objc2_foundation::NSSize::new(size.0, size.1));
    let ctx = objc2_app_kit::NSGraphicsContext::graphicsContextWithBitmapImageRep(&out)?;
    let previous = objc2_app_kit::NSGraphicsContext::currentContext();
    objc2_app_kit::NSGraphicsContext::setCurrentContext(Some(&ctx));
    let full = objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::ZERO,
        objc2_foundation::NSSize::new(size.0, size.1),
    );
    super::desktop::color(bg).setFill();
    objc2_app_kit::NSRectFill(full);
    // AppKit is y-up: the frame fills the bottom, leaving the strip at the top.
    let fh = frame.size().height;
    frame.drawInRect(objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::ZERO,
        objc2_foundation::NSSize::new(size.0, fh),
    ));
    ctx.flushGraphics();
    objc2_app_kit::NSGraphicsContext::setCurrentContext(previous.as_deref());
    Some(out)
}

impl DesktopPicture {
    /// The recorded pictures, if a previous run left some (on, or crashed).
    pub fn load() -> Self {
        let text = dir()
            .and_then(|d| std::fs::read_to_string(d.join(STATE)).ok())
            .unwrap_or_default();
        let originals = text
            .lines()
            .filter_map(|line| {
                let mut f = line.splitn(4, '\t');
                Some(Original {
                    screen: f.next()?.into(),
                    scaling: f.next()?.parse().ok()?,
                    clipping: f.next()? == "1",
                    path: f.next()?.into(),
                })
            })
            .collect();
        Self { originals }
    }

    /// True while some screen shows our picture instead of its own.
    pub fn active(&self) -> bool {
        !self.originals.is_empty()
    }

    fn save(&self) {
        let Some(path) = dir().map(|d| d.join(STATE)) else {
            return;
        };
        if self.originals.is_empty() {
            let _ = std::fs::remove_file(path);
            return;
        }
        let text: String = self
            .originals
            .iter()
            .map(|o| {
                format!(
                    "{}\t{}\t{}\t{}\n",
                    o.screen,
                    o.scaling,
                    u8::from(o.clipping),
                    o.path
                )
            })
            .collect();
        if let Err(e) = std::fs::write(path, text) {
            log!("desktop picture: could not record the user's pictures: {e}");
        }
    }

    /// Our pictures other than `keep`.
    fn remove_stale(keep: &[PathBuf]) {
        let Some(Ok(entries)) = dir().map(std::fs::read_dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if is_ours(&p.to_string_lossy()) && !keep.contains(&p) {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    /// Show `bg` on every screen: a small solid image, stretched.
    pub fn solid(&mut self, mtm: MainThreadMarker, bg: Rgb) {
        let Some(dir) = dir() else {
            return;
        };
        let file = dir.join(format!(
            "{}{:02x}{:02x}{:02x}.png",
            PREFIXES[0], bg.0, bg.1, bg.2
        ));
        if !file.exists() {
            if let Err(e) = write_solid(&file, bg) {
                log!("desktop picture: {e}");
                return;
            }
        }
        let n = NSScreen::screens(mtm).len();
        self.show(mtm, &vec![file; n], bg);
    }

    /// Show `files[i]` on screen `i` of the current Space (images already written), first
    /// recording any picture that isn't ours (the user's, or one they just picked). `bg`
    /// fills any margin. Cheap when nothing changed.
    pub fn show(&mut self, mtm: MainThreadMarker, files: &[PathBuf], bg: Rgb) {
        let screens = NSScreen::screens(mtm);
        for (screen, file) in screens.iter().zip(files) {
            let target = file.to_string_lossy().into_owned();
            let now = current(&screen);
            if now.as_deref() == Some(target.as_str()) {
                continue;
            }
            let name = screen.localizedName().to_string();
            if let Some(path) = now.filter(|p| !is_ours(p)) {
                let options = NSWorkspace::sharedWorkspace().desktopImageOptionsForScreen(&screen);
                let number = |key| {
                    options
                        .as_ref()
                        .and_then(|o| o.objectForKey(key))
                        .and_then(|v| v.downcast::<NSNumber>().ok())
                };
                // SAFETY: AppKit's constant keys.
                let scaling = number(unsafe { NSWorkspaceDesktopImageScalingKey }).map_or(
                    NSImageScaling::ScaleProportionallyUpOrDown.0 as isize,
                    |n| n.as_isize(),
                );
                let clipping = number(unsafe { NSWorkspaceDesktopImageAllowClippingKey })
                    .is_none_or(|n| n.as_bool());
                self.originals.retain(|o| o.screen != name);
                self.originals.push(Original {
                    screen: name,
                    path,
                    scaling,
                    clipping,
                });
                self.save(); // before changing anything
            }
            let independent = NSImageScaling::ScaleAxesIndependently.0 as isize;
            if let Err(e) = set(&screen, &target, independent, true, Some(bg)) {
                log!("desktop picture: could not set it: {e}");
            }
        }
        Self::remove_stale(files);
    }

    /// Give each connected screen its own picture back. Screens that aren't connected
    /// stay recorded for a later run.
    pub fn restore(&mut self, mtm: MainThreadMarker) {
        let screens = NSScreen::screens(mtm);
        let before = self.originals.len();
        self.originals.retain(|o| {
            let Some(screen) = screens
                .iter()
                .find(|s| s.localizedName().to_string() == o.screen)
            else {
                return true;
            };
            if let Err(e) = set(&screen, &o.path, o.scaling, o.clipping, None) {
                log!("desktop picture: could not restore it: {e}");
            }
            false
        });
        if self.originals.len() != before {
            self.save();
        }
        if self.originals.is_empty() {
            Self::remove_stale(&[]);
        }
    }
}
