//! # mcpanel-api
//!
//! The Application API: the only surface the UI (and future remote clients) use.
//! Transport-agnostic: v1 is carried over Tauri IPC; an HTTP/WebSocket adapter can map
//! the same methods 1:1 (`servers_start` ↔ `POST /api/v1/servers/{id}/start`).

mod api;
pub mod dto;
pub mod error;
pub mod grants;
pub mod principal;

pub use api::Api;
pub use error::{ApiError, ApiResult};
pub use grants::{GrantDto, GrantKind, GrantRegistry};
pub use principal::{Permission, Principal};
