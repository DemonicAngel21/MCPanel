//! The API error model: stable machine-readable codes plus a human-readable message.

use mcpanel_core::error::CoreError;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiError {
    /// Stable code, e.g. `SERVER_NOT_FOUND` (see `mcpanel_core::ErrorCode`).
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub retryable: bool,
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        let code = serde_json::to_value(e.code)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "INTERNAL".into());
        Self {
            code,
            message: e.message,
            details: e.details,
            retryable: e.retryable,
        }
    }
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
            retryable: false,
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_INPUT", message)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

pub type ApiResult<T> = Result<T, ApiError>;
