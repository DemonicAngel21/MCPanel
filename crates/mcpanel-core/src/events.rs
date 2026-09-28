//! Typed in-process event bus.
//!
//! Events carry identifiers and non-secret facts only. Fan-out uses a tokio broadcast
//! channel: producers never block, and slow subscribers observe `Lagged` and re-sync via
//! queries. Multi-step reactions are implemented as policies that run jobs, not as event
//! chains.

use crate::ids::{EventId, JavaRuntimeId, JobId, ServerId};
use crate::jobs::JobStatus;
use crate::lifecycle::LifecycleState;
use crate::time::Timestamp;
use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DomainEvent {
    ServerCreated {
        server_id: ServerId,
    },
    ServerUpdated {
        server_id: ServerId,
    },
    ServerDeleted {
        server_id: ServerId,
    },
    ServerStateChanged {
        server_id: ServerId,
        state: LifecycleState,
        previous: LifecycleState,
    },
    ServerReady {
        server_id: ServerId,
        startup_ms: i64,
    },
    ServerStopped {
        server_id: ServerId,
        exit_code: Option<i32>,
        forced: bool,
    },
    ServerCrashed {
        server_id: ServerId,
        exit_code: Option<i32>,
        diagnosis: Option<String>,
    },
    PlayerJoined {
        server_id: ServerId,
        player_name: String,
    },
    PlayerLeft {
        server_id: ServerId,
        player_name: String,
    },
    JobUpdated {
        job_id: JobId,
        server_id: Option<ServerId>,
        status: JobStatus,
        progress: Option<f32>,
        message: Option<String>,
    },
    JavaRuntimesChanged {
        added: Vec<JavaRuntimeId>,
    },
    SettingsChanged {
        key: String,
    },
    AuditRecorded,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub id: EventId,
    pub at: Timestamp,
    pub event: DomainEvent,
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<EventEnvelope>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: DomainEvent) {
        let envelope = EventEnvelope {
            id: EventId::new(),
            at: Timestamp::now(),
            event,
        };
        // No subscribers is not an error.
        let _ = self.tx.send(envelope);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}
