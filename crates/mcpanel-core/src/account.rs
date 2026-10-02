//! MCPanel accounts (Firebase Authentication).
//!
//! Sign-up and sign-in with email and password, or with Google (desktop OAuth with PKCE
//! and a loopback redirect, whose ID token Firebase exchanges for its own session). The
//! Firebase refresh token is kept in the Windows Credential Manager; the profile (name,
//! email, picture, verification state) is cached in the settings so the app can show it
//! offline. Passwords are sent to Firebase over HTTPS and never stored.

use crate::cloud::loopback::{Callback, Loopback};
use crate::cloud::pkce;
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::ports::{SecretStore, SettingsRepository};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const REFRESH_SECRET: &str = "account-refresh-token";
const PROFILE_KEY: &str = "account.profile";
pub const OAUTH_CREDENTIALS_SETTINGS_KEY: &str = "account.oauth_credentials";
pub const GOOGLE_SECRET_KEY: &str = "account-google-secret";
pub const MICROSOFT_SECRET_KEY: &str = "account-microsoft-secret";
pub const GUEST_KEY: &str = "account.is_guest";
const GOOGLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MICROSOFT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Temporary block for Microsoft authentication until Azure account is configured by owner.
pub const MICROSOFT_AUTH_TEMPORARILY_BLOCKED: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Guest,
    #[default]
    Email,
    Google,
    Microsoft,
}

impl std::fmt::Display for AccountType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Guest => write!(f, "guest"),
            Self::Email => write!(f, "email"),
            Self::Google => write!(f, "google"),
            Self::Microsoft => write!(f, "microsoft"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(pub String);

impl AccountId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn guest() -> Self {
        Self("guest".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SavedOAuthCredentials {
    pub firebase_api_key: Option<String>,
    pub google_client_id: Option<String>,
    pub microsoft_client_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountProfile {
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_type: AccountType,
    pub uid: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub display_name: Option<String>,
    pub photo_url: Option<String>,
    /// "password" | "google.com" | "microsoft.com" | "guest"
    pub provider: String,
}

impl AccountProfile {
    pub fn guest() -> Self {
        Self {
            account_id: "guest".to_string(),
            account_type: AccountType::Guest,
            uid: "guest".to_string(),
            email: None,
            email_verified: false,
            display_name: Some("Guest".to_string()),
            photo_url: None,
            provider: "guest".to_string(),
        }
    }

    pub fn is_guest(&self) -> bool {
        self.account_type == AccountType::Guest || self.provider == "guest"
    }
}

/// A Firebase session (the ID token is short-lived; the refresh token is kept).
pub struct AuthSession {
    pub id_token: SecretString,
    pub refresh_token: SecretString,
}

#[derive(Debug, Clone)]
pub struct GoogleAuthTokens {
    pub id_token: SecretString,
    pub refresh_token: Option<SecretString>,
    pub access_token: Option<SecretString>,
}

#[async_trait::async_trait]
pub trait AuthBackend: Send + Sync {
    /// A Firebase project is configured for this build.
    fn configured(&self) -> bool;
    /// Google sign-in is possible (Google OAuth client configured too).
    fn google_available(&self) -> bool;
    /// Microsoft sign-in is possible (Microsoft OAuth client configured too).
    fn microsoft_available(&self) -> bool {
        false
    }
    async fn sign_up(&self, email: &str, password: &str) -> CoreResult<AuthSession>;
    async fn sign_in(&self, email: &str, password: &str) -> CoreResult<AuthSession>;
    /// Firebase session from a Google ID token.
    async fn sign_in_with_google(&self, google_id_token: &SecretString) -> CoreResult<AuthSession>;
    /// Firebase session from a Microsoft ID token and optional access token.
    async fn sign_in_with_microsoft(
        &self,
        _id_token: &SecretString,
        _access_token: Option<&SecretString>,
    ) -> CoreResult<AuthSession> {
        Err(CoreError::new(
            ErrorCode::Unsupported,
            "Microsoft sign-in is not supported.",
        ))
    }
    async fn refresh(&self, refresh_token: &SecretString) -> CoreResult<AuthSession>;
    async fn lookup(&self, id_token: &SecretString) -> CoreResult<AccountProfile>;
    async fn send_verification(&self, id_token: &SecretString) -> CoreResult<()>;
    async fn send_password_reset(&self, email: &str) -> CoreResult<()>;
    async fn set_display_name(&self, id_token: &SecretString, name: &str) -> CoreResult<()>;
    /// Google's consent page for `redirect` with a PKCE `challenge`.
    fn google_authorize_url(
        &self,
        redirect: &str,
        challenge: &str,
        state: &str,
    ) -> CoreResult<String>;
    /// Exchange Google's code for its tokens.
    async fn google_exchange(
        &self,
        code: &str,
        verifier: &str,
        redirect: &str,
    ) -> CoreResult<GoogleAuthTokens>;
    /// Microsoft's consent page for `redirect` with a PKCE `challenge`.
    fn microsoft_authorize_url(
        &self,
        _redirect: &str,
        _challenge: &str,
        _state: &str,
    ) -> CoreResult<String> {
        Err(CoreError::new(
            ErrorCode::Unsupported,
            "Microsoft sign-in is not supported.",
        ))
    }
    /// Exchange Microsoft's code for its ID token and access token.
    async fn microsoft_exchange(
        &self,
        _code: &str,
        _verifier: &str,
        _redirect: &str,
    ) -> CoreResult<(SecretString, Option<SecretString>)> {
        Err(CoreError::new(
            ErrorCode::Unsupported,
            "Microsoft sign-in is not supported.",
        ))
    }
    /// Remote settings sync backend provider if supported.
    fn sync_backend(&self) -> Option<&dyn crate::sync::SettingsSyncBackend> {
        None
    }
    /// Update or configure credentials dynamically at runtime.
    fn configure_credentials(
        &self,
        _firebase_api_key: Option<String>,
        _google_client_id: Option<String>,
        _google_secret: Option<SecretString>,
        _microsoft_client_id: Option<String>,
        _microsoft_secret: Option<SecretString>,
    ) {
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum GoogleSignIn {
    Waiting { url: String },
    Done,
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MicrosoftSignIn {
    Waiting { url: String },
    Done,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStatus {
    pub configured: bool,
    pub google_available: bool,
    pub microsoft_available: bool,
    pub signed_in: bool,
    pub is_guest: bool,
    pub profile: Option<AccountProfile>,
    pub google: Option<GoogleSignIn>,
    pub microsoft: Option<MicrosoftSignIn>,
    pub custom_oauth_configured: bool,
}

/// A pending Google sign-in: its progress and the task waiting for the redirect.
type GoogleFlow = (Arc<Mutex<GoogleSignIn>>, tokio::task::JoinHandle<()>);
/// A pending Microsoft sign-in: its progress and the task waiting for the redirect.
type MicrosoftFlow = (Arc<Mutex<MicrosoftSignIn>>, tokio::task::JoinHandle<()>);

pub struct AccountService {
    backend: OnceLock<Arc<dyn AuthBackend>>,
    secrets: Arc<dyn SecretStore>,
    settings: Arc<dyn SettingsRepository>,
    google: Mutex<Option<GoogleFlow>>,
    microsoft: Mutex<Option<MicrosoftFlow>>,
    sync_service: crate::sync::SettingsSyncService,
    credentials_loaded: AtomicBool,
    cloud: OnceLock<Arc<crate::cloud::CloudService>>,
}

/// A plausible email address (Firebase does the real check).
fn valid_email(email: &str) -> bool {
    let e = email.trim();
    let Some((local, domain)) = e.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && e.len() <= 254
        && !e.chars().any(char::is_whitespace)
}

fn check_password(password: &str) -> CoreResult<()> {
    if password.chars().count() < 8 {
        return Err(CoreError::invalid(
            "Use a password with at least 8 characters.",
        ));
    }
    if password.len() > 256 {
        return Err(CoreError::invalid("The password is too long."));
    }
    Ok(())
}

impl AccountService {
    pub fn new(secrets: Arc<dyn SecretStore>, settings: Arc<dyn SettingsRepository>) -> Self {
        Self {
            backend: OnceLock::new(),
            secrets,
            sync_service: crate::sync::SettingsSyncService::new(Arc::clone(&settings)),
            settings,
            google: Mutex::new(None),
            microsoft: Mutex::new(None),
            credentials_loaded: AtomicBool::new(false),
            cloud: OnceLock::new(),
        }
    }

    pub fn set_backend(&self, backend: Arc<dyn AuthBackend>) {
        let _ = self.backend.set(backend);
    }

    pub fn set_cloud(&self, cloud: Arc<crate::cloud::CloudService>) {
        let _ = self.cloud.set(cloud);
    }

    fn backend(&self) -> CoreResult<Arc<dyn AuthBackend>> {
        self.backend
            .get()
            .filter(|b| b.configured())
            .cloned()
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::Unsupported,
                    "Accounts are not available in this build of MCPanel.",
                )
            })
    }

    pub async fn load_saved_credentials(&self) -> CoreResult<()> {
        let creds: Option<SavedOAuthCredentials> = self
            .settings
            .get(OAUTH_CREDENTIALS_SETTINGS_KEY)
            .await?
            .and_then(|v| serde_json::from_value(v).ok());
        let google_secret = self.secrets.get(GOOGLE_SECRET_KEY).ok().flatten();
        let microsoft_secret = self.secrets.get(MICROSOFT_SECRET_KEY).ok().flatten();
        if let Some(b) = self.backend.get() {
            if let Some(ref c) = creds {
                b.configure_credentials(
                    c.firebase_api_key.clone(),
                    c.google_client_id.clone(),
                    google_secret,
                    c.microsoft_client_id.clone(),
                    microsoft_secret,
                );
            } else if google_secret.is_some() || microsoft_secret.is_some() {
                b.configure_credentials(None, None, google_secret, None, microsoft_secret);
            }
        }
        self.credentials_loaded.store(true, Ordering::Release);
        Ok(())
    }

    pub async fn configure_credentials(
        &self,
        firebase_api_key: Option<String>,
        google_client_id: Option<String>,
        google_client_secret: Option<SecretString>,
        microsoft_client_id: Option<String>,
        microsoft_client_secret: Option<SecretString>,
    ) -> CoreResult<()> {
        let mut creds: SavedOAuthCredentials = self
            .settings
            .get(OAUTH_CREDENTIALS_SETTINGS_KEY)
            .await?
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        if let Some(key) = firebase_api_key {
            let k = key.trim().to_string();
            creds.firebase_api_key = if k.is_empty() { None } else { Some(k) };
        }
        if let Some(id) = google_client_id {
            let i = id.trim().to_string();
            creds.google_client_id = if i.is_empty() { None } else { Some(i) };
        }
        if let Some(id) = microsoft_client_id {
            let i = id.trim().to_string();
            creds.microsoft_client_id = if i.is_empty() { None } else { Some(i) };
        }

        let val = serde_json::to_value(&creds).map_err(|e| CoreError::internal(e.to_string()))?;
        self.settings
            .set(OAUTH_CREDENTIALS_SETTINGS_KEY, &val)
            .await?;

        if let Some(ref sec) = google_client_secret {
            let s = sec.expose_secret().trim();
            if s.is_empty() {
                self.secrets.delete(GOOGLE_SECRET_KEY)?;
            } else {
                self.secrets.set(GOOGLE_SECRET_KEY, sec)?;
            }
        }
        if let Some(ref sec) = microsoft_client_secret {
            let s = sec.expose_secret().trim();
            if s.is_empty() {
                self.secrets.delete(MICROSOFT_SECRET_KEY)?;
            } else {
                self.secrets.set(MICROSOFT_SECRET_KEY, sec)?;
            }
        }

        let current_google_secret = match google_client_secret {
            Some(sec) => {
                if sec.expose_secret().trim().is_empty() {
                    None
                } else {
                    Some(sec)
                }
            }
            None => self.secrets.get(GOOGLE_SECRET_KEY).ok().flatten(),
        };
        let current_microsoft_secret = match microsoft_client_secret {
            Some(sec) => {
                if sec.expose_secret().trim().is_empty() {
                    None
                } else {
                    Some(sec)
                }
            }
            None => self.secrets.get(MICROSOFT_SECRET_KEY).ok().flatten(),
        };

        if let Some(b) = self.backend.get() {
            b.configure_credentials(
                creds.firebase_api_key.clone(),
                creds.google_client_id.clone(),
                current_google_secret,
                creds.microsoft_client_id.clone(),
                current_microsoft_secret,
            );
        }
        self.credentials_loaded.store(true, Ordering::Release);
        Ok(())
    }

    async fn cached_profile(&self) -> CoreResult<Option<AccountProfile>> {
        Ok(self
            .settings
            .get(PROFILE_KEY)
            .await?
            .and_then(|v| serde_json::from_value(v).ok()))
    }

    async fn save_profile(&self, p: Option<&AccountProfile>) -> CoreResult<()> {
        let v = match p {
            Some(p) => serde_json::to_value(p).map_err(|e| CoreError::internal(e.to_string()))?,
            None => serde_json::Value::Null,
        };
        self.settings.set(PROFILE_KEY, &v).await
    }

    fn google_progress(&self) -> Option<GoogleSignIn> {
        self.google
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(p, _)| p.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    fn microsoft_progress(&self) -> Option<MicrosoftSignIn> {
        self.microsoft
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(p, _)| p.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    pub async fn is_guest(&self) -> CoreResult<bool> {
        let signed_in = self.secrets.get(REFRESH_SECRET)?.is_some();
        if signed_in {
            return Ok(false);
        }
        Ok(self
            .settings
            .get(GUEST_KEY)
            .await?
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    pub async fn enter_guest_mode(&self) -> CoreResult<AccountProfile> {
        self.cancel_google();
        self.cancel_microsoft();
        self.secrets.delete(REFRESH_SECRET)?;
        self.settings
            .set(GUEST_KEY, &serde_json::Value::Bool(true))
            .await?;
        let guest_profile = AccountProfile::guest();
        self.save_profile(Some(&guest_profile)).await?;
        Ok(guest_profile)
    }

    pub async fn status(&self) -> CoreResult<AccountStatus> {
        if !self.credentials_loaded.load(Ordering::Acquire) {
            let _ = self.load_saved_credentials().await;
        }
        let backend = self.backend.get();
        let signed_in = self.secrets.get(REFRESH_SECRET)?.is_some();
        let is_guest = if signed_in {
            false
        } else {
            self.settings
                .get(GUEST_KEY)
                .await?
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        let custom_oauth_configured = self
            .settings
            .get(OAUTH_CREDENTIALS_SETTINGS_KEY)
            .await?
            .and_then(|v| serde_json::from_value::<SavedOAuthCredentials>(v).ok())
            .map(|c| {
                c.google_client_id.is_some()
                    || c.microsoft_client_id.is_some()
                    || c.firebase_api_key.is_some()
            })
            .unwrap_or(false)
            || self.secrets.get(GOOGLE_SECRET_KEY)?.is_some()
            || self.secrets.get(MICROSOFT_SECRET_KEY)?.is_some();

        let profile = if signed_in {
            self.cached_profile().await?
        } else if is_guest {
            Some(AccountProfile::guest())
        } else {
            None
        };

        let microsoft_available = !MICROSOFT_AUTH_TEMPORARILY_BLOCKED
            && backend.is_some_and(|b| b.configured() && b.microsoft_available());

        Ok(AccountStatus {
            configured: backend.is_some_and(|b| b.configured()),
            google_available: backend.is_some_and(|b| b.configured() && b.google_available()),
            microsoft_available,
            signed_in,
            is_guest,
            profile,
            google: self.google_progress(),
            microsoft: self.microsoft_progress(),
            custom_oauth_configured,
        })
    }

    /// Keep the session and cache the profile.
    async fn establish(
        &self,
        backend: &dyn AuthBackend,
        s: AuthSession,
    ) -> CoreResult<AccountProfile> {
        let mut profile = backend.lookup(&s.id_token).await?;
        if profile.account_id.is_empty() {
            profile.account_id = profile.uid.clone();
        }
        profile.account_type = match profile.provider.as_str() {
            "google.com" => AccountType::Google,
            "microsoft.com" => AccountType::Microsoft,
            "guest" => AccountType::Guest,
            _ => AccountType::Email,
        };

        self.secrets.set(REFRESH_SECRET, &s.refresh_token)?;
        self.settings
            .set(GUEST_KEY, &serde_json::Value::Bool(false))
            .await?;
        self.save_profile(Some(&profile)).await?;

        // Sync preferences with cloud if sync backend is supported
        if let Some(sync_backend) = backend.sync_backend() {
            let app_settings = crate::settings::AppSettings::load(&*self.settings)
                .await
                .unwrap_or_default();
            if let Err(e) = self
                .sync_service
                .sync_on_login(sync_backend, &s.id_token, &profile.uid, &app_settings)
                .await
            {
                tracing::warn!(target: "mcpanel::account", "Cloud settings sync failed on login: {}", e.message);
            }
        }

        Ok(profile)
    }

    /// A fresh ID token for the signed-in account (rotating the refresh token).
    async fn id_token(&self, backend: &dyn AuthBackend) -> CoreResult<SecretString> {
        let refresh = self
            .secrets
            .get(REFRESH_SECRET)?
            .ok_or_else(|| CoreError::new(ErrorCode::Conflict, "You are not signed in."))?;
        let s = backend.refresh(&refresh).await?;
        self.secrets.set(REFRESH_SECRET, &s.refresh_token)?;
        Ok(s.id_token)
    }

    pub async fn sign_up(
        &self,
        email: &str,
        password: &str,
        display_name: Option<&str>,
    ) -> CoreResult<AccountProfile> {
        let email = email.trim();
        if !valid_email(email) {
            return Err(CoreError::invalid("Enter a valid email address."));
        }
        check_password(password)?;
        let backend = self.backend()?;
        let s = backend.sign_up(email, password).await?;
        if let Some(name) = display_name.map(str::trim).filter(|n| !n.is_empty()) {
            let name: String = name.chars().take(64).collect();
            backend.set_display_name(&s.id_token, &name).await?;
        }
        // Best effort: the account exists even if the email cannot be sent right now.
        if let Err(e) = backend.send_verification(&s.id_token).await {
            tracing::warn!(target: "mcpanel::account", "verification email not sent: {}", e.message);
        }
        self.establish(&*backend, s).await
    }

    pub async fn sign_in(&self, email: &str, password: &str) -> CoreResult<AccountProfile> {
        let email = email.trim();
        if !valid_email(email) {
            return Err(CoreError::invalid("Enter a valid email address."));
        }
        if password.is_empty() {
            return Err(CoreError::invalid("Enter your password."));
        }
        let backend = self.backend()?;
        let s = backend.sign_in(email, password).await?;
        self.establish(&*backend, s).await
    }

    /// Start "Continue with Google": returns Google's consent page; the result arrives
    /// through the loopback redirect and is reported by [`Self::status`].
    pub async fn begin_google(self: &Arc<Self>) -> CoreResult<String> {
        if let Some(GoogleSignIn::Waiting { url }) = self.google_progress() {
            return Ok(url);
        }
        let backend = self.backend()?;
        if !backend.google_available() {
            return Err(CoreError::new(
                ErrorCode::Unsupported,
                "Google sign-in is not available in this build of MCPanel.",
            ));
        }
        let loopback = Loopback::bind("127.0.0.1", 0, "/").await?;
        let redirect = format!("http://127.0.0.1:{}/", loopback.port);
        let verifier = pkce::verifier();
        let state = pkce::state();
        let url =
            backend.google_authorize_url(&redirect, &pkce::challenge_s256(&verifier), &state)?;
        let progress = Arc::new(Mutex::new(GoogleSignIn::Waiting { url: url.clone() }));
        let prog = Arc::clone(&progress);
        let this = Arc::clone(self);
        let task = tokio::spawn(async move {
            let result: CoreResult<()> = async {
                let code = match loopback.wait(GOOGLE_TIMEOUT).await? {
                    Callback::Code { code, state: got } if got == state => code,
                    Callback::Code { .. } => {
                        return Err(CoreError::new(
                            ErrorCode::GrantInvalid,
                            "The sign-in response did not match. Try again.",
                        ));
                    }
                    Callback::Error { error, .. } if error == "access_denied" => {
                        return Err(CoreError::new(
                            ErrorCode::Cancelled,
                            "Google sign-in was cancelled.",
                        ));
                    }
                    Callback::Error {
                        error, description, ..
                    } => {
                        return Err(CoreError::new(
                            ErrorCode::ProviderError,
                            format!("Google sign-in failed: {}", description.unwrap_or(error)),
                        ));
                    }
                };
                let tokens = backend.google_exchange(&code, &verifier, &redirect).await?;
                let s = backend.sign_in_with_google(&tokens.id_token).await?;
                let profile = this.establish(&*backend, s).await?;
                if let Some(ref rt) = tokens.refresh_token
                    && let Some(cloud) = this.cloud.get()
                {
                    let _ = cloud
                        .link_google_drive(&profile.uid, rt, tokens.access_token.as_ref())
                        .await;
                }
                Ok(())
            }
            .await;
            *prog.lock().unwrap_or_else(|e| e.into_inner()) = match result {
                Ok(()) => GoogleSignIn::Done,
                Err(e) => GoogleSignIn::Failed { message: e.message },
            };
        });
        if let Some((_, old)) = self
            .google
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace((progress, task))
        {
            old.abort();
        }
        Ok(url)
    }

    pub fn cancel_google(&self) {
        if let Some((_, task)) = self.google.lock().unwrap_or_else(|e| e.into_inner()).take() {
            task.abort();
        }
    }

    /// Start "Continue with Microsoft": returns Microsoft's consent page; the result arrives
    /// through the loopback redirect and is reported by [`Self::status`].
    pub async fn begin_microsoft(self: &Arc<Self>) -> CoreResult<String> {
        if MICROSOFT_AUTH_TEMPORARILY_BLOCKED {
            return Err(CoreError::new(
                ErrorCode::PermissionDenied,
                "Microsoft authentication is temporarily disabled.",
            ));
        }
        if let Some(MicrosoftSignIn::Waiting { url }) = self.microsoft_progress() {
            return Ok(url);
        }
        let backend = self.backend()?;
        if !backend.microsoft_available() {
            return Err(CoreError::new(
                ErrorCode::Unsupported,
                "Microsoft sign-in is not available in this build of MCPanel.",
            ));
        }
        let loopback = Loopback::bind("127.0.0.1", 0, "/").await?;
        let redirect = format!("http://127.0.0.1:{}/", loopback.port);
        let verifier = pkce::verifier();
        let state = pkce::state();
        let url =
            backend.microsoft_authorize_url(&redirect, &pkce::challenge_s256(&verifier), &state)?;
        let progress = Arc::new(Mutex::new(MicrosoftSignIn::Waiting { url: url.clone() }));
        let prog = Arc::clone(&progress);
        let this = Arc::clone(self);
        let task = tokio::spawn(async move {
            let result: CoreResult<()> = async {
                let code = match loopback.wait(MICROSOFT_TIMEOUT).await? {
                    Callback::Code { code, state: got } if got == state => code,
                    Callback::Code { .. } => {
                        return Err(CoreError::new(
                            ErrorCode::GrantInvalid,
                            "The sign-in response did not match. Try again.",
                        ));
                    }
                    Callback::Error { error, .. } if error == "access_denied" => {
                        return Err(CoreError::new(
                            ErrorCode::Cancelled,
                            "Microsoft sign-in was cancelled.",
                        ));
                    }
                    Callback::Error {
                        error, description, ..
                    } => {
                        return Err(CoreError::new(
                            ErrorCode::ProviderError,
                            format!("Microsoft sign-in failed: {}", description.unwrap_or(error)),
                        ));
                    }
                };
                let (ms_id_token, ms_access_token) = backend
                    .microsoft_exchange(&code, &verifier, &redirect)
                    .await?;
                let s = backend
                    .sign_in_with_microsoft(&ms_id_token, ms_access_token.as_ref())
                    .await?;
                this.establish(&*backend, s).await.map(drop)
            }
            .await;
            *prog.lock().unwrap_or_else(|e| e.into_inner()) = match result {
                Ok(()) => MicrosoftSignIn::Done,
                Err(e) => MicrosoftSignIn::Failed { message: e.message },
            };
        });
        if let Some((_, old)) = self
            .microsoft
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace((progress, task))
        {
            old.abort();
        }
        Ok(url)
    }

    pub fn cancel_microsoft(&self) {
        if let Some((_, task)) = self
            .microsoft
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            task.abort();
        }
    }

    /// Explicitly triggers a sync of non-sensitive preferences with the cloud.
    pub async fn sync_settings(&self) -> CoreResult<()> {
        let backend = self.backend()?;
        let sync_backend = match backend.sync_backend() {
            Some(sb) => sb,
            None => return Ok(()),
        };
        let token = self.id_token(&*backend).await?;
        let profile = backend.lookup(&token).await?;
        let app_settings = crate::settings::AppSettings::load(&*self.settings)
            .await
            .unwrap_or_default();
        let _ = self
            .sync_service
            .sync_on_login(sync_backend, &token, &profile.uid, &app_settings)
            .await?;
        Ok(())
    }

    pub async fn sign_out(&self) -> CoreResult<()> {
        self.cancel_google();
        self.cancel_microsoft();
        self.secrets.delete(REFRESH_SECRET)?;
        self.settings
            .set(GUEST_KEY, &serde_json::Value::Bool(false))
            .await?;
        self.save_profile(None).await?;
        if let Some(cloud) = self.cloud.get() {
            cloud.on_account_logout();
        }
        Ok(())
    }

    /// Re-read the profile (e.g. after the user verified their email).
    pub async fn refresh_profile(&self) -> CoreResult<AccountProfile> {
        let backend = self.backend()?;
        let token = self.id_token(&*backend).await?;
        let profile = backend.lookup(&token).await?;
        self.save_profile(Some(&profile)).await?;
        Ok(profile)
    }

    pub async fn resend_verification(&self) -> CoreResult<()> {
        let backend = self.backend()?;
        let token = self.id_token(&*backend).await?;
        backend.send_verification(&token).await
    }

    pub async fn send_password_reset(&self, email: &str) -> CoreResult<()> {
        let email = email.trim();
        if !valid_email(email) {
            return Err(CoreError::invalid("Enter a valid email address."));
        }
        self.backend()?.send_password_reset(email).await
    }

    pub async fn set_display_name(&self, name: &str) -> CoreResult<AccountProfile> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(CoreError::invalid("Use a name of 1 to 64 characters."));
        }
        let backend = self.backend()?;
        let token = self.id_token(&*backend).await?;
        backend.set_display_name(&token, name).await?;
        let profile = backend.lookup(&token).await?;
        self.save_profile(Some(&profile)).await?;
        Ok(profile)
    }

    /// At start: load saved credentials and refresh the cached profile when signed in (offline is fine).
    pub async fn on_startup(&self) {
        let _ = self.load_saved_credentials().await;
        if matches!(self.secrets.get(REFRESH_SECRET), Ok(Some(_)))
            && self.backend().is_ok()
            && let Err(e) = self.refresh_profile().await
        {
            tracing::info!(target: "mcpanel::account", "profile not refreshed: {}", e.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_and_passwords_are_checked() {
        assert!(valid_email("steve@example.com"));
        assert!(valid_email(" alex@mail.example.org "));
        for bad in [
            "",
            "steve",
            "steve@",
            "@example.com",
            "a b@example.com",
            "x@example",
            "x@.com",
        ] {
            assert!(!valid_email(bad), "{bad}");
        }
        assert!(check_password("short").is_err());
        assert!(check_password("long enough").is_ok());
    }
}
