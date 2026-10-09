//! "Start with Windows": a value under the per-user Run key. No admin rights needed, and
//! it shows up (and can be disabled) in Settings > Apps > Startup.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE: PCWSTR = w!("ascii-wallpaper");

/// The command line Windows runs at sign-in: this exe's path, quoted.
fn command() -> Option<String> {
    Some(format!("\"{}\"", std::env::current_exe().ok()?.display()))
}

/// True if the Run entry exists and points at this exe (a moved exe reads as off).
pub fn enabled() -> bool {
    let Some(cmd) = command() else {
        return false;
    };
    let mut buf = [0u16; 1024];
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    // SAFETY: `buf` and `bytes` are live locals; `bytes` holds the buffer size in bytes.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    let len = (bytes as usize / 2).saturating_sub(1); // drop the terminating NUL
    r == ERROR_SUCCESS && String::from_utf16_lossy(&buf[..len]).eq_ignore_ascii_case(&cmd)
}

pub fn set(on: bool) -> Result<(), String> {
    // SAFETY: all strings are NUL-terminated and outlive the calls; `data` length is in bytes.
    let r = unsafe {
        if on {
            let cmd = command().ok_or("cannot locate own exe")?;
            let data: Vec<u16> = cmd.encode_utf16().chain(Some(0)).collect();
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                VALUE,
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                (data.len() * 2) as u32,
            )
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE)
        }
    };
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("registry error {}", r.0))
    }
}
