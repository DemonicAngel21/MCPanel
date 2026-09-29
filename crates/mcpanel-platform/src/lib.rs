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
