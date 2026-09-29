//! Internet access through tunnels (spec §10). Playit.gg is supported through its
//! officially published interfaces only: the `playit` program (CLI) and the playit.gg
//! website. Verified 2026-09-30 against playit 1.0.10 (source: playit-cloud/playit-agent
//! @ 4c27794, `packages/playit-cli/src/main.rs`; ADR-0007):
//!
//! - The Windows installer installs `%ProgramFiles%\playit_gg\bin\playit.exe` and the
//!   `playitd` service (the installer lets users start and stop it).
//! - `playit version` / `status` report the agent; `playit start` / `stop` control the
//!   service; `playit setup` links an agent that has no secret yet: it prints
//!   "Open this link to finish setting up playit:" and a `https://playit.gg/claim/<hex>`
//!   URL, waits until the user approves the agent in the browser and hands the secret
//!   straight to the service. MCPanel never sees or stores the secret.
//! - The CLI has no tunnel commands (1.0 removed `tunnels prepare/list`). Tunnel creation
//!   and listing exist only in playit's private HTTP API ("not public, no ETA", issue
//!   #150) and the agent's internal IPC, so MCPanel does not create, list or change
//!   tunnels (`TunnelCaps::can_manage_tunnels = false`). Users create the tunnel in the
//!   playit.gg dashboard and may save its public address on the server in MCPanel.
//!
//! MCPanel only starts or stops the agent when the user asks: that publishes or
//! unpublishes every tunnel of the agent.

use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::ids::ServerId;
use crate::ports::{Platform, ProcessController, ProcessSpec, SettingsRepository};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelCaps {
    /// The agent can be started and stopped through the official CLI.
    pub can_control_agent: bool,
    /// The agent can be linked to a playit.gg account (`playit setup`).
    pub can_link: bool,
    /// Tunnels can be created, listed and changed through a supported interface.
    pub can_manage_tunnels: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelLink {
    pub label: String,
    pub url: String,
}

/// Progress of an account link started from MCPanel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LinkProgress {
    /// Waiting for the user to approve the agent at `claim_url`.
    Waiting {
        claim_url: String,
    },
    Linked,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelStatus {
    pub provider: String,
    pub display_name: String,
    pub installed: bool,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    /// `None` when the state could not be determined.
    pub agent_running: Option<bool>,
    /// The agent's phase as the CLI reports it (e.g. "running", "waiting for secret").
    pub phase: Option<String>,
    pub secret_configured: Option<bool>,
    pub link: Option<LinkProgress>,
    pub caps: TunnelCaps,
    pub links: Vec<TunnelLink>,
}

/// Parsed `playit status` output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayitServiceStatus {
    pub running: bool,
    pub phase: Option<String>,
    pub secret_configured: Option<bool>,
}

pub fn parse_playit_status(text: &str) -> Option<PlayitServiceStatus> {
    if text.contains("service is not running") || text.contains("is not reachable at socket") {
        return Some(PlayitServiceStatus::default());
    }
    if !text.contains("status") || !text.contains("Phase:") {
        return None;
    }
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(name).map(|v| v.trim().to_string()))
    };
    Some(PlayitServiceStatus {
        running: true,
        phase: field("Phase:"),
        secret_configured: field("Secret configured:").map(|v| v == "true"),
    })
}

/// The claim URL in a line printed by `playit setup`, if it is one.
pub fn parse_claim_url(line: &str) -> Option<String> {
    let line = line.trim();
    let code = line.strip_prefix("https://playit.gg/claim/")?;
    (!code.is_empty() && code.len() <= 64 && code.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| line.to_string())
}

/// Validate a public address copied from the playit.gg dashboard (`host` or
/// `host:port`). Empty input clears the address.
pub fn normalize_public_address(input: &str) -> CoreResult<Option<String>> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let invalid = || {
        CoreError::invalid(
            "Enter the address as shown in the playit.gg dashboard, e.g. example.joinmc.link or example.ply.gg:12345.",
        )
    };
    let (host, port) = match s.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (s, None),
    };
    let host_ok = host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok_and(|n| n > 0));
    if !host_ok || !port_ok {
        return Err(invalid());
    }
    Ok(Some(s.to_ascii_lowercase()))
}

struct LinkFlow {
    progress: Arc<Mutex<LinkProgress>>,
    controller: Arc<dyn ProcessController>,
}

pub struct PlayitTunnel {
    platform: Arc<dyn Platform>,
    settings: Arc<dyn SettingsRepository>,
    link: Mutex<Option<LinkFlow>>,
    /// `--socket-path` for a separate playitd instance (tests); `None` = the service.
    socket_path: Option<String>,
}

const TIMEOUT: Duration = Duration::from_secs(10);
/// `playit start` waits for the service to come up.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(45);
const CLAIM_URL_TIMEOUT: Duration = Duration::from_secs(30);

fn address_key(server: ServerId) -> String {
    format!("tunnel.playit.address.{server}")
}

/// First non-empty output line, shortened, for error messages.
fn first_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(200).collect())
}

impl PlayitTunnel {
    pub fn new(platform: Arc<dyn Platform>, settings: Arc<dyn SettingsRepository>) -> Self {
        Self {
            platform,
            settings,
            link: Mutex::new(None),
            socket_path: None,
        }
    }

    /// Talk to a separate playitd instance listening on `socket_path` instead of the
    /// installed service.
    pub fn with_socket_path(mut self, socket_path: impl Into<String>) -> Self {
        self.socket_path = Some(socket_path.into());
        self
    }

    fn args(&self, arg: &str) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(socket) = &self.socket_path {
            args.push("--socket-path".to_string());
            args.push(socket.clone());
        }
        args.push(arg.to_string());
        args
    }

    /// The installed `playit.exe` (installer location first, then PATH).
    pub fn locate() -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(dir) = std::env::var_os(var) {
                candidates.push(Path::new(&dir).join(r"playit_gg\bin\playit.exe"));
            }
        }
        if let Some(path) = std::env::var_os("PATH") {
            candidates.extend(std::env::split_paths(&path).map(|d| d.join("playit.exe")));
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    fn exe() -> CoreResult<PathBuf> {
        Self::locate().ok_or_else(|| {
            CoreError::new(
                ErrorCode::NotFound,
                "The playit program is not installed. Download it from playit.gg.",
            )
        })
    }

    async fn run(&self, exe: &Path, arg: &str) -> Option<String> {
        let out = self
            .platform
            .run_capture(exe, &self.args(arg), None, TIMEOUT)
            .await
            .ok()?;
        (!out.timed_out && out.code == Some(0))
            .then(|| String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn link_progress(&self) -> Option<LinkProgress> {
        let link = self.link.lock().unwrap_or_else(|e| e.into_inner());
        link.as_ref()
            .map(|f| f.progress.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    pub async fn status(&self) -> CoreResult<TunnelStatus> {
        let links = vec![
            TunnelLink {
                label: "Download playit".into(),
                url: "https://playit.gg/download".into(),
            },
            TunnelLink {
                label: "Tunnels on playit.gg".into(),
                url: "https://playit.gg/account/tunnels".into(),
            },
        ];
        let mut status = TunnelStatus {
            provider: "playit".into(),
            display_name: "playit.gg".into(),
            installed: false,
            executable: None,
            version: None,
            agent_running: None,
            phase: None,
            secret_configured: None,
            link: self.link_progress(),
            caps: TunnelCaps {
                can_control_agent: false,
                can_link: false,
                can_manage_tunnels: false,
            },
            links,
        };
        let Some(exe) = Self::locate() else {
            return Ok(status);
        };
        status.installed = true;
        status.caps.can_control_agent = true;
        status.version = self
            .run(&exe, "version")
            .await
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty() && v.len() < 40);
        if let Some(s) = self
            .run(&exe, "status")
            .await
            .and_then(|t| parse_playit_status(&t))
        {
            status.agent_running = Some(s.running);
            status.phase = s.phase;
            status.secret_configured = s.secret_configured;
        }
        status.caps.can_link =
            status.agent_running == Some(true) && status.secret_configured == Some(false);
        status.executable = Some(exe);
        Ok(status)
    }

    async fn control(&self, arg: &str, verb: &str) -> CoreResult<TunnelStatus> {
        let exe = Self::exe()?;
        let out = self
            .platform
            .run_capture(&exe, &self.args(arg), None, CONTROL_TIMEOUT)
            .await?;
        if out.timed_out {
            return Err(CoreError::new(
                ErrorCode::Io,
                format!("The playit program did not {verb} the agent in time."),
            ));
        }
        if out.code != Some(0) {
            let why = first_line(&out.stderr)
                .or_else(|| first_line(&out.stdout))
                .unwrap_or_else(|| format!("exit code {:?}", out.code));
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("playit could not {verb} the agent: {why}"),
            ));
        }
        self.status().await
    }

    /// `playit start`: starts the playitd service (publishes the agent's tunnels).
    pub async fn start_agent(&self) -> CoreResult<TunnelStatus> {
        self.control("start", "start").await
    }

    /// `playit stop`: stops the playitd service.
    pub async fn stop_agent(&self) -> CoreResult<TunnelStatus> {
        self.cancel_link();
        self.control("stop", "stop").await
    }

    /// Link the agent to a playit.gg account with `playit setup`. Returns the claim URL
    /// the user opens to approve the agent; the secret goes from playit to its service
    /// directly. The rest of setup's output (it includes a login link) is discarded.
    pub async fn begin_link(&self) -> CoreResult<String> {
        if let Some(LinkProgress::Waiting { claim_url }) = self.link_progress() {
            return Ok(claim_url);
        }
        let status = self.status().await?;
        let exe = status.executable.clone().ok_or_else(|| {
            CoreError::new(
                ErrorCode::NotFound,
                "The playit program is not installed. Download it from playit.gg.",
            )
        })?;
        if status.agent_running != Some(true) {
            return Err(CoreError::new(
                ErrorCode::Conflict,
                "Start the playit agent first.",
            ));
        }
        if status.secret_configured != Some(false) {
            return Err(CoreError::new(
                ErrorCode::Conflict,
                "This playit agent is already linked to a playit.gg account.",
            ));
        }
        let spawned = self.platform.spawn(&ProcessSpec {
            program: exe.clone(),
            args: self.args("setup"),
            cwd: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
            env: Vec::new(),
            group_name: None,
        })?;
        let controller = Arc::clone(&spawned.controller);
        let mut lines = BufReader::new(spawned.stdout).lines();
        let found = tokio::time::timeout(CLAIM_URL_TIMEOUT, async {
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(url) = parse_claim_url(&line) {
                    return Some(url);
                }
            }
            None
        })
        .await
        .ok()
        .flatten();
        let Some(claim_url) = found else {
            let _ = controller.terminate_tree();
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                "playit setup did not provide a link to approve the agent. Try again.",
            ));
        };
        let progress = Arc::new(Mutex::new(LinkProgress::Waiting {
            claim_url: claim_url.clone(),
        }));
        let done = Arc::clone(&progress);
        let mut stderr = spawned.stderr;
        let waiter = spawned.waiter;
        tokio::spawn(async move {
            // Drain both pipes so setup never blocks; their content is not kept.
            let drain_err = async {
                let mut sink = Vec::new();
                let _ = stderr.read_to_end(&mut sink).await;
            };
            let drain_out = async { while let Ok(Some(_)) = lines.next_line().await {} };
            tokio::join!(drain_err, drain_out);
            let exit = waiter.wait().await;
            let mut p = done.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*p, LinkProgress::Waiting { .. }) {
                *p = match exit {
                    Ok(e) if e.code == Some(0) => LinkProgress::Linked,
                    _ => LinkProgress::Failed {
                        message: "Linking did not finish. Try again.".into(),
                    },
                };
            }
        });
        *self.link.lock().unwrap_or_else(|e| e.into_inner()) = Some(LinkFlow {
            progress,
            controller,
        });
        Ok(claim_url)
    }

    /// Stop a pending link (ends the `playit setup` process).
    pub fn cancel_link(&self) {
        if let Some(flow) = self.link.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let mut p = flow.progress.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*p, LinkProgress::Waiting { .. }) {
                *p = LinkProgress::Failed {
                    message: "Cancelled".into(),
                };
                let _ = flow.controller.terminate_tree();
            }
        }
    }

    /// The public address the user saved for a server (from the playit.gg dashboard).
    pub async fn server_address(&self, server: ServerId) -> CoreResult<Option<String>> {
        Ok(self
            .settings
            .get(&address_key(server))
            .await?
            .and_then(|v| v.as_str().map(str::to_string)))
    }

    pub async fn set_server_address(
        &self,
        server: ServerId,
        address: &str,
    ) -> CoreResult<Option<String>> {
        let address = normalize_public_address(address)?;
        let value = address
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String);
        self.settings.set(&address_key(server), &value).await?;
        Ok(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_stopped_service() {
        let s = parse_playit_status("The playit service is not running.\n").unwrap();
        assert!(!s.running);
    }

    #[test]
    fn parses_a_running_service() {
        let text = "playit service status:\n  Phase: running\n  PID: 42\n  Uptime: 10 seconds\n  Version: 1.0.10\n  Socket: \\\\.\\pipe\\playitd-system\n  Secret path: C:\\x\\playit.toml\n  Secret configured: true\n  IPC version: 1\n";
        let s = parse_playit_status(text).unwrap();
        assert_eq!(
            s,
            PlayitServiceStatus {
                running: true,
                phase: Some("running".into()),
                secret_configured: Some(true),
            }
        );
    }

    #[test]
    fn parses_an_agent_waiting_for_its_secret() {
        // Real output of playit 1.0.10 for a daemon without a secret.
        let text = "playitd daemon status for socket \\\\.\\pipe\\x:\n  Phase: waiting for secret\n  PID: 28392\n  Secret configured: false\n  IPC version: 2\n";
        let s = parse_playit_status(text).unwrap();
        assert_eq!(s.phase.as_deref(), Some("waiting for secret"));
        assert_eq!(s.secret_configured, Some(false));
    }

    #[test]
    fn unknown_output_is_not_guessed() {
        assert_eq!(parse_playit_status("something else"), None);
    }

    #[test]
    fn claim_urls_are_recognised_strictly() {
        assert_eq!(
            parse_claim_url("https://playit.gg/claim/52564d60a7\r").as_deref(),
            Some("https://playit.gg/claim/52564d60a7")
        );
        for bad in [
            "Open this link to finish setting up playit:",
            "https://playit.gg/claim/",
            "https://playit.gg/claim/zz",
            "https://playit.gg/login/guest-account/abc",
            "https://evil.example/claim/52564d60a7",
        ] {
            assert_eq!(parse_claim_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn public_addresses_are_validated() {
        assert_eq!(normalize_public_address("  ").unwrap(), None);
        assert_eq!(
            normalize_public_address("Fancy-Name.joinmc.link")
                .unwrap()
                .as_deref(),
            Some("fancy-name.joinmc.link")
        );
        assert_eq!(
            normalize_public_address("abc.ply.gg:12345")
                .unwrap()
                .as_deref(),
            Some("abc.ply.gg:12345")
        );
        for bad in [
            "localhost",
            "abc.ply.gg:0",
            "abc.ply.gg:70000",
            "http://abc.ply.gg",
            "a b.ply.gg",
            "-a.ply.gg",
        ] {
            assert!(normalize_public_address(bad).is_err(), "{bad}");
        }
    }
}
