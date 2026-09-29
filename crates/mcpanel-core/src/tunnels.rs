//! Internet access through tunnels (spec §10). Playit.gg is supported through its
//! officially published interfaces only: the `playit` program (CLI) and the playit.gg
//! website. Verified 2026-09-29 against playit 1.0.10 (source: playit-cloud/playit-agent):
//!
//! - The Windows installer installs `%ProgramFiles%\playit_gg\bin\playit.exe` and the
//!   `playitd` service; `playit version`, `playit status` ("The playit service is not
//!   running." or "playit service status:" with `Phase:` / `Secret configured:` lines),
//!   `playit start|stop`, `playit claim …` and `playit setup` are the documented commands.
//! - Tunnels are created in the playit.gg dashboard. The agent's HTTP API client is an
//!   internal part of the agent and not documented for third parties, so MCPanel does
//!   not create tunnels; it detects the agent, reports its state and links to the
//!   dashboard (`TunnelCaps::can_create_via_api = false`).
//!
//! MCPanel never starts or stops the agent on its own: that would publish the user's
//! tunnels.

use crate::error::CoreResult;
use crate::ports::Platform;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelCaps {
    /// Tunnels can be created through a supported API.
    pub can_create_via_api: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelLink {
    pub label: String,
    pub url: String,
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

pub struct PlayitTunnel {
    platform: Arc<dyn Platform>,
}

const TIMEOUT: Duration = Duration::from_secs(10);

impl PlayitTunnel {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
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

    async fn run(&self, exe: &Path, arg: &str) -> Option<String> {
        let out = self
            .platform
            .run_capture(exe, &[arg.to_string()], None, TIMEOUT)
            .await
            .ok()?;
        (!out.timed_out && out.code == Some(0))
            .then(|| String::from_utf8_lossy(&out.stdout).to_string())
    }

    pub async fn status(&self) -> CoreResult<TunnelStatus> {
        let links = vec![
            TunnelLink {
                label: "Download playit".into(),
                url: "https://playit.gg/download".into(),
            },
            TunnelLink {
                label: "playit.gg dashboard".into(),
                url: "https://playit.gg/account/agents".into(),
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
            caps: TunnelCaps {
                can_create_via_api: false,
            },
            links,
        };
        let Some(exe) = Self::locate() else {
            return Ok(status);
        };
        status.installed = true;
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
        status.executable = Some(exe);
        Ok(status)
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
    fn unknown_output_is_not_guessed() {
        assert_eq!(parse_playit_status("something else"), None);
    }
}
