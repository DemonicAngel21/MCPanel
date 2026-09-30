//! # mcpanel-platform
//!
//! Implementations of the `mcpanel_core::ports::Platform` port. Windows is the v1
//! target; Linux/macOS implementations will live beside it without touching the core.

mod common;
#[cfg(windows)]
mod secrets;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use crate::secrets::CredentialStore as NativeSecretStore;
#[cfg(windows)]
pub use crate::windows::WindowsPlatform as NativePlatform;

/// Resolve MCPanel's standard directories for this OS.
pub use common::default_paths;

/// The Windows accent color as `#rrggbb` (`HKCU\Software\Microsoft\Windows\DWM`,
/// `AccentColor` = 0xAABBGGRR), or `None` when it cannot be read.
#[cfg(windows)]
pub fn system_accent_color() -> Option<String> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\DWM")
        .ok()?;
    let v: u32 = key.get_value("AccentColor").ok()?;
    Some(abgr_to_hex(v))
}

#[cfg(not(windows))]
pub fn system_accent_color() -> Option<String> {
    None
}

/// 0xAABBGGRR → `#rrggbb`.
pub fn abgr_to_hex(v: u32) -> String {
    let r = v & 0xff;
    let g = (v >> 8) & 0xff;
    let b = (v >> 16) & 0xff;
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod accent_tests {
    #[test]
    fn converts_dwm_colors() {
        // Windows' default blue accent is stored as 0xFFD77800.
        assert_eq!(super::abgr_to_hex(0xFFD7_7800), "#0078d7");
    }
}
