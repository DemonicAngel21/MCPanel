//! Cloud storage connections (spec §9): Google Drive and Dropbox behind
//! [`CloudStorageProvider`], connected with OAuth 2.0 authorization code + PKCE through the
//! system browser and a loopback redirect. MCPanel is a public client: no client secret
//! exists anywhere. Refresh tokens live in the OS secret store; access tokens only in
//! memory. Provider requirements are recorded in docs/architecture/verification-log.md.

pub mod loopback;
pub mod pkce;
pub mod service;

pub use service::{CloudService, CloudStatus, ConnectStart, FlowState};

use crate::error::CoreResult;
use crate::time::Timestamp;
use async_trait::async_trait;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

/// How the provider accepts loopback redirect URIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectSpec {
    /// Any free port on `host` (Google: `http://127.0.0.1:{port}`; Microsoft: the port of a
    /// `http://localhost` redirect is ignored when matching).
    AnyPort {
        host: &'static str,
        path: &'static str,
    },
    /// Only these exact ports are registered (Dropbox matches redirect URIs exactly).
    FixedPorts {
        host: &'static str,
        path: &'static str,
        ports: &'static [u16],
    },
}

impl RedirectSpec {
    pub fn host(&self) -> &'static str {
        match self {
            Self::AnyPort { host, .. } | Self::FixedPorts { host, .. } => host,
        }
    }

    pub fn path(&self) -> &'static str {
        match self {
            Self::AnyPort { path, .. } | Self::FixedPorts { path, .. } => path,
        }
    }

    /// The redirect URI for a bound port.
    pub fn uri(&self, port: u16) -> String {
        format!("http://{}:{port}{}", self.host(), self.path())
    }
}

/// Static description of a provider and its OAuth configuration.
#[derive(Debug, Clone)]
pub struct CloudProviderInfo {
    /// `google_drive`, `dropbox`.
    pub id: &'static str,
    pub display_name: &'static str,
    /// The public client ID; `None` until MCPanel's app registration is configured.
    pub client_id: Option<String>,
    /// Where the client ID comes from (for the "not configured" message).
    pub client_id_variable: &'static str,
    pub authorize_url: &'static str,
    pub scopes: &'static [&'static str],
    /// Extra authorization parameters (e.g. Dropbox `token_access_type=offline`).
    pub extra_authorize_params: &'static [(&'static str, &'static str)],
    pub redirect: RedirectSpec,
    /// Where the user can remove MCPanel's access when the provider has no revoke API.
    pub manage_access_url: &'static str,
}

/// Tokens from a code exchange or refresh.
pub struct TokenSet {
    pub access_token: SecretString,
    /// Returned on the first exchange; providers may rotate it on refresh.
    pub refresh_token: Option<SecretString>,
    pub expires_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudAccount {
    pub display_name: Option<String>,
    pub email: Option<String>,
}

/// What disconnecting did at the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevokeOutcome {
    /// The provider revoked the token.
    Revoked,
    /// The provider has no token revocation for this client type; the user removes the
    /// app in their account settings (`manage_access_url`).
    NotSupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudOperationType {
    Upload,
    Download,
    Restore,
}

impl CloudOperationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Restore => "restore",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudOperationState {
    Idle,
    Starting,
    Uploading,
    Downloading,
    Processing,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

impl CloudOperationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Uploading => "uploading",
            Self::Downloading => "downloading",
            Self::Processing => "processing",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed | Self::Failed)
    }
}

pub type CloudProgressFn = std::sync::Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

#[derive(Clone, Default)]
pub struct CloudTransferOptions {
    pub on_progress: Option<CloudProgressFn>,
    pub cancel: tokio_util::sync::CancellationToken,
}

impl CloudTransferOptions {
    pub fn new(
        on_progress: Option<CloudProgressFn>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Self {
        Self {
            on_progress,
            cancel,
        }
    }

    pub fn with_cancel(cancel: tokio_util::sync::CancellationToken) -> Self {
        Self {
            on_progress: None,
            cancel,
        }
    }

    pub fn report(&self, completed: u64, total: u64) {
        if let Some(cb) = &self.on_progress {
            cb(completed, Some(total));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudOperationSnapshot {
    pub id: String,
    pub provider: String,
    pub backup_id: Option<String>,
    pub backup_name: String,
    pub operation_type: CloudOperationType,
    pub state: CloudOperationState,
    pub bytes_completed: u64,
    pub total_bytes: Option<u64>,
    pub progress_percentage: Option<f32>,
    pub start_time: Timestamp,
    pub speed_bytes_per_sec: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudFileMetadata {
    pub id: String,
    pub name: String,
    pub size_bytes: u64,
    pub created_at: Option<Timestamp>,
    pub modified_at: Option<Timestamp>,
    pub provider: String,
}

/// A cloud storage service. Implementations only talk to documented endpoints.
#[async_trait]
pub trait CloudStorageProvider: Send + Sync {
    fn info(&self) -> &CloudProviderInfo;

    /// Exchange an authorization code (with the PKCE verifier) for tokens.
    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> CoreResult<TokenSet>;

    /// Get a new access token with a refresh token.
    async fn refresh(&self, refresh_token: &SecretString) -> CoreResult<TokenSet>;

    /// Who is signed in (shown in the UI).
    async fn account(&self, access_token: &SecretString) -> CoreResult<CloudAccount>;

    /// Revoke MCPanel's grant, where the provider supports it.
    async fn revoke(&self, refresh_token: &SecretString) -> CoreResult<RevokeOutcome>;

    /// Upload a file to cloud storage.
    async fn upload_file(
        &self,
        _access_token: &SecretString,
        _filename: &str,
        _path: &std::path::Path,
        _options: Option<CloudTransferOptions>,
    ) -> CoreResult<CloudFileMetadata> {
        Err(crate::error::CoreError::new(
            crate::error::ErrorCode::Unsupported,
            "Upload not supported by this provider",
        ))
    }

    /// List MCPanel backup files in cloud storage.
    async fn list_files(&self, _access_token: &SecretString) -> CoreResult<Vec<CloudFileMetadata>> {
        Err(crate::error::CoreError::new(
            crate::error::ErrorCode::Unsupported,
            "Listing files not supported by this provider",
        ))
    }

    /// Download a file from cloud storage to destination path.
    async fn download_file(
        &self,
        _access_token: &SecretString,
        _file_id: &str,
        _destination: &std::path::Path,
        _options: Option<CloudTransferOptions>,
    ) -> CoreResult<u64> {
        Err(crate::error::CoreError::new(
            crate::error::ErrorCode::Unsupported,
            "Download not supported by this provider",
        ))
    }

    /// Delete a file from cloud storage.
    async fn delete_file(&self, _access_token: &SecretString, _file_id: &str) -> CoreResult<()> {
        Err(crate::error::CoreError::new(
            crate::error::ErrorCode::Unsupported,
            "Delete not supported by this provider",
        ))
    }
}
