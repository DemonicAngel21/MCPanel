//! Update checker and automatic updater domain models and version utilities.

use crate::error::CoreResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub published_at: Option<String>,
    pub prerelease: bool,
    pub asset: Option<ReleaseAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release: Option<ReleaseInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Downloading {
        progress: f32,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Ready {
        installer_path: String,
        version: String,
    },
    Installing,
    Error {
        message: String,
    },
}

/// Compares two semantic version strings. Returns true if candidate is newer than current.
pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    fn parse_semver(s: &str) -> Option<((u64, u64, u64), bool)> {
        let trimmed = s.trim().trim_start_matches(['v', 'V']);
        let (ver_part, is_pre) = if let Some((base, _pre)) = trimmed.split_once('-') {
            (base, true)
        } else {
            (trimmed, false)
        };
        let mut parts = ver_part.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        Some(((major, minor, patch), is_pre))
    }

    match (parse_semver(current), parse_semver(candidate)) {
        (Some((c_ver, c_pre)), Some((cand_ver, cand_pre))) => {
            if cand_ver > c_ver {
                true
            } else if cand_ver == c_ver {
                // Same base version: full release is newer than pre-release
                c_pre && !cand_pre
            } else {
                false
            }
        }
        _ => false,
    }
}

#[async_trait]
pub trait UpdateClient: Send + Sync {
    /// Check GitHub releases for updates.
    async fn check_for_updates(
        &self,
        current_version: &str,
        allow_prereleases: bool,
    ) -> CoreResult<UpdateCheckResult>;

    /// Download the installer asset into the specified directory with progress callback.
    async fn download_asset(
        &self,
        asset: &ReleaseAsset,
        target_file: &std::path::Path,
        progress_cb: Box<dyn Fn(u64, Option<u64>) + Send + Sync>,
    ) -> CoreResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.1.0", "v0.1.1"));
        assert!(is_newer_version("0.1.0", "1.0.0"));
        assert!(is_newer_version("0.1.0-alpha.1", "0.1.0"));

        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.2.0", "0.1.0"));
        assert!(!is_newer_version("1.0.0", "0.9.9"));
        assert!(!is_newer_version("0.1.0", "0.1.0-beta.1"));
    }
}
