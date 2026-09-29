//! # mcpanel-core
//!
//! The headless core of MCPanel: domain model, services, ports (traits) and
//! orchestration. It has no dependency on the UI, Tauri, the database driver, HTTP
//! clients or OS APIs — those are adapters that implement the traits in [`ports`] and
//! [`software`]. See `docs/architecture/README.md`.

pub mod audit;
pub mod backup;
pub mod bedrock;
pub mod config;
pub mod console;
pub mod content;
pub mod core;
pub mod crash;
pub mod error;
pub mod events;
pub mod files;
pub mod ids;
pub mod java;
pub mod jobs;
pub mod lifecycle;
pub mod model;
pub mod monitoring;
pub mod paths;
pub mod players;
pub mod ports;
pub mod server;
pub mod server_files;
pub mod settings;
pub mod software;
pub mod templates;
pub mod time;
pub mod tunnels;

pub use crate::core::{Core, CoreDeps, Repositories};
pub use error::{CoreError, CoreResult, ErrorCode};
