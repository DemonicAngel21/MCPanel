//! In-memory runtime state of one server (process + active operations).

use crate::console::ConsoleHub;
use crate::ids::ServerId;
use crate::lifecycle::LifecycleState;
use crate::ports::ProcessController;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{Notify, mpsc};

/// Operations that run alongside the lifecycle (ADR-0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Installing,
    Importing,
    Deleting,
    EditingConfig,
}

impl Operation {
    /// Whether the server may be started while this operation is active.
    pub fn blocks_start(self) -> bool {
        matches!(self, Self::Installing | Self::Importing | Self::Deleting)
    }

    /// Whether two operations may run concurrently on the same server.
    pub fn compatible_with(self, other: Operation) -> bool {
        matches!((self, other), (Self::EditingConfig, Self::EditingConfig))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    pub kind: String,
    pub message: String,
}

pub(crate) struct Inner {
    pub state: LifecycleState,
    pub pid: Option<u32>,
    pub process_start_time: Option<u64>,
    pub started_at: Option<Timestamp>,
    pub ready_at: Option<Timestamp>,
    pub stdin: Option<mpsc::Sender<String>>,
    pub controller: Option<Arc<dyn ProcessController>>,
    pub stop_requested: bool,
    pub forced: bool,
    pub restart_after_exit: bool,
    pub seen_stopping_line: bool,
    pub online_players: BTreeSet<String>,
    pub diagnosis: Option<Diagnosis>,
    pub last_exit_code: Option<i32>,
    pub operations: BTreeSet<Operation>,
    /// A start is being prepared (preflight running); blocks concurrent starts.
    pub start_pending: bool,
    /// Incremented for every spawned process so stale tasks can detect replacement.
    pub generation: u64,
}

pub struct ServerRuntime {
    pub server_id: ServerId,
    pub console: Arc<ConsoleHub>,
    inner: Mutex<Inner>,
    /// Notified whenever the process exits.
    pub exited: Notify,
}

/// Point-in-time view for queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub state: LifecycleState,
    pub pid: Option<u32>,
    pub started_at: Option<Timestamp>,
    pub ready_at: Option<Timestamp>,
    pub online_players: Vec<String>,
    pub diagnosis: Option<Diagnosis>,
    pub last_exit_code: Option<i32>,
    pub operations: Vec<Operation>,
    pub console_attached: bool,
}

impl ServerRuntime {
    pub fn new(server_id: ServerId, initial: LifecycleState, console_capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            server_id,
            console: ConsoleHub::new(console_capacity),
            inner: Mutex::new(Inner {
                state: initial,
                pid: None,
                process_start_time: None,
                started_at: None,
                ready_at: None,
                stdin: None,
                controller: None,
                stop_requested: false,
                forced: false,
                restart_after_exit: false,
                seen_stopping_line: false,
                online_players: BTreeSet::new(),
                diagnosis: None,
                last_exit_code: None,
                operations: BTreeSet::new(),
                start_pending: false,
                generation: 0,
            }),
            exited: Notify::new(),
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn state(&self) -> LifecycleState {
        self.lock().state
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        let i = self.lock();
        RuntimeSnapshot {
            state: i.state,
            pid: i.pid,
            started_at: i.started_at,
            ready_at: i.ready_at,
            online_players: i.online_players.iter().cloned().collect(),
            diagnosis: i.diagnosis.clone(),
            last_exit_code: i.last_exit_code,
            operations: i.operations.iter().copied().collect(),
            console_attached: i.stdin.is_some(),
        }
    }

    /// Try to begin an operation; returns a guard that ends it on drop.
    pub fn begin(self: &Arc<Self>, op: Operation) -> crate::error::CoreResult<OperationGuard> {
        let mut i = self.lock();
        if let Some(conflict) = i.operations.iter().find(|o| !op.compatible_with(**o)) {
            return Err(crate::error::CoreError::new(
                crate::error::ErrorCode::ServerBusy,
                format!("Another operation is in progress ({conflict:?})"),
            ));
        }
        if op.blocks_start() && i.state.has_process() {
            return Err(crate::error::CoreError::new(
                crate::error::ErrorCode::ServerBusy,
                "Stop the server first",
            ));
        }
        i.operations.insert(op);
        Ok(OperationGuard {
            runtime: Arc::clone(self),
            op,
        })
    }
}

pub struct OperationGuard {
    runtime: Arc<ServerRuntime>,
    op: Operation,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.runtime.lock().operations.remove(&self.op);
    }
}
