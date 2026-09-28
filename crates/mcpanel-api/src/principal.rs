//! Callers and authorization. v1 has a single principal (the local desktop user), but
//! every API call already passes through `authorize` so remote/token principals can be
//! added without restructuring the API.

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ServersRead,
    ServersControl,
    ServersManage,
    FilesRead,
    FilesWrite,
    JavaManage,
    BackupsRead,
    BackupsManage,
    /// Replaces a server's files with a backup.
    BackupsRestore,
    SettingsWrite,
    ActivityRead,
    SystemRead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// The user of the desktop application (full access).
    LocalUser,
}

impl Principal {
    /// Identifier recorded as the audit `actor`.
    pub fn actor(&self) -> &'static str {
        match self {
            Self::LocalUser => "user",
        }
    }

    pub fn authorize(&self, _permission: Permission) -> ApiResult<()> {
        match self {
            Self::LocalUser => Ok(()),
        }
    }
}

#[allow(dead_code)]
fn forbidden() -> ApiError {
    ApiError::new("FORBIDDEN", "You are not allowed to perform this action")
}
