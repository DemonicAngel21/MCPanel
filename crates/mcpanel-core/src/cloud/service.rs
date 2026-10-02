//! Connecting, refreshing and disconnecting cloud accounts.
//!
//! Connect: bind the loopback redirect → build the authorization URL (PKCE S256 +
//! `state`) → the host opens it in the system browser → the browser returns to the
//! loopback listener → `state` is checked → the code is exchanged → the refresh token goes
//! to the secret store, the account name to settings. Access tokens stay in memory.

use super::loopback::{Callback, Loopback};
use super::{
    CloudAccount, CloudOperationSnapshot, CloudOperationState, CloudOperationType,
    CloudStorageProvider, CloudTransferOptions, RedirectSpec, RevokeOutcome, pkce,
};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::ports::{SecretStore, SettingsRepository};
use crate::time::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// How long the browser sign-in may take.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Refresh access tokens this long before they expire.
const EXPIRY_MARGIN_MS: i64 = 60_000;

fn secret_name(uid: Option<&str>, id: &str) -> String {
    match uid {
        Some(u) => format!("cloud-{u}-{id}-refresh-token"),
        None => format!("cloud-{id}-refresh-token"),
    }
}

fn meta_key(uid: Option<&str>, id: &str) -> String {
    match uid {
        Some(u) => format!("cloud.{u}.{id}"),
        None => format!("cloud.{id}"),
    }
}

/// Non-secret connection facts (stored in settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectionMeta {
    account: CloudAccount,
    connected_at: Timestamp,
    scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudStatus {
    pub id: String,
    pub display_name: String,
    /// A client ID is available (MCPanel's app registration is configured).
    pub configured: bool,
    /// The build/environment variable that supplies the client ID.
    pub client_id_variable: String,
    pub connected: bool,
    /// Connected, but the refresh token is missing from this computer's secret store.
    pub needs_reconnect: bool,
    pub account: Option<CloudAccount>,
    pub connected_at: Option<Timestamp>,
    pub scopes: Vec<String>,
    /// Redirect URI(s) MCPanel uses, as registered with the provider.
    pub redirect_uris: Vec<String>,
    pub manage_access_url: String,
    #[serde(default)]
    pub is_guest: bool,
    #[serde(default)]
    pub auto_linked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FlowState {
    Waiting,
    Connected,
    Failed { message: String },
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectStart {
    pub flow_id: String,
    /// Open this in the system browser.
    pub authorize_url: String,
}

struct Flow {
    state: FlowState,
    cancel: CancellationToken,
}

struct ActiveCloudOp {
    snapshot: CloudOperationSnapshot,
    cancel: CancellationToken,
}

pub struct CloudService {
    providers: Vec<Arc<dyn CloudStorageProvider>>,
    secrets: Arc<dyn SecretStore>,
    settings: Arc<dyn SettingsRepository>,
    events: EventBus,
    flows: Mutex<HashMap<String, Flow>>,
    access: Mutex<HashMap<String, (SecretString, Option<Timestamp>)>>,
    operations: Mutex<HashMap<String, ActiveCloudOp>>,
    account_service: std::sync::OnceLock<Arc<crate::account::AccountService>>,
}

/// The redirect URIs to register for a provider (documentation and UI).
pub fn registered_redirect_uris(spec: &RedirectSpec) -> Vec<String> {
    match spec {
        RedirectSpec::AnyPort { host, path } => vec![format!("http://{host}{path}")],
        RedirectSpec::FixedPorts { ports, .. } => ports.iter().map(|p| spec.uri(*p)).collect(),
    }
}

impl CloudService {
    pub fn new(
        providers: Vec<Arc<dyn CloudStorageProvider>>,
        secrets: Arc<dyn SecretStore>,
        settings: Arc<dyn SettingsRepository>,
        events: EventBus,
    ) -> Arc<Self> {
        Arc::new(Self {
            providers,
            secrets,
            settings,
            events,
            flows: Mutex::new(HashMap::new()),
            access: Mutex::new(HashMap::new()),
            operations: Mutex::new(HashMap::new()),
            account_service: std::sync::OnceLock::new(),
        })
    }

    pub fn set_account_service(&self, account: Arc<crate::account::AccountService>) {
        let _ = self.account_service.set(account);
    }

    /// (uid, is_guest, is_google)
    async fn current_account(&self) -> (Option<String>, bool, bool) {
        if let Some(acc) = self.account_service.get() {
            if let Ok(st) = acc.status().await {
                if st.signed_in {
                    let uid = st.profile.as_ref().map(|p| p.uid.clone());
                    let is_google = st.profile.as_ref().is_some_and(|p| {
                        p.provider == "google.com"
                            || p.account_type == crate::account::AccountType::Google
                    });
                    return (uid, false, is_google);
                } else if st.is_guest {
                    return (None, true, false);
                }
            }
            return (None, false, false);
        }
        (None, false, false)
    }

    async fn ensure_migrated(&self, uid: &str, provider: &str) -> CoreResult<()> {
        let scoped_sec = secret_name(Some(uid), provider);
        if self.secrets.get(&scoped_sec)?.is_none() {
            let legacy_sec = secret_name(None, provider);
            if let Some(token) = self.secrets.get(&legacy_sec)? {
                self.secrets.set(&scoped_sec, &token)?;
                let _ = self.secrets.delete(&legacy_sec);
                let legacy_meta = meta_key(None, provider);
                if let Some(val) = self.settings.get(&legacy_meta).await? {
                    let scoped_meta = meta_key(Some(uid), provider);
                    self.settings.set(&scoped_meta, &val).await?;
                    let _ = self
                        .settings
                        .set(&legacy_meta, &serde_json::Value::Null)
                        .await;
                }
            }
        }
        Ok(())
    }

    fn provider(&self, id: &str) -> CoreResult<Arc<dyn CloudStorageProvider>> {
        self.providers
            .iter()
            .find(|p| p.info().id == id)
            .cloned()
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::NotFound,
                    format!("Unknown cloud provider '{id}'"),
                )
            })
    }

    fn changed(&self, id: &str) {
        self.events.publish(DomainEvent::CloudChanged {
            provider: id.to_string(),
        });
    }

    async fn meta(&self, uid: Option<&str>, id: &str) -> CoreResult<Option<ConnectionMeta>> {
        Ok(self
            .settings
            .get(&meta_key(uid, id))
            .await?
            .and_then(|v| serde_json::from_value(v).ok()))
    }

    async fn status_of(&self, p: &Arc<dyn CloudStorageProvider>) -> CoreResult<CloudStatus> {
        let info = p.info();
        let (uid_opt, is_guest, is_google) = self.current_account().await;
        let (has_token, meta) = if let Some(ref uid) = uid_opt {
            let _ = self.ensure_migrated(uid, info.id).await;
            let has = self
                .secrets
                .get(&secret_name(Some(uid), info.id))?
                .is_some();
            let m = self.meta(Some(uid), info.id).await?;
            (has, m)
        } else if self.account_service.get().is_none() {
            let has = self.secrets.get(&secret_name(None, info.id))?.is_some();
            let m = self.meta(None, info.id).await?;
            (has, m)
        } else {
            (false, None)
        };
        let auto_linked = is_google && info.id == "google_drive" && has_token;
        Ok(CloudStatus {
            id: info.id.into(),
            display_name: info.display_name.into(),
            configured: info.client_id.is_some(),
            client_id_variable: info.client_id_variable.into(),
            connected: meta.is_some() && has_token,
            needs_reconnect: meta.is_some() && !has_token,
            account: meta.as_ref().map(|m| m.account.clone()),
            connected_at: meta.as_ref().map(|m| m.connected_at),
            scopes: info.scopes.iter().map(|s| s.to_string()).collect(),
            redirect_uris: registered_redirect_uris(&info.redirect),
            manage_access_url: info.manage_access_url.into(),
            is_guest,
            auto_linked,
        })
    }

    pub async fn list(&self) -> CoreResult<Vec<CloudStatus>> {
        let mut out = Vec::new();
        for p in &self.providers {
            out.push(self.status_of(p).await?);
        }
        Ok(out)
    }

    pub async fn status(&self, id: &str) -> CoreResult<CloudStatus> {
        let p = self.provider(id)?;
        self.status_of(&p).await
    }

    fn not_configured(info: &super::CloudProviderInfo) -> CoreError {
        CoreError::new(
            ErrorCode::Unsupported,
            format!(
                "{} is not configured in this build of MCPanel: its OAuth client ID ({}) is missing.",
                info.display_name, info.client_id_variable
            ),
        )
    }

    /// Start a browser sign-in. The caller opens `authorize_url`.
    pub async fn begin_connect(self: &Arc<Self>, id: &str) -> CoreResult<ConnectStart> {
        let (uid_opt, is_guest, _) = self.current_account().await;
        if is_guest || (self.account_service.get().is_some() && uid_opt.is_none()) {
            return Err(CoreError::new(
                ErrorCode::PermissionDenied,
                "Sign in or create an account to use cloud storage.",
            ));
        }
        let uid = uid_opt;
        let p = self.provider(id)?;
        let info = p.info().clone();
        let client_id = info
            .client_id
            .clone()
            .ok_or_else(|| Self::not_configured(&info))?;
        // Only one sign-in at a time per provider.
        {
            let flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
            if flows
                .iter()
                .any(|(k, f)| k.starts_with(&format!("{id}:")) && f.state == FlowState::Waiting)
            {
                return Err(CoreError::new(
                    ErrorCode::Conflict,
                    format!(
                        "A {} sign-in is already open in your browser",
                        info.display_name
                    ),
                ));
            }
        }
        let spec = info.redirect.clone();
        let listener = match &spec {
            RedirectSpec::AnyPort { host, path } => Loopback::bind(host, 0, path).await?,
            RedirectSpec::FixedPorts { host, path, ports } => {
                let mut last = None;
                let mut bound = None;
                for port in *ports {
                    match Loopback::bind(host, *port, path).await {
                        Ok(l) => {
                            bound = Some(l);
                            break;
                        }
                        Err(e) => last = Some(e),
                    }
                }
                bound.ok_or_else(|| {
                    CoreError::new(
                        ErrorCode::PortInUse,
                        format!(
                            "{} needs one of the local ports {:?} for the sign-in, but all are in use{}",
                            info.display_name,
                            ports,
                            last.map(|e| format!(" ({})", e.message)).unwrap_or_default()
                        ),
                    )
                })?
            }
        };
        let redirect_uri = spec.uri(listener.port);
        let verifier = pkce::verifier();
        let state = pkce::state();
        let mut url = info.authorize_url.to_string();
        let scope = info.scopes.join(" ");
        let challenge = pkce::challenge_s256(&verifier);
        let mut params: Vec<(&str, &str)> = vec![
            ("client_id", &client_id),
            ("response_type", "code"),
            ("redirect_uri", &redirect_uri),
            ("scope", &scope),
            ("state", &state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ];
        params.extend(info.extra_authorize_params.iter().copied());
        url.push('?');
        url.push_str(&encode_form(&params));

        let flow_id = format!("{id}:{}", uuid::Uuid::new_v4().simple());
        let cancel = CancellationToken::new();
        self.flows.lock().unwrap_or_else(|e| e.into_inner()).insert(
            flow_id.clone(),
            Flow {
                state: FlowState::Waiting,
                cancel: cancel.clone(),
            },
        );
        let this = Arc::clone(self);
        let fid = flow_id.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => Err(CoreError::cancelled()),
                r = this.finish_connect(&p, listener, &state, &verifier, &redirect_uri, uid.as_deref()) => r,
            };
            let state = match result {
                Ok(()) => FlowState::Connected,
                Err(e) if e.code == ErrorCode::Cancelled => FlowState::Cancelled,
                Err(e) => FlowState::Failed { message: e.message },
            };
            if let Some(f) = this
                .flows
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&fid)
            {
                f.state = state;
            }
            this.changed(p.info().id);
        });
        Ok(ConnectStart {
            flow_id,
            authorize_url: url,
        })
    }

    async fn finish_connect(
        &self,
        p: &Arc<dyn CloudStorageProvider>,
        listener: Loopback,
        state: &str,
        verifier: &str,
        redirect_uri: &str,
        uid: Option<&str>,
    ) -> CoreResult<()> {
        let name = p.info().display_name;
        let code = match listener.wait(SIGN_IN_TIMEOUT).await? {
            Callback::Error {
                error, description, ..
            } => {
                return Err(CoreError::new(
                    ErrorCode::ProviderError,
                    match error.as_str() {
                        "access_denied" => format!("{name}: access was not granted"),
                        _ => format!(
                            "{name} returned an error: {error}{}",
                            description.map(|d| format!(" — {d}")).unwrap_or_default()
                        ),
                    },
                ));
            }
            Callback::Code { code, state: s } => {
                if s != state {
                    return Err(CoreError::new(
                        ErrorCode::ProviderError,
                        format!(
                            "{name}: the sign-in response did not match this request (state mismatch)"
                        ),
                    ));
                }
                code
            }
        };
        let tokens = p.exchange_code(&code, verifier, redirect_uri).await?;
        let refresh = tokens.refresh_token.ok_or_else(|| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!(
                    "{name} did not return a refresh token, so MCPanel could not stay connected"
                ),
            )
        })?;
        let account = p.account(&tokens.access_token).await?;
        let id = p.info().id;
        self.secrets.set(&secret_name(uid, id), &refresh)?;
        let meta = ConnectionMeta {
            account,
            connected_at: Timestamp::now(),
            scopes: p.info().scopes.iter().map(|s| s.to_string()).collect(),
        };
        let v = serde_json::to_value(&meta).map_err(|e| CoreError::internal(e.to_string()))?;
        self.settings.set(&meta_key(uid, id), &v).await?;
        self.access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                format!("{}:{id}", uid.unwrap_or("default")),
                (tokens.access_token, tokens.expires_at),
            );
        Ok(())
    }

    pub fn flow(&self, flow_id: &str) -> CoreResult<FlowState> {
        self.flows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(flow_id)
            .map(|f| f.state.clone())
            .ok_or_else(|| CoreError::new(ErrorCode::NotFound, "Unknown sign-in"))
    }

    pub fn cancel_connect(&self, flow_id: &str) {
        if let Some(f) = self
            .flows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(flow_id)
        {
            f.cancel.cancel();
        }
    }

    /// A valid access token (refreshing it when needed).
    pub async fn access_token(&self, id: &str) -> CoreResult<SecretString> {
        let (uid_opt, is_guest, _) = self.current_account().await;
        if is_guest || (self.account_service.get().is_some() && uid_opt.is_none()) {
            return Err(CoreError::new(
                ErrorCode::PermissionDenied,
                "Sign in to an MCPanel account to use cloud storage.",
            ));
        }
        let uid = uid_opt.as_deref();
        let p = self.provider(id)?;
        let cache_key = format!("{}:{id}", uid.unwrap_or("default"));
        if let Some((t, exp)) = self
            .access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&cache_key)
            && exp.is_none_or(|e| e.millis() - EXPIRY_MARGIN_MS > Timestamp::now().millis())
        {
            return Ok(t.clone());
        }
        let refresh = self.secrets.get(&secret_name(uid, id))?.ok_or_else(|| {
            CoreError::new(
                ErrorCode::NotFound,
                format!("{} is not connected to this account", p.info().display_name),
            )
        })?;
        let tokens = p.refresh(&refresh).await?;
        if let Some(new) = &tokens.refresh_token
            && new.expose_secret() != refresh.expose_secret()
        {
            self.secrets.set(&secret_name(uid, id), new)?;
        }
        self.access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(cache_key, (tokens.access_token.clone(), tokens.expires_at));
        Ok(tokens.access_token)
    }

    /// Verify the connection end to end (refresh + account lookup) and update the account.
    pub async fn check(&self, id: &str) -> CoreResult<CloudStatus> {
        let p = self.provider(id)?;
        let token = self.access_token(id).await?;
        let account = p.account(&token).await?;
        let (uid_opt, _, _) = self.current_account().await;
        if let Some(mut meta) = self.meta(uid_opt.as_deref(), id).await? {
            meta.account = account;
            let v = serde_json::to_value(&meta).map_err(|e| CoreError::internal(e.to_string()))?;
            self.settings
                .set(&meta_key(uid_opt.as_deref(), id), &v)
                .await?;
        }
        self.changed(id);
        self.status_of(&p).await
    }

    /// Remove the connection: revoke at the provider where supported, then delete the
    /// stored token and account facts.
    pub async fn disconnect(&self, id: &str) -> CoreResult<RevokeOutcome> {
        let p = self.provider(id)?;
        let (uid_opt, is_guest, _) = self.current_account().await;
        if is_guest || (self.account_service.get().is_some() && uid_opt.is_none()) {
            return Err(CoreError::new(
                ErrorCode::PermissionDenied,
                "Sign in to manage cloud storage.",
            ));
        }
        let uid = uid_opt.as_deref();
        let sec_key = secret_name(uid, id);
        let outcome = match self.secrets.get(&sec_key)? {
            Some(refresh) => match p.revoke(&refresh).await {
                Ok(o) => o,
                Err(e) => {
                    tracing::warn!(target: "mcpanel::cloud", provider = id, "revocation failed: {}", e.message);
                    RevokeOutcome::NotSupported
                }
            },
            None => RevokeOutcome::NotSupported,
        };
        self.secrets.delete(&sec_key)?;
        self.settings
            .set(&meta_key(uid, id), &serde_json::Value::Null)
            .await?;
        self.access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&format!("{}:{id}", uid.unwrap_or("default")));
        self.changed(id);
        Ok(outcome)
    }

    /// Called when an account signs out: clears memory token caches and flows.
    pub fn on_account_logout(&self) {
        self.access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        for flow in flows.values_mut() {
            flow.cancel.cancel();
        }
        flows.clear();
        let mut ops = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        for op in ops.values_mut() {
            op.cancel.cancel();
            op.snapshot.state = CloudOperationState::Cancelled;
        }
        ops.clear();
        self.events.publish(DomainEvent::CloudChanged {
            provider: "all".to_string(),
        });
    }

    /// Automatically link Google Drive for a user who signed in with Google.
    pub async fn link_google_drive(
        &self,
        uid: &str,
        refresh_token: &SecretString,
        access_token: Option<&SecretString>,
    ) -> CoreResult<()> {
        let p = match self.provider("google_drive") {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };
        let sec_key = secret_name(Some(uid), "google_drive");
        self.secrets.set(&sec_key, refresh_token)?;

        let account = if let Some(at) = access_token {
            p.account(at).await.ok()
        } else {
            match p.refresh(refresh_token).await {
                Ok(tokens) => {
                    self.access
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(
                            format!("{uid}:google_drive"),
                            (tokens.access_token.clone(), tokens.expires_at),
                        );
                    p.account(&tokens.access_token).await.ok()
                }
                Err(_) => None,
            }
        };

        let meta = ConnectionMeta {
            account: account.unwrap_or_default(),
            connected_at: Timestamp::now(),
            scopes: p.info().scopes.iter().map(|s| s.to_string()).collect(),
        };
        let v = serde_json::to_value(&meta).map_err(|e| CoreError::internal(e.to_string()))?;
        self.settings
            .set(&meta_key(Some(uid), "google_drive"), &v)
            .await?;
        self.changed("google_drive");
        Ok(())
    }

    pub fn start_operation(
        &self,
        id: String,
        provider: String,
        backup_id: Option<String>,
        backup_name: String,
        operation_type: CloudOperationType,
        total_bytes: Option<u64>,
    ) -> (CloudOperationSnapshot, CancellationToken) {
        let cancel = CancellationToken::new();
        let initial_state = CloudOperationState::Starting;
        let snap = CloudOperationSnapshot {
            id: id.clone(),
            provider,
            backup_id,
            backup_name,
            operation_type,
            state: initial_state,
            bytes_completed: 0,
            total_bytes,
            progress_percentage: if total_bytes == Some(0) {
                Some(100.0)
            } else {
                Some(0.0)
            },
            start_time: Timestamp::now(),
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
        };
        self.operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id,
                ActiveCloudOp {
                    snapshot: snap.clone(),
                    cancel: cancel.clone(),
                },
            );
        self.events.publish(DomainEvent::CloudOperationUpdated {
            operation: snap.clone(),
        });
        (snap, cancel)
    }

    pub fn update_operation_progress(
        &self,
        id: &str,
        bytes_completed: u64,
        total_bytes: Option<u64>,
    ) {
        let mut ops = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(op) = ops.get_mut(id) {
            op.snapshot.bytes_completed = bytes_completed;
            if total_bytes.is_some() {
                op.snapshot.total_bytes = total_bytes;
            }
            let total = op.snapshot.total_bytes;
            let elapsed = (Timestamp::now()
                .millis()
                .saturating_sub(op.snapshot.start_time.millis())) as f64
                / 1000.0;
            let speed = if elapsed > 0.5 && bytes_completed > 0 {
                Some((bytes_completed as f64 / elapsed) as u64)
            } else {
                None
            };
            op.snapshot.speed_bytes_per_sec = speed;
            op.snapshot.eta_seconds = match (total, speed) {
                (Some(tot), Some(spd)) if spd > 0 && tot > bytes_completed => {
                    Some((tot - bytes_completed) / spd)
                }
                _ => None,
            };
            op.snapshot.progress_percentage = total.map(|tot| {
                if tot == 0 {
                    100.0
                } else {
                    ((bytes_completed as f64 / tot as f64) * 100.0).clamp(0.0, 100.0) as f32
                }
            });
            let snap = op.snapshot.clone();
            drop(ops);
            self.events
                .publish(DomainEvent::CloudOperationUpdated { operation: snap });
        }
    }

    pub fn set_operation_state(
        &self,
        id: &str,
        state: CloudOperationState,
        error_message: Option<String>,
    ) {
        let mut ops = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(op) = ops.get_mut(id) {
            op.snapshot.state = state;
            if state == CloudOperationState::Completed {
                if let Some(total) = op.snapshot.total_bytes {
                    op.snapshot.bytes_completed = total;
                }
                op.snapshot.progress_percentage = Some(100.0);
                op.snapshot.eta_seconds = Some(0);
            }
            if error_message.is_some() {
                op.snapshot.error_message = error_message;
            }
            let snap = op.snapshot.clone();
            drop(ops);
            self.events
                .publish(DomainEvent::CloudOperationUpdated { operation: snap });
        }
    }

    pub fn cancel_operation(&self, id: &str) -> bool {
        let mut ops = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(op) = ops.get_mut(id)
            && !op.snapshot.state.is_terminal()
        {
            op.snapshot.state = CloudOperationState::Cancelling;
            op.cancel.cancel();
            let snap = op.snapshot.clone();
            drop(ops);
            self.events
                .publish(DomainEvent::CloudOperationUpdated { operation: snap });
            return true;
        }
        false
    }

    pub fn get_operation(&self, id: &str) -> Option<CloudOperationSnapshot> {
        self.operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|op| op.snapshot.clone())
    }

    pub fn list_operations(&self) -> Vec<CloudOperationSnapshot> {
        let ops = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<_> = ops.values().map(|v| v.snapshot.clone()).collect();
        list.sort_by_key(|a| std::cmp::Reverse(a.start_time));
        list
    }

    /// Upload a file to a connected cloud provider.
    pub async fn upload_file(
        &self,
        provider_id: &str,
        filename: &str,
        path: &std::path::Path,
        options: Option<CloudTransferOptions>,
    ) -> CoreResult<super::CloudFileMetadata> {
        let p = self.provider(provider_id)?;
        let mut token = self.access_token(provider_id).await?;
        let res = p.upload_file(&token, filename, path, options.clone()).await;
        match res {
            Err(ref e)
                if e.message.contains("HTTP 401")
                    || e.message.contains("invalid_access_token")
                    || e.message.contains("expired_access_token") =>
            {
                // Invalidate cached in-memory token and refresh from credential store
                let (uid_opt, _, _) = self.current_account().await;
                let cache_key =
                    format!("{}:{provider_id}", uid_opt.as_deref().unwrap_or("default"));
                self.access
                    .lock()
                    .unwrap_or_else(|lock| lock.into_inner())
                    .remove(&cache_key);
                if let Ok(new_token) = self.access_token(provider_id).await {
                    token = new_token;
                    return p.upload_file(&token, filename, path, options).await;
                }
                res
            }
            other => other,
        }
    }

    /// List MCPanel backup files across one or all connected cloud providers.
    pub async fn list_files(
        &self,
        provider_id: Option<&str>,
    ) -> CoreResult<Vec<super::CloudFileMetadata>> {
        let mut results = Vec::new();
        match provider_id {
            Some(pid) => {
                let p = self.provider(pid)?;
                let token = self.access_token(pid).await?;
                results.extend(p.list_files(&token).await?);
            }
            None => {
                for p in &self.providers {
                    let id = p.info().id;
                    if let Ok(token) = self.access_token(id).await
                        && let Ok(files) = p.list_files(&token).await
                    {
                        results.extend(files);
                    }
                }
            }
        }
        results.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(results)
    }

    /// Download a file from cloud storage to a local destination.
    pub async fn download_file(
        &self,
        provider_id: &str,
        file_id: &str,
        destination: &std::path::Path,
        options: Option<CloudTransferOptions>,
    ) -> CoreResult<u64> {
        let p = self.provider(provider_id)?;
        let mut token = self.access_token(provider_id).await?;
        let res = p
            .download_file(&token, file_id, destination, options.clone())
            .await;
        match res {
            Err(ref e)
                if e.message.contains("HTTP 401")
                    || e.message.contains("invalid_access_token")
                    || e.message.contains("expired_access_token") =>
            {
                let (uid_opt, _, _) = self.current_account().await;
                let cache_key =
                    format!("{}:{provider_id}", uid_opt.as_deref().unwrap_or("default"));
                self.access
                    .lock()
                    .unwrap_or_else(|lock| lock.into_inner())
                    .remove(&cache_key);
                if let Ok(new_token) = self.access_token(provider_id).await {
                    token = new_token;
                    return p.download_file(&token, file_id, destination, options).await;
                }
                res
            }
            other => other,
        }
    }

    /// Delete a file from cloud storage.
    pub async fn delete_file(&self, provider_id: &str, file_id: &str) -> CoreResult<()> {
        let p = self.provider(provider_id)?;
        let token = self.access_token(provider_id).await?;
        p.delete_file(&token, file_id).await
    }
}

/// `application/x-www-form-urlencoded` (also used for query strings).
pub fn encode_form(params: &[(&str, &str)]) -> String {
    fn enc(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    out.push(b as char)
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }
    params
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}
