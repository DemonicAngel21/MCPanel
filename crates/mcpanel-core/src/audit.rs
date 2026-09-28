//! Audit log. Records who did what, to which server, and whether it succeeded.
//! Metadata must never contain secrets (callers pass keys/ids, not values).

use crate::error::CoreResult;
use crate::events::{DomainEvent, EventBus};
use crate::ids::{AuditId, ServerId};
use crate::model::{AuditEntry, AuditQuery, AuditResult};
use crate::ports::AuditRepository;
use crate::time::Timestamp;
use std::sync::Arc;

pub struct AuditLog {
    repo: Arc<dyn AuditRepository>,
    events: EventBus,
}

impl AuditLog {
    pub fn new(repo: Arc<dyn AuditRepository>, events: EventBus) -> Self {
        Self { repo, events }
    }

    /// Record an entry. Failures are logged, never propagated: auditing must not break
    /// the operation being audited.
    pub async fn record(
        &self,
        actor: &str,
        action: &str,
        server_id: Option<ServerId>,
        target: Option<String>,
        result: AuditResult,
        metadata: serde_json::Value,
    ) {
        let entry = AuditEntry {
            id: AuditId::new(),
            occurred_at: Timestamp::now(),
            actor: actor.to_string(),
            action: action.to_string(),
            server_id,
            target,
            result,
            metadata,
        };
        if let Err(e) = self.repo.insert(&entry).await {
            tracing::error!(target: "mcpanel::audit", action, "failed to record audit entry: {}", e.message);
            return;
        }
        self.events.publish(DomainEvent::AuditRecorded);
    }

    pub async fn query(&self, query: &AuditQuery) -> CoreResult<Vec<AuditEntry>> {
        let mut q = query.clone();
        q.limit = q.limit.clamp(1, 500);
        self.repo.query(&q).await
    }
}
