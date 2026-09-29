//! Cloud storage connections (spec §9): Google Drive, OneDrive and Dropbox behind
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
    /// `google_drive`, `onedrive`, `dropbox`.
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
}
