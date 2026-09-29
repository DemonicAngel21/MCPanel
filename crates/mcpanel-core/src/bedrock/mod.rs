//! Bedrock crossplay (spec §10): Geyser (+ Floodgate) installation per server software
//! from a data table (`data/bedrock.json`), targeted Geyser config patching, a UDP port
//! preflight and a RakNet ping probe.
//!
//! Geyser writes its config on first start. A partial config written before that (only
//! the keys MCPanel manages, no `config-version`) is completed by Geyser with defaults
//! while keeping MCPanel's values (verified with Geyser 2.11.3), so settings apply from
//! the very first start. The Floodgate key is never read: only its presence is reported.

pub mod ping;
pub mod yaml;

pub use ping::BedrockPong;

use crate::audit::AuditLog;
use crate::config::PropertiesDocument;
use crate::config::properties::decode_bytes;
use crate::content::{ContentService, InstallRequest, PendingKind};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::files::{SafeRoot, fsx};
use crate::ids::{JobId, ServerId};
use crate::jobs::JobContext;
use crate::model::{AuditResult, Server};
use crate::ports::PortStatus;
use crate::server::runtime::{Operation, ServerRuntime};
use crate::server::{LaunchHook, ServerManager};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

// ───────────────────────────── data table ─────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Source {
    provider: String,
    project: String,
    names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Platform {
    software: Vec<String>,
    min_game_version: Option<String>,
    #[serde(default)]
    latest_only: bool,
    geyser: Source,
    floodgate: Source,
    via_version: Option<Source>,
    geyser_config: String,
    floodgate_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Table {
    default_port: u16,
    via_version_below: String,
    geyser_started_pattern: String,
    via_version_warning_pattern: String,
    platforms: Vec<Platform>,
}

struct Data {
    table: Table,
    started: Regex,
}

static DATA: LazyLock<Option<Data>> = LazyLock::new(|| {
    let raw = include_str!("../../../../data/bedrock.json");
    let table: Table = serde_json::from_str(raw)
        .map_err(|e| tracing::error!("invalid bedrock.json: {e}"))
        .ok()?;
    let started = Regex::new(&table.geyser_started_pattern)
        .map_err(|e| tracing::error!("invalid Geyser pattern: {e}"))
        .ok()?;
    Some(Data { table, started })
});

fn data() -> CoreResult<&'static Data> {
    DATA.as_ref()
        .ok_or_else(|| CoreError::internal("Bedrock data table is invalid"))
}

fn platform_for(software: &str) -> Option<&'static Platform> {
    data()
        .ok()?
        .table
        .platforms
        .iter()
        .find(|p| p.software.iter().any(|s| s == software))
}

// ───────────────────────────── public types ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    /// Bedrock players also need a Java account.
    Online,
    Offline,
    /// Floodgate: Bedrock players join with their Xbox account.
    Floodgate,
}

impl AuthType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Floodgate => "floodgate",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "online" => Some(Self::Online),
            "offline" => Some(Self::Offline),
            "floodgate" => Some(Self::Floodgate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BedrockSettings {
    pub port: u16,
    pub auth_type: AuthType,
}

/// An installed (or queued) Geyser/Floodgate/ViaVersion file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BedrockPiece {
    pub file_name: String,
    pub version: Option<String>,
    pub enabled: bool,
    /// Queued while the server runs; installed when it stops or next starts.
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BedrockStatus {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub geyser: Option<BedrockPiece>,
    pub floodgate: Option<BedrockPiece>,
    pub via_version: Option<BedrockPiece>,
    /// ViaVersion can be installed for this software.
    pub via_version_available: bool,
    /// The server's Minecraft version is older than the one Geyser targets.
    pub via_version_suggested: bool,
    /// Geyser reported at the last start that it needs ViaVersion.
    pub via_version_required: bool,
    pub config_path: Option<String>,
    pub config_exists: bool,
    /// Effective settings (Geyser's defaults when the config does not exist yet).
    pub settings: BedrockSettings,
    /// The UDP port Geyser reported at the last start (while running).
    pub active_port: Option<u16>,
    pub running: bool,
    /// Settings changed since the server started.
    pub restart_required: bool,
    pub floodgate_key_present: bool,
    /// Whether another program listens on the Bedrock port (checked while stopped).
    pub port_status: Option<PortStatus>,
    pub default_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnableRequest {
    pub floodgate: bool,
    pub via_version: bool,
    pub port: Option<u16>,
}

// ───────────────────────────── service ─────────────────────────────

pub struct BedrockService {
    servers: Arc<ServerManager>,
    content: Arc<ContentService>,
    audit: Arc<AuditLog>,
    events: EventBus,
}

fn root(server: &Server) -> CoreResult<SafeRoot> {
    SafeRoot::open(&server.directory).map_err(|e| {
        CoreError::new(
            ErrorCode::PathNotFound,
            format!("Server directory is not accessible: {}", e.message),
        )
    })
}

fn read_small(root: &SafeRoot, rel: &str) -> CoreResult<Option<String>> {
    let p = root.resolve(rel)?;
    p.ensure_no_reparse_points()?;
    match std::fs::read(p.absolute()) {
        Ok(b) if b.len() > 1024 * 1024 => Err(CoreError::new(
            ErrorCode::FileTooLarge,
            format!("{rel} is too large"),
        )),
        Ok(b) => Ok(Some(decode_bytes(&b).to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::io(format!("Cannot read {rel}"), &e)),
    }
}

fn validate_port(port: u16) -> CoreResult<u16> {
    if port < 1024 {
        return Err(CoreError::invalid(
            "Use a Bedrock port from 1024 to 65535 (the default is 19132)",
        ));
    }
    Ok(port)
}

/// Console lines of the current (or last) session, oldest first.
fn session_lines(rt: &ServerRuntime) -> Vec<String> {
    let lines = rt.console.snapshot(None, usize::MAX);
    let start = lines
        .iter()
        .rposition(|l| {
            l.stream == crate::console::ConsoleStream::System && l.text.starts_with("Starting ")
        })
        .unwrap_or(0);
    lines[start..].iter().map(|l| l.text.clone()).collect()
}

impl BedrockService {
    pub fn new(
        servers: Arc<ServerManager>,
        content: Arc<ContentService>,
        audit: Arc<AuditLog>,
        events: EventBus,
    ) -> Arc<Self> {
        Arc::new(Self {
            servers,
            content,
            audit,
            events,
        })
    }

    fn changed(&self, server_id: ServerId) {
        self.events
            .publish(DomainEvent::BedrockChanged { server_id });
    }

    /// The platform entry for the server, or why Bedrock is unavailable.
    async fn platform(&self, server: &Server) -> Result<&'static Platform, String> {
        let sw = &server.software;
        let Some(p) = platform_for(&sw.software_id) else {
            return Err(format!(
                "Geyser is not available for {}. Use Paper, Purpur, Fabric or NeoForge.",
                self.servers
                    .registry()
                    .get_software(&sw.software_id)
                    .map_or_else(
                        |_| sw.software_id.clone(),
                        |s| s.descriptor.display_name.clone()
                    )
            ));
        };
        if let Some(min) = &p.min_game_version
            && self
                .servers
                .versions
                .is_at_least(&sw.game_version, min)
                .await
                == Some(false)
        {
            return Err(format!(
                "Geyser needs Minecraft {min} or newer on this server software (this server runs {}).",
                sw.game_version
            ));
        }
        Ok(p)
    }

    fn piece(list: &crate::content::service::ContentList, src: &Source) -> Option<BedrockPiece> {
        let matches_source = |r: &crate::content::InstalledContent| {
            r.source
                .as_ref()
                .is_some_and(|s| s.provider == src.provider && s.project_id == src.project)
        };
        let matches_name = |n: &str| src.names.iter().any(|x| x.eq_ignore_ascii_case(n));
        if let Some(e) = list.entries.iter().find(|e| {
            e.record.as_ref().is_some_and(matches_source)
                || e.descriptor
                    .as_ref()
                    .and_then(|d| d.name.as_deref())
                    .is_some_and(matches_name)
        }) {
            return Some(BedrockPiece {
                file_name: e.file_name.clone(),
                version: e
                    .record
                    .as_ref()
                    .and_then(|r| r.version_number.clone())
                    .or_else(|| e.descriptor.as_ref().and_then(|d| d.version.clone())),
                enabled: e.enabled,
                pending: e.pending.is_some(),
            });
        }
        list.pending_installs.iter().find_map(|p| match &p.change {
            PendingKind::Install { record, .. } if matches_source(record.as_ref()) => {
                Some(BedrockPiece {
                    file_name: record.file_name.clone(),
                    version: record.version_number.clone(),
                    enabled: true,
                    pending: true,
                })
            }
            _ => None,
        })
    }

    fn online_mode(root: &SafeRoot) -> bool {
        read_small(root, "server.properties")
            .ok()
            .flatten()
            .and_then(|t| {
                PropertiesDocument::parse(&t)
                    .get("online-mode")
                    .map(|v| v.trim().eq_ignore_ascii_case("true"))
            })
            .unwrap_or(true)
    }

    pub async fn status(&self, server_id: ServerId) -> CoreResult<BedrockStatus> {
        let server = self.servers.get(server_id).await?;
        let d = data()?;
        let rt = self.servers.runtime(server_id);
        let running = rt.state().has_process();
        let default = BedrockSettings {
            port: d.table.default_port,
            auth_type: AuthType::Online,
        };
        let mut status = BedrockStatus {
            supported: false,
            unsupported_reason: None,
            geyser: None,
            floodgate: None,
            via_version: None,
            via_version_available: false,
            via_version_suggested: false,
            via_version_required: false,
            config_path: None,
            config_exists: false,
            settings: default,
            active_port: None,
            running,
            restart_required: false,
            floodgate_key_present: false,
            port_status: None,
            default_port: d.table.default_port,
        };
        let p = match self.platform(&server).await {
            Ok(p) => p,
            Err(reason) => {
                status.unsupported_reason = Some(reason);
                return Ok(status);
            }
        };
        status.supported = true;
        let list = self.content.list(server_id).await?;
        status.geyser = Self::piece(&list, &p.geyser);
        status.floodgate = Self::piece(&list, &p.floodgate);
        if let Some(v) = &p.via_version {
            status.via_version_available = true;
            status.via_version = Self::piece(&list, v);
            status.via_version_suggested = self
                .servers
                .versions
                .is_at_least(&server.software.game_version, &d.table.via_version_below)
                .await
                == Some(false);
        }
        let root = root(&server)?;
        status.config_path = Some(p.geyser_config.clone());
        let config = read_small(&root, &p.geyser_config)?;
        status.config_exists = config.is_some();
        if let Some(text) = &config {
            status.settings = BedrockSettings {
                port: yaml::get(text, "bedrock", "port")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(d.table.default_port),
                auth_type: yaml::get(text, "java", "auth-type")
                    .and_then(|v| AuthType::parse(&v))
                    .unwrap_or(AuthType::Online),
            };
        }
        status.floodgate_key_present = root
            .resolve(&p.floodgate_key)
            .is_ok_and(|k| k.absolute().is_file());

        let lines = session_lines(&rt);
        if running {
            status.active_port = lines
                .iter()
                .rev()
                .find_map(|l| d.started.captures(l))
                .and_then(|c| c.get(1)?.as_str().parse().ok());
            status.restart_required = status
                .active_port
                .is_some_and(|p| p != status.settings.port);
        } else if status.geyser.is_some() {
            status.port_status = Some(self.servers.platform.udp_port_status(status.settings.port));
        }
        status.via_version_required = status.via_version.is_none()
            && lines
                .iter()
                .any(|l| l.contains(&d.table.via_version_warning_pattern));
        Ok(status)
    }

    /// Other MCPanel servers whose Geyser uses `port`.
    async fn port_taken_by(&self, server_id: ServerId, port: u16) -> Option<String> {
        let views = self.servers.list().await.ok()?;
        for v in views {
            if v.server.id == server_id {
                continue;
            }
            let Some(p) = platform_for(&v.server.software.software_id) else {
                continue;
            };
            let Ok(root) = SafeRoot::open(&v.server.directory) else {
                continue;
            };
            let Ok(Some(text)) = read_small(&root, &p.geyser_config) else {
                continue;
            };
            if yaml::get(&text, "bedrock", "port").and_then(|v| v.parse().ok()) == Some(port) {
                return Some(v.server.name);
            }
        }
        None
    }

    /// Write MCPanel's settings into the Geyser config (seeding it when absent).
    fn write_settings(root: &SafeRoot, rel: &str, s: &BedrockSettings) -> CoreResult<()> {
        let current = read_small(root, rel)?.unwrap_or_default();
        let text = yaml::set(&current, "bedrock", "port", &s.port.to_string());
        let text = yaml::set(&text, "java", "auth-type", s.auth_type.as_str());
        if text == current {
            return Ok(());
        }
        let path = root.resolve(rel)?;
        if let Some(parent) = path.absolute().parent() {
            fsx::ensure_real_dir_chain(root.path(), parent)?;
        }
        path.ensure_no_reparse_points()?;
        fsx::atomic_write(&path.absolute(), text.as_bytes())
    }

    pub async fn configure(
        &self,
        server_id: ServerId,
        settings: BedrockSettings,
        actor: &str,
    ) -> CoreResult<BedrockStatus> {
        let server = self.servers.get(server_id).await?;
        let p = self
            .platform(&server)
            .await
            .map_err(|m| CoreError::new(ErrorCode::Unsupported, m))?;
        validate_port(settings.port)?;
        if let Some(other) = self.port_taken_by(server_id, settings.port).await {
            return Err(CoreError::new(
                ErrorCode::Conflict,
                format!(
                    "UDP port {} is already the Bedrock port of \"{other}\"",
                    settings.port
                ),
            ));
        }
        let status = self.status(server_id).await?;
        if settings.auth_type == AuthType::Floodgate && status.floodgate.is_none() {
            return Err(CoreError::invalid(
                "Install Floodgate before selecting Floodgate authentication",
            ));
        }
        let rt = self.servers.runtime(server_id);
        let _op = rt.begin(Operation::EditingConfig)?;
        let root = root(&server)?;
        let rel = p.geyser_config.clone();
        let s2 = settings.clone();
        tokio::task::spawn_blocking(move || Self::write_settings(&root, &rel, &s2))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        drop(_op);
        self.audit
            .record(
                actor,
                "bedrock.configure",
                Some(server_id),
                Some(server.name.clone()),
                AuditResult::Success,
                serde_json::json!({ "port": settings.port, "authType": settings.auth_type.as_str() }),
            )
            .await;
        self.changed(server_id);
        self.status(server_id).await
    }

    /// Install Geyser (+ Floodgate, + ViaVersion) and write the settings, as a job.
    pub async fn enable(
        self: &Arc<Self>,
        server_id: ServerId,
        req: EnableRequest,
        actor: &str,
    ) -> CoreResult<JobId> {
        let server = self.servers.get(server_id).await?;
        let p = self
            .platform(&server)
            .await
            .map_err(|m| CoreError::new(ErrorCode::Unsupported, m))?;
        if req.via_version && p.via_version.is_none() {
            return Err(CoreError::invalid(
                "ViaVersion is not needed on this server software",
            ));
        }
        let current = self.status(server_id).await?;
        // Keep the configured port unless a new one is given.
        let port = validate_port(req.port.unwrap_or(current.settings.port))?;
        if let Some(other) = self.port_taken_by(server_id, port).await {
            return Err(CoreError::new(
                ErrorCode::Conflict,
                format!("UDP port {port} is already the Bedrock port of \"{other}\""),
            ));
        }
        // Fail early (before the job) when Geyser has no compatible build.
        let geyser_req = InstallRequest {
            provider: p.geyser.provider.clone(),
            project_id: p.geyser.project.clone(),
            version_id: None,
            with_dependencies: true,
        };
        if current.geyser.is_none() {
            self.content
                .plan(server_id, &geyser_req)
                .await
                .map_err(|e| {
                    if e.code == ErrorCode::VersionNotFound && p.latest_only {
                        CoreError::new(
                            ErrorCode::VersionNotFound,
                            format!(
                                "Geyser for {} supports only the latest Minecraft version, and there is no build for {} yet.",
                                server.software.software_id, server.software.game_version
                            ),
                        )
                    } else {
                        e
                    }
                })?;
        }
        let this = Arc::clone(self);
        let actor = actor.to_string();
        let name = server.name.clone();
        self.servers
            .jobs
            .spawn("bedrock.enable", Some(server_id), move |ctx| async move {
                let result = this.run_enable(server_id, p, &req, port, &ctx).await;
                this.audit
                    .record(
                        &actor,
                        "bedrock.enable",
                        Some(server_id),
                        Some(name),
                        if result.is_ok() {
                            AuditResult::Success
                        } else {
                            AuditResult::Failure
                        },
                        serde_json::json!({
                            "floodgate": req.floodgate,
                            "viaVersion": req.via_version,
                            "port": port,
                        }),
                    )
                    .await;
                this.changed(server_id);
                result.map(Some)
            })
            .await
    }

    async fn run_enable(
        &self,
        server_id: ServerId,
        p: &'static Platform,
        req: &EnableRequest,
        port: u16,
        ctx: &JobContext,
    ) -> CoreResult<serde_json::Value> {
        let status = self.status(server_id).await?;
        let mut wanted: Vec<(&Source, bool)> = vec![(&p.geyser, status.geyser.is_some())];
        if req.floodgate {
            wanted.push((&p.floodgate, status.floodgate.is_some()));
        }
        if req.via_version
            && let Some(v) = &p.via_version
        {
            wanted.push((v, status.via_version.is_some()));
        }
        let mut installed = Vec::new();
        let mut deferred = false;
        for (src, present) in wanted {
            ctx.check_cancelled()?;
            if present {
                continue;
            }
            ctx.progress(None, format!("Installing {}", src.names[0]));
            let r = self
                .content
                .install_in_job(
                    server_id,
                    &InstallRequest {
                        provider: src.provider.clone(),
                        project_id: src.project.clone(),
                        version_id: None,
                        with_dependencies: true,
                    },
                    ctx,
                )
                .await?;
            deferred |= r["deferred"].as_bool().unwrap_or(false);
            installed.push(src.project.clone());
        }
        let server = self.servers.get(server_id).await?;
        let root = root(&server)?;
        let floodgate = req.floodgate || status.floodgate.is_some();
        let auth_type = if floodgate {
            AuthType::Floodgate
        } else if Self::online_mode(&root) {
            AuthType::Online
        } else {
            AuthType::Offline
        };
        let settings = BedrockSettings { port, auth_type };
        let rel = p.geyser_config.clone();
        let rt = self.servers.runtime(server_id);
        let _op = rt.begin(Operation::EditingConfig)?;
        tokio::task::spawn_blocking(move || Self::write_settings(&root, &rel, &settings))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        Ok(serde_json::json!({
            "installed": installed,
            "deferred": deferred,
            "port": port,
            "authType": auth_type.as_str(),
        }))
    }

    /// Ping the running server's Bedrock listener on this computer.
    pub async fn ping(&self, server_id: ServerId) -> CoreResult<BedrockPong> {
        let status = self.status(server_id).await?;
        if !status.running {
            return Err(CoreError::new(
                ErrorCode::ServerNotRunning,
                "Start the server to test the Bedrock connection",
            ));
        }
        let port = status.active_port.unwrap_or(status.settings.port);
        ping::ping(
            std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_secs(2),
        )
        .await
    }
}

#[async_trait::async_trait]
impl LaunchHook for BedrockService {
    /// Refuse to start when another program already uses the Bedrock UDP port (Geyser
    /// would otherwise fail to bind while the Java side starts normally).
    async fn before_launch(&self, server: &Server, _runtime: &ServerRuntime) -> CoreResult<()> {
        let Ok(status) = self.status(server.id).await else {
            return Ok(());
        };
        let Some(geyser) = &status.geyser else {
            return Ok(());
        };
        if !geyser.enabled {
            return Ok(());
        }
        let port = status.settings.port;
        if let PortStatus::InUse { pid, process_name } = self.servers.platform.udp_port_status(port)
        {
            let who = match (process_name, pid) {
                (Some(n), Some(p)) => format!(" by {n} (PID {p})"),
                (None, Some(p)) => format!(" by PID {p}"),
                _ => String::new(),
            };
            return Err(CoreError::new(
                ErrorCode::PortInUse,
                format!(
                    "The Bedrock port {port} (UDP) is already in use{who}. Stop the other program or change the Bedrock port."
                ),
            )
            .with_details(serde_json::json!({ "port": port, "pid": pid, "protocol": "udp" })));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_table_is_valid() {
        let d = data().unwrap();
        assert_eq!(d.table.default_port, 19132);
        for sw in ["paper", "purpur", "fabric", "neoforge"] {
            let p = platform_for(sw).unwrap();
            assert!(!p.geyser.names.is_empty() && !p.floodgate.names.is_empty());
            assert!(p.floodgate_key.ends_with("floodgate/key.pem"));
            assert!(
                crate::files::sensitivity::classify(
                    &crate::files::safepath::parse_relative(&p.floodgate_key).unwrap()
                ) != crate::files::sensitivity::Sensitivity::Normal,
                "{sw}: key path is protected"
            );
        }
        assert!(platform_for("vanilla").is_none());
        assert_eq!(
            d.started
                .captures("[Geyser-Spigot] Started Geyser on UDP port 19140")
                .and_then(|c| c.get(1))
                .map(|m| m.as_str()),
            Some("19140")
        );
    }

    #[test]
    fn settings_seed_a_partial_config_and_patch_an_existing_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = SafeRoot::open(dir.path()).unwrap();
        let rel = "plugins/Geyser-Spigot/config.yml";
        let s = BedrockSettings {
            port: 19140,
            auth_type: AuthType::Floodgate,
        };
        BedrockService::write_settings(&root, rel, &s).unwrap();
        let text = std::fs::read_to_string(dir.path().join(rel)).unwrap();
        assert_eq!(
            text,
            "bedrock:\n  port: 19140\njava:\n  auth-type: floodgate\n"
        );
        assert!(!text.contains("config-version"));

        let full = "# c\nbedrock:\n  address: 0.0.0.0\n  port: 19140\njava:\n  auth-type: floodgate\nconfig-version: 8\n";
        std::fs::write(dir.path().join(rel), full).unwrap();
        let s = BedrockSettings {
            port: 19200,
            auth_type: AuthType::Online,
        };
        BedrockService::write_settings(&root, rel, &s).unwrap();
        let text = std::fs::read_to_string(dir.path().join(rel)).unwrap();
        assert_eq!(
            text,
            full.replace("19140", "19200")
                .replace("floodgate", "online")
        );
    }

    #[test]
    fn ports_below_1024_are_rejected() {
        assert!(validate_port(80).is_err());
        assert_eq!(validate_port(19132).unwrap(), 19132);
    }
}
