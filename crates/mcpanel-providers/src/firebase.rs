//! Firebase Authentication through its documented REST API
//! (https://firebase.google.com/docs/reference/rest/auth): Identity Toolkit
//! `accounts:signUp`, `accounts:signInWithPassword`, `accounts:signInWithIdp`,
//! `accounts:lookup`, `accounts:update`, `accounts:sendOobCode`, and the Secure Token
//! service for refreshing. Google sign-in uses the installed-app OAuth client (the same
//! one as Google Drive) with scopes `openid email profile`; Firebase accepts its ID token
//! when the client belongs to the Firebase project's Google Cloud project.
//!
//! The Web API key identifies the Firebase project (it is not a secret) and comes from
//! `MCPANEL_FIREBASE_API_KEY` at run time, else from the value compiled into the build.

use crate::cloud::CloudClientIds;
use crate::http::HttpClient;
use mcpanel_core::account::{AccountProfile, AuthBackend, AuthSession};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::time::Duration;

pub const FIREBASE_API_KEY_VAR: &str = "MCPANEL_FIREBASE_API_KEY";
const IDENTITY: &str = "https://identitytoolkit.googleapis.com/v1";
const SECURE_TOKEN: &str = "https://securetoken.googleapis.com/v1/token";
const GOOGLE_AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";

/// The Firebase Web API key: runtime environment first, then the build.
pub fn firebase_api_key() -> Option<String> {
    select_api_key(
        std::env::var(FIREBASE_API_KEY_VAR).ok(),
        option_env!("MCPANEL_FIREBASE_API_KEY"),
    )
}

fn select_api_key(runtime: Option<String>, build: Option<&str>) -> Option<String> {
    runtime
        .or_else(|| build.map(str::to_string))
        .map(|s| s.trim().to_string())
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 100
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
}

pub struct FirebaseAuth {
    client: reqwest::Client,
    api_key: Option<String>,
    google_client_id: Option<String>,
    google_secret: Option<SecretString>,
    identity: String,
    secure_token: String,
    google_token: String,
}

impl FirebaseAuth {
    pub fn new(http: &HttpClient, api_key: Option<String>, ids: &CloudClientIds) -> Self {
        Self {
            client: http.inner().clone(),
            api_key,
            google_client_id: ids.google.clone(),
            google_secret: ids.google_secret.clone(),
            identity: IDENTITY.into(),
            secure_token: SECURE_TOKEN.into(),
            google_token: GOOGLE_TOKEN.into(),
        }
    }

    /// Against a local plain-HTTP stand-in (tests).
    #[cfg(test)]
    fn for_test(base: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: Some("test-key".into()),
            google_client_id: Some("123-abc.apps.googleusercontent.com".into()),
            google_secret: Some(SecretString::from("GOCSPX-test".to_string())),
            identity: format!("{base}/v1"),
            secure_token: format!("{base}/token"),
            google_token: format!("{base}/google-token"),
        }
    }

    fn key(&self) -> CoreResult<&str> {
        self.api_key.as_deref().ok_or_else(|| {
            CoreError::new(
                ErrorCode::Unsupported,
                "Accounts are not available in this build of MCPanel.",
            )
        })
    }

    async fn identity(&self, method: &str, body: Value) -> CoreResult<Value> {
        let url = format!("{}/accounts:{method}?key={}", self.identity, self.key()?);
        let resp = self
            .client
            .post(url)
            .timeout(Duration::from_secs(30))
            .json(&body)
            .send()
            .await
            .map_err(unreachable_err)?;
        read(resp).await
    }
}

#[cfg(test)]
mod config_tests {
    use super::select_api_key;

    #[test]
    fn uses_the_compiled_firebase_key_when_no_runtime_value_is_set() {
        assert_eq!(
            select_api_key(None, Some("compiled-key")),
            Some("compiled-key".into())
        );
    }

    #[test]
    fn runtime_firebase_key_takes_precedence_over_the_compiled_value() {
        assert_eq!(
            select_api_key(Some("runtime-key".into()), Some("compiled-key")),
            Some("runtime-key".into())
        );
    }

    #[test]
    fn no_firebase_configuration_stays_unconfigured() {
        assert_eq!(select_api_key(None, None), None);
    }

    #[test]
    fn invalid_firebase_values_are_rejected() {
        assert_eq!(select_api_key(Some("not a key".into()), None), None);
    }
}

fn unreachable_err(e: reqwest::Error) -> CoreError {
    CoreError::new(
        ErrorCode::ProviderUnavailable,
        format!(
            "The account service could not be reached: {}",
            e.without_url()
        ),
    )
    .retryable()
}

async fn read(resp: reqwest::Response) -> CoreResult<Value> {
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    parse(status, &text)
}

/// A successful body, or Firebase's error in plain language.
pub fn parse(status: u16, text: &str) -> CoreResult<Value> {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(v);
    }
    // Identity Toolkit: {"error":{"message":"EMAIL_EXISTS"}}; Secure Token and Google's
    // token endpoint: {"error":"invalid_grant","error_description":…} or the same shape.
    // A rejected Web API key: "API key not valid…" with reason API_KEY_INVALID.
    let reason = v
        .pointer("/error/details/0/reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let message = v
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("");
    if reason == "API_KEY_INVALID" || message.starts_with("API key not valid") {
        return Err(firebase_error(status, "API_KEY_INVALID"));
    }
    let code = v
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| v.get("error").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    Err(firebase_error(status, &code))
}

fn firebase_error(status: u16, code: &str) -> CoreError {
    let head = code.split([' ', ':']).next().unwrap_or(code);
    let (c, m) = match head {
        "EMAIL_EXISTS" => (
            ErrorCode::Conflict,
            "An account with this email already exists. Sign in instead.",
        ),
        "EMAIL_NOT_FOUND" | "INVALID_PASSWORD" | "INVALID_LOGIN_CREDENTIALS" => {
            (ErrorCode::InvalidInput, "The email or password is wrong.")
        }
        "USER_DISABLED" => (ErrorCode::Conflict, "This account has been disabled."),
        "USER_NOT_FOUND" => (ErrorCode::NotFound, "This account no longer exists."),
        "WEAK_PASSWORD" => (ErrorCode::InvalidInput, "Choose a stronger password."),
        "INVALID_EMAIL" | "MISSING_EMAIL" => {
            (ErrorCode::InvalidInput, "Enter a valid email address.")
        }
        "MISSING_PASSWORD" => (ErrorCode::InvalidInput, "Enter your password."),
        "TOO_MANY_ATTEMPTS_TRY_LATER" => (
            ErrorCode::ProviderUnavailable,
            "Too many attempts. Try again later.",
        ),
        "OPERATION_NOT_ALLOWED" | "PASSWORD_LOGIN_DISABLED" => (
            ErrorCode::Unsupported,
            "This sign-in method is turned off in the Firebase project.",
        ),
        "INVALID_IDP_RESPONSE" => (
            ErrorCode::ProviderError,
            "Google sign-in was not accepted. Try again.",
        ),
        "TOKEN_EXPIRED"
        | "INVALID_REFRESH_TOKEN"
        | "INVALID_ID_TOKEN"
        | "invalid_grant"
        | "CREDENTIAL_TOO_OLD_LOGIN_AGAIN" => (
            ErrorCode::GrantInvalid,
            "Your sign-in expired. Sign in again.",
        ),
        "API_KEY_INVALID" | "INVALID_API_KEY" => (
            ErrorCode::ProviderError,
            "This build's Firebase configuration was rejected.",
        ),
        "invalid_client" => (
            ErrorCode::ProviderError,
            "Google rejected this build's sign-in configuration.",
        ),
        _ if status == 429 => (
            ErrorCode::ProviderUnavailable,
            "Too many attempts. Try again later.",
        ),
        _ => {
            return CoreError::new(
                ErrorCode::ProviderError,
                format!(
                    "The account service refused the request ({}).",
                    if head.is_empty() {
                        format!("HTTP {status}")
                    } else {
                        head.chars().take(80).collect()
                    }
                ),
            );
        }
    };
    CoreError::new(c, m)
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn session(v: &Value) -> CoreResult<AuthSession> {
    let id = str_of(v, "idToken").or_else(|| str_of(v, "id_token"));
    let refresh = str_of(v, "refreshToken").or_else(|| str_of(v, "refresh_token"));
    match (id, refresh) {
        (Some(i), Some(r)) => Ok(AuthSession {
            id_token: SecretString::from(i),
            refresh_token: SecretString::from(r),
        }),
        _ => Err(CoreError::new(
            ErrorCode::ProviderError,
            "The account service sent an incomplete answer.",
        )),
    }
}

pub fn parse_profile(v: &Value) -> CoreResult<AccountProfile> {
    let u = v
        .get("users")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or_else(|| CoreError::new(ErrorCode::NotFound, "This account no longer exists."))?;
    let provider = u
        .get("providerUserInfo")
        .and_then(Value::as_array)
        .and_then(|p| {
            p.iter()
                .filter_map(|x| x.get("providerId").and_then(Value::as_str))
                .find(|id| *id == "google.com")
                .or_else(|| {
                    p.first()
                        .and_then(|x| x.get("providerId").and_then(Value::as_str))
                })
        })
        .unwrap_or("password")
        .to_string();
    Ok(AccountProfile {
        uid: str_of(u, "localId").unwrap_or_default(),
        email: str_of(u, "email"),
        email_verified: u
            .get("emailVerified")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        display_name: str_of(u, "displayName").filter(|s| !s.is_empty()),
        photo_url: str_of(u, "photoUrl").filter(|s| s.starts_with("https://")),
        provider,
    })
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[async_trait::async_trait]
impl AuthBackend for FirebaseAuth {
    fn configured(&self) -> bool {
        self.api_key.is_some()
    }

    fn google_available(&self) -> bool {
        self.google_client_id.is_some() && self.google_secret.is_some()
    }

    async fn sign_up(&self, email: &str, password: &str) -> CoreResult<AuthSession> {
        session(
            &self
                .identity(
                    "signUp",
                    json!({ "email": email, "password": password, "returnSecureToken": true }),
                )
                .await?,
        )
    }

    async fn sign_in(&self, email: &str, password: &str) -> CoreResult<AuthSession> {
        session(
            &self
                .identity(
                    "signInWithPassword",
                    json!({ "email": email, "password": password, "returnSecureToken": true }),
                )
                .await?,
        )
    }

    async fn sign_in_with_google(&self, google_id_token: &SecretString) -> CoreResult<AuthSession> {
        let post_body = form(&[
            ("id_token", google_id_token.expose_secret()),
            ("providerId", "google.com"),
        ]);
        session(
            &self
                .identity(
                    "signInWithIdp",
                    json!({
                        "postBody": post_body,
                        "requestUri": "http://localhost",
                        "returnIdpCredential": false,
                        "returnSecureToken": true,
                    }),
                )
                .await?,
        )
    }

    async fn refresh(&self, refresh_token: &SecretString) -> CoreResult<AuthSession> {
        let url = format!("{}?key={}", self.secure_token, self.key()?);
        let resp = self
            .client
            .post(url)
            .timeout(Duration::from_secs(30))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.expose_secret()),
            ]))
            .send()
            .await
            .map_err(unreachable_err)?;
        session(&read(resp).await?)
    }

    async fn lookup(&self, id_token: &SecretString) -> CoreResult<AccountProfile> {
        parse_profile(
            &self
                .identity("lookup", json!({ "idToken": id_token.expose_secret() }))
                .await?,
        )
    }

    async fn send_verification(&self, id_token: &SecretString) -> CoreResult<()> {
        self.identity(
            "sendOobCode",
            json!({ "requestType": "VERIFY_EMAIL", "idToken": id_token.expose_secret() }),
        )
        .await
        .map(drop)
    }

    async fn send_password_reset(&self, email: &str) -> CoreResult<()> {
        self.identity(
            "sendOobCode",
            json!({ "requestType": "PASSWORD_RESET", "email": email }),
        )
        .await
        .map(drop)
    }

    async fn set_display_name(&self, id_token: &SecretString, name: &str) -> CoreResult<()> {
        self.identity(
            "update",
            json!({ "idToken": id_token.expose_secret(), "displayName": name, "returnSecureToken": false }),
        )
        .await
        .map(drop)
    }

    fn google_authorize_url(
        &self,
        redirect: &str,
        challenge: &str,
        state: &str,
    ) -> CoreResult<String> {
        let id = self.google_client_id.as_deref().ok_or_else(|| {
            CoreError::new(
                ErrorCode::Unsupported,
                "Google sign-in is not available in this build.",
            )
        })?;
        Ok(format!(
            "{GOOGLE_AUTH}?{}",
            form(&[
                ("client_id", id),
                ("redirect_uri", redirect),
                ("response_type", "code"),
                ("scope", "openid email profile"),
                ("code_challenge", challenge),
                ("code_challenge_method", "S256"),
                ("state", state),
                ("prompt", "select_account"),
            ])
        ))
    }

    async fn google_exchange(
        &self,
        code: &str,
        verifier: &str,
        redirect: &str,
    ) -> CoreResult<SecretString> {
        let (Some(id), Some(secret)) = (
            self.google_client_id.as_deref(),
            self.google_secret.as_ref(),
        ) else {
            return Err(CoreError::new(
                ErrorCode::Unsupported,
                "Google sign-in is not available in this build.",
            ));
        };
        let resp = self
            .client
            .post(&self.google_token)
            .timeout(Duration::from_secs(30))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form(&[
                ("client_id", id),
                ("client_secret", secret.expose_secret()),
                ("code", code),
                ("code_verifier", verifier),
                ("grant_type", "authorization_code"),
                ("redirect_uri", redirect),
            ]))
            .send()
            .await
            .map_err(unreachable_err)?;
        let v = read(resp).await?;
        str_of(&v, "id_token")
            .map(SecretString::from)
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::ProviderError,
                    "Google did not return an identity token.",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type Seen = Arc<Mutex<Vec<(String, String)>>>;

    /// Answers each request with the next canned (status, body).
    async fn stand_in(replies: Vec<(u16, &'static str)>) -> (String, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            for (status, body) in replies {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        let len: usize = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                            let body =
                                String::from_utf8_lossy(&buf[end + 4..end + 4 + len]).to_string();
                            log.lock().unwrap().push((path, body));
                            break;
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        (base, seen)
    }

    #[tokio::test]
    async fn sign_up_and_lookup_use_the_documented_endpoints() {
        let (base, seen) = stand_in(vec![
            (200, r#"{"idToken":"id-1","refreshToken":"rt-1","expiresIn":"3600","localId":"uid-1","email":"a@b.co"}"#),
            (200, r#"{"users":[{"localId":"uid-1","email":"a@b.co","emailVerified":false,"displayName":"Alex","providerUserInfo":[{"providerId":"password"}]}]}"#),
        ])
        .await;
        let f = FirebaseAuth::for_test(&base);
        let s = f.sign_up("a@b.co", "correct horse").await.unwrap();
        assert_eq!(s.id_token.expose_secret(), "id-1");
        assert_eq!(s.refresh_token.expose_secret(), "rt-1");
        let p = f.lookup(&s.id_token).await.unwrap();
        assert_eq!(p.uid, "uid-1");
        assert_eq!(p.display_name.as_deref(), Some("Alex"));
        assert_eq!(p.provider, "password");
        assert!(!p.email_verified);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].0, "/v1/accounts:signUp?key=test-key");
        let body: Value = serde_json::from_str(&seen[0].1).unwrap();
        assert_eq!(
            body,
            json!({"email":"a@b.co","password":"correct horse","returnSecureToken":true})
        );
        assert_eq!(seen[1].0, "/v1/accounts:lookup?key=test-key");
    }

    #[tokio::test]
    async fn refresh_uses_the_secure_token_form() {
        let (base, seen) = stand_in(vec![(
            200,
            r#"{"id_token":"id-2","refresh_token":"rt-2","expires_in":"3600","user_id":"uid-1"}"#,
        )])
        .await;
        let f = FirebaseAuth::for_test(&base);
        let s = f
            .refresh(&SecretString::from("rt-1".to_string()))
            .await
            .unwrap();
        assert_eq!(s.refresh_token.expose_secret(), "rt-2");
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].0, "/token?key=test-key");
        assert_eq!(seen[0].1, "grant_type=refresh_token&refresh_token=rt-1");
    }

    #[tokio::test]
    async fn google_sign_in_exchanges_the_code_then_signs_in_with_the_id_token() {
        let (base, seen) = stand_in(vec![
            (200, r#"{"access_token":"at","id_token":"google-id","expires_in":3599,"token_type":"Bearer"}"#),
            (200, r#"{"idToken":"id-3","refreshToken":"rt-3","localId":"uid-3"}"#),
        ])
        .await;
        let f = FirebaseAuth::for_test(&base);
        let url = f
            .google_authorize_url("http://127.0.0.1:5000/", "chal", "st")
            .unwrap();
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=123-abc.apps.googleusercontent.com"));
        assert!(url.contains("scope=openid%20email%20profile"));
        assert!(url.contains("code_challenge_method=S256"));
        let gid = f
            .google_exchange("the-code", "verifier", "http://127.0.0.1:5000/")
            .await
            .unwrap();
        assert_eq!(gid.expose_secret(), "google-id");
        let s = f.sign_in_with_google(&gid).await.unwrap();
        assert_eq!(s.id_token.expose_secret(), "id-3");
        let seen = seen.lock().unwrap().clone();
        assert!(seen[0].1.contains("code_verifier=verifier"));
        assert!(seen[0].1.contains("client_secret=GOCSPX-test"));
        assert_eq!(seen[1].0, "/v1/accounts:signInWithIdp?key=test-key");
        let body: Value = serde_json::from_str(&seen[1].1).unwrap();
        assert_eq!(body["postBody"], "id_token=google-id&providerId=google.com");
    }

    #[test]
    fn errors_are_explained() {
        let e = parse(400, r#"{"error":{"code":400,"message":"EMAIL_EXISTS"}}"#).unwrap_err();
        assert_eq!(e.code, ErrorCode::Conflict);
        let e = parse(400, r#"{"error":{"code":400,"message":"WEAK_PASSWORD : Password should be at least 6 characters"}}"#).unwrap_err();
        assert_eq!(e.message, "Choose a stronger password.");
        let e = parse(400, r#"{"error":{"message":"INVALID_LOGIN_CREDENTIALS"}}"#).unwrap_err();
        assert_eq!(e.message, "The email or password is wrong.");
        let e = parse(
            400,
            r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::GrantInvalid);
        let e = parse(400, r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"reason":"API_KEY_INVALID"}]}}"#).unwrap_err();
        assert_eq!(
            e.message,
            "This build's Firebase configuration was rejected."
        );
        let e = parse(500, "oops").unwrap_err();
        assert!(e.message.contains("HTTP 500"));
    }

    #[test]
    fn google_profiles_are_recognised() {
        let p = parse_profile(&json!({"users":[{"localId":"u","email":"g@gmail.com","emailVerified":true,
            "photoUrl":"https://lh3.googleusercontent.com/a/x","providerUserInfo":[{"providerId":"google.com"}]}]}))
        .unwrap();
        assert_eq!(p.provider, "google.com");
        assert!(p.email_verified);
        assert!(p.photo_url.is_some());
        let p = parse_profile(&json!({"users":[{"localId":"u","photoUrl":"http://insecure"}]}))
            .unwrap();
        assert!(p.photo_url.is_none(), "only https pictures");
    }
}
