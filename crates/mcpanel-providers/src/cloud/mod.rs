//! Cloud storage providers (Google Drive, OneDrive, Dropbox): OAuth 2.0 public clients
//! with PKCE. Verified requirements: docs/architecture/verification-log.md
//! ("Cloud storage OAuth").

pub mod dropbox;
pub mod google_drive;
pub mod onedrive;

use crate::http::HttpClient;
use mcpanel_core::cloud::TokenSet;
use mcpanel_core::cloud::service::encode_form;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::time::Timestamp;
use secrecy::SecretString;
use serde::Deserialize;

/// OAuth client IDs of MCPanel's app registrations. Public client IDs are not secrets,
/// but they are kept out of the repository: a release build bakes them in from its build
/// environment, and the same variables can be set at runtime (development).
#[derive(Clone, Default)]
pub struct CloudClientIds {
    pub google: Option<String>,
    /// Google's "Desktop app" client secret. Google's token endpoint requires it for this
    /// client type even with PKCE, and Google documents that for installed apps it is
    /// embedded in the application and "obviously not treated as a secret". It is still
    /// kept out of Git, logs, diagnostics and the UI.
    pub google_secret: Option<SecretString>,
    pub microsoft: Option<String>,
    pub dropbox: Option<String>,
}

impl std::fmt::Debug for CloudClientIds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudClientIds")
            .field("google", &self.google)
            .field(
                "google_secret",
                &self.google_secret.as_ref().map(|_| "[set]"),
            )
            .field("microsoft", &self.microsoft)
            .field("dropbox", &self.dropbox)
            .finish()
    }
}

pub const GOOGLE_CLIENT_ID_VAR: &str = "MCPANEL_GOOGLE_CLIENT_ID";
pub const MICROSOFT_CLIENT_ID_VAR: &str = "MCPANEL_MICROSOFT_CLIENT_ID";
pub const DROPBOX_CLIENT_ID_VAR: &str = "MCPANEL_DROPBOX_CLIENT_ID";
pub const GOOGLE_CLIENT_SECRET_VAR: &str = "MCPANEL_GOOGLE_CLIENT_SECRET";

fn pick(runtime: Option<String>, build: Option<&'static str>) -> Option<String> {
    runtime
        .or_else(|| build.map(str::to_string))
        .map(|s| s.trim().to_string())
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 200
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
        })
}

impl CloudClientIds {
    /// Runtime environment first, then the value compiled into this build.
    pub fn from_env() -> Self {
        Self {
            google: pick(
                std::env::var(GOOGLE_CLIENT_ID_VAR).ok(),
                option_env!("MCPANEL_GOOGLE_CLIENT_ID"),
            ),
            google_secret: pick(
                std::env::var(GOOGLE_CLIENT_SECRET_VAR).ok(),
                option_env!("MCPANEL_GOOGLE_CLIENT_SECRET"),
            )
            .map(SecretString::from),
            microsoft: pick(
                std::env::var(MICROSOFT_CLIENT_ID_VAR).ok(),
                option_env!("MCPANEL_MICROSOFT_CLIENT_ID"),
            ),
            dropbox: pick(
                std::env::var(DROPBOX_CLIENT_ID_VAR).ok(),
                option_env!("MCPANEL_DROPBOX_CLIENT_ID"),
            ),
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Which token request failed (the same OAuth error means different things).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Exchange,
    Refresh,
}

/// Map an OAuth token-endpoint error to a clear message. The message always says that
/// the provider's token server answered (not MCPanel), at which step, and quotes the raw
/// OAuth error so it can be looked up.
fn token_error(
    provider: &str,
    stage: Stage,
    status: u16,
    error: Option<&str>,
    description: Option<&str>,
) -> CoreError {
    let step = match stage {
        Stage::Exchange => "the sign-in code exchange",
        Stage::Refresh => "the token refresh",
    };
    // Some providers append a description to the code ("invalid_client: Invalid client_id").
    let code = error.map(|e| e.split(':').next().unwrap_or(e).trim());
    let raw = match (error, description) {
        (Some(e), Some(d)) => format!(" [{provider} said: {e} — {d}]"),
        (Some(e), None) => format!(" [{provider} said: {e}]"),
        (None, _) => format!(" [HTTP {status}]"),
    };
    let secret_demanded =
        description.is_some_and(|d| d.to_ascii_lowercase().contains("client_secret"));
    let msg = match code {
        Some("invalid_request") if secret_demanded => format!(
            "{provider}'s token server rejected {step} because it expects a client secret for this OAuth client. MCPanel is a public client and sends none by design (PKCE instead).{raw}"
        ),
        Some("invalid_grant") if stage == Stage::Exchange => format!(
            "{provider}'s token server did not accept the sign-in code (it may have expired). Try connecting again.{raw}"
        ),
        Some("invalid_grant") => format!(
            "{provider}'s token server no longer accepts MCPanel's authorization — it expired or was revoked. Connect again.{raw}"
        ),
        Some("invalid_client") | Some("unauthorized_client") => format!(
            "{provider}'s token server rejected {step}: MCPanel's OAuth client credentials (client ID or client secret) were not accepted.{raw}"
        ),
        Some(_) => format!("{provider}'s token server rejected {step}.{raw}"),
        None => format!("{provider}'s token server failed during {step}.{raw}"),
    };
    let err = CoreError::new(ErrorCode::ProviderError, msg);
    if status >= 500 || status == 429 {
        err.retryable()
    } else {
        err
    }
}

/// POST a form to a token endpoint and parse the standard OAuth token response.
pub(crate) async fn token_request(
    http: &HttpClient,
    provider: &str,
    stage: Stage,
    url: &str,
    params: &[(&str, &str)],
) -> CoreResult<TokenSet> {
    let (status, body) = http
        .send_raw(
            reqwest::Method::POST,
            url,
            None,
            Some(("application/x-www-form-urlencoded", encode_form(params))),
        )
        .await?;
    parse_token_response(provider, stage, status, &body)
}

pub(crate) fn parse_token_response(
    provider: &str,
    stage: Stage,
    status: u16,
    body: &[u8],
) -> CoreResult<TokenSet> {
    let parsed: Option<TokenResponse> = serde_json::from_slice(body).ok();
    match parsed {
        Some(TokenResponse {
            access_token: Some(at),
            refresh_token,
            expires_in,
            error: None,
            ..
        }) if (200..300).contains(&status) => Ok(TokenSet {
            access_token: SecretString::from(at),
            refresh_token: refresh_token.map(SecretString::from),
            expires_at: expires_in.map(|s| Timestamp(Timestamp::now().millis() + s * 1000)),
        }),
        Some(r) => Err(token_error(
            provider,
            stage,
            status,
            r.error.as_deref(),
            r.error_description.as_deref(),
        )),
        None => Err(token_error(provider, stage, status, None, None)),
    }
}

/// Parse a JSON API response, mapping HTTP errors to provider-specific messages.
pub(crate) fn api_json<T: serde::de::DeserializeOwned>(
    provider: &str,
    what: &str,
    status: u16,
    body: &[u8],
) -> CoreResult<T> {
    if (200..300).contains(&status) {
        return serde_json::from_slice(body).map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("{provider}: unexpected {what} response ({e})"),
            )
        });
    }
    let msg = match status {
        401 => format!("{provider}: the sign-in is no longer valid. Connect again."),
        403 => format!(
            "{provider}: MCPanel is not allowed to do this ({what}). Check the granted permissions."
        ),
        429 => format!("{provider}: too many requests; try again later."),
        s if s >= 500 => format!("{provider} is temporarily unavailable (HTTP {s})."),
        s => format!("{provider}: {what} failed (HTTP {s})."),
    };
    let err = CoreError::new(ErrorCode::ProviderError, msg);
    Err(if status >= 500 || status == 429 {
        err.retryable()
    } else {
        err
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn token_responses_and_errors() {
        let t = parse_token_response(
            "Google Drive",
            Stage::Exchange,
            200,
            br#"{"access_token":"at","expires_in":3599,"refresh_token":"rt","token_type":"Bearer"}"#,
        )
        .unwrap();
        assert_eq!(t.access_token.expose_secret(), "at");
        assert_eq!(t.refresh_token.unwrap().expose_secret(), "rt");
        assert!(t.expires_at.unwrap().millis() > Timestamp::now().millis());
        let e = parse_token_response(
            "Dropbox",
            Stage::Refresh,
            400,
            br#"{"error":"invalid_grant","error_description":"refresh token is malformed"}"#,
        )
        .err()
        .unwrap();
        assert!(e.message.contains("Connect again") && e.message.contains("malformed"));
        let e = parse_token_response("OneDrive", Stage::Refresh, 503, b"<html>")
            .err()
            .unwrap();
        assert!(e.retryable);
        let e = parse_token_response(
            "OneDrive",
            Stage::Exchange,
            400,
            br#"{"error":"invalid_client: Invalid client_id"}"#,
        )
        .err()
        .unwrap();
        assert!(e.message.contains("client credentials"), "{}", e.message);
    }

    #[test]
    fn provider_answers_are_attributed_to_the_provider_and_step() {
        // Google's real answer for a "Desktop app" client without a secret (2026-09-30).
        let e = parse_token_response(
            "Google Drive",
            Stage::Exchange,
            400,
            br#"{"error":"invalid_request","error_description":"client_secret is missing."}"#,
        )
        .err()
        .unwrap();
        assert!(
            e.message
                .starts_with("Google Drive's token server rejected the sign-in code exchange"),
            "{}",
            e.message
        );
        assert!(
            e.message.contains("MCPanel is a public client"),
            "{}",
            e.message
        );
        assert!(
            e.message
                .contains("[Google Drive said: invalid_request — client_secret is missing.]"),
            "{}",
            e.message
        );
        let e = parse_token_response(
            "OneDrive",
            Stage::Refresh,
            400,
            br#"{"error":"invalid_request","error_description":"x"}"#,
        )
        .err()
        .unwrap();
        assert_eq!(
            e.message,
            "OneDrive's token server rejected the token refresh. [OneDrive said: invalid_request — x]"
        );
    }

    #[test]
    fn client_ids_are_validated() {
        assert_eq!(
            pick(Some(" abc-1.apps.googleusercontent.com ".into()), None).as_deref(),
            Some("abc-1.apps.googleusercontent.com")
        );
        assert_eq!(pick(Some("".into()), Some("x")).as_deref(), None);
        assert_eq!(
            pick(None, Some("00001111-aaaa-2222")).as_deref(),
            Some("00001111-aaaa-2222")
        );
        assert_eq!(pick(Some("bad id\n".into()), None), None);
        assert_eq!(pick(None, None), None);
    }

    #[test]
    fn providers_describe_their_verified_configuration() {
        use mcpanel_core::cloud::CloudStorageProvider;
        use mcpanel_core::cloud::service::registered_redirect_uris;
        let http = crate::http_client().unwrap();
        let g = google_drive::GoogleDrive::new(http.clone(), Some("g".into()));
        assert_eq!(
            g.info().scopes,
            ["https://www.googleapis.com/auth/drive.file"]
        );
        assert_eq!(g.info().redirect.uri(5000), "http://127.0.0.1:5000/");
        let o = onedrive::OneDrive::new(http.clone(), Some("o".into()));
        assert_eq!(
            o.info().scopes,
            ["offline_access", "User.Read", "Files.ReadWrite.AppFolder"]
        );
        assert_eq!(
            registered_redirect_uris(&o.info().redirect),
            vec!["http://localhost/mcpanel/oauth"]
        );
        let d = dropbox::Dropbox::new(http, None);
        assert!(d.info().client_id.is_none());
        assert_eq!(
            registered_redirect_uris(&d.info().redirect),
            vec![
                "http://localhost:43917/mcpanel/oauth",
                "http://localhost:43918/mcpanel/oauth",
                "http://localhost:43919/mcpanel/oauth"
            ]
        );
        assert_eq!(
            d.info().extra_authorize_params,
            [("token_access_type", "offline")]
        );
    }
}
