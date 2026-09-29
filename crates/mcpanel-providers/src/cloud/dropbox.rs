//! Dropbox (API v2). Verified 2026-09-30 against Dropbox's documentation and developer
//! forum answers from Dropbox staff:
//! - OAuth guide (docs.dropboxapi.com/dropbox-api/docs/oauth): PKCE with
//!   `code_challenge_method=S256` for apps that cannot keep a secret; the code exchange
//!   sends `code_verifier` instead of `client_secret`; `token_access_type=offline` returns
//!   a refresh token; refresh via `/oauth2/token` with `grant_type=refresh_token`.
//! - Redirect URIs must be pre-registered in the App Console and match exactly, including
//!   the port — Dropbox has no variable loopback port, so MCPanel uses three fixed ports
//!   (all three must be registered) and takes the first free one.
//! - Scoped apps: `account_info.read` is required for user-linked apps; files access via
//!   `files.metadata.read`, `files.content.read`, `files.content.write`. "App folder"
//!   access limits the app to its own folder.
//! - Development status: up to 500 linked users; after 50 users there are two weeks to
//!   get production approval.
//!
//! Endpoints: `https://www.dropbox.com/oauth2/authorize`,
//! `https://api.dropboxapi.com/oauth2/token`, `POST /2/users/get_current_account`,
//! `POST /2/auth/token/revoke`.

use super::{Stage, api_json, token_request};
use crate::http::HttpClient;
use async_trait::async_trait;
use mcpanel_core::cloud::{
    CloudAccount, CloudProviderInfo, CloudStorageProvider, RedirectSpec, RevokeOutcome, TokenSet,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

const NAME: &str = "Dropbox";
const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";
/// Loopback ports registered as redirect URIs (tried in order).
pub const PORTS: &[u16] = &[43917, 43918, 43919];

pub struct Dropbox {
    http: HttpClient,
    info: CloudProviderInfo,
}

impl Dropbox {
    pub fn new(http: HttpClient, client_id: Option<String>) -> Self {
        Self {
            http,
            info: CloudProviderInfo {
                id: "dropbox",
                display_name: NAME,
                client_id,
                client_id_variable: super::DROPBOX_CLIENT_ID_VAR,
                authorize_url: "https://www.dropbox.com/oauth2/authorize",
                scopes: &[
                    "account_info.read",
                    "files.metadata.read",
                    "files.content.read",
                    "files.content.write",
                ],
                extra_authorize_params: &[("token_access_type", "offline")],
                redirect: RedirectSpec::FixedPorts {
                    host: "localhost",
                    path: "/mcpanel/oauth",
                    ports: PORTS,
                },
                manage_access_url: "https://www.dropbox.com/account/connected_apps",
            },
        }
    }

    fn client_id(&self) -> CoreResult<&str> {
        self.info
            .client_id
            .as_deref()
            .ok_or_else(|| CoreError::new(ErrorCode::Unsupported, "Dropbox is not configured"))
    }
}

#[derive(Deserialize)]
struct Account {
    name: Option<Name>,
    email: Option<String>,
}

#[derive(Deserialize)]
struct Name {
    display_name: Option<String>,
}

#[async_trait]
impl CloudStorageProvider for Dropbox {
    fn info(&self) -> &CloudProviderInfo {
        &self.info
    }

    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> CoreResult<TokenSet> {
        token_request(
            &self.http,
            NAME,
            Stage::Exchange,
            TOKEN_URL,
            &[
                ("code", code),
                ("grant_type", "authorization_code"),
                ("code_verifier", verifier),
                ("client_id", self.client_id()?),
                ("redirect_uri", redirect_uri),
            ],
        )
        .await
    }

    async fn refresh(&self, refresh_token: &SecretString) -> CoreResult<TokenSet> {
        token_request(
            &self.http,
            NAME,
            Stage::Refresh,
            TOKEN_URL,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.expose_secret()),
                ("client_id", self.client_id()?),
            ],
        )
        .await
    }

    async fn account(&self, access_token: &SecretString) -> CoreResult<CloudAccount> {
        let (status, body) = self
            .http
            .send_raw(
                reqwest::Method::POST,
                "https://api.dropboxapi.com/2/users/get_current_account",
                Some(access_token.expose_secret()),
                None,
            )
            .await?;
        let a: Account = api_json(NAME, "account lookup", status, &body)?;
        Ok(CloudAccount {
            display_name: a.name.and_then(|n| n.display_name),
            email: a.email,
        })
    }

    async fn revoke(&self, refresh_token: &SecretString) -> CoreResult<RevokeOutcome> {
        // Revoking needs an access token; a refresh gives one for this grant.
        let tokens = match self.refresh(refresh_token).await {
            Ok(t) => t,
            // Already invalid: nothing to revoke.
            Err(_) => return Ok(RevokeOutcome::Revoked),
        };
        let (status, _) = self
            .http
            .send_raw(
                reqwest::Method::POST,
                "https://api.dropboxapi.com/2/auth/token/revoke",
                Some(tokens.access_token.expose_secret()),
                None,
            )
            .await?;
        if (200..300).contains(&status) {
            Ok(RevokeOutcome::Revoked)
        } else {
            Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME}: revocation failed (HTTP {status})"),
            ))
        }
    }
}
