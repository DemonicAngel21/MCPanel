//! Core error type. Every error carries a stable [`ErrorCode`] that the Application API
//! exposes to clients; messages are human-readable and must never contain secrets.

use serde::{Deserialize, Serialize};

/// Stable, machine-readable error codes. Adding a variant is fine; renaming or removing
/// one is a breaking API change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Internal,
    InvalidInput,
    NotFound,
    Conflict,
    ServerNotFound,
    ServerNotRunning,
    ServerAlreadyRunning,
    ServerBusy,
    EulaNotAccepted,
    JavaNotFound,
    JavaIncompatible,
    JavaInvalid,
    PortInUse,
    DirectoryNotAllowed,
    DirectoryNotEmpty,
    PathRejected,
    PathNotFound,
    PathExists,
    SensitiveFile,
    FileTooLarge,
    FileChangedOnDisk,
    ArchiveRejected,
    DownloadFailed,
    HashMismatch,
    ProviderUnavailable,
    ProviderError,
    VersionNotFound,
    Database,
    SchemaTooNew,
    Io,
    Cancelled,
    GrantInvalid,
    InsufficientDiskSpace,
    Unsupported,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct CoreError {
    pub code: ErrorCode,
    pub message: String,
    /// Optional structured details (never secrets).
    pub details: Option<serde_json::Value>,
    pub retryable: bool,
}

impl CoreError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            retryable: false,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn io(context: impl AsRef<str>, err: &std::io::Error) -> Self {
        let code = match err.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::PathNotFound,
            std::io::ErrorKind::AlreadyExists => ErrorCode::PathExists,
            _ => ErrorCode::Io,
        };
        Self::new(code, format!("{}: {err}", context.as_ref()))
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "Operation was cancelled")
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
