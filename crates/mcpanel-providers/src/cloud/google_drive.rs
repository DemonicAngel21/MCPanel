//! Google Drive (Drive API v3). Verified 2026-09-30 against Google's documentation:
//! - OAuth 2.0 for installed apps (developers.google.com/identity/protocols/oauth2/native-app):
//!   client type "Desktop app"; loopback redirect `http://127.0.0.1:{port}` on any free
//!   port (not pre-registered); PKCE S256; authorization endpoint
//!   `https://accounts.google.com/o/oauth2/v2/auth`, token endpoint
//!   `https://oauth2.googleapis.com/token` where `client_secret` is documented as
//!   *Optional* for code exchange and refresh ("installed apps … cannot keep secrets");
//!   refresh tokens are returned for installed apps; revocation at
//!   `https://oauth2.googleapis.com/revoke`.
//! - The actual endpoint, however, answers `invalid_request` "client_secret is missing."
//!   for "Desktop app" clients without it (checked 2026-09-30), and Google's OAuth overview
//!   says installed apps embed that secret: "(In this context, the client secret is
//!   obviously not treated as a secret.)" MCPanel therefore sends the embedded Desktop
//!   client secret — only on the code exchange and the refresh — in addition to PKCE.
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
    /// Desktop client secret (non-confidential, see the module docs). Never logged.
    client_secret: Option<SecretString>,
}

impl GoogleDrive {
    pub fn new(http: HttpClient, client_id: Option<String>) -> Self {
        Self::with_secret(http, client_id, None)
    }

    pub fn with_secret(
        http: HttpClient,
        client_id: Option<String>,
        client_secret: Option<SecretString>,
    ) -> Self {
        Self {
            http,
            client_secret,
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

    /// Form fields of the authorization-code exchange (PKCE verifier + client secret).
    fn exchange_params<'a>(
        &'a self,
        code: &'a str,
        verifier: &'a str,
        redirect_uri: &'a str,
    ) -> CoreResult<Vec<(&'a str, &'a str)>> {
        let mut p = vec![
            ("client_id", self.client_id()?),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ];
        if let Some(s) = &self.client_secret {
            p.push(("client_secret", s.expose_secret()));
        }
        Ok(p)
    }

    /// Form fields of the refresh-token exchange (client secret included).
    fn refresh_params<'a>(
        &'a self,
        refresh_token: &'a SecretString,
    ) -> CoreResult<Vec<(&'a str, &'a str)>> {
        let mut p = vec![
            ("client_id", self.client_id()?),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose_secret()),
        ];
        if let Some(s) = &self.client_secret {
            p.push(("client_secret", s.expose_secret()));
        }
        Ok(p)
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
        let params = self.exchange_params(code, verifier, redirect_uri)?;
        token_request(&self.http, NAME, Stage::Exchange, TOKEN_URL, &params).await
    }

    async fn refresh(&self, refresh_token: &SecretString) -> CoreResult<TokenSet> {
        let params = self.refresh_params(refresh_token)?;
        token_request(&self.http, NAME, Stage::Refresh, TOKEN_URL, &params).await
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

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "GOCSPX-test-only-not-real";

    fn provider(secret: Option<&str>) -> GoogleDrive {
        GoogleDrive::with_secret(
            crate::http_client().unwrap(),
            Some("123-abc.apps.googleusercontent.com".into()),
            secret.map(|s| SecretString::from(s.to_string())),
        )
    }

    #[test]
    fn code_exchange_sends_pkce_verifier_and_client_secret() {
        let g = provider(Some(SECRET));
        let p = g
            .exchange_params("the-code", "verifier-43-chars", "http://127.0.0.1:5000/")
            .unwrap();
        let get = |k: &str| p.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        assert_eq!(get("code_verifier"), Some("verifier-43-chars"));
        assert_eq!(get("client_secret"), Some(SECRET));
        assert_eq!(get("grant_type"), Some("authorization_code"));
        assert_eq!(get("client_id"), Some("123-abc.apps.googleusercontent.com"));
        assert_eq!(get("redirect_uri"), Some("http://127.0.0.1:5000/"));
    }

    #[test]
    fn refresh_sends_client_secret() {
        let g = provider(Some(SECRET));
        let rt = SecretString::from("1//refresh".to_string());
        let p = g.refresh_params(&rt).unwrap();
        let get = |k: &str| p.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        assert_eq!(get("grant_type"), Some("refresh_token"));
        assert_eq!(get("refresh_token"), Some("1//refresh"));
        assert_eq!(get("client_secret"), Some(SECRET));
        // Without a configured secret nothing is invented.
        let none = provider(None);
        let p = none.refresh_params(&rt).unwrap();
        assert!(!p.iter().any(|(k, _)| *k == "client_secret"));
    }

    #[test]
    fn the_secret_never_appears_in_errors_or_debug_output() {
        let e = crate::cloud::parse_token_response(
            NAME,
            Stage::Exchange,
            401,
            br#"{"error":"invalid_client","error_description":"The provided client secret is invalid."}"#,
        )
        .err()
        .unwrap();
        assert!(!e.message.contains(SECRET));
        let ids = crate::cloud::CloudClientIds {
            google_secret: Some(SecretString::from(SECRET.to_string())),
            ..Default::default()
        };
        let dbg = format!("{ids:?}");
        assert!(!dbg.contains(SECRET) && dbg.contains("[set]"), "{dbg}");
        let info = format!("{:?}", provider(Some(SECRET)).info());
        assert!(!info.contains(SECRET), "{info}");
    }
}
