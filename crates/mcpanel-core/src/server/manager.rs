//! Server manager: registry of servers and their lifecycle.

use super::process::Supervised;
use super::runtime::{RuntimeSnapshot, ServerRuntime};
use crate::audit::AuditLog;
use crate::config::PropertiesDocument;
use crate::config::properties::decode_bytes;
use crate::console::{ConsoleStream, dialect};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::files::SafeRoot;
use crate::ids::ServerId;
use crate::java::{JavaCompatibility, JavaManager, check_compatibility};
use crate::jobs::JobManager;
use crate::lifecycle::LifecycleState;
use crate::model::{AuditResult, RuntimeStateRecord, Server};
use crate::paths::AppPaths;
use crate::ports::{Platform, PortStatus, ProcessSpec, ServerRepository};
use crate::software::executor::PlanExecutor;
use crate::software::{ProviderRegistry, VersionCatalog};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const DEFAULT_CONSOLE_CAPACITY: usize = 20_000;
const MIN_FREE_DISK_BYTES: u64 = 512 * 1024 * 1024;

pub struct ServerManagerDeps {
    pub repo: Arc<dyn ServerRepository>,
    pub platform: Arc<dyn Platform>,
    pub registry: ProviderRegistry,
    pub versions: Arc<VersionCatalog>,
    pub jobs: Arc<JobManager>,
    pub java: Arc<JavaManager>,
    pub events: EventBus,
    pub audit: Arc<AuditLog>,
    pub paths: AppPaths,
    pub executor: PlanExecutor,
    pub console_capacity: usize,
}

pub struct ServerManager {
    pub(crate) repo: Arc<dyn ServerRepository>,
    pub(crate) platform: Arc<dyn Platform>,
    pub(crate) registry: ProviderRegistry,
    pub(crate) versions: Arc<VersionCatalog>,
    pub(crate) jobs: Arc<JobManager>,
    pub(crate) java: Arc<JavaManager>,
    pub(crate) events: EventBus,
    pub(crate) audit: Arc<AuditLog>,
    pub(crate) paths: AppPaths,
    pub(crate) executor: PlanExecutor,
    runtimes: Mutex<HashMap<ServerId, Arc<ServerRuntime>>>,
    console_capacity: usize,
    launch_hook: std::sync::OnceLock<Arc<dyn LaunchHook>>,
}

/// Runs right before a server process is spawned (after preflight checks).
#[async_trait::async_trait]
pub trait LaunchHook: Send + Sync {
    async fn before_launch(&self, server: &Server, runtime: &ServerRuntime) -> CoreResult<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerView {
    pub server: Server,
    pub runtime: RuntimeSnapshot,
    pub port: Option<u16>,
    pub eula_accepted: bool,
    pub directory_exists: bool,
}

pub(crate) fn group_name(id: ServerId) -> String {
    format!("MCPanel.Server.{}", id.0.simple())
}

impl ServerManager {
    pub fn new(deps: ServerManagerDeps) -> Arc<Self> {
        Arc::new(Self {
            repo: deps.repo,
            platform: deps.platform,
            registry: deps.registry,
            versions: deps.versions,
            jobs: deps.jobs,
            java: deps.java,
            events: deps.events,
            audit: deps.audit,
            paths: deps.paths,
            executor: deps.executor,
            runtimes: Mutex::new(HashMap::new()),
            console_capacity: deps.console_capacity,
            launch_hook: std::sync::OnceLock::new(),
        })
    }

    /// Install the pre-launch hook (once, by the composition root).
    pub fn set_launch_hook(&self, hook: Arc<dyn LaunchHook>) {
        let _ = self.launch_hook.set(hook);
    }

    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    pub(crate) fn runtime(&self, id: ServerId) -> Arc<ServerRuntime> {
        let mut map = self.runtimes.lock().unwrap_or_else(|p| p.into_inner());
        Arc::clone(map.entry(id).or_insert_with(|| {
            ServerRuntime::new(id, LifecycleState::Stopped, self.console_capacity)
        }))
    }

    pub(crate) fn insert_runtime(&self, id: ServerId, state: LifecycleState) -> Arc<ServerRuntime> {
        let rt = ServerRuntime::new(id, state, self.console_capacity);
        self.runtimes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, Arc::clone(&rt));
        rt
    }

    pub(crate) fn remove_runtime(&self, id: ServerId) {
        self.runtimes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
    }

    fn all_runtimes(&self) -> Vec<Arc<ServerRuntime>> {
        self.runtimes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// Apply a lifecycle transition and publish it. Illegal transitions are refused.
    pub(crate) fn transition(&self, rt: &ServerRuntime, next: LifecycleState) -> bool {
        let prev = {
            let mut i = rt.lock();
            let prev = i.state;
            if !prev.can_transition_to(next) {
                if prev != next {
                    tracing::warn!(target: "mcpanel::server", server = %rt.server_id, "refused transition {prev:?} -> {next:?}");
                }
                return false;
            }
            i.state = next;
            prev
        };
        self.events.publish(DomainEvent::ServerStateChanged {
            server_id: rt.server_id,
            state: next,
            previous: prev,
        });
        true
    }

    pub(crate) async fn persist_runtime(&self, rt: &ServerRuntime) {
        let record = {
            let i = rt.lock();
            RuntimeStateRecord {
                server_id: Some(rt.server_id),
                last_state: Some(i.state),
                pid: i.pid,
                process_start_time: i.process_start_time,
                last_started_at: i.started_at,
                last_ready_at: i.ready_at,
                last_stopped_at: (!i.state.has_process()).then(Timestamp::now),
                last_exit_code: i.last_exit_code,
            }
        };
        if let Err(e) = self.repo.save_runtime_state(rt.server_id, &record).await {
            tracing::error!(target: "mcpanel::server", "failed to persist runtime state: {}", e.message);
        }
    }

    /// Load servers and detect processes orphaned by a previous MCPanel session.
    pub async fn initialize(self: &Arc<Self>) -> CoreResult<()> {
        let servers = self.repo.list().await?;
        let states: HashMap<ServerId, RuntimeStateRecord> = self
            .repo
            .runtime_states()
            .await?
            .into_iter()
            .filter_map(|r| r.server_id.map(|id| (id, r)))
            .collect();
        for server in servers {
            let record = states.get(&server.id).cloned().unwrap_or_default();
            let initial = match record.last_state {
                Some(LifecycleState::Created) => LifecycleState::Created,
                Some(LifecycleState::Crashed) => LifecycleState::Crashed,
                Some(LifecycleState::Error) => LifecycleState::Error,
                None => LifecycleState::Created,
                _ => LifecycleState::Stopped,
            };
            let rt = self.insert_runtime(server.id, initial);
            {
                let mut i = rt.lock();
                i.last_exit_code = record.last_exit_code;
                i.started_at = record.last_started_at;
                i.ready_at = record.last_ready_at;
            }
            if let (Some(pid), Some(start)) = (record.pid, record.process_start_time) {
                let alive = self.platform.process_start_time(pid) == Some(start);
                if alive {
                    self.adopt_orphan(&rt, pid, start).await;
                } else {
                    if record.last_state.is_some_and(|s| s.has_process()) {
                        rt.console.push(
                            ConsoleStream::System,
                            "MCPanel was closed while this server was running. The server process has since exited.",
                        );
                    }
                    self.persist_runtime(&rt).await;
                }
            }
        }
        Ok(())
    }

    async fn adopt_orphan(self: &Arc<Self>, rt: &Arc<ServerRuntime>, pid: u32, start: u64) {
        let controller = self.platform.attach_orphan(pid, &group_name(rt.server_id));
        {
            let mut i = rt.lock();
            i.pid = Some(pid);
            i.process_start_time = Some(start);
            i.controller = controller;
            i.generation += 1;
        }
        self.transition(rt, LifecycleState::Detached);
        rt.console.push(
            ConsoleStream::System,
            "This server kept running while MCPanel was closed. The console is not connected: you can wait for it to exit or force-stop it.",
        );
        let this = Arc::clone(self);
        let rt2 = Arc::clone(rt);
        let generation = rt.lock().generation;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if rt2.lock().generation != generation {
                    return;
                }
                if this.platform.process_start_time(pid) != Some(start) {
                    {
                        let mut i = rt2.lock();
                        i.pid = None;
                        i.process_start_time = None;
                        i.controller = None;
                        i.last_exit_code = None;
                    }
                    rt2.console.push(
                        ConsoleStream::System,
                        "The detached server process has exited.",
                    );
                    this.transition(&rt2, LifecycleState::Stopped);
                    this.persist_runtime(&rt2).await;
                    this.events.publish(DomainEvent::ServerStopped {
                        server_id: rt2.server_id,
                        exit_code: None,
                        forced: false,
                    });
                    rt2.exited.notify_waiters();
                    return;
                }
            }
        });
    }

    pub async fn get(&self, id: ServerId) -> CoreResult<Server> {
        self.repo
            .get(id)
            .await?
            .ok_or_else(|| CoreError::new(ErrorCode::ServerNotFound, "Server not found"))
    }

    pub async fn view(&self, id: ServerId) -> CoreResult<ServerView> {
        let server = self.get(id).await?;
        Ok(self.build_view(server).await)
    }

    pub async fn list(&self) -> CoreResult<Vec<ServerView>> {
        let mut out = Vec::new();
        for s in self.repo.list().await? {
            out.push(self.build_view(s).await);
        }
        out.sort_by(|a, b| {
            a.server
                .name
                .to_lowercase()
                .cmp(&b.server.name.to_lowercase())
        });
        Ok(out)
    }

    async fn build_view(&self, server: Server) -> ServerView {
        let rt = self.runtime(server.id);
        let dir = server.directory.clone();
        let (props, eula, exists) = tokio::task::spawn_blocking(move || {
            let exists = dir.is_dir();
            let props = read_root_file(&dir, "server.properties")
                .map(|b| PropertiesDocument::parse(&decode_bytes(&b)));
            let eula = read_root_file(&dir, "eula.txt")
                .map(|b| eula_accepted_text(&decode_bytes(&b)))
                .unwrap_or(false);
            (props, eula, exists)
        })
        .await
        .unwrap_or((None, false, false));
        let port = props
            .as_ref()
            .and_then(|p| p.get("server-port").and_then(|v| v.trim().parse().ok()))
            .or(Some(25565));
        ServerView {
            runtime: rt.snapshot(),
            server,
            port,
            eula_accepted: eula,
            directory_exists: exists,
        }
    }

    // ───────────────────────────── lifecycle ─────────────────────────────

    pub async fn start(self: &Arc<Self>, id: ServerId, actor: &str) -> CoreResult<()> {
        let result = self.start_inner(id, false).await;
        self.audit
            .record(
                actor,
                "server.start",
                Some(id),
                None,
                if result.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
                match &result {
                    Ok(()) => serde_json::json!({}),
                    Err(e) => serde_json::json!({ "error": e.code }),
                },
            )
            .await;
        result
    }

    pub(crate) async fn start_from_restart(self: &Arc<Self>, id: ServerId) -> CoreResult<()> {
        self.start_inner(id, true).await
    }

    async fn start_inner(self: &Arc<Self>, id: ServerId, restarting: bool) -> CoreResult<()> {
        let server = self.get(id).await?;
        let rt = self.runtime(id);
        {
            let mut i = rt.lock();
            let allowed =
                i.state.can_start() || (restarting && i.state == LifecycleState::Restarting);
            if !allowed || i.start_pending {
                return Err(CoreError::new(
                    ErrorCode::ServerAlreadyRunning,
                    format!("The server cannot be started while {}", i.state.as_str()),
                ));
            }
            if let Some(op) = i.operations.iter().find(|o| o.blocks_start()) {
                return Err(CoreError::new(
                    ErrorCode::ServerBusy,
                    format!("The server is busy ({op:?})"),
                ));
            }
            i.start_pending = true;
        }
        let result = self.launch(&server, &rt).await;
        rt.lock().start_pending = false;
        if let Err(e) = &result {
            rt.console.push(
                ConsoleStream::System,
                &format!("Cannot start: {}", e.message),
            );
            if restarting {
                self.transition(&rt, LifecycleState::Error);
                self.persist_runtime(&rt).await;
            }
        }
        result
    }

    async fn launch(self: &Arc<Self>, server: &Server, rt: &Arc<ServerRuntime>) -> CoreResult<()> {
        let provider = self.registry.get_software(&server.software.software_id)?;
        let root = SafeRoot::open(&server.directory).map_err(|e| {
            CoreError::new(
                ErrorCode::PathNotFound,
                format!("Server directory is not accessible: {}", e.message),
            )
        })?;

        // Jar
        let jar = root.resolve(&server.software.jar)?;
        jar.ensure_no_reparse_points()?;
        if !jar.absolute().is_file() {
            return Err(CoreError::new(
                ErrorCode::PathNotFound,
                format!("Server jar '{}' is missing", server.software.jar),
            ));
        }

        // EULA
        if provider.descriptor.caps.eula_required {
            let accepted = read_root_file(root.path(), "eula.txt")
                .map(|b| eula_accepted_text(&decode_bytes(&b)))
                .unwrap_or(false);
            if !accepted {
                return Err(CoreError::new(
                    ErrorCode::EulaNotAccepted,
                    "The Minecraft EULA has not been accepted for this server",
                ));
            }
        }

        // Java
        let java_id = server.launch.java_runtime_id.ok_or_else(|| {
            CoreError::new(
                ErrorCode::JavaNotFound,
                "No Java runtime selected for this server",
            )
        })?;
        let java = self.java.get(java_id).await?;
        if !java.valid {
            return Err(CoreError::new(
                ErrorCode::JavaInvalid,
                java.validation_error
                    .unwrap_or_else(|| "The selected Java runtime is not valid".into()),
            ));
        }
        if !java.path.is_file() {
            return Err(CoreError::new(
                ErrorCode::JavaNotFound,
                "The selected Java runtime no longer exists",
            ));
        }
        if let JavaCompatibility::TooOld { required } =
            check_compatibility(&java, &server.software.java_requirement())
        {
            return Err(CoreError::new(
                ErrorCode::JavaIncompatible,
                format!(
                    "This server needs Java {required} or newer; the selected runtime is Java {}",
                    java.major
                ),
            ));
        }

        // Memory
        let snapshot = {
            let platform = Arc::clone(&self.platform);
            tokio::task::spawn_blocking(move || platform.system_snapshot())
                .await
                .map_err(|e| CoreError::internal(e.to_string()))?
        };
        let max_bytes = server.launch.max_memory_mb as u64 * 1024 * 1024;
        if snapshot.memory_total_bytes > 0 && max_bytes > snapshot.memory_total_bytes {
            return Err(CoreError::invalid(format!(
                "Maximum memory ({} MB) exceeds this computer's total memory ({} MB)",
                server.launch.max_memory_mb,
                snapshot.memory_total_bytes / 1024 / 1024
            )));
        }

        // Port
        let props = read_root_file(root.path(), "server.properties")
            .map(|b| PropertiesDocument::parse(&decode_bytes(&b)));
        let port: u16 = props
            .as_ref()
            .and_then(|p| p.get("server-port").and_then(|v| v.trim().parse().ok()))
            .unwrap_or(25565);
        if let PortStatus::InUse { pid, process_name } = self.platform.tcp_port_status(port) {
            let who = match (process_name, pid) {
                (Some(n), Some(p)) => format!(" by {n} (PID {p})"),
                (None, Some(p)) => format!(" by PID {p}"),
                _ => String::new(),
            };
            return Err(CoreError::new(
                ErrorCode::PortInUse,
                format!("Port {port} is already in use{who}. Stop the other program or change the server port."),
            )
            .with_details(serde_json::json!({ "port": port, "pid": pid })));
        }

        // Disk
        if let Ok(space) = self.platform.disk_space(root.path())
            && space.available_bytes < MIN_FREE_DISK_BYTES
        {
            return Err(CoreError::new(
                ErrorCode::InsufficientDiskSpace,
                "Less than 512 MB of disk space is free on the server's drive",
            ));
        }

        // Launch spec (argv only).
        let launch_args =
            provider
                .launcher
                .launch_args(root.path(), &server.software, &server.launch)?;
        let mut args: Vec<String> = Vec::new();
        if server.launch.min_memory_mb > 0 {
            args.push(format!("-Xms{}M", server.launch.min_memory_mb));
        }
        args.push(format!("-Xmx{}M", server.launch.max_memory_mb));
        args.extend([
            "-Dfile.encoding=UTF-8".to_string(),
            "-Dstdout.encoding=UTF-8".to_string(),
            "-Dstderr.encoding=UTF-8".to_string(),
            // Log4Shell hardening; harmless where not applicable.
            "-Dlog4j2.formatMsgNoLookups=true".to_string(),
        ]);
        args.extend(server.launch.jvm_args.iter().cloned());
        args.extend(launch_args.jvm_args);
        if !launch_args.arg_files.is_empty() {
            for f in &launch_args.arg_files {
                let file = root.resolve(f)?;
                file.ensure_no_reparse_points()?;
                if !file.absolute().is_file() {
                    return Err(CoreError::new(
                        ErrorCode::PathNotFound,
                        format!("The launch arguments file '{f}' is missing"),
                    ));
                }
                args.push(format!("@{f}"));
            }
        } else {
            match launch_args.main_class {
                Some(main) => {
                    // Class-path launch: every entry must stay inside the server root.
                    for entry in &launch_args.class_path {
                        root.resolve(entry)?.ensure_no_reparse_points()?;
                    }
                    args.push("-cp".to_string());
                    args.push(launch_args.class_path.join(";"));
                    args.push(main);
                }
                None => {
                    args.push("-jar".to_string());
                    args.push(launch_args.jar);
                }
            }
        }
        args.extend(launch_args.server_args);
        args.extend(server.launch.server_args.iter().cloned());

        // Pending content changes (queued while the server ran) are applied now, while
        // no process holds the files.
        if let Some(hook) = self.launch_hook.get() {
            hook.before_launch(server, rt).await?;
        }

        let spec = ProcessSpec {
            program: java.path.clone(),
            args: args.clone(),
            cwd: root.path().to_path_buf(),
            env: Vec::new(),
            group_name: Some(group_name(server.id)),
        };

        rt.console
            .start_capture(self.paths.console_dir(&server.id), 20);
        rt.console.push(
            ConsoleStream::System,
            &format!("Starting {} with Java {}…", server.name, java.major),
        );
        rt.console.push(
            ConsoleStream::System,
            &format!("{} {}", java.path.display(), args.join(" ")),
        );

        let spawned = match self.platform.spawn(&spec) {
            Ok(s) => s,
            Err(e) => {
                rt.console.stop_capture();
                return Err(e);
            }
        };
        let generation = {
            let mut i = rt.lock();
            i.pid = Some(spawned.pid);
            i.process_start_time = spawned.start_time;
            i.controller = Some(Arc::clone(&spawned.controller));
            i.started_at = Some(Timestamp::now());
            i.ready_at = None;
            i.stop_requested = false;
            i.forced = false;
            i.seen_stopping_line = false;
            i.diagnosis = None;
            i.online_players.clear();
            i.generation += 1;
            i.generation
        };
        if !self.transition(rt, LifecycleState::Starting) {
            // Should not happen (checked above); make the state consistent anyway.
            rt.lock().state = LifecycleState::Starting;
        }
        self.persist_runtime(rt).await;
        let sup = Supervised {
            runtime: Arc::clone(rt),
            dialect: dialect::dialect(&provider.descriptor.caps.log_dialect),
            generation,
        };
        self.supervise(sup, spawned);
        Ok(())
    }

    fn stop_command(&self, server: &Server) -> String {
        self.registry
            .get_software(&server.software.software_id)
            .map(|p| p.descriptor.caps.stop_command.clone())
            .unwrap_or_else(|_| "stop".to_string())
    }

    pub async fn stop(self: &Arc<Self>, id: ServerId, force: bool, actor: &str) -> CoreResult<()> {
        let result = self.stop_inner(id, force, false).await;
        self.audit
            .record(
                actor,
                if force {
                    "server.force_stop"
                } else {
                    "server.stop"
                },
                Some(id),
                None,
                if result.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
                serde_json::json!({}),
            )
            .await;
        result
    }

    pub async fn restart(self: &Arc<Self>, id: ServerId, actor: &str) -> CoreResult<()> {
        let rt = self.runtime(id);
        let (state, blocking) = {
            let i = rt.lock();
            (
                i.state,
                i.operations.iter().find(|o| o.blocks_start()).copied(),
            )
        };
        let result = if let Some(op) = blocking {
            Err(CoreError::new(
                ErrorCode::ServerBusy,
                format!("The server cannot be restarted while busy ({op:?})"),
            ))
        } else if state.can_start() {
            self.start_inner(id, false).await
        } else {
            self.stop_inner(id, false, true).await
        };
        self.audit
            .record(
                actor,
                "server.restart",
                Some(id),
                None,
                if result.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
                serde_json::json!({}),
            )
            .await;
        result
    }

    async fn stop_inner(
        self: &Arc<Self>,
        id: ServerId,
        force: bool,
        restart: bool,
    ) -> CoreResult<()> {
        let server = self.get(id).await?;
        let rt = self.runtime(id);
        let state = rt.state();
        let terminate = |rt: &ServerRuntime| -> CoreResult<()> {
            let controller = {
                let mut i = rt.lock();
                i.forced = true;
                i.stop_requested = true;
                i.controller.clone()
            };
            match controller {
                Some(c) => c.terminate_tree(),
                None => Err(CoreError::internal(
                    "No process handle is available to terminate",
                )),
            }
        };
        match state {
            LifecycleState::Detached => {
                if !force {
                    return Err(CoreError::new(
                        ErrorCode::Unsupported,
                        "The console of a detached server is not connected. Wait for it to exit or force-stop it.",
                    ));
                }
                rt.console
                    .push(ConsoleStream::System, "Force-stopping detached server…");
                terminate(&rt)
            }
            LifecycleState::Starting
            | LifecycleState::Running
            | LifecycleState::Stopping
            | LifecycleState::Restarting => {
                if force {
                    rt.console.push(
                        ConsoleStream::System,
                        "Force-stopping server (unsaved world changes may be lost)…",
                    );
                    return terminate(&rt);
                }
                if state == LifecycleState::Stopping || state == LifecycleState::Restarting {
                    if restart {
                        rt.lock().restart_after_exit = true;
                    }
                    return Ok(());
                }
                let (stdin, generation) = {
                    let mut i = rt.lock();
                    i.stop_requested = true;
                    i.restart_after_exit = restart;
                    (i.stdin.clone(), i.generation)
                };
                self.transition(
                    &rt,
                    if restart {
                        LifecycleState::Restarting
                    } else {
                        LifecycleState::Stopping
                    },
                );
                let cmd = self.stop_command(&server);
                rt.console.push(ConsoleStream::Command, &cmd);
                let sent = match stdin {
                    Some(tx) => tx.send(cmd).await.is_ok(),
                    None => false,
                };
                if !sent {
                    rt.console.push(
                        ConsoleStream::System,
                        "The server is not accepting commands; terminating the process.",
                    );
                    return terminate(&rt);
                }
                // Grace period, then terminate.
                let timeout = Duration::from_secs(server.launch.stop_timeout_secs as u64);
                let rt2 = Arc::clone(&rt);
                tokio::spawn(async move {
                    tokio::time::sleep(timeout).await;
                    let alive = {
                        let i = rt2.lock();
                        i.generation == generation && i.state.has_process()
                    };
                    if alive {
                        rt2.console.push(
                            ConsoleStream::System,
                            &format!(
                                "The server did not stop within {}s; terminating the process.",
                                timeout.as_secs()
                            ),
                        );
                        let controller = {
                            let mut i = rt2.lock();
                            i.forced = true;
                            i.controller.clone()
                        };
                        if let Some(c) = controller
                            && let Err(e) = c.terminate_tree()
                        {
                            tracing::error!(target: "mcpanel::server", "terminate failed: {}", e.message);
                        }
                    }
                });
                Ok(())
            }
            _ => Err(CoreError::new(
                ErrorCode::ServerNotRunning,
                "The server is not running",
            )),
        }
    }

    pub async fn send_command(&self, id: ServerId, command: &str) -> CoreResult<()> {
        let command = command.trim();
        if command.is_empty() {
            return Err(CoreError::invalid("Command is empty"));
        }
        if command.len() > 4096 || command.contains(['\n', '\r', '\0']) {
            return Err(CoreError::invalid(
                "Commands must be a single line of at most 4096 characters",
            ));
        }
        let server = self.get(id).await?;
        let rt = self.runtime(id);
        let stdin = {
            let i = rt.lock();
            if !matches!(i.state, LifecycleState::Starting | LifecycleState::Running) {
                return Err(CoreError::new(
                    ErrorCode::ServerNotRunning,
                    "The server is not running",
                ));
            }
            i.stdin.clone()
        };
        let Some(tx) = stdin else {
            return Err(CoreError::new(
                ErrorCode::ServerNotRunning,
                "The console is not connected",
            ));
        };
        let stop_cmd = self.stop_command(&server);
        let bare = command.trim_start_matches('/');
        if bare.eq_ignore_ascii_case(&stop_cmd) {
            rt.lock().stop_requested = true;
            self.transition(&rt, LifecycleState::Stopping);
        }
        rt.console.push(ConsoleStream::Command, command);
        tx.send(command.to_string()).await.map_err(|_| {
            CoreError::new(ErrorCode::ServerNotRunning, "The console is not connected")
        })
    }

    pub fn console(&self, id: ServerId) -> Arc<crate::console::ConsoleHub> {
        Arc::clone(&self.runtime(id).console)
    }

    /// Wait until the server has no process (or the timeout elapses).
    pub async fn wait_for_exit(&self, id: ServerId, timeout: Duration) -> bool {
        let rt = self.runtime(id);
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = rt.exited.notified();
            if !rt.state().has_process() || rt.state() == LifecycleState::Detached {
                return !rt.state().has_process();
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return !rt.state().has_process();
            }
        }
    }

    /// Servers with an attached process (Detached ones are excluded).
    pub fn running_servers(&self) -> Vec<ServerId> {
        self.all_runtimes()
            .into_iter()
            .filter(|r| {
                let s = r.state();
                s.has_process() && s != LifecycleState::Detached
            })
            .map(|r| r.server_id)
            .collect()
    }

    /// Gracefully stop every attached server; force-stop those that exceed `timeout`.
    pub async fn stop_all(self: &Arc<Self>, timeout: Duration) {
        let ids = self.running_servers();
        for id in &ids {
            if let Err(e) = self.stop_inner(*id, false, false).await {
                tracing::warn!(target: "mcpanel::server", server = %id, "stop failed: {}", e.message);
            }
        }
        let waits = ids.iter().map(|id| async move {
            if !self.wait_for_exit(*id, timeout).await {
                let _ = self.stop_inner(*id, true, false).await;
                self.wait_for_exit(*id, Duration::from_secs(10)).await;
            }
        });
        futures::future::join_all(waits).await;
    }

    pub(crate) fn running_pids_internal(&self) -> Vec<(ServerId, u32)> {
        self.all_runtimes()
            .into_iter()
            .filter_map(|r| r.lock().pid.map(|p| (r.server_id, p)))
            .collect()
    }

    pub fn is_port_owned_by_server(&self, pid: u32) -> Option<ServerId> {
        self.all_runtimes()
            .into_iter()
            .find(|r| r.lock().pid == Some(pid))
            .map(|r| r.server_id)
    }
}

/// Read a small file directly in a server root (no links followed). `None` if absent.
pub(crate) fn read_root_file(root: &std::path::Path, name: &str) -> Option<Vec<u8>> {
    let path = root.join(name);
    let md = std::fs::symlink_metadata(&path).ok()?;
    if !md.is_file() || crate::files::fsx::is_reparse_point(&md) || md.len() > 1024 * 1024 {
        return None;
    }
    std::fs::read(path).ok()
}

pub(crate) fn eula_accepted_text(text: &str) -> bool {
    PropertiesDocument::parse(text)
        .get("eula")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}
