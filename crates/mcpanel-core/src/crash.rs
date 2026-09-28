//! Crash handling and the automatic restart policy (spec §5).
//!
//! On a crash MCPanel records the exit code, a console tail and the newest
//! `crash-reports/` file, classifies the crash, and — if the server's restart policy
//! allows — restarts it after a delay with exponential backoff. At most
//! `max_attempts` restarts happen within `window_secs`; a server that ran for
//! `stable_secs` before crashing starts counting from one again. Crashes that a restart
//! cannot fix (wrong Java, EULA, port in use, missing jar, invalid memory) end in the
//! `Error` state and are never restarted.

use crate::audit::AuditLog;
use crate::backup::{BackupService, CreateBackupRequest};
use crate::console::ConsoleStream;
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::ids::ServerId;
use crate::lifecycle::LifecycleState;
use crate::model::AuditResult;
use crate::ports::CrashRepository;
use crate::server::ServerManager;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

/// Longest delay between restart attempts.
const MAX_DELAY_SECS: u64 = 300;
const CONSOLE_TAIL_LINES: usize = 60;
/// How long an automatic restart waits for a busy server (backup, plugin changes).
const BUSY_RETRY_FOR: Duration = Duration::from_secs(30 * 60);
const BUSY_RETRY_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub server_id: ServerId,
    pub enabled: bool,
    pub max_attempts: u32,
    pub window_secs: u32,
    /// Delay before the first restart; doubled for each further attempt.
    pub delay_secs: u32,
    /// Uptime after which the attempt counter starts again.
    pub stable_secs: u32,
    /// Take a backup before restarting.
    pub crash_backup: bool,
}

impl RestartPolicy {
    pub fn default_for(server_id: ServerId) -> Self {
        Self {
            server_id,
            enabled: true,
            max_attempts: 3,
            window_secs: 600,
            delay_secs: 10,
            stable_secs: 300,
            crash_backup: false,
        }
    }

    pub fn validate(&self) -> CoreResult<()> {
        let ok = (1..=10).contains(&self.max_attempts)
            && (60..=86_400).contains(&self.window_secs)
            && self.delay_secs <= 600
            && (10..=86_400).contains(&self.stable_secs);
        if ok {
            Ok(())
        } else {
            Err(CoreError::invalid(
                "Restart policy: 1–10 attempts, a window of 1 minute to 1 day, a delay of at most 10 minutes and a stable time of at least 10 seconds",
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashAction {
    /// A restart was scheduled.
    Restart,
    /// The attempt limit was reached; the server stays stopped.
    GaveUp,
    /// The cause cannot be fixed by restarting (e.g. wrong Java).
    NotRestartable,
    /// Automatic restarts are turned off for this server.
    Disabled,
}

impl CrashAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::GaveUp => "gave_up",
            Self::NotRestartable => "not_restartable",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "restart" => Self::Restart,
            "gave_up" => Self::GaveUp,
            "not_restartable" => Self::NotRestartable,
            _ => Self::Disabled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashEvent {
    pub id: String,
    pub server_id: ServerId,
    pub occurred_at: Timestamp,
    pub exit_code: Option<i32>,
    /// e.g. `out_of_memory`, `watchdog`, `crash_report`, `unknown`.
    pub kind: String,
    pub message: String,
    /// Which consecutive crash this is (1 = first since the server was stable).
    pub attempt: u32,
    pub action: CrashAction,
    /// When the restart is due (`Restart` only).
    pub restart_at: Option<Timestamp>,
    /// Relative path of the crash report the server wrote, if any.
    pub crash_report: Option<String>,
    pub console_tail: Vec<String>,
}

/// What to do about a crash, given the policy and the previous crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub attempt: u32,
    pub action: CrashAction,
    pub delay_secs: u64,
}

/// Pure restart decision (testable without processes).
pub fn decide(
    policy: &RestartPolicy,
    restartable: bool,
    now: Timestamp,
    ready_at: Option<Timestamp>,
    previous: Option<&CrashEvent>,
) -> Decision {
    let stable =
        ready_at.is_some_and(|r| now.millis() - r.millis() >= policy.stable_secs as i64 * 1000);
    let in_window = previous
        .filter(|p| now.millis() - p.occurred_at.millis() <= policy.window_secs as i64 * 1000);
    let attempt = match in_window {
        Some(p) if !stable && p.action == CrashAction::Restart => p.attempt + 1,
        Some(p) if !stable && p.action == CrashAction::GaveUp => p.attempt + 1,
        _ => 1,
    };
    let action = if !restartable {
        CrashAction::NotRestartable
    } else if !policy.enabled {
        CrashAction::Disabled
    } else if attempt > policy.max_attempts {
        CrashAction::GaveUp
    } else {
        CrashAction::Restart
    };
    let delay_secs = (policy.delay_secs as u64)
        .saturating_mul(1u64 << (attempt.saturating_sub(1)).min(10))
        .min(MAX_DELAY_SECS);
    Decision {
        attempt,
        action,
        delay_secs,
    }
}

/// The newest crash report written since `since`, with its description line.
pub fn find_crash_report(
    root: &Path,
    since: Option<Timestamp>,
) -> Option<(String, Option<String>)> {
    let dir = root.join("crash-reports");
    let newest = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let md = std::fs::symlink_metadata(e.path()).ok()?;
            let modified = Timestamp::from_system_time(md.modified().ok()?)?;
            let name = e.file_name().to_string_lossy().to_string();
            (md.is_file() && name.ends_with(".txt") && since.is_none_or(|s| modified >= s))
                .then_some((modified, name))
        })
        .max()?;
    let text = std::fs::read(dir.join(&newest.1)).ok()?;
    let head = String::from_utf8_lossy(&text[..text.len().min(64 * 1024)]).to_string();
    let description = head
        .lines()
        .find_map(|l| l.strip_prefix("Description: "))
        .map(|d| d.trim().to_string());
    Some((format!("crash-reports/{}", newest.1), description))
}

pub struct CrashService {
    servers: Arc<ServerManager>,
    repo: Arc<dyn CrashRepository>,
    backups: Arc<BackupService>,
    audit: Arc<AuditLog>,
    events: EventBus,
}

impl CrashService {
    pub fn new(
        servers: Arc<ServerManager>,
        repo: Arc<dyn CrashRepository>,
        backups: Arc<BackupService>,
        audit: Arc<AuditLog>,
        events: EventBus,
    ) -> Arc<Self> {
        Arc::new(Self {
            servers,
            repo,
            backups,
            audit,
            events,
        })
    }

    pub async fn policy(&self, server_id: ServerId) -> CoreResult<RestartPolicy> {
        self.servers.get(server_id).await?;
        Ok(self
            .repo
            .policy(server_id)
            .await?
            .unwrap_or_else(|| RestartPolicy::default_for(server_id)))
    }

    pub async fn set_policy(
        &self,
        policy: RestartPolicy,
        actor: &str,
    ) -> CoreResult<RestartPolicy> {
        self.servers.get(policy.server_id).await?;
        policy.validate()?;
        self.repo.save_policy(&policy).await?;
        self.audit
            .record(
                actor,
                "server.restart_policy",
                Some(policy.server_id),
                None,
                AuditResult::Success,
                serde_json::json!({ "enabled": policy.enabled, "maxAttempts": policy.max_attempts }),
            )
            .await;
        Ok(policy)
    }

    pub async fn history(&self, server_id: ServerId, limit: u32) -> CoreResult<Vec<CrashEvent>> {
        self.servers.get(server_id).await?;
        self.repo.recent(server_id, limit.clamp(1, 100)).await
    }

    pub fn spawn_listener(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let mut rx = self.events.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(env) => {
                        if let DomainEvent::ServerCrashed {
                            server_id,
                            exit_code,
                            ..
                        } = env.event
                        {
                            let this = Arc::clone(&this);
                            tokio::spawn(async move {
                                if let Err(e) =
                                    this.handle_crash(server_id, exit_code, env.at).await
                                {
                                    tracing::warn!(target: "mcpanel::server", server = %server_id, "crash handling failed: {}", e.message);
                                }
                            });
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(target: "mcpanel::server", n, "missed events; a crash may not have been handled");
                    }
                    Err(RecvError::Closed) => return,
                }
            }
        });
    }

    async fn handle_crash(
        &self,
        server_id: ServerId,
        exit_code: Option<i32>,
        at: Timestamp,
    ) -> CoreResult<()> {
        let server = self.servers.get(server_id).await?;
        let rt = self.servers.runtime(server_id);
        let snap = rt.snapshot();
        let restartable = snap.state == LifecycleState::Crashed;
        let root = server.directory.clone();
        let since = snap.started_at;
        let report = tokio::task::spawn_blocking(move || find_crash_report(&root, since))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))?;
        let (kind, message) = match (&snap.diagnosis, &report) {
            (Some(d), _) => (d.kind.clone(), d.message.clone()),
            (None, Some((_, Some(desc)))) => ("crash_report".to_string(), desc.clone()),
            (None, Some((_, None))) => (
                "crash_report".to_string(),
                "The server wrote a crash report".to_string(),
            ),
            (None, None) => (
                "unknown".to_string(),
                format!(
                    "The server exited unexpectedly (exit code {})",
                    exit_code.map_or("unknown".to_string(), |c| c.to_string())
                ),
            ),
        };
        let policy = self.policy(server_id).await?;
        let previous = self.repo.recent(server_id, 1).await?.into_iter().next();
        let d = decide(&policy, restartable, at, snap.ready_at, previous.as_ref());
        let tail = rt
            .console
            .snapshot(None, CONSOLE_TAIL_LINES)
            .into_iter()
            .map(|l| l.text)
            .collect();
        let event = CrashEvent {
            id: uuid::Uuid::new_v4().simple().to_string(),
            server_id,
            occurred_at: at,
            exit_code,
            kind,
            message,
            attempt: d.attempt,
            action: d.action,
            restart_at: (d.action == CrashAction::Restart)
                .then(|| Timestamp(at.millis() + d.delay_secs as i64 * 1000)),
            crash_report: report.map(|(p, _)| p),
            console_tail: tail,
        };
        self.repo.insert(&event).await?;
        self.events.publish(DomainEvent::CrashRecorded {
            server_id,
            action: d.action.as_str().into(),
        });

        let note = match d.action {
            CrashAction::Restart => format!(
                "Automatic restart {} of {} in {} s…",
                d.attempt, policy.max_attempts, d.delay_secs
            ),
            CrashAction::GaveUp => format!(
                "The server crashed {} times within {} minutes; automatic restarts stopped. Check the console and crash report.",
                d.attempt,
                policy.window_secs / 60
            ),
            CrashAction::NotRestartable => {
                "This problem cannot be fixed by restarting; the server was not restarted.".into()
            }
            CrashAction::Disabled => "Automatic restarts are off for this server.".into(),
        };
        rt.console.push(ConsoleStream::System, &note);
        if d.action != CrashAction::Restart {
            return Ok(());
        }

        if policy.crash_backup {
            match self
                .backups
                .create(
                    CreateBackupRequest {
                        server_id,
                        note: Some("Taken automatically after a crash".into()),
                    },
                    "auto-restart",
                )
                .await
            {
                Ok(job) => {
                    // Wait for the backup so the restart does not race it.
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(1800);
                    while tokio::time::Instant::now() < deadline {
                        match self.servers.jobs.get(job).await? {
                            Some(j) if !j.status.is_terminal() => {
                                tokio::time::sleep(Duration::from_millis(500)).await
                            }
                            _ => break,
                        }
                    }
                }
                Err(e) => {
                    rt.console.push(
                        ConsoleStream::System,
                        &format!("Crash backup skipped: {}", e.message),
                    );
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(d.delay_secs)).await;
        // Only restart if nothing else happened meanwhile (manual start, deletion, …).
        if self.servers.runtime(server_id).state() != LifecycleState::Crashed
            || self.servers.get(server_id).await.is_err()
        {
            return Ok(());
        }
        // Another operation (a backup, applying queued plugin changes) may briefly hold
        // the server; wait for it instead of giving up.
        let deadline = tokio::time::Instant::now() + BUSY_RETRY_FOR;
        loop {
            match self.servers.start(server_id, "auto-restart").await {
                Ok(()) => return Ok(()),
                Err(e)
                    if e.code == ErrorCode::ServerBusy
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(BUSY_RETRY_EVERY).await;
                    if self.servers.runtime(server_id).state() != LifecycleState::Crashed {
                        return Ok(());
                    }
                }
                Err(e) => {
                    rt.console.push(
                        ConsoleStream::System,
                        &format!("Automatic restart failed: {}", e.message),
                    );
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(at: i64, attempt: u32, action: CrashAction) -> CrashEvent {
        CrashEvent {
            id: "x".into(),
            server_id: ServerId::new(),
            occurred_at: Timestamp(at),
            exit_code: Some(1),
            kind: "unknown".into(),
            message: String::new(),
            attempt,
            action,
            restart_at: None,
            crash_report: None,
            console_tail: vec![],
        }
    }

    #[test]
    fn attempts_count_up_within_the_window_with_backoff() {
        let p = RestartPolicy::default_for(ServerId::new());
        let first = decide(&p, true, Timestamp(100_000), Some(Timestamp(90_000)), None);
        assert_eq!(
            (first.attempt, first.action, first.delay_secs),
            (1, CrashAction::Restart, 10)
        );
        let prev = ev(100_000, 1, CrashAction::Restart);
        let second = decide(
            &p,
            true,
            Timestamp(130_000),
            Some(Timestamp(125_000)),
            Some(&prev),
        );
        assert_eq!((second.attempt, second.delay_secs), (2, 20));
        let third = decide(
            &p,
            true,
            Timestamp(160_000),
            None,
            Some(&ev(130_000, 2, CrashAction::Restart)),
        );
        assert_eq!((third.attempt, third.delay_secs), (3, 40));
        let fourth = decide(
            &p,
            true,
            Timestamp(190_000),
            None,
            Some(&ev(160_000, 3, CrashAction::Restart)),
        );
        assert_eq!((fourth.attempt, fourth.action), (4, CrashAction::GaveUp));
    }

    #[test]
    fn stable_uptime_or_an_old_crash_resets_the_counter() {
        let p = RestartPolicy::default_for(ServerId::new());
        let prev = ev(0, 3, CrashAction::Restart);
        // Ran for 5 minutes before crashing again.
        let d = decide(
            &p,
            true,
            Timestamp(400_000),
            Some(Timestamp(100_000)),
            Some(&prev),
        );
        assert_eq!(d.attempt, 1);
        // The previous crash is outside the 10-minute window.
        let d = decide(
            &p,
            true,
            Timestamp(700_000),
            Some(Timestamp(690_000)),
            Some(&prev),
        );
        assert_eq!(d.attempt, 1);
    }

    #[test]
    fn unfixable_or_disabled_never_restart() {
        let mut p = RestartPolicy::default_for(ServerId::new());
        assert_eq!(
            decide(&p, false, Timestamp(1), None, None).action,
            CrashAction::NotRestartable
        );
        p.enabled = false;
        assert_eq!(
            decide(&p, true, Timestamp(1), None, None).action,
            CrashAction::Disabled
        );
    }

    #[test]
    fn delay_is_capped() {
        let mut p = RestartPolicy::default_for(ServerId::new());
        p.delay_secs = 200;
        p.max_attempts = 10;
        let d = decide(
            &p,
            true,
            Timestamp(10),
            None,
            Some(&ev(0, 5, CrashAction::Restart)),
        );
        assert_eq!(d.delay_secs, MAX_DELAY_SECS);
    }

    #[test]
    fn crash_report_description_is_read() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("crash-reports")).unwrap();
        std::fs::write(
            d.path().join("crash-reports/crash-2026-09-28_19.00.00-server.txt"),
            "---- Minecraft Crash Report ----\n// Oops.\n\nTime: 2026-09-28\nDescription: Exception in server tick loop\n\njava.lang.IllegalStateException\n",
        )
        .unwrap();
        let (path, desc) = find_crash_report(d.path(), None).unwrap();
        assert_eq!(path, "crash-reports/crash-2026-09-28_19.00.00-server.txt");
        assert_eq!(desc.as_deref(), Some("Exception in server tick loop"));
        assert!(
            find_crash_report(d.path(), Some(Timestamp(i64::MAX / 2))).is_none(),
            "older reports are ignored"
        );
    }
}
