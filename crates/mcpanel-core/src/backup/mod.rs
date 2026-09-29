//! Local backups: consistent ZIP archives of a server directory with a manifest,
//! verification, GFS retention, schedules and staged restore (spec §8).

pub mod archive;
pub mod restore;
pub mod retention;
pub mod service;

pub use service::{BackupService, BackupServiceDeps, BackupView, CreateBackupRequest};

use crate::ids::{BackupId, ServerId};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Name of the manifest entry at the root of every MCPanel backup archive.
pub const MANIFEST_NAME: &str = "mcpanel-manifest.json";
/// Identifies the archive format in the manifest.
pub const MANIFEST_FORMAT: &str = "mcpanel-backup";
pub const MANIFEST_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupKind {
    Manual,
    Scheduled,
    /// Taken automatically before a restore; never removed by retention.
    PreRestore,
}

impl BackupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Scheduled => "scheduled",
            Self::PreRestore => "pre_restore",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => Self::Manual,
            "scheduled" => Self::Scheduled,
            "pre_restore" => Self::PreRestore,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStatus {
    Creating,
    Ready,
    Failed,
}

impl BackupStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "creating" => Self::Creating,
            "ready" => Self::Ready,
            _ => Self::Failed,
        }
    }
}

/// A file that could not be included (for example locked by another program).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: BackupId,
    /// `None` once the server has been removed from MCPanel (the archive remains).
    pub server_id: Option<ServerId>,
    pub server_name: String,
    pub kind: BackupKind,
    pub status: BackupStatus,
    pub path: PathBuf,
    pub created_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    /// Size of the archive file.
    pub size_bytes: u64,
    /// Uncompressed size of the files in the archive.
    pub content_bytes: u64,
    pub file_count: u64,
    /// SHA-256 of the archive file, computed after writing.
    pub sha256: Option<String>,
    /// Taken while the server was running (with `save-off` / `save-all flush`).
    pub live: bool,
    /// Contains highly sensitive files (e.g. the Floodgate key) — decision #6.
    pub contains_sensitive: bool,
    /// The archive is age-encrypted with the Backup Master Key (`.zip.age`).
    #[serde(default)]
    pub encrypted: bool,
    pub software_id: String,
    pub game_version: String,
    pub note: Option<String>,
    /// Exempt from automatic retention.
    pub protected: bool,
    pub skipped: Vec<SkippedFile>,
    pub error_message: Option<String>,
}

/// GFS retention for scheduled backups. Manual and pre-restore backups are never
/// removed automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retention {
    pub keep_last: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            keep_last: 4,
            keep_daily: 7,
            keep_weekly: 4,
            keep_monthly: 3,
        }
    }
}

/// Per-server schedule and retention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupPolicy {
    pub server_id: ServerId,
    pub enabled: bool,
    pub interval_minutes: u32,
    /// Skip a scheduled backup when the server has not run since the newest backup.
    pub skip_if_idle: bool,
    pub retention: Retention,
    /// Scheduled backups of this server may use at most this many GiB (0 = no cap);
    /// the oldest are removed first and the newest is always kept.
    #[serde(default)]
    pub max_total_gb: u32,
    /// Last time the scheduler considered this server (a backup ran or was skipped).
    pub last_run_at: Option<Timestamp>,
}

impl BackupPolicy {
    pub fn default_for(server_id: ServerId) -> Self {
        Self {
            server_id,
            enabled: false,
            interval_minutes: 6 * 60,
            skip_if_idle: true,
            retention: Retention::default(),
            max_total_gb: 0,
            last_run_at: None,
        }
    }
}

pub const MIN_INTERVAL_MINUTES: u32 = 15;
pub const MAX_INTERVAL_MINUTES: u32 = 7 * 24 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestServer {
    pub id: ServerId,
    pub name: String,
    /// The software installed when the backup was taken; a restore puts it back.
    pub software: crate::model::InstalledSoftware,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// `mcpanel-manifest.json`: describes and authenticates the archive contents (by hash).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format: String,
    pub format_version: u32,
    pub generator: String,
    pub backup_id: BackupId,
    pub created_at: Timestamp,
    pub kind: BackupKind,
    pub live: bool,
    pub server: ManifestServer,
    pub contains_sensitive: bool,
    pub total_bytes: u64,
    pub files: Vec<ManifestFile>,
    pub skipped: Vec<SkippedFile>,
}
