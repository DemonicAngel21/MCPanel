//! Notifications: domain events become inbox entries and/or desktop notifications,
//! according to per-category rules the user chooses. The desktop notification itself
//! is shown by the host (it owns the OS integration) from `NotificationCreated`.

use crate::crash::{CrashAction, CrashService};
use crate::error::{CoreError, CoreResult};
use crate::events::{DomainEvent, EventBus};
use crate::ids::ServerId;
use crate::jobs::JobStatus;
use crate::ports::{NotificationRepository, SettingsRepository};
use crate::server::ServerManager;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

/// Entries kept in the inbox (oldest are removed first).
const KEEP: u32 = 500;
const PREFS_KEY: &str = "notifications";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "success" => Self::Success,
            "warning" => Self::Warning,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// A server crashed (with what the restart policy did).
    Crash,
    BackupFailed,
    /// Any other background task failed (install, restore, setup…).
    TaskFailed,
    PlayerJoined,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crash => "crash",
            Self::BackupFailed => "backup_failed",
            Self::TaskFailed => "task_failed",
            Self::PlayerJoined => "player_joined",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "crash" => Some(Self::Crash),
            "backup_failed" => Some(Self::BackupFailed),
            "task_failed" => Some(Self::TaskFailed),
            "player_joined" => Some(Self::PlayerJoined),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub created_at: Timestamp,
    pub server_id: Option<ServerId>,
    pub category: Category,
    pub severity: Severity,
    pub title: String,
    pub body: String,
    pub read: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channels {
    pub inbox: bool,
    pub desktop: bool,
}

/// What to do per category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationPrefs {
    pub crash: Channels,
    pub backup_failed: Channels,
    pub task_failed: Channels,
    pub player_joined: Channels,
}

impl Default for NotificationPrefs {
    fn default() -> Self {
        let on = Channels {
            inbox: true,
            desktop: true,
        };
        Self {
            crash: on,
            backup_failed: on,
            // Failed tasks already show an error toast while MCPanel is open.
            task_failed: Channels {
                inbox: true,
                desktop: false,
            },
            player_joined: Channels {
                inbox: false,
                desktop: false,
            },
        }
    }
}

impl NotificationPrefs {
    pub fn channels(&self, c: Category) -> Channels {
        match c {
            Category::Crash => self.crash,
            Category::BackupFailed => self.backup_failed,
            Category::TaskFailed => self.task_failed,
            Category::PlayerJoined => self.player_joined,
        }
    }
}

/// A notification before it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub server_id: Option<ServerId>,
    pub category: Category,
    pub severity: Severity,
    pub title: String,
    pub body: String,
}

fn task_label(kind: &str) -> &'static str {
    match kind {
        "server.create" => "Creating the server",
        "server.import" => "Importing the server",
        "content.install" => "Installing a plugin/mod",
        "backup.verify" => "Verifying a backup",
        "backup.restore" => "Restoring a backup",
        "bedrock.enable" => "Setting up Bedrock crossplay",
        _ => "A background task",
    }
}

/// The restart policy's decision in words.
pub fn crash_body(message: &str, action: CrashAction, delay: Option<i64>) -> String {
    let what = match action {
        CrashAction::Restart => match delay {
            Some(s) if s > 0 => format!("MCPanel restarts it in {s} s."),
            _ => "MCPanel restarts it.".into(),
        },
        CrashAction::GaveUp => "It crashed repeatedly; automatic restart gave up.".into(),
        CrashAction::NotRestartable => {
            "A restart would not fix this, so it was not restarted.".into()
        }
        CrashAction::Disabled => "Automatic restart is off for this server.".into(),
    };
    let message = message.trim();
    if message.is_empty() {
        what
    } else if message.ends_with(['.', '!', '?']) {
        format!("{message} {what}")
    } else {
        format!("{message}. {what}")
    }
}

pub struct NotificationService {
    repo: Arc<dyn NotificationRepository>,
    settings: Arc<dyn SettingsRepository>,
    servers: Arc<ServerManager>,
    crashes: Arc<CrashService>,
    events: EventBus,
}

impl NotificationService {
    pub fn new(
        repo: Arc<dyn NotificationRepository>,
        settings: Arc<dyn SettingsRepository>,
        servers: Arc<ServerManager>,
        crashes: Arc<CrashService>,
        events: EventBus,
    ) -> Arc<Self> {
        Arc::new(Self {
            repo,
            settings,
            servers,
            crashes,
            events,
        })
    }

    pub async fn prefs(&self) -> CoreResult<NotificationPrefs> {
        Ok(self
            .settings
            .get(PREFS_KEY)
            .await?
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default())
    }

    pub async fn set_prefs(&self, prefs: NotificationPrefs) -> CoreResult<NotificationPrefs> {
        let v = serde_json::to_value(prefs).map_err(|e| CoreError::internal(e.to_string()))?;
        self.settings.set(PREFS_KEY, &v).await?;
        self.events.publish(DomainEvent::SettingsChanged {
            key: PREFS_KEY.into(),
        });
        Ok(prefs)
    }

    pub async fn list(&self, limit: u32) -> CoreResult<Vec<Notification>> {
        self.repo.list(limit.clamp(1, KEEP)).await
    }

    pub async fn unread_count(&self) -> CoreResult<u32> {
        self.repo.unread_count().await
    }

    /// Mark the given notifications read (`None` = all).
    pub async fn mark_read(&self, ids: Option<Vec<String>>) -> CoreResult<()> {
        self.repo.mark_read(ids.as_deref()).await?;
        self.events.publish(DomainEvent::NotificationsChanged);
        Ok(())
    }

    pub async fn clear(&self) -> CoreResult<()> {
        self.repo.clear().await?;
        self.events.publish(DomainEvent::NotificationsChanged);
        Ok(())
    }

    async fn server_name(&self, id: ServerId) -> String {
        self.servers
            .get(id)
            .await
            .map_or_else(|_| "A server".into(), |s| s.name)
    }

    /// The notification an event should produce, if any.
    async fn draft(&self, event: &DomainEvent) -> Option<Draft> {
        match event {
            DomainEvent::CrashRecorded { server_id, .. } => {
                let name = self.server_name(*server_id).await;
                let last = self.crashes.history(*server_id, 1).await.ok()?.pop()?;
                let delay = last
                    .restart_at
                    .map(|t| (t.millis() - last.occurred_at.millis()) / 1000);
                Some(Draft {
                    server_id: Some(*server_id),
                    category: Category::Crash,
                    severity: Severity::Error,
                    title: format!("{name} crashed"),
                    body: crash_body(&last.message, last.action, delay),
                })
            }
            DomainEvent::JobUpdated {
                kind,
                server_id,
                status: JobStatus::Failed,
                message,
                ..
            } => {
                let name = match server_id {
                    Some(id) => Some(self.server_name(*id).await),
                    None => None,
                };
                let backup = kind == "backup.create" || kind == "backup.scheduled";
                let title = if backup {
                    match &name {
                        Some(n) => format!("Backup of {n} failed"),
                        None => "A backup failed".into(),
                    }
                } else {
                    let label = task_label(kind);
                    match &name {
                        Some(n) => format!("{label} failed ({n})"),
                        None => format!("{label} failed"),
                    }
                };
                Some(Draft {
                    server_id: *server_id,
                    category: if backup {
                        Category::BackupFailed
                    } else {
                        Category::TaskFailed
                    },
                    severity: Severity::Error,
                    title,
                    body: message.clone().unwrap_or_default(),
                })
            }
            DomainEvent::PlayerJoined {
                server_id,
                player_name,
            } => {
                let name = self.server_name(*server_id).await;
                Some(Draft {
                    server_id: Some(*server_id),
                    category: Category::PlayerJoined,
                    severity: Severity::Info,
                    title: format!("{player_name} joined {name}"),
                    body: String::new(),
                })
            }
            _ => None,
        }
    }

    /// Store and/or announce a draft according to the rules.
    pub async fn deliver(&self, d: Draft) -> CoreResult<()> {
        let channels = self.prefs().await?.channels(d.category);
        if !channels.inbox && !channels.desktop {
            return Ok(());
        }
        let n = Notification {
            id: uuid::Uuid::new_v4().simple().to_string(),
            created_at: Timestamp::now(),
            server_id: d.server_id,
            category: d.category,
            severity: d.severity,
            title: d.title,
            body: d.body,
            read: false,
        };
        if channels.inbox {
            self.repo.insert(&n).await?;
            self.repo.prune(KEEP).await?;
        }
        self.events.publish(DomainEvent::NotificationCreated {
            id: n.id,
            server_id: n.server_id,
            severity: n.severity.as_str().into(),
            title: n.title,
            body: n.body,
            desktop: channels.desktop,
            inbox: channels.inbox,
        });
        Ok(())
    }

    pub fn spawn_listener(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let mut rx = self.events.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(env) => {
                        if let Some(d) = this.draft(&env.event).await
                            && let Err(e) = this.deliver(d).await
                        {
                            tracing::warn!(target: "mcpanel::notify", "cannot deliver notification: {}", e.message);
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(target: "mcpanel::notify", "missed {n} events");
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_bodies_say_what_happened() {
        assert_eq!(
            crash_body("Out of memory.", CrashAction::Restart, Some(10)),
            "Out of memory. MCPanel restarts it in 10 s."
        );
        assert_eq!(
            crash_body(
                "The server exited unexpectedly (exit code 1)",
                CrashAction::Restart,
                None
            ),
            "The server exited unexpectedly (exit code 1). MCPanel restarts it."
        );
        assert!(crash_body("", CrashAction::GaveUp, None).contains("gave up"));
        assert!(crash_body("x", CrashAction::Disabled, None).ends_with("off for this server."));
    }

    #[test]
    fn default_rules() {
        let p = NotificationPrefs::default();
        assert!(p.crash.desktop && p.crash.inbox);
        assert!(!p.player_joined.inbox && !p.player_joined.desktop);
        assert!(p.task_failed.inbox && !p.task_failed.desktop);
        // Stored prefs missing newer fields fall back to the defaults for them.
        let old: NotificationPrefs =
            serde_json::from_str(r#"{"crash":{"inbox":false,"desktop":false}}"#).unwrap();
        assert!(!old.crash.inbox);
        assert_eq!(old.backup_failed, p.backup_failed);
    }

    #[test]
    fn category_round_trip() {
        for c in [
            Category::Crash,
            Category::BackupFailed,
            Category::TaskFailed,
            Category::PlayerJoined,
        ] {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
    }
}
