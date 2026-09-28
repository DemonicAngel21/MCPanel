//! Ports: the interfaces the core needs from the outside world. Adapters (database,
//! platform, HTTP/providers) implement them. Nothing here names a concrete technology.

use crate::backup::{BackupPolicy, BackupRecord};
use crate::content::{ContentKind, InstalledContent, PendingChange};
use crate::crash::{CrashEvent, RestartPolicy};
use crate::error::{CoreResult, ErrorCode};
use crate::ids::{BackupId, JavaRuntimeId, JobId, ServerId};
use crate::jobs::{JobRecord, JobStatus};
use crate::model::{AuditEntry, AuditQuery, JavaRuntime, RuntimeStateRecord, Server};
use crate::time::Timestamp;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

// ───────────────────────────── Repositories ─────────────────────────────

#[async_trait]
pub trait ServerRepository: Send + Sync {
    async fn list(&self) -> CoreResult<Vec<Server>>;
    async fn get(&self, id: ServerId) -> CoreResult<Option<Server>>;
    async fn insert(&self, server: &Server) -> CoreResult<()>;
    async fn update(&self, server: &Server) -> CoreResult<()>;
    async fn delete(&self, id: ServerId) -> CoreResult<()>;
    async fn runtime_states(&self) -> CoreResult<Vec<RuntimeStateRecord>>;
    async fn save_runtime_state(&self, id: ServerId, state: &RuntimeStateRecord) -> CoreResult<()>;
}

#[async_trait]
pub trait JavaRuntimeRepository: Send + Sync {
    async fn list(&self) -> CoreResult<Vec<JavaRuntime>>;
    async fn get(&self, id: JavaRuntimeId) -> CoreResult<Option<JavaRuntime>>;
    /// Insert or update by `path` (case-insensitive). Returns the stored id.
    async fn upsert(&self, runtime: &JavaRuntime) -> CoreResult<JavaRuntimeId>;
    async fn delete(&self, id: JavaRuntimeId) -> CoreResult<()>;
}

#[async_trait]
pub trait AuditRepository: Send + Sync {
    async fn insert(&self, entry: &AuditEntry) -> CoreResult<()>;
    async fn query(&self, query: &AuditQuery) -> CoreResult<Vec<AuditEntry>>;
}

#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn insert(&self, job: &JobRecord) -> CoreResult<()>;
    async fn finish(
        &self,
        id: JobId,
        status: JobStatus,
        at: Timestamp,
        error_code: Option<ErrorCode>,
        error_message: Option<String>,
        result: Option<serde_json::Value>,
    ) -> CoreResult<()>;
    async fn get(&self, id: JobId) -> CoreResult<Option<JobRecord>>;
    async fn recent(&self, limit: u32) -> CoreResult<Vec<JobRecord>>;
    async fn mark_interrupted(&self, at: Timestamp) -> CoreResult<u64>;
}

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self, key: &str) -> CoreResult<Option<serde_json::Value>>;
    async fn set(&self, key: &str, value: &serde_json::Value) -> CoreResult<()>;
    async fn all(&self) -> CoreResult<Vec<(String, serde_json::Value)>>;
}

#[async_trait]
pub trait BackupRepository: Send + Sync {
    async fn insert(&self, backup: &BackupRecord) -> CoreResult<()>;
    async fn update(&self, backup: &BackupRecord) -> CoreResult<()>;
    async fn get(&self, id: BackupId) -> CoreResult<Option<BackupRecord>>;
    /// Newest first; all servers when `server_id` is `None`.
    async fn list(&self, server_id: Option<ServerId>) -> CoreResult<Vec<BackupRecord>>;
    async fn delete(&self, id: BackupId) -> CoreResult<()>;
    async fn policy(&self, server_id: ServerId) -> CoreResult<Option<BackupPolicy>>;
    async fn policies(&self) -> CoreResult<Vec<BackupPolicy>>;
    async fn save_policy(&self, policy: &BackupPolicy) -> CoreResult<()>;
}

#[async_trait]
pub trait ContentRepository: Send + Sync {
    async fn installed(&self, server_id: ServerId) -> CoreResult<Vec<InstalledContent>>;
    /// Insert or replace the record for (server, kind, file name).
    async fn upsert(&self, content: &InstalledContent) -> CoreResult<()>;
    async fn remove(
        &self,
        server_id: ServerId,
        kind: ContentKind,
        file_name: &str,
    ) -> CoreResult<()>;
    async fn pending(&self, server_id: ServerId) -> CoreResult<Vec<PendingChange>>;
    async fn add_pending(&self, change: &PendingChange) -> CoreResult<()>;
    async fn remove_pending(&self, id: &str) -> CoreResult<()>;
    async fn servers_with_pending(&self) -> CoreResult<Vec<ServerId>>;
}

#[async_trait]
pub trait CrashRepository: Send + Sync {
    async fn policy(&self, server_id: ServerId) -> CoreResult<Option<RestartPolicy>>;
    async fn save_policy(&self, policy: &RestartPolicy) -> CoreResult<()>;
    async fn insert(&self, event: &CrashEvent) -> CoreResult<()>;
    /// Newest first.
    async fn recent(&self, server_id: ServerId, limit: u32) -> CoreResult<Vec<CrashEvent>>;
}

/// Aggregated play history of one player on one server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerStats {
    pub name: String,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    /// Completed sessions only.
    pub total_play_ms: i64,
    pub sessions: u32,
}

#[async_trait]
pub trait PlayerRepository: Send + Sync {
    async fn session_started(
        &self,
        server_id: ServerId,
        name: &str,
        at: Timestamp,
    ) -> CoreResult<()>;
    async fn session_ended(&self, server_id: ServerId, name: &str, at: Timestamp)
    -> CoreResult<()>;
    /// End every open session of a server (it stopped). Returns how many were open.
    async fn end_open_sessions(&self, server_id: ServerId, at: Timestamp) -> CoreResult<u64>;
    /// Sessions left open by a previous MCPanel run: their end is unknown, so they are
    /// closed as interrupted and not counted as play time.
    async fn interrupt_open_sessions(&self) -> CoreResult<u64>;
    async fn stats(&self, server_id: ServerId) -> CoreResult<Vec<PlayerStats>>;
}

// ──────────────────────────── Player profiles ───────────────────────────

/// Resolves Minecraft (Java Edition) account names to UUIDs.
#[async_trait]
pub trait ProfileLookup: Send + Sync {
    /// `Ok(None)` when no account has this name.
    async fn uuid_for_name(&self, name: &str) -> CoreResult<Option<(uuid::Uuid, String)>>;
}

// ─────────────────────────────── Platform ───────────────────────────────

/// A process to launch. Arguments are passed as argv entries — never through a shell.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Extra environment variables on top of the platform's minimal allowlisted set.
    pub env: Vec<(String, String)>,
    /// Name used for the OS process group (Windows: named Job Object) so an orphaned
    /// process tree can be re-attached for termination after an MCPanel restart.
    pub group_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ExitInfo {
    pub code: Option<i32>,
}

#[async_trait]
pub trait ProcessWaiter: Send {
    async fn wait(self: Box<Self>) -> CoreResult<ExitInfo>;
}

pub trait ProcessController: Send + Sync {
    /// Forcefully terminate the whole process tree.
    fn terminate_tree(&self) -> CoreResult<()>;
}

pub struct SpawnedProcess {
    pub pid: u32,
    pub start_time: Option<u64>,
    pub stdin: Box<dyn AsyncWrite + Send + Unpin>,
    pub stdout: Box<dyn AsyncRead + Send + Unpin>,
    pub stderr: Box<dyn AsyncRead + Send + Unpin>,
    pub waiter: Box<dyn ProcessWaiter>,
    pub controller: Arc<dyn ProcessController>,
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DiskSpace {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationWarning {
    /// Inside a folder synchronised by OneDrive (sync locks corrupt worlds).
    OneDriveSynced,
    NetworkDrive,
    ProgramFiles,
    SystemDirectory,
    DriveRoot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount_point: String,
    pub name: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSnapshot {
    /// Whole-system CPU usage, 0–100.
    pub cpu_percent: f32,
    pub cpu_count: u32,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub disks: Vec<DiskInfo>,
    pub os_name: String,
    pub os_version: String,
    pub host_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProcessUsage {
    /// CPU usage of the process tree normalised to the whole machine, 0–100.
    pub cpu_percent: f32,
    /// Resident memory of the process tree.
    pub memory_bytes: u64,
    pub process_count: u32,
}

#[async_trait]
pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;

    /// Spawn a supervised child process with piped stdio in its own process group.
    fn spawn(&self, spec: &ProcessSpec) -> CoreResult<SpawnedProcess>;

    /// Run a short-lived program and capture its output (no console window).
    async fn run_capture(
        &self,
        program: &Path,
        args: &[String],
        timeout: Duration,
    ) -> CoreResult<CommandOutput>;

    /// Start time of a live process, used to confirm PID identity.
    fn process_start_time(&self, pid: u32) -> Option<u64>;

    /// Obtain a controller for an orphaned process tree from a previous MCPanel run.
    fn attach_orphan(&self, pid: u32, group_name: &str) -> Option<Arc<dyn ProcessController>>;

    /// Candidate `java` executables discovered on this machine (unvalidated).
    fn java_candidates(&self) -> Vec<PathBuf>;

    fn disk_space(&self, path: &Path) -> CoreResult<DiskSpace>;

    fn location_warnings(&self, path: &Path) -> Vec<LocationWarning>;

    fn system_snapshot(&self) -> SystemSnapshot;

    fn process_tree_usage(&self, pid: u32) -> Option<ProcessUsage>;

    /// Whether a TCP port is already being listened on (without binding it ourselves,
    /// which would trigger a firewall prompt for MCPanel).
    fn tcp_port_status(&self, port: u16) -> PortStatus;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PortStatus {
    Free,
    InUse {
        pid: Option<u32>,
        process_name: Option<String>,
    },
    Unknown,
}

// ─────────────────────────────── Downloads ──────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    Sha1,
    Sha256,
    Sha512,
    /// Integrity only — not collision resistant.
    Md5,
}

impl HashAlgorithm {
    /// Whether the algorithm is strong enough to be called "verified" in the UI.
    pub fn is_strong(self) -> bool {
        matches!(self, Self::Sha256 | Self::Sha512)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedHash {
    pub algorithm: HashAlgorithm,
    pub hex: String,
}

#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub url: String,
    pub expected_hash: Option<ExpectedHash>,
    pub expected_size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    pub bytes: u64,
    pub sha256: String,
}

pub type ProgressFn = dyn Fn(u64, Option<u64>) + Send + Sync;

#[async_trait]
pub trait Downloader: Send + Sync {
    /// Download `req.url` (HTTPS only) to `dest`, verifying size and hash. On any failure
    /// the partial file is removed.
    async fn download(
        &self,
        req: &DownloadRequest,
        dest: &Path,
        progress: &ProgressFn,
        cancel: &CancellationToken,
    ) -> CoreResult<DownloadOutcome>;
}
