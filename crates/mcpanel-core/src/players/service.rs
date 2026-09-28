//! Player management service: combines the server's list files, the live console and
//! MCPanel's session history.

use super::lists::{
    BANNED_IPS, BANNED_PLAYERS, Ban, LEGACY_FILES, ListFile, ListedPlayer, OPS, Operator,
    USER_CACHE, WHITELIST, vanilla_timestamp,
};
use super::offline_uuid;
use crate::audit::AuditLog;
use crate::config::PropertiesDocument;
use crate::config::properties::decode_bytes;
use crate::console::{ConsoleStream, dialect};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::files::{SafeRoot, fsx};
use crate::ids::ServerId;
use crate::lifecycle::LifecycleState;
use crate::model::{AuditResult, Server};
use crate::ports::{PlayerRepository, PlayerStats, ProfileLookup};
use crate::server::runtime::Operation;
use crate::server::{PropertyChange, ServerManager};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

/// How long to collect the server's reply to a console command.
const REPLY_WINDOW: Duration = Duration::from_millis(1500);
const REPLY_QUIET: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PlayerAction {
    Op {
        name: String,
    },
    Deop {
        name: String,
    },
    WhitelistAdd {
        name: String,
    },
    WhitelistRemove {
        name: String,
    },
    SetWhitelist {
        enabled: bool,
    },
    Ban {
        name: String,
        reason: Option<String>,
    },
    Pardon {
        name: String,
    },
    BanIp {
        ip: String,
        reason: Option<String>,
    },
    PardonIp {
        ip: String,
    },
    Kick {
        name: String,
        reason: Option<String>,
    },
}

impl PlayerAction {
    fn audit_name(&self) -> &'static str {
        match self {
            Self::Op { .. } => "player.op",
            Self::Deop { .. } => "player.deop",
            Self::WhitelistAdd { .. } => "player.whitelist_add",
            Self::WhitelistRemove { .. } => "player.whitelist_remove",
            Self::SetWhitelist { .. } => "player.set_whitelist",
            Self::Ban { .. } => "player.ban",
            Self::Pardon { .. } => "player.pardon",
            Self::BanIp { .. } => "player.ban_ip",
            Self::PardonIp { .. } => "player.pardon_ip",
            Self::Kick { .. } => "player.kick",
        }
    }

    /// The audited target (a player name; IP addresses are not recorded).
    fn audit_target(&self) -> Option<String> {
        match self {
            Self::Op { name }
            | Self::Deop { name }
            | Self::WhitelistAdd { name }
            | Self::WhitelistRemove { name }
            | Self::Ban { name, .. }
            | Self::Pardon { name }
            | Self::Kick { name, .. } => Some(name.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppliedVia {
    /// Sent to the running server as a console command.
    Console,
    /// Written to the server's files (server stopped).
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub via: AppliedVia,
    /// The server's reply (console) or MCPanel's description (files).
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownPlayer {
    pub name: String,
    pub uuid: Option<Uuid>,
    pub online: bool,
    pub op_level: Option<u8>,
    pub whitelisted: bool,
    pub banned: bool,
    pub stats: Option<PlayerStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerPlayers {
    /// Changes are sent to the running server through its console.
    pub live: bool,
    /// Why changes are not possible right now, if so.
    pub read_only_reason: Option<String>,
    /// Whether the online player list is known (console attached).
    pub online_known: bool,
    pub online: Vec<String>,
    pub online_mode: bool,
    pub whitelist_enabled: bool,
    pub enforce_whitelist: bool,
    pub max_players: Option<u32>,
    pub operators: Vec<Operator>,
    pub whitelist: Vec<ListedPlayer>,
    pub bans: Vec<Ban>,
    pub ip_bans: Vec<Ban>,
    pub known: Vec<KnownPlayer>,
}

pub fn validate_player_name(name: &str) -> CoreResult<String> {
    let name = name.trim();
    // Java names are [A-Za-z0-9_]{3,16}; Bedrock players via Floodgate get a prefix
    // such as '.' and may be longer. Never spaces: names are command arguments.
    let ok = (1..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '*' | '-'));
    if !ok {
        return Err(CoreError::invalid(
            "A player name may only contain letters, digits and _ (at most 16 characters for Java players)",
        ));
    }
    Ok(name.to_string())
}

fn validate_reason(reason: Option<String>) -> CoreResult<Option<String>> {
    let reason = reason
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    if reason
        .as_ref()
        .is_some_and(|r| r.chars().count() > 256 || r.chars().any(char::is_control))
    {
        return Err(CoreError::invalid(
            "A reason must be a single line of at most 256 characters",
        ));
    }
    Ok(reason)
}

fn validate_ip(ip: &str) -> CoreResult<String> {
    ip.trim()
        .parse::<std::net::IpAddr>()
        .map(|a| a.to_string())
        .map_err(|_| CoreError::invalid("Enter a valid IPv4 or IPv6 address"))
}

fn command_for(action: &PlayerAction) -> String {
    let with_reason = |base: String, r: &Option<String>| match r {
        Some(r) => format!("{base} {r}"),
        None => base,
    };
    match action {
        PlayerAction::Op { name } => format!("op {name}"),
        PlayerAction::Deop { name } => format!("deop {name}"),
        PlayerAction::WhitelistAdd { name } => format!("whitelist add {name}"),
        PlayerAction::WhitelistRemove { name } => format!("whitelist remove {name}"),
        PlayerAction::SetWhitelist { enabled } => {
            format!("whitelist {}", if *enabled { "on" } else { "off" })
        }
        PlayerAction::Ban { name, reason } => with_reason(format!("ban {name}"), reason),
        PlayerAction::Pardon { name } => format!("pardon {name}"),
        PlayerAction::BanIp { ip, reason } => with_reason(format!("ban-ip {ip}"), reason),
        PlayerAction::PardonIp { ip } => format!("pardon-ip {ip}"),
        PlayerAction::Kick { name, reason } => with_reason(format!("kick {name}"), reason),
    }
}

/// Strip the log prefix and 26.x's "System chat: " from a server reply line.
fn reply_text(raw: &str) -> String {
    let msg = dialect::parse_line(raw).message;
    msg.strip_prefix("System chat: ")
        .unwrap_or(&msg)
        .to_string()
}

pub struct PlayerService {
    servers: Arc<ServerManager>,
    repo: Arc<dyn PlayerRepository>,
    profiles: Arc<dyn ProfileLookup>,
    audit: Arc<AuditLog>,
    events: EventBus,
}

struct FileSet {
    ops: ListFile,
    whitelist: ListFile,
    bans: ListFile,
    ip_bans: ListFile,
    cache: ListFile,
    props: PropertiesDocument,
    legacy: bool,
}

fn read_text(root: &SafeRoot, name: &str) -> CoreResult<Option<String>> {
    let p = root.resolve(name)?;
    p.ensure_no_reparse_points()?;
    match std::fs::read(p.absolute()) {
        Ok(b) if b.len() > 64 * 1024 * 1024 => Err(CoreError::new(
            ErrorCode::FileTooLarge,
            format!("{name} is too large"),
        )),
        Ok(b) => Ok(Some(decode_bytes(&b).to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::io(format!("Cannot read {name}"), &e)),
    }
}

fn read_files(root: &SafeRoot) -> CoreResult<FileSet> {
    let list = |name: &str| -> CoreResult<ListFile> {
        ListFile::parse(read_text(root, name)?.as_deref(), name)
    };
    let legacy = LEGACY_FILES.iter().any(|f| root.path().join(f).exists())
        && ![OPS, WHITELIST, BANNED_PLAYERS]
            .iter()
            .any(|f| root.path().join(f).exists());
    Ok(FileSet {
        ops: list(OPS)?,
        whitelist: list(WHITELIST)?,
        bans: list(BANNED_PLAYERS)?,
        ip_bans: list(BANNED_IPS)?,
        cache: list(USER_CACHE).unwrap_or_default(),
        props: PropertiesDocument::parse(
            &read_text(root, "server.properties")?.unwrap_or_default(),
        ),
        legacy,
    })
}

fn bool_prop(p: &PropertiesDocument, key: &str, default: bool) -> bool {
    p.get(key)
        .map_or(default, |v| v.trim().eq_ignore_ascii_case("true"))
}

impl PlayerService {
    pub fn new(
        servers: Arc<ServerManager>,
        repo: Arc<dyn PlayerRepository>,
        profiles: Arc<dyn ProfileLookup>,
        audit: Arc<AuditLog>,
        events: EventBus,
    ) -> Arc<Self> {
        Arc::new(Self {
            servers,
            repo,
            profiles,
            audit,
            events,
        })
    }

    fn root(server: &Server) -> CoreResult<SafeRoot> {
        SafeRoot::open(&server.directory).map_err(|e| {
            CoreError::new(
                ErrorCode::PathNotFound,
                format!("Server directory is not accessible: {}", e.message),
            )
        })
    }

    /// How changes can be applied now: `Ok(true)` = console, `Ok(false)` = files.
    fn mode(&self, server_id: ServerId) -> CoreResult<bool> {
        let snap = self.servers.runtime(server_id).snapshot();
        match snap.state {
            LifecycleState::Running if snap.console_attached => Ok(true),
            LifecycleState::Created
            | LifecycleState::Stopped
            | LifecycleState::Crashed
            | LifecycleState::Error => Ok(false),
            LifecycleState::Starting => Err(CoreError::new(
                ErrorCode::ServerBusy,
                "Wait until the server has started",
            )),
            LifecycleState::Stopping | LifecycleState::Restarting => Err(CoreError::new(
                ErrorCode::ServerBusy,
                "The server is stopping; try again when it has stopped",
            )),
            LifecycleState::Detached | LifecycleState::Running => Err(CoreError::new(
                ErrorCode::Unsupported,
                "The console of this server is not connected. Player changes are possible once it has stopped or MCPanel started it.",
            )),
        }
    }

    pub async fn view(&self, server_id: ServerId) -> CoreResult<ServerPlayers> {
        let server = self.servers.get(server_id).await?;
        let root = Self::root(&server)?;
        let files = tokio::task::spawn_blocking(move || read_files(&root))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        let snap = self.servers.runtime(server_id).snapshot();
        let mode = self.mode(server_id);
        let stats = self.repo.stats(server_id).await?;

        let operators = files.ops.operators();
        let whitelist = files.whitelist.players();
        let bans = files.bans.bans("name");
        let ip_bans = files.ip_bans.bans("ip");

        let mut known: BTreeMap<String, KnownPlayer> = BTreeMap::new();
        let mut touch = |name: &str, uuid: Option<Uuid>| {
            let e = known
                .entry(name.to_lowercase())
                .or_insert_with(|| KnownPlayer {
                    name: name.to_string(),
                    uuid: None,
                    online: false,
                    op_level: None,
                    whitelisted: false,
                    banned: false,
                    stats: None,
                });
            if e.uuid.is_none() {
                e.uuid = uuid;
            }
        };
        for p in files.cache.players() {
            touch(&p.name, p.uuid);
        }
        for s in &stats {
            touch(&s.name, None);
        }
        for o in &operators {
            touch(&o.name, o.uuid);
        }
        for w in &whitelist {
            touch(&w.name, w.uuid);
        }
        for b in &bans {
            touch(&b.target, b.uuid);
        }
        for n in &snap.online_players {
            touch(n, None);
        }
        for k in known.values_mut() {
            let lower = k.name.to_lowercase();
            k.online = snap
                .online_players
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&k.name));
            k.op_level = operators
                .iter()
                .find(|o| o.name.eq_ignore_ascii_case(&k.name))
                .map(|o| o.level);
            k.whitelisted = whitelist
                .iter()
                .any(|w| w.name.eq_ignore_ascii_case(&k.name));
            k.banned = bans.iter().any(|b| b.target.eq_ignore_ascii_case(&k.name));
            k.stats = stats
                .iter()
                .find(|s| s.name.to_lowercase() == lower)
                .cloned();
            if k.uuid.is_none() {
                k.uuid = files.cache.cached_uuid(&k.name);
            }
        }
        let mut known: Vec<KnownPlayer> = known.into_values().collect();
        known.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then_with(|| {
                    b.stats
                        .as_ref()
                        .map(|s| s.last_seen)
                        .cmp(&a.stats.as_ref().map(|s| s.last_seen))
                })
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        let read_only_reason = if files.legacy {
            Some(
                "This server uses the pre-1.7.6 text player lists, which MCPanel does not edit."
                    .to_string(),
            )
        } else {
            mode.as_ref().err().map(|e| e.message.clone())
        };
        Ok(ServerPlayers {
            live: matches!(mode, Ok(true)),
            read_only_reason,
            online_known: snap.console_attached,
            online: snap.online_players.clone(),
            online_mode: bool_prop(&files.props, "online-mode", true),
            whitelist_enabled: bool_prop(&files.props, "white-list", false),
            enforce_whitelist: bool_prop(&files.props, "enforce-whitelist", false),
            max_players: files
                .props
                .get("max-players")
                .and_then(|v| v.trim().parse().ok()),
            operators,
            whitelist,
            bans,
            ip_bans,
            known,
        })
    }

    fn validate(action: PlayerAction) -> CoreResult<PlayerAction> {
        Ok(match action {
            PlayerAction::Op { name } => PlayerAction::Op {
                name: validate_player_name(&name)?,
            },
            PlayerAction::Deop { name } => PlayerAction::Deop {
                name: validate_player_name(&name)?,
            },
            PlayerAction::WhitelistAdd { name } => PlayerAction::WhitelistAdd {
                name: validate_player_name(&name)?,
            },
            PlayerAction::WhitelistRemove { name } => PlayerAction::WhitelistRemove {
                name: validate_player_name(&name)?,
            },
            PlayerAction::SetWhitelist { enabled } => PlayerAction::SetWhitelist { enabled },
            PlayerAction::Ban { name, reason } => PlayerAction::Ban {
                name: validate_player_name(&name)?,
                reason: validate_reason(reason)?,
            },
            PlayerAction::Pardon { name } => PlayerAction::Pardon {
                name: validate_player_name(&name)?,
            },
            PlayerAction::BanIp { ip, reason } => PlayerAction::BanIp {
                ip: validate_ip(&ip)?,
                reason: validate_reason(reason)?,
            },
            PlayerAction::PardonIp { ip } => PlayerAction::PardonIp {
                ip: validate_ip(&ip)?,
            },
            PlayerAction::Kick { name, reason } => PlayerAction::Kick {
                name: validate_player_name(&name)?,
                reason: validate_reason(reason)?,
            },
        })
    }

    pub async fn apply(
        &self,
        server_id: ServerId,
        action: PlayerAction,
        actor: &str,
    ) -> CoreResult<ActionOutcome> {
        let action = Self::validate(action)?;
        let server = self.servers.get(server_id).await?;
        let result = match self.mode(server_id) {
            Ok(true) => self.apply_live(&server, &action).await,
            Ok(false) => self.apply_files(&server, &action, actor).await,
            Err(e) => Err(e),
        };
        self.audit
            .record(
                actor,
                action.audit_name(),
                Some(server_id),
                action.audit_target(),
                if result.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
                serde_json::json!({ "via": result.as_ref().ok().map(|o| o.via) }),
            )
            .await;
        if result.is_ok() {
            self.events
                .publish(DomainEvent::PlayersChanged { server_id });
        }
        result
    }

    async fn apply_live(
        &self,
        server: &Server,
        action: &PlayerAction,
    ) -> CoreResult<ActionOutcome> {
        let console = self.servers.console(server.id);
        let mut sub = console.subscribe(None, 0);
        self.servers
            .send_command(server.id, &command_for(action))
            .await?;
        // Collect the server's reply: stop shortly after it goes quiet.
        let deadline = tokio::time::Instant::now() + REPLY_WINDOW;
        let mut messages = Vec::new();
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                break;
            }
            let wait = if messages.is_empty() {
                deadline - now
            } else {
                REPLY_QUIET.min(deadline - now)
            };
            match tokio::time::timeout(wait, sub.next_batch(50, Duration::from_millis(50))).await {
                Ok(Some(batch)) => messages.extend(
                    batch
                        .lines
                        .iter()
                        .filter(|l| {
                            matches!(l.stream, ConsoleStream::Stdout | ConsoleStream::Stderr)
                        })
                        .map(|l| reply_text(&l.text))
                        .filter(|t| !t.is_empty()),
                ),
                Ok(None) => break,
                Err(_) => {
                    if !messages.is_empty() {
                        break;
                    }
                }
            }
        }
        messages.truncate(10);
        Ok(ActionOutcome {
            via: AppliedVia::Console,
            messages,
        })
    }

    /// Name → UUID for editing files: the server's user cache, then (online mode) the
    /// Mojang profile service, else the offline UUID.
    async fn resolve(&self, name: &str, files: &FileSet) -> CoreResult<(Uuid, String)> {
        if let Some(u) = files.cache.cached_uuid(name) {
            let exact = files
                .cache
                .players()
                .into_iter()
                .find(|p| p.name.eq_ignore_ascii_case(name))
                .map_or(name.to_string(), |p| p.name);
            return Ok((u, exact));
        }
        if !bool_prop(&files.props, "online-mode", true) {
            return Ok((offline_uuid(name), name.to_string()));
        }
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.len() > 16 {
            return Err(CoreError::new(
                ErrorCode::Unsupported,
                "This is not a Java Edition name. Add Bedrock players while the server is running.",
            ));
        }
        self.profiles.uuid_for_name(name).await?.ok_or_else(|| {
            CoreError::new(
                ErrorCode::NotFound,
                format!("There is no Minecraft account named \"{name}\""),
            )
        })
    }

    async fn apply_files(
        &self,
        server: &Server,
        action: &PlayerAction,
        actor: &str,
    ) -> CoreResult<ActionOutcome> {
        if let PlayerAction::Kick { .. } = action {
            return Err(CoreError::new(
                ErrorCode::ServerNotRunning,
                "Players can only be kicked while the server is running",
            ));
        }
        if let PlayerAction::SetWhitelist { enabled } = action {
            self.servers
                .update_properties(
                    server.id,
                    vec![PropertyChange {
                        key: "white-list".into(),
                        value: Some(enabled.to_string()),
                    }],
                    actor,
                )
                .await?;
            return Ok(ActionOutcome {
                via: AppliedVia::Files,
                messages: vec![format!(
                    "Whitelist turned {}",
                    if *enabled { "on" } else { "off" }
                )],
            });
        }

        let rt = self.servers.runtime(server.id);
        let _guard = rt.begin(Operation::EditingConfig)?;
        let root = Self::root(server)?;
        let r2 = root.clone();
        let files = tokio::task::spawn_blocking(move || read_files(&r2))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        if files.legacy {
            return Err(CoreError::new(
                ErrorCode::Unsupported,
                "MCPanel does not edit pre-1.7.6 player lists",
            ));
        }
        let now = vanilla_timestamp(jiff::Zoned::now());
        let player_fields = |uuid: Uuid, name: &str| {
            let mut m = Map::new();
            m.insert("uuid".into(), Value::String(uuid.to_string()));
            m.insert("name".into(), Value::String(name.to_string()));
            m
        };
        let ban_fields = |m: &mut Map<String, Value>, reason: &Option<String>| {
            m.insert("created".into(), Value::String(now.clone()));
            m.insert("source".into(), Value::String("MCPanel".into()));
            m.insert("expires".into(), Value::String("forever".into()));
            m.insert(
                "reason".into(),
                Value::String(
                    reason
                        .clone()
                        .unwrap_or_else(|| "Banned by an operator.".into()),
                ),
            );
        };

        let (file, mut list, message) = match action {
            PlayerAction::Op { name } => {
                let (uuid, name) = self.resolve(name, &files).await?;
                let level: u64 = files
                    .props
                    .get("op-permission-level")
                    .and_then(|v| v.trim().parse().ok())
                    .filter(|l| (1..=4).contains(l))
                    .unwrap_or(4);
                let mut list = files.ops;
                let i = list.find_player(&name, Some(uuid));
                let mut f = player_fields(uuid, &name);
                f.insert("level".into(), level.into());
                if i.is_none() {
                    f.insert("bypassesPlayerLimit".into(), false.into());
                }
                let changed = list.upsert(i, f);
                (
                    OPS,
                    list,
                    if changed {
                        format!("Made {name} a server operator")
                    } else {
                        format!("{name} is already an operator")
                    },
                )
            }
            PlayerAction::Deop { name } => {
                let mut list = files.ops;
                let removed = list.remove(list.find_player(name, None));
                (
                    OPS,
                    list,
                    if removed {
                        format!("{name} is no longer an operator")
                    } else {
                        format!("{name} is not an operator")
                    },
                )
            }
            PlayerAction::WhitelistAdd { name } => {
                let (uuid, name) = self.resolve(name, &files).await?;
                let mut list = files.whitelist;
                let i = list.find_player(&name, Some(uuid));
                let changed = i.is_none() && list.upsert(None, player_fields(uuid, &name));
                (
                    WHITELIST,
                    list,
                    if changed {
                        format!("Added {name} to the whitelist")
                    } else {
                        format!("{name} is already whitelisted")
                    },
                )
            }
            PlayerAction::WhitelistRemove { name } => {
                let mut list = files.whitelist;
                let removed = list.remove(list.find_player(name, None));
                (
                    WHITELIST,
                    list,
                    if removed {
                        format!("Removed {name} from the whitelist")
                    } else {
                        format!("{name} is not whitelisted")
                    },
                )
            }
            PlayerAction::Ban { name, reason } => {
                let (uuid, name) = self.resolve(name, &files).await?;
                let mut list = files.bans;
                let i = list.find_player(&name, Some(uuid));
                let mut f = player_fields(uuid, &name);
                ban_fields(&mut f, reason);
                list.upsert(i, f);
                (BANNED_PLAYERS, list, format!("Banned {name}"))
            }
            PlayerAction::Pardon { name } => {
                let mut list = files.bans;
                let removed = list.remove(list.find_player(name, None));
                (
                    BANNED_PLAYERS,
                    list,
                    if removed {
                        format!("Unbanned {name}")
                    } else {
                        format!("{name} is not banned")
                    },
                )
            }
            PlayerAction::BanIp { ip, reason } => {
                let mut list = files.ip_bans;
                let i = list.find_ip(ip);
                let mut f = Map::new();
                f.insert("ip".into(), Value::String(ip.clone()));
                ban_fields(&mut f, reason);
                list.upsert(i, f);
                (BANNED_IPS, list, format!("Banned IP {ip}"))
            }
            PlayerAction::PardonIp { ip } => {
                let mut list = files.ip_bans;
                let removed = list.remove(list.find_ip(ip));
                (
                    BANNED_IPS,
                    list,
                    if removed {
                        format!("Unbanned IP {ip}")
                    } else {
                        format!("{ip} is not banned")
                    },
                )
            }
            PlayerAction::Kick { .. } | PlayerAction::SetWhitelist { .. } => {
                unreachable!("handled above")
            }
        };
        let text = list.to_text();
        list.entries.clear();
        tokio::task::spawn_blocking(move || -> CoreResult<()> {
            let p = root.resolve(file)?;
            p.ensure_no_reparse_points()?;
            fsx::atomic_write(&p.absolute(), text.as_bytes())
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))??;
        Ok(ActionOutcome {
            via: AppliedVia::Files,
            messages: vec![message],
        })
    }

    // ───────────────────────────── history ─────────────────────────────

    /// Record play sessions from join/leave events (and close them when a server stops).
    pub fn spawn_session_tracker(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let mut rx = self.events.subscribe();
        tokio::spawn(async move {
            match this.repo.interrupt_open_sessions().await {
                Ok(n) if n > 0 => {
                    tracing::info!(target: "mcpanel::players", n, "closed sessions left open by the previous run")
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(target: "mcpanel::players", "cannot close stale sessions: {}", e.message)
                }
            }
            loop {
                let env = match rx.recv().await {
                    Ok(env) => env,
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(target: "mcpanel::players", n, "missed events; player history may be incomplete");
                        continue;
                    }
                    Err(RecvError::Closed) => return,
                };
                let r = match &env.event {
                    DomainEvent::PlayerJoined {
                        server_id,
                        player_name,
                    } => {
                        this.repo
                            .session_started(*server_id, player_name, env.at)
                            .await
                    }
                    DomainEvent::PlayerLeft {
                        server_id,
                        player_name,
                    } => {
                        this.repo
                            .session_ended(*server_id, player_name, env.at)
                            .await
                    }
                    DomainEvent::ServerStateChanged {
                        server_id, state, ..
                    } if !state.has_process() => this
                        .repo
                        .end_open_sessions(*server_id, env.at)
                        .await
                        .map(|_| ()),
                    _ => Ok(()),
                };
                if let Err(e) = r {
                    tracing::warn!(target: "mcpanel::players", "cannot record player session: {}", e.message);
                }
            }
        });
    }

    pub async fn history(&self, server_id: ServerId) -> CoreResult<Vec<PlayerStats>> {
        self.servers.get(server_id).await?;
        self.repo.stats(server_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_single_safe_lines() {
        let a = PlayerService::validate(PlayerAction::Ban {
            name: " Steve ".into(),
            reason: Some(" Griefing  ".into()),
        })
        .unwrap();
        assert_eq!(command_for(&a), "ban Steve Griefing");
        assert!(PlayerService::validate(PlayerAction::Op { name: "a b".into() }).is_err());
        assert!(
            PlayerService::validate(PlayerAction::Op {
                name: "x;stop".into()
            })
            .is_err()
        );
        assert!(
            PlayerService::validate(PlayerAction::Kick {
                name: "A".into(),
                reason: Some("line\nstop".into())
            })
            .is_err()
        );
        assert!(
            PlayerService::validate(PlayerAction::BanIp {
                ip: "10.0.0.999".into(),
                reason: None
            })
            .is_err()
        );
        let ip = PlayerService::validate(PlayerAction::PardonIp { ip: " ::1 ".into() }).unwrap();
        assert_eq!(command_for(&ip), "pardon-ip ::1");
        assert_eq!(
            command_for(&PlayerAction::SetWhitelist { enabled: true }),
            "whitelist on"
        );
        assert!(validate_player_name(".BedrockGuy").is_ok());
    }

    #[test]
    fn replies_lose_log_and_system_chat_prefixes() {
        assert_eq!(
            reply_text(
                "[19:33:22] [Server thread/INFO]: System chat: Made McpTester a server operator"
            ),
            "Made McpTester a server operator"
        );
        assert_eq!(reply_text("[12:00:00 INFO]: Opped Steve"), "Opped Steve");
    }
}
