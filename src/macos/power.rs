//! Decides when rendering should pause. A covered screen (a fullscreen app's Space, a
//! window over the whole desktop, a locked screen) is detected per window through AppKit's
//! occlusion state, in `App::refresh_pause`.

use objc2_foundation::NSProcessInfo;

/// True when Low Power Mode is on.
pub fn low_power_mode() -> bool {
    NSProcessInfo::processInfo().isLowPowerModeEnabled()
}
