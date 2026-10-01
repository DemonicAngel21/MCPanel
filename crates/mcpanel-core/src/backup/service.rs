//! Backup orchestration: consistency with a running server, jobs, records, schedules,
//! retention and restore.

use super::archive::{self, VerifyReport};
use super::restore::{self, RestorePreview};
use super::retention::{self, Candidate};
use super::{
    BackupKind, BackupManifest, BackupPolicy, BackupRecord, BackupStatus, MANIFEST_FORMAT,
    MANIFEST_FORMAT_VERSION, MAX_INTERVAL_MINUTES, MIN_INTERVAL_MINUTES, ManifestServer,
};
use crate::audit::AuditLog;
use crate::console::{ConsoleStream, dialect};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::files::fsx::path_starts_with_ci;
use crate::ids::{BackupId, JobId, ServerId};
use crate::jobs::{JobContext, JobManager};
use crate::lifecycle::LifecycleState;
use crate::model::{AuditResult, Server};
use crate::paths::AppPaths;
use crate::ports::{BackupRepository, Platform};
use crate::server::runtime::{Operation, OperationGuard, ServerRuntime};
use crate::server::{ServerManager, provisioning::slugify};
use crate::settings::SettingsService;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Free space kept in reserve beyond the (uncompressed) size of a backup.
const DISK_RESERVE_BYTES: u64 = 256 * 1024 * 1024;
const SCHEDULER_TICK: Duration = Duration::from_secs(30);

pub struct BackupServiceDeps {
    pub repo: Arc<dyn BackupRepository>,
    pub servers: Arc<ServerManager>,
    pub jobs: Arc<JobManager>,
    pub events: EventBus,
    pub audit: Arc<AuditLog>,
    pub settings: Arc<SettingsService>,
    pub paths: AppPaths,
    pub platform: Arc<dyn Platform>,
    pub encryption: Arc<crate::crypto::EncryptionService>,
}

pub struct BackupService {
    repo: Arc<dyn BackupRepository>,
    servers: Arc<ServerManager>,
    jobs: Arc<JobManager>,
    events: EventBus,
    audit: Arc<AuditLog>,
    settings: Arc<SettingsService>,
    paths: AppPaths,
    platform: Arc<dyn Platform>,
    encryption: Arc<crate::crypto::EncryptionService>,
    session_started: Timestamp,
    tz: jiff::tz::TimeZone,
    /// How long to wait for `save-all flush` to be confirmed.
    save_timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBackupRequest {
    pub server_id: ServerId,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupView {
    pub backup: BackupRecord,
    /// Whether the archive file still exists.
    pub file_present: bool,
}

/// The archive being written for a record (renamed on success).
fn partial_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".partial");
    PathBuf::from(s)
}

/// Prefix of decrypted temporary copies next to encrypted backups.
const PLAIN_PREFIX: &str = ".mcpanel-plain-";

/// A plain ZIP for reading a backup: the archive itself, or a decrypted temporary copy
/// next to it (deleted when dropped).
struct PlainArchive {
    path: PathBuf,
    temporary: bool,
}

impl Drop for PlainArchive {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn not_found() -> CoreError {
    CoreError::new(ErrorCode::NotFound, "Backup not found")
}

/// Whether a backup of a server in `state` is possible, and whether it is live.
fn backup_mode(state: LifecycleState, console_attached: bool) -> CoreResult<bool> {
    match state {
        LifecycleState::Running if console_attached => Ok(true),
        LifecycleState::Created
        | LifecycleState::Stopped
        | LifecycleState::Crashed
        | LifecycleState::Error => Ok(false),
        LifecycleState::Detached | LifecycleState::Running => Err(CoreError::new(
            ErrorCode::Unsupported,
            "The console of this server is not connected, so saving cannot be paused for a consistent backup. Stop the server first.",
        )),
        LifecycleState::Starting => Err(CoreError::new(
            ErrorCode::ServerBusy,
            "Wait until the server has finished starting",
        )),
        LifecycleState::Stopping | LifecycleState::Restarting => Err(CoreError::new(
            ErrorCode::ServerBusy,
            "The server is stopping; try again when it has stopped",
        )),
    }
}

fn check_note(note: Option<String>) -> CoreResult<Option<String>> {
    let note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    if note
        .as_ref()
        .is_some_and(|n| n.chars().count() > 200 || n.contains(['\n', '\r']))
    {
        return Err(CoreError::invalid(
            "A backup note must be a single line of at most 200 characters",
        ));
    }
    Ok(note)
}

impl BackupService {
    pub fn new(deps: BackupServiceDeps) -> Arc<Self> {
        Arc::new(Self {
            repo: deps.repo,
            servers: deps.servers,
            jobs: deps.jobs,
            events: deps.events,
            audit: deps.audit,
            settings: deps.settings,
            paths: deps.paths,
            platform: deps.platform,
            encryption: deps.encryption,
            session_started: Timestamp::now(),
            tz: jiff::tz::TimeZone::system(),
            save_timeout: Duration::from_secs(300),
        })
    }

    fn changed(&self, server_id: Option<ServerId>, backup_id: BackupId) {
        self.events.publish(DomainEvent::BackupsChanged {
            server_id,
            backup_id,
        });
    }

    /// Backups left `creating` by a previous session are marked failed and their
    /// partial files removed.
    pub async fn recover_interrupted(&self) -> CoreResult<u64> {
        let mut n = 0;
        for mut b in self.repo.list(None).await? {
            if b.status != BackupStatus::Creating {
                continue;
            }
            let _ = tokio::fs::remove_file(partial_path(&b.path)).await;
            let _ = tokio::fs::remove_file(partial_path(&b.path.with_extension("age-tmp"))).await;
            b.status = BackupStatus::Failed;
            b.error_message =
                Some("Interrupted: MCPanel was closed while the backup was running".into());
            b.finished_at = Some(Timestamp::now());
            self.repo.update(&b).await?;
            n += 1;
        }
        // Decrypted temporary copies left by an interrupted verify/restore.
        let mut dirs = std::collections::BTreeSet::new();
        for b in self.repo.list(None).await? {
            if let Some(p) = b.path.parent() {
                dirs.insert(p.to_path_buf());
            }
        }
        for d in dirs {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with(PLAIN_PREFIX) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        Ok(n)
    }

    /// A readable plain archive for `b` (decrypting an encrypted backup to a temporary
    /// file next to it).
    async fn plain_archive(
        &self,
        b: &BackupRecord,
        ctx: Option<&JobContext>,
    ) -> CoreResult<PlainArchive> {
        if !b.encrypted {
            return Ok(PlainArchive {
                path: b.path.clone(),
                temporary: false,
            });
        }
        let identity = self.encryption.identity()?;
        let dir = b
            .path
            .parent()
            .ok_or_else(|| CoreError::internal("backup without a folder"))?
            .to_path_buf();
        if let Ok(space) = self.platform.disk_space(&dir)
            && space.available_bytes < b.size_bytes.saturating_add(DISK_RESERVE_BYTES)
        {
            return Err(CoreError::new(
                ErrorCode::InsufficientDiskSpace,
                "Not enough free space to decrypt the backup",
            ));
        }
        let plain = PlainArchive {
            path: dir.join(format!(
                "{PLAIN_PREFIX}{}-{}.zip",
                b.id,
                uuid::Uuid::new_v4().simple()
            )),
            temporary: true,
        };
        if let Some(c) = ctx {
            c.progress(None, "Decrypting the backup…");
        }
        let (src, dst) = (b.path.clone(), plain.path.clone());
        let job = ctx.cloned();
        tokio::task::spawn_blocking(move || {
            crate::crypto::decrypt_file(&src, &dst, &identity, &|| {
                job.as_ref().is_some_and(|j| j.is_cancelled())
            })
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))??;
        Ok(plain)
    }

    // ───────────────────────────── location ─────────────────────────────

    /// The directory new backups are written to.
    pub async fn backups_dir(&self) -> CoreResult<PathBuf> {
        Ok(self
            .settings
            .get()
            .await?
            .backups_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| self.paths.default_backups_dir.clone()))
    }

    async fn check_location(&self, dir: &Path) -> CoreResult<()> {
        if !dir.is_absolute() {
            return Err(CoreError::invalid(
                "The backups folder must be an absolute path",
            ));
        }
        for s in self.servers.repo.list().await? {
            if path_starts_with_ci(dir, &s.directory) || path_starts_with_ci(&s.directory, dir) {
                return Err(CoreError::new(
                    ErrorCode::DirectoryNotAllowed,
                    format!(
                        "The backups folder must not be inside a server folder or contain one (\"{}\")",
                        s.name
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Choose the backups folder (`None` = default). Existing backups stay where they are.
    pub async fn set_backups_dir(&self, dir: Option<PathBuf>, actor: &str) -> CoreResult<PathBuf> {
        let target = dir
            .clone()
            .unwrap_or_else(|| self.paths.default_backups_dir.clone());
        self.check_location(&target).await?;
        tokio::fs::create_dir_all(&target)
            .await
            .map_err(|e| CoreError::io("Cannot create the backups folder", &e))?;
        self.settings.set_backups_dir(dir).await?;
        self.audit
            .record(
                actor,
                "backup.set_location",
                None,
                None,
                AuditResult::Success,
                serde_json::json!({}),
            )
            .await;
        Ok(target)
    }

    // ───────────────────────────── queries ─────────────────────────────

    pub async fn list(&self, server_id: Option<ServerId>) -> CoreResult<Vec<BackupView>> {
        let records = self.repo.list(server_id).await?;
        Ok(records
            .into_iter()
            .map(|b| BackupView {
                file_present: b.status != BackupStatus::Ready || b.path.is_file(),
                backup: b,
            })
            .collect())
    }

    pub async fn get(&self, id: BackupId) -> CoreResult<BackupRecord> {
        self.repo.get(id).await?.ok_or_else(not_found)
    }

    // ───────────────────────────── create ─────────────────────────────

    /// Start a manual backup job.
    pub async fn create(
        self: &Arc<Self>,
        req: CreateBackupRequest,
        actor: &str,
    ) -> CoreResult<JobId> {
        let note = check_note(req.note)?;
        let server = self.servers.get(req.server_id).await?;
        let rt = self.servers.runtime(server.id);
        {
            let snap = rt.snapshot();
            backup_mode(snap.state, snap.console_attached)?;
            if snap
                .operations
                .iter()
                .any(|o| !Operation::BackingUp.compatible_with(*o))
            {
                return Err(CoreError::new(
                    ErrorCode::ServerBusy,
                    "Another operation is in progress",
                ));
            }
        }
        let this = Arc::clone(self);
        let actor = actor.to_string();
        self.jobs
            .spawn("backup.create", Some(server.id), move |ctx| async move {
                let guard = rt.begin(Operation::BackingUp)?;
                let result = this
                    .run_backup(&server, &rt, BackupKind::Manual, note, false, &ctx)
                    .await;
                drop(guard);
                this.audit_result(&actor, "backup.create", server.id, &result)
                    .await;
                let b = result?;
                Ok(Some(serde_json::json!({ "backupId": b.id })))
            })
            .await
    }

    async fn audit_result<T>(
        &self,
        actor: &str,
        action: &str,
        server_id: ServerId,
        r: &CoreResult<T>,
    ) {
        self.audit
            .record(
                actor,
                action,
                Some(server_id),
                None,
                if r.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
                match r {
                    Ok(_) => serde_json::json!({}),
                    Err(e) => serde_json::json!({ "error": e.code }),
                },
            )
            .await;
    }

    fn backup_file_name(&self, dir: &Path, at: Timestamp, kind: BackupKind) -> PathBuf {
        let stamp = jiff::Timestamp::from_millisecond(at.millis())
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_zoned(self.tz.clone())
            .strftime("%Y-%m-%d_%H-%M-%S")
            .to_string();
        let kind = kind.as_str().replace('_', "-");
        let mut n = 1;
        loop {
            let name = if n == 1 {
                format!("{stamp}_{kind}.zip")
            } else {
                format!("{stamp}_{kind}-{n}.zip")
            };
            let p = dir.join(name);
            let enc = p.with_extension("zip.age");
            if !p.exists()
                && !partial_path(&p).exists()
                && !enc.exists()
                && !partial_path(&enc).exists()
            {
                return p;
            }
            n += 1;
        }
    }

    /// Wait for the server to confirm `save-all flush` after disabling automatic saving.
    async fn pause_saves(&self, server: &Server, rt: &ServerRuntime) -> CoreResult<()> {
        let dialect_id = self
            .servers
            .registry()
            .get_software(&server.software.software_id)
            .map(|p| p.descriptor.caps.log_dialect.clone())
            .unwrap_or_else(|_| "vanilla".into());
        let d = dialect::dialect(&dialect_id);
        let mut sub = rt.console.subscribe(None, 0);
        self.servers.send_command(server.id, "save-off").await?;
        self.servers
            .send_command(server.id, "save-all flush")
            .await?;
        let deadline = tokio::time::Instant::now() + self.save_timeout;
        loop {
            if rt.state() != LifecycleState::Running {
                return Err(CoreError::new(
                    ErrorCode::ServerNotRunning,
                    "The server stopped while saving for the backup",
                ));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(CoreError::new(
                    ErrorCode::Internal,
                    format!(
                        "The server did not confirm saving within {}s",
                        self.save_timeout.as_secs()
                    ),
                ));
            }
            let batch = tokio::time::timeout(
                Duration::from_secs(1),
                sub.next_batch(500, Duration::from_millis(100)),
            )
            .await;
            if let Ok(Some(batch)) = batch {
                let saved = batch.lines.iter().any(|l| {
                    matches!(l.stream, ConsoleStream::Stdout | ConsoleStream::Stderr) && {
                        let msg = dialect::parse_line(&l.text).message;
                        d.save_complete.iter().any(|r| r.is_match(&msg))
                    }
                });
                if saved {
                    return Ok(());
                }
            }
        }
    }

    async fn resume_saves(&self, server_id: ServerId, rt: &ServerRuntime) {
        if rt.state() != LifecycleState::Running {
            return; // A new process starts with automatic saving enabled.
        }
        if let Err(e) = self.servers.send_command(server_id, "save-on").await {
            tracing::warn!(target: "mcpanel::backup", server = %server_id, "could not re-enable saving: {}", e.message);
            rt.console.push(
                ConsoleStream::System,
                "MCPanel could not re-enable automatic saving after the backup. Run 'save-on'.",
            );
        }
    }

    /// Create a backup. The caller holds the `BackingUp` or `Restoring` operation.
    async fn run_backup(
        self: &Arc<Self>,
        server: &Server,
        rt: &Arc<ServerRuntime>,
        kind: BackupKind,
        note: Option<String>,
        protected: bool,
        ctx: &JobContext,
    ) -> CoreResult<BackupRecord> {
        let live = {
            let snap = rt.snapshot();
            backup_mode(snap.state, snap.console_attached)?
        };
        let base = self.backups_dir().await?;
        self.check_location(&base).await?;
        let dir = base.join(format!(
            "{}-{}",
            slugify(&server.name),
            &server.id.0.simple().to_string()[..8]
        ));
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| CoreError::io("Cannot create the backups folder", &e))?;

        if live {
            ctx.progress(None, "Saving the world…");
            rt.console
                .push(ConsoleStream::System, "Backup: pausing automatic saving…");
            if let Err(e) = self.pause_saves(server, rt).await {
                self.resume_saves(server.id, rt).await;
                return Err(e);
            }
        }
        let outcome = self
            .write_backup(server, &dir, kind, note, protected, live, ctx)
            .await;
        if live {
            self.resume_saves(server.id, rt).await;
            rt.console.push(
                ConsoleStream::System,
                match &outcome {
                    Ok(_) => "Backup finished; automatic saving re-enabled.",
                    Err(_) => "Backup failed; automatic saving re-enabled.",
                },
            );
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    async fn write_backup(
        self: &Arc<Self>,
        server: &Server,
        dir: &Path,
        kind: BackupKind,
        note: Option<String>,
        protected: bool,
        live: bool,
        ctx: &JobContext,
    ) -> CoreResult<BackupRecord> {
        let root = server.directory.clone();
        let scan = tokio::task::spawn_blocking(move || archive::scan(&root))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        let total = scan.total_bytes();
        let space = self.platform.disk_space(dir)?;
        if space.available_bytes < total.saturating_add(DISK_RESERVE_BYTES) {
            return Err(CoreError::new(
                ErrorCode::InsufficientDiskSpace,
                format!(
                    "Not enough free space for the backup ({} MB needed, {} MB free)",
                    (total + DISK_RESERVE_BYTES) / 1_048_576,
                    space.available_bytes / 1_048_576
                ),
            ));
        }

        let now = Timestamp::now();
        let recipient = self.encryption.backup_recipient().await?;
        let mut path = self.backup_file_name(dir, now, kind);
        if recipient.is_some() {
            path.set_extension("zip.age");
        }
        let mut record = BackupRecord {
            id: BackupId::new(),
            server_id: Some(server.id),
            server_name: server.name.clone(),
            kind,
            status: BackupStatus::Creating,
            path: path.clone(),
            created_at: now,
            finished_at: None,
            size_bytes: 0,
            content_bytes: 0,
            file_count: 0,
            sha256: None,
            live,
            contains_sensitive: false,
            encrypted: recipient.is_some(),
            software_id: server.software.software_id.clone(),
            game_version: server.software.game_version.clone(),
            note,
            protected,
            skipped: vec![],
            error_message: None,
        };
        self.repo.insert(&record).await?;
        self.changed(record.server_id, record.id);

        let manifest = BackupManifest {
            format: MANIFEST_FORMAT.into(),
            format_version: MANIFEST_FORMAT_VERSION,
            generator: format!("MCPanel {}", env!("CARGO_PKG_VERSION")),
            backup_id: record.id,
            created_at: now,
            kind,
            live,
            server: ManifestServer {
                id: server.id,
                name: server.name.clone(),
                software: server.software.clone(),
            },
            contains_sensitive: false,
            total_bytes: 0,
            files: vec![],
            skipped: vec![],
        };
        let partial = partial_path(&path);
        let final_path = path.clone();
        let job = ctx.clone();
        let written =
            tokio::task::spawn_blocking(move || -> CoreResult<(BackupManifest, u64, String)> {
                let mut done = 0u64;
                let mut report = |n: u64| {
                    done += n;
                    job.progress(
                        Some(if total == 0 {
                            1.0
                        } else {
                            done as f32 / total as f32
                        }),
                        "Archiving files…",
                    );
                };
                let m = archive::write_archive(
                    &scan,
                    manifest,
                    &partial,
                    &|| job.is_cancelled(),
                    &mut report,
                )?;
                if let Some(r) = &recipient {
                    job.progress(Some(1.0), "Encrypting…");
                    let enc = partial_path(&final_path.with_extension("age-tmp"));
                    let res =
                        crate::crypto::encrypt_file(&partial, &enc, r, &|| job.is_cancelled());
                    let _ = std::fs::remove_file(&partial);
                    if let Err(e) = res {
                        let _ = std::fs::remove_file(&enc);
                        return Err(e);
                    }
                    std::fs::rename(&enc, &final_path)
                        .map_err(|e| CoreError::io("Cannot finish the backup file", &e))?;
                } else {
                    std::fs::rename(&partial, &final_path)
                        .map_err(|e| CoreError::io("Cannot finish the backup file", &e))?;
                }
                let (size, sha) = archive::hash_file(&final_path)?;
                Ok((m, size, sha))
            })
            .await
            .map_err(|e| CoreError::internal(e.to_string()))
            .and_then(|r| r);

        match written {
            Ok((m, size, sha)) => {
                record.status = BackupStatus::Ready;
                record.finished_at = Some(Timestamp::now());
                record.size_bytes = size;
                record.content_bytes = m.total_bytes;
                record.file_count = m.files.len() as u64;
                record.sha256 = Some(sha);
                record.contains_sensitive = m.contains_sensitive;
                record.skipped = m.skipped;
                self.repo.update(&record).await?;
                self.changed(record.server_id, record.id);
                Ok(record)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(partial_path(&path)).await;
                let _ = tokio::fs::remove_file(&path).await;
                if e.code == ErrorCode::Cancelled {
                    self.repo.delete(record.id).await?;
                } else {
                    record.status = BackupStatus::Failed;
                    record.finished_at = Some(Timestamp::now());
                    record.error_message = Some(e.message.clone());
                    self.repo.update(&record).await?;
                }
                self.changed(record.server_id, record.id);
                Err(e)
            }
        }
    }

    // ───────────────────────────── delete / verify ─────────────────────────────

    pub async fn delete(&self, id: BackupId, actor: &str) -> CoreResult<()> {
        let b = self.get(id).await?;
        if b.status == BackupStatus::Creating {
            return Err(CoreError::new(
                ErrorCode::ServerBusy,
                "This backup is still being created; cancel its job first",
            ));
        }
        match tokio::fs::remove_file(&b.path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(CoreError::io("Cannot delete the backup file", &e)),
        }
        if let Some(parent) = b.path.parent()
            && let Ok(base) = self.backups_dir().await
            && parent != base
        {
            let _ = tokio::fs::remove_dir(parent).await;
        }
        self.repo.delete(id).await?;
        self.changed(b.server_id, id);
        self.audit
            .record(
                actor,
                "backup.delete",
                b.server_id,
                Some(id.to_string()),
                AuditResult::Success,
                serde_json::json!({ "kind": b.kind.as_str() }),
            )
            .await;
        Ok(())
    }

    fn ready(b: &BackupRecord) -> CoreResult<()> {
        if b.status != BackupStatus::Ready {
            return Err(CoreError::invalid("This backup did not complete"));
        }
        if !b.path.is_file() {
            return Err(CoreError::new(
                ErrorCode::PathNotFound,
                "The backup file is missing",
            ));
        }
        Ok(())
    }

    /// Compare the archive with the hash recorded when it was written.
    async fn check_file_hash(&self, b: &BackupRecord) -> CoreResult<()> {
        let path = b.path.clone();
        let (_, sha) = tokio::task::spawn_blocking(move || archive::hash_file(&path))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        if b.sha256.as_deref() != Some(sha.as_str()) {
            return Err(CoreError::new(
                ErrorCode::HashMismatch,
                "The backup file has changed since it was created",
            ));
        }
        Ok(())
    }

    /// Start a job that reads the whole archive and checks every file.
    pub async fn verify(self: &Arc<Self>, id: BackupId, actor: &str) -> CoreResult<JobId> {
        let b = self.get(id).await?;
        Self::ready(&b)?;
        let this = Arc::clone(self);
        let actor = actor.to_string();
        self.jobs
            .spawn("backup.verify", b.server_id, move |ctx| async move {
                let report = match this.check_file_hash(&b).await {
                    Err(e) if e.code == ErrorCode::HashMismatch => VerifyReport {
                        ok: false,
                        problems: vec![e.message],
                        ..Default::default()
                    },
                    Err(e) => return Err(e),
                    Ok(()) => {
                        let plain = match this.plain_archive(&b, Some(&ctx)).await {
                            Ok(p) => p,
                            Err(e) if e.code == ErrorCode::ArchiveRejected => {
                                return Ok(Some(
                                    serde_json::to_value(VerifyReport {
                                        ok: false,
                                        problems: vec![e.message],
                                        ..Default::default()
                                    })
                                    .map_err(|e| CoreError::internal(e.to_string()))?,
                                ));
                            }
                            Err(e) => return Err(e),
                        };
                        let path = plain.path.clone();
                        let total = b.content_bytes;
                        let job = ctx.clone();
                        let r = tokio::task::spawn_blocking(move || {
                            let mut done = 0u64;
                            archive::verify_archive(&path, &|| job.is_cancelled(), &mut |n| {
                                done += n;
                                job.progress(Some(done as f32 / total.max(1) as f32), "Verifying…");
                            })
                        })
                        .await
                        .map_err(|e| CoreError::internal(e.to_string()))??;
                        drop(plain);
                        r
                    }
                };
                this.audit
                    .record(
                        &actor,
                        "backup.verify",
                        b.server_id,
                        Some(b.id.to_string()),
                        if report.ok {
                            AuditResult::Success
                        } else {
                            AuditResult::Failure
                        },
                        serde_json::json!({ "problems": report.problems.len() }),
                    )
                    .await;
                Ok(Some(
                    serde_json::to_value(&report)
                        .map_err(|e| CoreError::internal(e.to_string()))?,
                ))
            })
            .await
    }

    // ───────────────────────────── restore ─────────────────────────────

    async fn restore_server(&self, b: &BackupRecord) -> CoreResult<Server> {
        Self::ready(b)?;
        let server_id = b.server_id.ok_or_else(|| {
            CoreError::new(
                ErrorCode::ServerNotFound,
                "The server of this backup no longer exists",
            )
        })?;
        self.servers.get(server_id).await
    }

    async fn restore_target(
        &self,
        b: &BackupRecord,
        plain: &Path,
    ) -> CoreResult<(Server, BackupManifest)> {
        let server = self.restore_server(b).await?;
        let path = plain.to_path_buf();
        let manifest = tokio::task::spawn_blocking(move || archive::read_manifest(&path))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        if manifest.server.id != server.id || manifest.backup_id != b.id {
            return Err(CoreError::new(
                ErrorCode::ArchiveRejected,
                "The backup file does not belong to this server",
            ));
        }
        Ok((server, manifest))
    }

    pub async fn restore_preview(&self, id: BackupId) -> CoreResult<RestorePreview> {
        let b = self.get(id).await?;
        self.restore_server(&b).await?;
        let plain = self.plain_archive(&b, None).await?;
        let (server, manifest) = self.restore_target(&b, &plain.path).await?;
        drop(plain);
        tokio::task::spawn_blocking(move || restore::preview(&server.directory, &manifest))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))?
    }

    /// Start a restore job: pre-restore backup, verified staged extraction, swap.
    pub async fn restore(self: &Arc<Self>, id: BackupId, actor: &str) -> CoreResult<JobId> {
        let b = self.get(id).await?;
        let server = if b.encrypted {
            // The key must be here; the archive itself is checked inside the job.
            self.encryption.identity()?;
            self.restore_server(&b).await?
        } else {
            self.restore_target(&b, &b.path.clone()).await?.0
        };
        let rt = self.servers.runtime(server.id);
        if rt.state().has_process() {
            return Err(CoreError::new(
                ErrorCode::ServerBusy,
                "Stop the server before restoring a backup",
            ));
        }
        // Take the lock now so nothing starts the server before the job runs.
        let guard = rt.begin(Operation::Restoring)?;
        let this = Arc::clone(self);
        let actor = actor.to_string();
        self.jobs
            .spawn("backup.restore", Some(server.id), move |ctx| async move {
                let result = this.run_restore(&b, &server, &rt, guard, &ctx).await;
                this.audit_result(&actor, "backup.restore", server.id, &result)
                    .await;
                result.map(|pre| Some(serde_json::json!({ "preRestoreBackupId": pre })))
            })
            .await
    }

    async fn run_restore(
        self: &Arc<Self>,
        b: &BackupRecord,
        server: &Server,
        rt: &Arc<ServerRuntime>,
        _guard: OperationGuard,
        ctx: &JobContext,
    ) -> CoreResult<BackupId> {
        ctx.progress(None, "Checking the backup…");
        self.check_file_hash(b).await?;
        let plain = self.plain_archive(b, Some(ctx)).await?;
        let (_, manifest) = self.restore_target(b, &plain.path).await?;

        ctx.progress(None, "Backing up the current state…");
        let when = jiff::Timestamp::from_millisecond(b.created_at.millis())
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_zoned(self.tz.clone())
            .strftime("%Y-%m-%d %H:%M")
            .to_string();
        let pre = self
            .run_backup(
                server,
                rt,
                BackupKind::PreRestore,
                Some(format!("Before restoring the backup from {when}")),
                true,
                ctx,
            )
            .await?;

        rt.console.push(
            ConsoleStream::System,
            &format!("Restoring the backup from {when}…"),
        );
        let root = server.directory.clone();
        let archive_path = plain.path.clone();
        let work = restore::work_dir(&root, &ctx.id.to_string());
        let total = b.content_bytes;
        let job = ctx.clone();
        let restored = tokio::task::spawn_blocking(move || {
            let mut done = 0u64;
            restore::restore_into(
                &root,
                &archive_path,
                &work,
                &|| job.is_cancelled(),
                &mut |n| {
                    done += n;
                    job.progress(Some(done as f32 / total.max(1) as f32), "Restoring files…");
                },
            )
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))?;
        drop(plain);
        let files = match restored {
            Ok(n) => n,
            Err(e) => {
                rt.console.push(
                    ConsoleStream::System,
                    &format!(
                        "Restore failed; the server files were left unchanged: {}",
                        e.message
                    ),
                );
                return Err(e);
            }
        };

        // The restored files carry the software of the backup.
        if server.software != manifest.server.software {
            let mut updated = server.clone();
            updated.software = manifest.server.software.clone();
            updated.updated_at = Timestamp::now();
            self.servers.repo.update(&updated).await?;
            self.events.publish(DomainEvent::ServerUpdated {
                server_id: server.id,
            });
        }
        rt.console.push(
            ConsoleStream::System,
            &format!("Restore complete ({files} files)."),
        );
        Ok(pre.id)
    }

    // ───────────────────────────── schedules ─────────────────────────────

    pub async fn policy(&self, server_id: ServerId) -> CoreResult<BackupPolicy> {
        self.servers.get(server_id).await?;
        Ok(self
            .repo
            .policy(server_id)
            .await?
            .unwrap_or_else(|| BackupPolicy::default_for(server_id)))
    }

    pub async fn set_policy(
        &self,
        mut policy: BackupPolicy,
        actor: &str,
    ) -> CoreResult<BackupPolicy> {
        self.servers.get(policy.server_id).await?;
        if !(MIN_INTERVAL_MINUTES..=MAX_INTERVAL_MINUTES).contains(&policy.interval_minutes) {
            return Err(CoreError::invalid(format!(
                "The interval must be between {MIN_INTERVAL_MINUTES} minutes and 7 days"
            )));
        }
        let r = policy.retention;
        if [r.keep_last, r.keep_daily, r.keep_weekly, r.keep_monthly]
            .iter()
            .any(|n| *n > 1000)
        {
            return Err(CoreError::invalid("Retention counts must be at most 1000"));
        }
        if r.keep_last == 0 {
            return Err(CoreError::invalid(
                "Keep at least the newest scheduled backup",
            ));
        }
        let previous = self.repo.policy(policy.server_id).await?;
        // The schedule starts counting when it is enabled.
        policy.last_run_at = match previous {
            Some(p) if p.enabled && policy.enabled => p.last_run_at,
            _ if policy.enabled => Some(Timestamp::now()),
            Some(p) => p.last_run_at,
            None => None,
        };
        self.repo.save_policy(&policy).await?;
        self.audit
            .record(
                actor,
                "backup.policy",
                Some(policy.server_id),
                None,
                AuditResult::Success,
                serde_json::json!({ "enabled": policy.enabled, "intervalMinutes": policy.interval_minutes }),
            )
            .await;
        Ok(policy)
    }

    pub fn spawn_scheduler(self: &Arc<Self>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(SCHEDULER_TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                if let Err(e) = this.tick(Timestamp::now()).await {
                    tracing::warn!(target: "mcpanel::backup", "backup scheduler: {}", e.message);
                }
            }
        });
    }

    /// Whether the server has not run since its newest completed backup.
    async fn is_idle(&self, server_id: ServerId, rt: &ServerRuntime) -> CoreResult<bool> {
        let (state, last_exit) = {
            let i = rt.lock();
            (i.state, i.last_exit_at)
        };
        if state.has_process() {
            return Ok(false);
        }
        let newest = self
            .repo
            .list(Some(server_id))
            .await?
            .into_iter()
            .filter(|b| b.status == BackupStatus::Ready)
            .map(|b| b.created_at)
            .max();
        Ok(newest.is_some_and(|t| t >= last_exit.unwrap_or(self.session_started)))
    }

    /// Run the backups that are due at `now`. Returns the jobs started.
    pub async fn tick(self: &Arc<Self>, now: Timestamp) -> CoreResult<Vec<JobId>> {
        let mut started = Vec::new();
        for mut policy in self.repo.policies().await? {
            if !policy.enabled {
                continue;
            }
            let interval = policy.interval_minutes as i64 * 60_000;
            if policy
                .last_run_at
                .is_some_and(|t| now.millis() - t.millis() < interval)
            {
                continue;
            }
            let Ok(server) = self.servers.get(policy.server_id).await else {
                continue;
            };
            let rt = self.servers.runtime(server.id);
            if policy.skip_if_idle && self.is_idle(server.id, &rt).await? {
                policy.last_run_at = Some(now);
                self.repo.save_policy(&policy).await?;
                tracing::info!(target: "mcpanel::backup", server = %server.id, "scheduled backup skipped: the server has not run since the last backup");
                continue;
            }
            // Not possible right now (starting, stopping, busy…): retry on a later tick.
            let snap = rt.snapshot();
            if backup_mode(snap.state, snap.console_attached).is_err() {
                continue;
            }
            let Ok(guard) = rt.begin(Operation::BackingUp) else {
                continue;
            };
            policy.last_run_at = Some(now);
            self.repo.save_policy(&policy).await?;
            let this = Arc::clone(self);
            let retention = policy.retention;
            let job = self
                .jobs
                .spawn("backup.scheduled", Some(server.id), move |ctx| async move {
                    let result = this
                        .run_backup(&server, &rt, BackupKind::Scheduled, None, false, &ctx)
                        .await;
                    drop(guard);
                    this.audit_result("scheduler", "backup.create", server.id, &result)
                        .await;
                    let b = result?;
                    let removed = this.apply_retention(server.id, retention).await?;
                    Ok(Some(
                        serde_json::json!({ "backupId": b.id, "removed": removed }),
                    ))
                })
                .await?;
            started.push(job);
        }
        Ok(started)
    }

    /// Delete expired scheduled backups of a server. Returns how many were removed.
    pub async fn apply_retention(
        &self,
        server_id: ServerId,
        r: super::Retention,
    ) -> CoreResult<u64> {
        let candidates: Vec<Candidate> = self
            .repo
            .list(Some(server_id))
            .await?
            .into_iter()
            .filter(|b| {
                b.kind == BackupKind::Scheduled && b.status == BackupStatus::Ready && !b.protected
            })
            .map(|b| Candidate {
                id: b.id,
                created_at: b.created_at,
                size_bytes: b.size_bytes,
            })
            .collect();
        let mut doomed = retention::expired(&candidates, r, &self.tz);
        let cap_gb = self
            .policy(server_id)
            .await
            .map(|p| p.max_total_gb)
            .unwrap_or(0);
        if cap_gb > 0 {
            let left: Vec<Candidate> = candidates
                .iter()
                .filter(|c| !doomed.contains(&c.id))
                .copied()
                .collect();
            doomed.extend(retention::over_cap(&left, u64::from(cap_gb) << 30));
        }
        let mut removed = 0;
        for id in doomed {
            match self.delete(id, "retention").await {
                Ok(()) => removed += 1,
                Err(e) => {
                    tracing::warn!(target: "mcpanel::backup", backup = %id, "retention could not delete a backup: {}", e.message)
                }
            }
        }
        Ok(removed)
    }
}
