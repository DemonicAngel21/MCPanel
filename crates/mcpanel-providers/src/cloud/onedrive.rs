//! OneDrive (Microsoft Graph). Verified 2026-09-30 against Microsoft Learn:
//! - Auth code flow (entra/identity-platform/v2-oauth2-auth-code-flow): endpoints
//!   `https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize|token`, tenant
//!   `common` = personal and work/school accounts; PKCE S256; "Public clients, which
//!   include native applications …, must not use secrets or certificates"; a refresh
//!   token is only returned when `offline_access` is requested; refresh tokens may be
//!   rotated ("discard the old refresh token").
//! - Redirect URIs (entra/identity-platform/reply-url): register under "Mobile and
//!   desktop applications"; `http://localhost` is allowed and "the port component … is
//!   ignored for the purposes of matching a localhost redirect URI"; the path must match
//!   (case-sensitive); query parameters are not allowed for apps that sign in personal
//!   accounts.
//! - App folder (onedrive/developer/rest-api/concepts/special-folders-appfolder):
//!   `Files.ReadWrite.AppFolder` gives access to `/drive/special/approot` (created on first
//!   use, named after the app registration, in the user's "Apps" folder).
//! - `User.Read` for `GET /me` (display name / sign-in name).
//!
//! Microsoft has no token revocation endpoint for this flow; disconnecting removes the
//! token locally and points to the account's app permissions page.

use super::{Stage, api_json, token_request};
use crate::http::HttpClient;
use async_trait::async_trait;
use mcpanel_core::cloud::{
    CloudAccount, CloudProviderInfo, CloudStorageProvider, RedirectSpec, RevokeOutcome, TokenSet,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

const NAME: &str = "OneDrive";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
pub const SCOPES: &[&str] = &["offline_access", "User.Read", "Files.ReadWrite.AppFolder"];

pub struct OneDrive {
    http: HttpClient,
    info: CloudProviderInfo,
}

impl OneDrive {
    pub fn new(http: HttpClient, client_id: Option<String>) -> Self {
        Self {
            http,
            info: CloudProviderInfo {
                id: "onedrive",
                display_name: NAME,
                client_id,
                client_id_variable: super::MICROSOFT_CLIENT_ID_VAR,
                authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                scopes: SCOPES,
                extra_authorize_params: &[("response_mode", "query")],
                redirect: RedirectSpec::AnyPort {
                    host: "localhost",
                    path: "/mcpanel/oauth",
                },
                manage_access_url: "https://account.live.com/consent/Manage",
            },
        }
    }

    fn client_id(&self) -> CoreResult<&str> {
        self.info
            .client_id
            .as_deref()
            .ok_or_else(|| CoreError::new(ErrorCode::Unsupported, "OneDrive is not configured"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Me {
    display_name: Option<String>,
    user_principal_name: Option<String>,
    mail: Option<String>,
}

#[async_trait]
impl CloudStorageProvider for OneDrive {
    fn info(&self) -> &CloudProviderInfo {
        &self.info
    }

    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> CoreResult<TokenSet> {
        let scope = SCOPES.join(" ");
        token_request(
            &self.http,
            NAME,
            Stage::Exchange,
            TOKEN_URL,
            &[
                ("client_id", self.client_id()?),
                ("scope", &scope),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("grant_type", "authorization_code"),
                ("code_verifier", verifier),
            ],
        )
        .await
    }

    async fn refresh(&self, refresh_token: &SecretString) -> CoreResult<TokenSet> {
        let scope = SCOPES.join(" ");
        token_request(
            &self.http,
            NAME,
            Stage::Refresh,
            TOKEN_URL,
            &[
                ("client_id", self.client_id()?),
                ("scope", &scope),
                ("refresh_token", refresh_token.expose_secret()),
                ("grant_type", "refresh_token"),
            ],
        )
        .await
    }

    async fn account(&self, access_token: &SecretString) -> CoreResult<CloudAccount> {
        let (status, body) = self
            .http
            .send_raw(
                reqwest::Method::GET,
                "https://graph.microsoft.com/v1.0/me?$select=displayName,userPrincipalName,mail",
                Some(access_token.expose_secret()),
                None,
            )
            .await?;
        let me: Me = api_json(NAME, "account lookup", status, &body)?;
        Ok(CloudAccount {
            display_name: me.display_name,
            email: me.mail.or(me.user_principal_name),
        })
    }

    async fn revoke(&self, _refresh_token: &SecretString) -> CoreResult<RevokeOutcome> {
        Ok(RevokeOutcome::NotSupported)
    }
}
