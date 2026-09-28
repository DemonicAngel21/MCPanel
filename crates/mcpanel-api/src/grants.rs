//! Path grants: the UI never passes arbitrary host paths. The host shows a native dialog
//! (or receives an OS drag-and-drop), registers the chosen path here, and hands the UI
//! an opaque single-use token. API calls that need a host path accept only tokens.

use crate::error::{ApiError, ApiResult};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantKind {
    /// An existing directory (server location, import source).
    Directory,
    /// An existing file or directory to read from (import, Java executable).
    Source,
    /// A destination path chosen in a save dialog.
    SaveTarget,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrantDto {
    pub token: String,
    /// Shown to the user; never sent back.
    pub display_path: String,
    pub name: String,
}

struct Entry {
    kind: GrantKind,
    path: PathBuf,
    issued: Instant,
}

pub struct GrantRegistry {
    entries: Mutex<HashMap<String, Entry>>,
    ttl: Duration,
}

impl Default for GrantRegistry {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(15 * 60),
        }
    }
}

impl GrantRegistry {
    pub fn issue(&self, kind: GrantKind, path: PathBuf) -> GrantDto {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let display_path = mcpanel_core::files::fsx::simplify(&path)
            .to_string_lossy()
            .to_string();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| display_path.clone());
        let mut map = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        map.retain(|_, e| e.issued.elapsed() < self.ttl);
        map.insert(
            token.clone(),
            Entry {
                kind,
                path,
                issued: Instant::now(),
            },
        );
        GrantDto {
            token,
            display_path,
            name,
        }
    }

    /// Resolve and consume a token. Directory grants may be peeked (for previews) with
    /// [`GrantRegistry::peek`] before being consumed.
    pub fn take(&self, token: &str, kind: GrantKind) -> ApiResult<PathBuf> {
        let mut map = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        match map.remove(token) {
            Some(e) if e.kind == kind && e.issued.elapsed() < self.ttl => Ok(e.path),
            _ => Err(ApiError::new(
                "GRANT_INVALID",
                "The selected location has expired; please choose it again",
            )),
        }
    }

    pub fn peek(&self, token: &str, kind: GrantKind) -> ApiResult<PathBuf> {
        let map = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        match map.get(token) {
            Some(e) if e.kind == kind && e.issued.elapsed() < self.ttl => Ok(e.path.clone()),
            _ => Err(ApiError::new(
                "GRANT_INVALID",
                "The selected location has expired; please choose it again",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_are_single_use_and_typed() {
        let r = GrantRegistry::default();
        let g = r.issue(GrantKind::Directory, PathBuf::from(r"C:\x"));
        assert!(r.take(&g.token, GrantKind::Source).is_err());
        let g = r.issue(GrantKind::Directory, PathBuf::from(r"C:\x"));
        assert!(r.peek(&g.token, GrantKind::Directory).is_ok());
        assert_eq!(
            r.take(&g.token, GrantKind::Directory).unwrap(),
            PathBuf::from(r"C:\x")
        );
        assert!(r.take(&g.token, GrantKind::Directory).is_err());
        assert!(r.take("forged", GrantKind::Directory).is_err());
    }
}
