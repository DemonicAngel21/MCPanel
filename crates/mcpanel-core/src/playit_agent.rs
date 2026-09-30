//! MCPanel's own playit.gg agent (ADR-0007, amendment 2).
//!
//! Like playit's official Minecraft plugin, MCPanel links a *self-managed* agent to the
//! user's playit.gg account: it asks playit for a claim (`/claim/setup`), the user
//! approves it at `https://playit.gg/claim/<code>`, and `/claim/exchange` returns the
//! agent's secret key. The key is kept in the Windows Credential Manager. MCPanel runs
//! the agent itself with the installed playit program's `playitd.exe` on its own named
//! pipe, and manages the account's tunnels with the key through playit's web API
//! (self-managed agents may only change their own tunnels).
//!
//! While the agent runs, `playitd` reads the key from a file in MCPanel's data folder
//! (`playit\agent.toml`, the user's profile); the file is removed when the agent stops.

use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::playit_api::{
    NewPlayitTunnel, PlayitAgentInfo, PlayitApi, PlayitTunnelInfo, PlayitTunnelKind,
};
use crate::ports::{Platform, ProcessController, ProcessSpec, SecretStore, SettingsRepository};
use crate::tunnels::{PlayitTunnel, parse_playit_status};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::AsyncReadExt;

const SECRET_NAME: &str = "playit-agent-secret";
const AUTOSTART_KEY: &str = "tunnel.playit.agent_autostart";
/// Poll interval and overall limit for the user's approval on playit.gg.
const CLAIM_POLL: Duration = Duration::from_secs(1);
const CLAIM_LIMIT: Duration = Duration::from_secs(10 * 60);
const CLI_TIMEOUT: Duration = Duration::from_secs(15);

/// Progress of linking MCPanel's agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentLink {
    /// Waiting for the user to approve the agent at `url`.
    Waiting {
        url: String,
    },
    Linked,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    /// The playit program (with `playitd.exe`) is installed.
    pub installed: bool,
    /// MCPanel has an agent key.
    pub linked: bool,
    pub link: Option<AgentLink>,
    /// MCPanel's agent process is running.
    pub running: bool,
    /// The agent's phase (e.g. "running", "invalid secret") while it runs.
    pub phase: Option<String>,
    /// Start the agent when MCPanel starts.
    pub autostart: bool,
}

/// A tunnel of the account, and whether MCPanel's agent may change it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedTunnel {
    #[serde(flatten)]
    pub tunnel: PlayitTunnelInfo,
    pub editable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelList {
    pub agent: PlayitAgentInfo,
    pub tunnels: Vec<ManagedTunnel>,
}

struct Claim {
    progress: Arc<Mutex<AgentLink>>,
    task: tokio::task::JoinHandle<()>,
}

pub struct PlayitAgent {
    platform: Arc<dyn Platform>,
    secrets: Arc<dyn SecretStore>,
    settings: Arc<dyn SettingsRepository>,
    api: OnceLock<Arc<dyn PlayitApi>>,
    dir: PathBuf,
    socket: String,
    claim: Mutex<Option<Claim>>,
    process: Mutex<Option<Arc<dyn ProcessController>>>,
    /// Tests: `Some(None)` = no playitd available.
    daemon_override: Option<Option<PathBuf>>,
}

/// 10 hex characters (5 random bytes), as playit's CLI and plugin generate.
fn claim_code() -> String {
    let b = uuid::Uuid::new_v4().into_bytes();
    b[..5].iter().map(|x| format!("{x:02x}")).collect()
}

fn fail_code(e: &CoreError) -> Option<&str> {
    e.details.as_ref()?.get("playit")?.as_str()
}

fn tunnel_id(id: &str) -> CoreResult<&str> {
    uuid::Uuid::parse_str(id)
        .map(|_| id)
        .map_err(|_| CoreError::invalid("Invalid tunnel id"))
}

fn tunnel_name(name: &str) -> CoreResult<String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 64 {
        return Err(CoreError::invalid(
            "The tunnel name must be 1 to 64 characters long.",
        ));
    }
    if !name.bytes().all(|b| b == b' ' || b.is_ascii_graphic()) {
        return Err(CoreError::invalid(
            "Use only English letters, digits and punctuation in the tunnel name.",
        ));
    }
    Ok(name.to_string())
}

fn local_port(port: u16) -> CoreResult<u16> {
    if port == 0 {
        return Err(CoreError::invalid("Enter a port between 1 and 65535."));
    }
    Ok(port)
}

fn format_claim_version(stdout: &str) -> Option<String> {
    let version = stdout.trim();
    (!version.is_empty()
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')))
    .then(|| format!("playit {version}"))
}

impl PlayitAgent {
    /// `data_dir` is MCPanel's data folder; `socket` the agent's named pipe.
    pub fn new(
        platform: Arc<dyn Platform>,
        secrets: Arc<dyn SecretStore>,
        settings: Arc<dyn SettingsRepository>,
        data_dir: &Path,
        socket: impl Into<String>,
    ) -> Self {
        Self {
            platform,
            secrets,
            settings,
            api: OnceLock::new(),
            dir: data_dir.join("playit"),
            socket: socket.into(),
            claim: Mutex::new(None),
            process: Mutex::new(None),
            daemon_override: None,
        }
    }

    /// Use this `playitd.exe` (or none) instead of the installed one.
    pub fn with_daemon(mut self, exe: Option<PathBuf>) -> Self {
        self.daemon_override = Some(exe);
        self
    }

    pub fn set_api(&self, api: Arc<dyn PlayitApi>) {
        let _ = self.api.set(api);
    }

    fn api(&self) -> CoreResult<Arc<dyn PlayitApi>> {
        self.api.get().cloned().ok_or_else(|| {
            CoreError::new(
                ErrorCode::Unsupported,
                "playit.gg is not available in this build.",
            )
        })
    }

    fn key(&self) -> CoreResult<Option<SecretString>> {
        self.secrets.get(SECRET_NAME)
    }

    fn require_key(&self) -> CoreResult<SecretString> {
        self.key()?.ok_or_else(|| {
            CoreError::new(
                ErrorCode::Conflict,
                "Link MCPanel's playit agent to your playit.gg account first.",
            )
        })
    }

    fn secret_file(&self) -> PathBuf {
        self.dir.join("agent.toml")
    }

    /// `playitd.exe` of the installed playit program.
    fn daemon_exe(&self) -> Option<PathBuf> {
        if let Some(o) = &self.daemon_override {
            return o.clone();
        }
        PlayitTunnel::locate()
            .map(|p| p.with_file_name("playitd.exe"))
            .filter(|p| p.is_file())
    }

    fn link_progress(&self) -> Option<AgentLink> {
        let c = self.claim.lock().unwrap_or_else(|e| e.into_inner());
        c.as_ref()
            .map(|c| c.progress.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    async fn cli(&self, arg: &str) -> Option<String> {
        let exe = PlayitTunnel::locate()?;
        let out = self
            .platform
            .run_capture(
                &exe,
                &["--socket-path".into(), self.socket.clone(), arg.into()],
                None,
                CLI_TIMEOUT,
            )
            .await
            .ok()?;
        (!out.timed_out && out.code == Some(0))
            .then(|| String::from_utf8_lossy(&out.stdout).to_string())
    }

    pub async fn autostart(&self) -> CoreResult<bool> {
        Ok(self
            .settings
            .get(AUTOSTART_KEY)
            .await?
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    pub async fn set_autostart(&self, on: bool) -> CoreResult<()> {
        self.settings
            .set(AUTOSTART_KEY, &serde_json::Value::Bool(on))
            .await
    }

    pub async fn status(&self) -> CoreResult<AgentStatus> {
        let mut s = AgentStatus {
            installed: self.daemon_exe().is_some(),
            linked: self.key()?.is_some(),
            link: self.link_progress(),
            running: false,
            phase: None,
            autostart: self.autostart().await?,
        };
        if let Some(st) = self
            .cli("status")
            .await
            .and_then(|t| parse_playit_status(&t))
        {
            s.running = st.running;
            s.phase = st.phase;
        }
        Ok(s)
    }

    // ── linking ──

    async fn claim_version(&self) -> CoreResult<String> {
        let exe = PlayitTunnel::locate().ok_or_else(|| {
            CoreError::new(
                ErrorCode::NotFound,
                "The playit program is not installed. Download it from playit.gg.",
            )
        })?;
        let output = self
            .platform
            .run_capture(&exe, &["version".into()], None, CLI_TIMEOUT)
            .await?;
        let version = (!output.timed_out && output.code == Some(0))
            .then(|| format_claim_version(&String::from_utf8_lossy(&output.stdout)))
            .flatten()
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::ProviderUnavailable,
                    "Could not read the installed playit agent version. Update playit and try again.",
                )
            })?;
        Ok(version)
    }

    /// Start linking: returns the playit.gg page where the user approves the agent.
    /// Approval is polled in the background; the key is stored when it arrives and the
    /// agent is started.
    pub async fn begin_link(self: &Arc<Self>) -> CoreResult<String> {
        if let Some(AgentLink::Waiting { url }) = self.link_progress() {
            return Ok(url);
        }
        if self.key()?.is_some() {
            return Err(CoreError::new(
                ErrorCode::Conflict,
                "MCPanel's playit agent is already linked. Unlink it first to link another account.",
            ));
        }
        self.start_link(self.claim_version().await?).await
    }

    async fn start_link(self: &Arc<Self>, version: String) -> CoreResult<String> {
        let api = self.api()?;
        let code = claim_code();
        // Register the claim before sending the user to it.
        api.claim_setup(&code, &version).await?;
        let url = format!("https://playit.gg/claim/{code}");
        let progress = Arc::new(Mutex::new(AgentLink::Waiting { url: url.clone() }));
        let this = Arc::clone(self);
        let prog = Arc::clone(&progress);
        let poll_version = version.clone();
        let task = tokio::spawn(async move {
            let result = this.wait_for_approval(&*api, &code, &poll_version).await;
            let outcome = match result {
                Ok(key) => match this.secrets.set(SECRET_NAME, &key) {
                    Ok(()) => {
                        if let Err(e) = this.start().await {
                            tracing::warn!(target: "mcpanel::tunnels", "linked, but the agent did not start: {}", e.message);
                        }
                        AgentLink::Linked
                    }
                    Err(e) => AgentLink::Failed {
                        message: format!("Could not store the agent's key: {}", e.message),
                    },
                },
                Err(e) => AgentLink::Failed { message: e.message },
            };
            *prog.lock().unwrap_or_else(|e| e.into_inner()) = outcome;
        });
        if let Some(old) = self
            .claim
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(Claim { progress, task })
        {
            old.task.abort();
        }
        Ok(url)
    }

    /// Replace MCPanel's current agent credential and begin a fresh approval flow.
    /// The caller must make this an explicit user action because it takes the current
    /// agent offline until the replacement is approved.
    pub async fn relink(self: &Arc<Self>) -> CoreResult<String> {
        let version = self.claim_version().await?;
        self.unlink().await?;
        self.start_link(version).await
    }

    async fn wait_for_approval(
        &self,
        api: &dyn PlayitApi,
        code: &str,
        version: &str,
    ) -> CoreResult<SecretString> {
        let deadline = tokio::time::Instant::now() + CLAIM_LIMIT;
        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(CoreError::new(
                    ErrorCode::Cancelled,
                    "Linking timed out. Start again when you are ready to approve it on playit.gg.",
                ));
            }
            match api.claim_setup(code, version).await {
                Ok(state) if state == "UserAccepted" => break,
                Ok(state) if state == "UserRejected" => {
                    return Err(CoreError::new(
                        ErrorCode::Cancelled,
                        "The agent was declined on playit.gg.",
                    ));
                }
                Ok(_) => {}
                Err(e) if e.retryable => {}
                Err(e) => return Err(e),
            }
            tokio::time::sleep(CLAIM_POLL).await;
        }
        // playit may need a moment between acceptance and the key being ready.
        for _ in 0..30 {
            match api.claim_exchange(code).await {
                Ok(key) => return Ok(key),
                Err(e) if fail_code(&e) == Some("NotAccepted") || e.retryable => {}
                Err(e) => return Err(e),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Err(CoreError::new(
            ErrorCode::ProviderError,
            "playit.gg did not hand over the agent's key. Try linking again.",
        ))
    }

    pub fn cancel_link(&self) {
        if let Some(c) = self.claim.lock().unwrap_or_else(|e| e.into_inner()).take() {
            c.task.abort();
        }
    }

    /// Stop the agent and forget its key. The agent stays listed on playit.gg until the
    /// user removes it there.
    pub async fn unlink(&self) -> CoreResult<()> {
        self.cancel_link();
        self.stop().await?;
        self.secrets.delete(SECRET_NAME)?;
        Ok(())
    }

    // ── the agent process ──

    /// Start MCPanel's agent (no-op when it already runs).
    pub async fn start(&self) -> CoreResult<()> {
        let key = self.require_key()?;
        if self.status().await?.running {
            return Ok(());
        }
        let exe = self.daemon_exe().ok_or_else(|| {
            CoreError::new(
                ErrorCode::NotFound,
                "The playit program is not installed. Download it from playit.gg.",
            )
        })?;
        tokio::fs::create_dir_all(&self.dir).await.map_err(|e| {
            CoreError::new(
                ErrorCode::Io,
                format!("Cannot create {}: {e}", self.dir.display()),
            )
        })?;
        let file = self.secret_file();
        tokio::fs::write(&file, format!("secret_key = \"{}\"\n", key.expose_secret()))
            .await
            .map_err(|e| CoreError::new(ErrorCode::Io, format!("Cannot prepare the agent: {e}")))?;
        let spawned = self.platform.spawn(&ProcessSpec {
            program: exe,
            args: vec![
                "--secret-path".into(),
                file.to_string_lossy().into_owned(),
                "--socket-path".into(),
                self.socket.clone(),
                "-l".into(),
                self.dir.join("playitd.log").to_string_lossy().into_owned(),
            ],
            cwd: self.dir.clone(),
            env: Vec::new(),
            group_name: None,
        });
        let spawned = match spawned {
            Ok(s) => s,
            Err(e) => {
                let _ = tokio::fs::remove_file(&file).await;
                return Err(e);
            }
        };
        *self.process.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Arc::clone(&spawned.controller));
        let mut out = spawned.stdout;
        let mut err = spawned.stderr;
        let waiter = spawned.waiter;
        tokio::spawn(async move {
            // Drain the pipes (playitd logs to its file); remove the key file on exit.
            let mut sink = Vec::new();
            let mut sink2 = Vec::new();
            let _ = tokio::join!(out.read_to_end(&mut sink), err.read_to_end(&mut sink2));
            let _ = waiter.wait().await;
            let _ = tokio::fs::remove_file(&file).await;
        });
        // Wait until the daemon answers on its pipe.
        for _ in 0..40 {
            if self.status().await?.running {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(CoreError::new(
            ErrorCode::Io,
            "The playit agent did not start. See playit\\playitd.log in MCPanel's data folder.",
        ))
    }

    /// Stop MCPanel's agent (graceful through its pipe, then forcefully).
    pub async fn stop(&self) -> CoreResult<()> {
        let _ = self.cli("stop").await;
        let controller = self
            .process
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        for _ in 0..20 {
            if !self.status().await?.running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if self.status().await?.running
            && let Some(c) = controller
        {
            let _ = c.terminate_tree();
        }
        let _ = tokio::fs::remove_file(self.secret_file()).await;
        Ok(())
    }

    /// At MCPanel start: remove a key file left by a crash, then start the agent if it is
    /// linked and set to start automatically.
    pub async fn on_startup(&self) {
        if !self.status().await.map(|s| s.running).unwrap_or(false) {
            let _ = tokio::fs::remove_file(self.secret_file()).await;
        }
        if matches!(self.key(), Ok(Some(_)))
            && self.autostart().await.unwrap_or(false)
            && let Err(e) = self.start().await
        {
            tracing::warn!(target: "mcpanel::tunnels", "playit agent did not start: {}", e.message);
        }
    }

    // ── tunnels ──

    pub async fn tunnels(&self) -> CoreResult<TunnelList> {
        let key = self.require_key()?;
        let api = self.api()?;
        let agent = api.agent(&key).await?;
        let tunnels = api
            .tunnels(&key)
            .await?
            .into_iter()
            .map(|t| ManagedTunnel {
                editable: t.agent_id.as_deref() == Some(agent.agent_id.as_str()),
                tunnel: t,
            })
            .collect();
        Ok(TunnelList { agent, tunnels })
    }

    /// Create a tunnel on MCPanel's agent to `127.0.0.1:port`. Returns its id.
    pub async fn create_tunnel(
        &self,
        name: &str,
        kind: PlayitTunnelKind,
        port: u16,
    ) -> CoreResult<String> {
        let name = tunnel_name(name)?;
        let port = local_port(port)?;
        let key = self.require_key()?;
        let api = self.api()?;
        let agent = api.agent(&key).await?;
        api.create(
            &key,
            &agent.agent_id,
            &NewPlayitTunnel {
                name,
                kind,
                local_ip: "127.0.0.1".into(),
                local_port: port,
                enabled: true,
            },
        )
        .await
    }

    pub async fn rename_tunnel(&self, id: &str, name: &str) -> CoreResult<()> {
        let (id, name) = (tunnel_id(id)?, tunnel_name(name)?);
        self.api()?.rename(&self.require_key()?, id, &name).await
    }

    pub async fn set_tunnel_port(&self, id: &str, port: u16) -> CoreResult<()> {
        let (id, port) = (tunnel_id(id)?, local_port(port)?);
        self.api()?
            .set_local_address(&self.require_key()?, id, "127.0.0.1", port)
            .await
    }

    pub async fn set_tunnel_enabled(&self, id: &str, enabled: bool) -> CoreResult<()> {
        let id = tunnel_id(id)?;
        self.api()?
            .set_enabled(&self.require_key()?, id, enabled)
            .await
    }

    pub async fn delete_tunnel(&self, id: &str) -> CoreResult<()> {
        let id = tunnel_id(id)?;
        self.api()?.delete(&self.require_key()?, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_codes_look_like_playits() {
        let c = claim_code();
        assert_eq!(c.len(), 10);
        assert!(c.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(claim_code(), c);
    }

    #[test]
    fn claim_version_uses_the_installed_playit_version_format() {
        assert_eq!(
            format_claim_version("1.0.10\r\n"),
            Some("playit 1.0.10".into())
        );
        assert_eq!(format_claim_version(""), None);
        assert_eq!(format_claim_version("1.0.10\nInjected"), None);
    }

    #[test]
    fn tunnel_inputs_are_validated() {
        assert!(tunnel_id("3f1b2c4d-0000-4000-8000-000000000001").is_ok());
        assert!(tunnel_id("../x").is_err());
        assert_eq!(tunnel_name("  Survival ").unwrap(), "Survival");
        assert!(tunnel_name("").is_err());
        assert!(tunnel_name("Überwelt").is_err());
        assert!(tunnel_name(&"x".repeat(65)).is_err());
        assert!(local_port(0).is_err());
        assert_eq!(local_port(25565).unwrap(), 25565);
    }
}
