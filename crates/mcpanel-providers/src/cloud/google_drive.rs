//! Google Drive (Drive API v3). Verified 2026-09-30 against Google's documentation:
//! - OAuth 2.0 for installed apps (developers.google.com/identity/protocols/oauth2/native-app):
//!   client type "Desktop app"; loopback redirect `http://127.0.0.1:{port}` on any free
//!   port (not pre-registered); PKCE S256; authorization endpoint
//!   `https://accounts.google.com/o/oauth2/v2/auth`, token endpoint
//!   `https://oauth2.googleapis.com/token` where `client_secret` is documented as
//!   *Optional* for code exchange and refresh ("installed apps … cannot keep secrets");
//!   refresh tokens are returned for installed apps; revocation at
//!   `https://oauth2.googleapis.com/revoke`.
//! - Scope `https://www.googleapis.com/auth/drive.file` (non-sensitive): files the app
//!   creates or the user opens with it. `about.get` accepts it (`fields` is required).

use super::{Stage, api_json, token_request};
use crate::http::HttpClient;
use async_trait::async_trait;
use mcpanel_core::cloud::service::encode_form;
use mcpanel_core::cloud::{
    CloudAccount, CloudProviderInfo, CloudStorageProvider, RedirectSpec, RevokeOutcome, TokenSet,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

const NAME: &str = "Google Drive";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub struct GoogleDrive {
    http: HttpClient,
    info: CloudProviderInfo,
}

impl GoogleDrive {
    pub fn new(http: HttpClient, client_id: Option<String>) -> Self {
        Self {
            http,
            info: CloudProviderInfo {
                id: "google_drive",
                display_name: NAME,
                client_id,
                client_id_variable: super::GOOGLE_CLIENT_ID_VAR,
                authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
                scopes: &["https://www.googleapis.com/auth/drive.file"],
                extra_authorize_params: &[],
                redirect: RedirectSpec::AnyPort {
                    host: "127.0.0.1",
                    path: "/",
                },
                manage_access_url: "https://myaccount.google.com/connections",
            },
        }
    }

    fn client_id(&self) -> CoreResult<&str> {
        self.info
            .client_id
            .as_deref()
            .ok_or_else(|| CoreError::new(ErrorCode::Unsupported, "Google Drive is not configured"))
    }
}

#[derive(Deserialize)]
struct About {
    user: Option<User>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct User {
    display_name: Option<String>,
    email_address: Option<String>,
}

#[async_trait]
impl CloudStorageProvider for GoogleDrive {
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
                ("client_id", self.client_id()?),
                ("code", code),
                ("code_verifier", verifier),
                ("grant_type", "authorization_code"),
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
                ("client_id", self.client_id()?),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.expose_secret()),
            ],
        )
        .await
    }

    async fn account(&self, access_token: &SecretString) -> CoreResult<CloudAccount> {
        let (status, body) = self
            .http
            .send_raw(
                reqwest::Method::GET,
                "https://www.googleapis.com/drive/v3/about?fields=user(displayName,emailAddress)",
                Some(access_token.expose_secret()),
                None,
            )
            .await?;
        let about: About = api_json(NAME, "account lookup", status, &body)?;
        let user = about.user.unwrap_or(User {
            display_name: None,
            email_address: None,
        });
        Ok(CloudAccount {
            display_name: user.display_name,
            email: user.email_address,
        })
    }

    async fn revoke(&self, refresh_token: &SecretString) -> CoreResult<RevokeOutcome> {
        let (status, _) = self
            .http
            .send_raw(
                reqwest::Method::POST,
                "https://oauth2.googleapis.com/revoke",
                None,
                Some((
                    "application/x-www-form-urlencoded",
                    encode_form(&[("token", refresh_token.expose_secret())]),
                )),
            )
            .await?;
        // 400 means the token is already invalid — nothing left to revoke.
        if (200..300).contains(&status) || status == 400 {
            Ok(RevokeOutcome::Revoked)
        } else {
            Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME}: revocation failed (HTTP {status})"),
            ))
        }
    }
}
