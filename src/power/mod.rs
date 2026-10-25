//! Decides when rendering should pause.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, RegisterPowerSettingNotification, POWERBROADCAST_SETTING,
    SYSTEM_POWER_STATUS,
};
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_WINDOW_HANDLE, PBT_POWERSETTINGCHANGE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE,
    WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

/// Set from the messages `watch` asks for.
static LOCKED: AtomicBool = AtomicBool::new(false);
static DISPLAY_OFF: AtomicBool = AtomicBool::new(false);

/// True when a fullscreen app owns the screen.
pub fn fullscreen_app_active() -> bool {
    // SAFETY: no preconditions; failure reads as not fullscreen.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) => s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE,
        Err(_) => false,
    }
}

/// True when battery saver is on.
pub fn battery_saver_on() -> bool {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: `s` is a live local; failure reads as off.
    unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.SystemStatusFlag == 1
}

/// Have `window` told when the session locks or unlocks and when the display turns off or
/// on; it passes those messages to `on_message`. Windows sends the display state at once.
pub fn watch(window: HWND) {
    // SAFETY: `window` is a live top-level window; `unwatch` runs before it's destroyed.
    unsafe {
        if let Err(e) = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) {
            log!("power: no lock notifications: {e}");
        }
        // Unregistered when the process ends.
        if let Err(e) = RegisterPowerSettingNotification(
            HANDLE(window.0),
            &GUID_CONSOLE_DISPLAY_STATE,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        ) {
            log!("power: no display notifications: {e}");
        }
    }
}

/// Before destroying the window given to `watch`.
pub fn unwatch(window: HWND) {
    // SAFETY: plain call; fails harmlessly if never registered.
    let _ = unsafe { WTSUnRegisterSessionNotification(window) };
}

/// Note a message `watch` asked for; false if it's another message.
pub fn on_message(msg: u32, wp: WPARAM, lp: LPARAM) -> bool {
    match msg {
        WM_WTSSESSION_CHANGE => {
            match wp.0 as u32 {
                WTS_SESSION_LOCK => LOCKED.store(true, Ordering::Relaxed),
                WTS_SESSION_UNLOCK => LOCKED.store(false, Ordering::Relaxed),
                _ => {}
            }
            true
        }
        WM_POWERBROADCAST if wp.0 as u32 == PBT_POWERSETTINGCHANGE => {
            // SAFETY: for PBT_POWERSETTINGCHANGE, `lp` points at a POWERBROADCAST_SETTING.
            let s = unsafe { &*(lp.0 as *const POWERBROADCAST_SETTING) };
            if s.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                // A DWORD: 0 off, 1 on, 2 dimmed.
                DISPLAY_OFF.store(s.Data[0] == 0, Ordering::Relaxed);
            }
            true
        }
        _ => false,
    }
}

/// Why the wallpaper can't be seen, if it can't: always worth pausing for.
pub fn hidden() -> Option<&'static str> {
    if LOCKED.load(Ordering::Relaxed) {
        Some("the screen is locked")
    } else if DISPLAY_OFF.load(Ordering::Relaxed) {
        Some("the display is off")
    } else {
        None
    }
}
