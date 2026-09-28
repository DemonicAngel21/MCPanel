//! Long-running operations: progress, cancellation and persisted status.

use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::ids::{JobId, ServerId};
use crate::ports::JobRepository;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: JobId,
    pub kind: String,
    pub server_id: Option<ServerId>,
    pub status: JobStatus,
    pub progress: Option<f32>,
    pub message: Option<String>,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    pub error_code: Option<ErrorCode>,
    pub error_message: Option<String>,
    /// Non-secret result payload (e.g. the id of a created server).
    pub result: Option<serde_json::Value>,
}

/// Handle given to a running job body.
#[derive(Clone)]
pub struct JobContext {
    pub id: JobId,
    pub server_id: Option<ServerId>,
    cancel: CancellationToken,
    reporter: Arc<ProgressReporter>,
}

struct ProgressReporter {
    events: EventBus,
    kind: String,
    last_emit: Mutex<Option<Instant>>,
}

impl JobContext {
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn check_cancelled(&self) -> CoreResult<()> {
        if self.cancel.is_cancelled() {
            Err(CoreError::cancelled())
        } else {
            Ok(())
        }
    }

    /// Report progress (0.0..=1.0). Throttled to ~5 updates per second.
    pub fn progress(&self, fraction: Option<f32>, message: impl Into<String>) {
        let message = message.into();
        {
            let mut last = self
                .reporter
                .last_emit
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(t) = *last
                && t.elapsed() < Duration::from_millis(200)
            {
                return;
            }
            *last = Some(Instant::now());
        }
        self.reporter.events.publish(DomainEvent::JobUpdated {
            job_id: self.id,
            kind: self.reporter.kind.clone(),
            server_id: self.server_id,
            status: JobStatus::Running,
            progress: fraction.map(|f| f.clamp(0.0, 1.0)),
            message: Some(message),
        });
    }
}

pub struct JobManager {
    repo: Arc<dyn JobRepository>,
    events: EventBus,
    active: Mutex<HashMap<JobId, CancellationToken>>,
}

impl JobManager {
    pub fn new(repo: Arc<dyn JobRepository>, events: EventBus) -> Self {
        Self {
            repo,
            events,
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Jobs that were running when MCPanel last exited cannot be resumed (yet); mark them.
    pub async fn recover_interrupted(&self) -> CoreResult<u64> {
        self.repo.mark_interrupted(Timestamp::now()).await
    }

    /// Start a job in the background. Returns immediately with the job id.
    pub async fn spawn<F, Fut>(
        self: &Arc<Self>,
        kind: &str,
        server_id: Option<ServerId>,
        body: F,
    ) -> CoreResult<JobId>
    where
        F: FnOnce(JobContext) -> Fut + Send + 'static,
        Fut: Future<Output = CoreResult<Option<serde_json::Value>>> + Send + 'static,
    {
        let now = Timestamp::now();
        let record = JobRecord {
            id: JobId::new(),
            kind: kind.to_string(),
            server_id,
            status: JobStatus::Running,
            progress: None,
            message: None,
            created_at: now,
            started_at: Some(now),
            finished_at: None,
            error_code: None,
            error_message: None,
            result: None,
        };
        self.repo.insert(&record).await?;
        let cancel = CancellationToken::new();
        self.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(record.id, cancel.clone());

        let ctx = JobContext {
            id: record.id,
            server_id,
            cancel,
            reporter: Arc::new(ProgressReporter {
                events: self.events.clone(),
                kind: kind.to_string(),
                last_emit: Mutex::new(None),
            }),
        };
        self.events.publish(DomainEvent::JobUpdated {
            job_id: record.id,
            kind: kind.to_string(),
            server_id,
            status: JobStatus::Running,
            progress: None,
            message: None,
        });

        let this = Arc::clone(self);
        let job_id = record.id;
        let kind_owned = kind.to_string();
        tokio::spawn(async move {
            let outcome = body(ctx).await;
            this.finish(job_id, server_id, &kind_owned, outcome).await;
        });
        Ok(job_id)
    }

    async fn finish(
        &self,
        job_id: JobId,
        server_id: Option<ServerId>,
        kind: &str,
        outcome: CoreResult<Option<serde_json::Value>>,
    ) {
        self.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&job_id);
        let now = Timestamp::now();
        let (status, code, message, result) = match outcome {
            Ok(result) => (JobStatus::Succeeded, None, None, result),
            Err(e) if e.code == ErrorCode::Cancelled => {
                (JobStatus::Cancelled, Some(e.code), Some(e.message), None)
            }
            Err(e) => {
                tracing::warn!(target: "mcpanel::jobs", %job_id, kind, code = ?e.code, "job failed: {}", e.message);
                (JobStatus::Failed, Some(e.code), Some(e.message), None)
            }
        };
        if let Err(e) = self
            .repo
            .finish(job_id, status, now, code, message.clone(), result)
            .await
        {
            tracing::error!(target: "mcpanel::jobs", %job_id, "failed to persist job result: {}", e.message);
        }
        self.events.publish(DomainEvent::JobUpdated {
            job_id,
            kind: kind.to_string(),
            server_id,
            status,
            progress: if status == JobStatus::Succeeded {
                Some(1.0)
            } else {
                None
            },
            message,
        });
    }

    pub fn cancel(&self, job_id: JobId) -> bool {
        if let Some(token) = self
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&job_id)
        {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn get(&self, job_id: JobId) -> CoreResult<Option<JobRecord>> {
        self.repo.get(job_id).await
    }

    pub async fn recent(&self, limit: u32) -> CoreResult<Vec<JobRecord>> {
        self.repo.recent(limit).await
    }
}
