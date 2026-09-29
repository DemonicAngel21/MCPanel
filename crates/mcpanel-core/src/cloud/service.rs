//! Connecting, refreshing and disconnecting cloud accounts.
//!
//! Connect: bind the loopback redirect → build the authorization URL (PKCE S256 +
//! `state`) → the host opens it in the system browser → the browser returns to the
//! loopback listener → `state` is checked → the code is exchanged → the refresh token goes
//! to the secret store, the account name to settings. Access tokens stay in memory.

use super::loopback::{Callback, Loopback};
use super::{CloudAccount, CloudStorageProvider, RedirectSpec, RevokeOutcome, pkce};
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

fn secret_name(id: &str) -> String {
    format!("cloud-{id}-refresh-token")
}

fn meta_key(id: &str) -> String {
    format!("cloud.{id}")
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

pub struct CloudService {
    providers: Vec<Arc<dyn CloudStorageProvider>>,
    secrets: Arc<dyn SecretStore>,
    settings: Arc<dyn SettingsRepository>,
    events: EventBus,
    flows: Mutex<HashMap<String, Flow>>,
    access: Mutex<HashMap<String, (SecretString, Option<Timestamp>)>>,
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
        })
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

    async fn meta(&self, id: &str) -> CoreResult<Option<ConnectionMeta>> {
        Ok(self
            .settings
            .get(&meta_key(id))
            .await?
            .and_then(|v| serde_json::from_value(v).ok()))
    }

    async fn status_of(&self, p: &Arc<dyn CloudStorageProvider>) -> CoreResult<CloudStatus> {
        let info = p.info();
        let meta = self.meta(info.id).await?;
        let has_token = self.secrets.get(&secret_name(info.id))?.is_some();
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
                r = this.finish_connect(&p, listener, &state, &verifier, &redirect_uri) => r,
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
        self.secrets.set(&secret_name(id), &refresh)?;
        let meta = ConnectionMeta {
            account,
            connected_at: Timestamp::now(),
            scopes: p.info().scopes.iter().map(|s| s.to_string()).collect(),
        };
        let v = serde_json::to_value(&meta).map_err(|e| CoreError::internal(e.to_string()))?;
        self.settings.set(&meta_key(id), &v).await?;
        self.access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string(), (tokens.access_token, tokens.expires_at));
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
        let p = self.provider(id)?;
        if let Some((t, exp)) = self
            .access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            && exp.is_none_or(|e| e.millis() - EXPIRY_MARGIN_MS > Timestamp::now().millis())
        {
            return Ok(t.clone());
        }
        let refresh = self.secrets.get(&secret_name(id))?.ok_or_else(|| {
            CoreError::new(
                ErrorCode::NotFound,
                format!(
                    "{} is not connected on this computer",
                    p.info().display_name
                ),
            )
        })?;
        let tokens = p.refresh(&refresh).await?;
        if let Some(new) = &tokens.refresh_token
            && new.expose_secret() != refresh.expose_secret()
        {
            self.secrets.set(&secret_name(id), new)?;
        }
        self.access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.to_string(),
                (tokens.access_token.clone(), tokens.expires_at),
            );
        Ok(tokens.access_token)
    }

    /// Verify the connection end to end (refresh + account lookup) and update the account.
    pub async fn check(&self, id: &str) -> CoreResult<CloudStatus> {
        let p = self.provider(id)?;
        let token = self.access_token(id).await?;
        let account = p.account(&token).await?;
        if let Some(mut meta) = self.meta(id).await? {
            meta.account = account;
            let v = serde_json::to_value(&meta).map_err(|e| CoreError::internal(e.to_string()))?;
            self.settings.set(&meta_key(id), &v).await?;
        }
        self.changed(id);
        self.status_of(&p).await
    }

    /// Remove the connection: revoke at the provider where supported, then delete the
    /// stored token and account facts.
    pub async fn disconnect(&self, id: &str) -> CoreResult<RevokeOutcome> {
        let p = self.provider(id)?;
        let outcome = match self.secrets.get(&secret_name(id))? {
            Some(refresh) => match p.revoke(&refresh).await {
                Ok(o) => o,
                Err(e) => {
                    tracing::warn!(target: "mcpanel::cloud", provider = id, "revocation failed: {}", e.message);
                    RevokeOutcome::NotSupported
                }
            },
            None => RevokeOutcome::NotSupported,
        };
        self.secrets.delete(&secret_name(id))?;
        self.settings
            .set(&meta_key(id), &serde_json::Value::Null)
            .await?;
        self.access
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        self.changed(id);
        Ok(outcome)
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
