//! Host state shared by commands.

use mcpanel_api::{Api, Principal};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub struct AppState {
    pub api: Arc<Api>,
    /// Closed on quit so SQLite checkpoints the WAL and releases the file.
    pub db: mcpanel_db::Database,
    /// Active console stream subscriptions (id → cancel token).
    pub subscriptions: Mutex<HashMap<String, CancellationToken>>,
    /// Set once the user confirmed quitting; lets the exit proceed.
    pub quitting: std::sync::atomic::AtomicBool,
}

impl AppState {
    pub fn new(api: Arc<Api>, db: mcpanel_db::Database) -> Self {
        Self {
            api,
            db,
            subscriptions: Mutex::new(HashMap::new()),
            quitting: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The desktop user is the only principal in v1.
    pub fn principal(&self) -> Principal {
        Principal::LocalUser
    }
}
