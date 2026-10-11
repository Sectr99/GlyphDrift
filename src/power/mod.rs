//! Decides when rendering should pause.

use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};

/// True when a fullscreen app, game, or presentation owns the screen.
pub fn fullscreen_app_active() -> bool {
    // SAFETY: no preconditions; failure is treated as "not fullscreen".
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) => s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE,
        Err(_) => false,
    }
}

/// True when Windows battery saver (energy saver) is on.
pub fn battery_saver_on() -> bool {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: `s` is a live local the call fills in; failure is treated as "off".
    unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.SystemStatusFlag == 1
}
