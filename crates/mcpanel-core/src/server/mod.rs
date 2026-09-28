//! Servers: registry, lifecycle supervision, provisioning and configuration.

pub mod manager;
mod process;
pub mod props;
pub mod provisioning;
pub mod runtime;

pub use manager::{ServerManager, ServerManagerDeps, ServerView};
pub use props::{PropertyChange, ServerProperties};
pub use provisioning::{
    CreateServerRequest, ImportDetection, ImportServerRequest, InstallPreview, LocationCheck,
    UpdateServerRequest,
};
pub use runtime::{Operation, RuntimeSnapshot};
