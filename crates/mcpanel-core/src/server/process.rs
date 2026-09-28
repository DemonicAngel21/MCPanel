//! Process supervision: spawning, stdio pumping, output analysis and exit classification.

use super::manager::ServerManager;
use super::runtime::{Diagnosis, ServerRuntime};
use crate::console::ConsoleStream;
use crate::console::dialect::{LogDialect, parse_line};
use crate::events::DomainEvent;
use crate::lifecycle::LifecycleState;
use crate::model::AuditResult;
use crate::ports::{ExitInfo, SpawnedProcess};
use crate::time::Timestamp;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// Diagnoses that restarting cannot fix.
const TERMINAL_DIAGNOSES: &[&str] = &[
    "port_in_use",
    "eula",
    "java_too_old",
    "jar_missing",
    "memory",
    "jvm_option",
];

/// Read one line, bounded to `max` bytes (longer output is split). Returns Ok(false) at EOF.
async fn read_line_bounded<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    out: &mut Vec<u8>,
    max: usize,
) -> std::io::Result<bool> {
    out.clear();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(!out.is_empty());
        }
        let room = max.saturating_sub(out.len());
        let search = &available[..available.len().min(room.max(1))];
        if let Some(pos) = search.iter().position(|b| *b == b'\n') {
            out.extend_from_slice(&search[..pos]);
            reader.consume(pos + 1);
            return Ok(true);
        }
        let n = search.len();
        out.extend_from_slice(search);
        reader.consume(n);
        if out.len() >= max {
            return Ok(true);
        }
    }
}

pub(crate) struct Supervised {
    pub runtime: Arc<ServerRuntime>,
    pub dialect: &'static LogDialect,
    pub generation: u64,
}

impl ServerManager {
    /// Wire up a freshly spawned process. Called with the runtime already in `Starting`.
    pub(crate) fn supervise(self: &Arc<Self>, sup: Supervised, process: SpawnedProcess) {
        let SpawnedProcess {
            stdin,
            stdout,
            stderr,
            waiter,
            ..
        } = process;

        // stdin writer
        let (tx, mut rx) = mpsc::channel::<String>(256);
        sup.runtime.lock().stdin = Some(tx);
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(cmd) = rx.recv().await {
                if stdin.write_all(cmd.as_bytes()).await.is_err()
                    || stdin.write_all(b"\n").await.is_err()
                    || stdin.flush().await.is_err()
                {
                    break;
                }
            }
        });

        let readers = vec![
            self.spawn_reader(&sup, stdout, ConsoleStream::Stdout),
            self.spawn_reader(&sup, stderr, ConsoleStream::Stderr),
        ];

        // Slow-start warning (never kills).
        {
            let rt = Arc::clone(&sup.runtime);
            let generation = sup.generation;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(300)).await;
                let still = {
                    let i = rt.lock();
                    i.generation == generation && i.state == LifecycleState::Starting
                };
                if still {
                    rt.console.push(
                        ConsoleStream::System,
                        "The server is still starting after 5 minutes. Large modpacks can take a while; MCPanel will keep waiting.",
                    );
                }
            });
        }

        let this = Arc::clone(self);
        tokio::spawn(async move {
            let exit = match waiter.wait().await {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!(target: "mcpanel::server", "waiting for process failed: {}", e.message);
                    ExitInfo { code: None }
                }
            };
            // Let readers drain remaining output (bounded: grandchildren may hold pipes).
            let _ =
                tokio::time::timeout(Duration::from_secs(3), futures::future::join_all(readers))
                    .await;
            this.on_exit(sup, exit).await;
        });
    }

    fn spawn_reader(
        self: &Arc<Self>,
        sup: &Supervised,
        stream: Box<dyn AsyncRead + Send + Unpin>,
        kind: ConsoleStream,
    ) -> tokio::task::JoinHandle<()> {
        let this = Arc::clone(self);
        let rt = Arc::clone(&sup.runtime);
        let dialect = sup.dialect;
        tokio::spawn(async move {
            let mut reader = BufReader::with_capacity(64 * 1024, stream);
            let mut buf = Vec::with_capacity(1024);
            loop {
                match read_line_bounded(&mut reader, &mut buf, 32 * 1024).await {
                    Ok(true) => {
                        let text = String::from_utf8_lossy(&buf);
                        let text = text.trim_end_matches('\r');
                        rt.console.push(kind, text);
                        this.analyze_line(&rt, dialect, text);
                    }
                    Ok(false) => break,
                    Err(e) => {
                        tracing::debug!(target: "mcpanel::server", "console stream closed: {e}");
                        break;
                    }
                }
            }
        })
    }

    fn analyze_line(self: &Arc<Self>, rt: &Arc<ServerRuntime>, dialect: &LogDialect, raw: &str) {
        // Fatal patterns: cheap substring checks.
        for f in &dialect.fatal {
            if raw.contains(&f.needle) {
                let first = {
                    let mut i = rt.lock();
                    if i.diagnosis.is_none() {
                        i.diagnosis = Some(Diagnosis {
                            kind: f.kind.clone(),
                            message: f.message.clone(),
                        });
                        true
                    } else {
                        false
                    }
                };
                if first {
                    rt.console
                        .push(ConsoleStream::System, &format!("MCPanel: {}", f.message));
                }
                break;
            }
        }

        let parsed = parse_line(raw);
        let msg = parsed.message.as_str();
        let state = rt.state();

        if state == LifecycleState::Starting
            && msg.starts_with("Done")
            && dialect.ready.iter().any(|r| r.is_match(msg))
        {
            let startup_ms = {
                let mut i = rt.lock();
                i.ready_at = Some(Timestamp::now());
                i.started_at
                    .map(|s| Timestamp::now().millis() - s.millis())
                    .unwrap_or(0)
            };
            if self.transition(rt, LifecycleState::Running) {
                self.events.publish(DomainEvent::ServerReady {
                    server_id: rt.server_id,
                    startup_ms,
                });
                let this_rt = Arc::clone(rt);
                let this = Arc::clone(self);
                tokio::spawn(async move { this.persist_runtime(&this_rt).await });
            }
            return;
        }

        if msg.starts_with("Stopping") && dialect.stopping.iter().any(|r| r.is_match(msg)) {
            rt.lock().seen_stopping_line = true;
            return;
        }

        if msg.ends_with(" the game") {
            if let Some(c) = dialect.player_joined.iter().find_map(|r| r.captures(msg)) {
                let name = c["name"].to_string();
                rt.lock().online_players.insert(name.clone());
                self.events.publish(DomainEvent::PlayerJoined {
                    server_id: rt.server_id,
                    player_name: name,
                });
            } else if let Some(c) = dialect.player_left.iter().find_map(|r| r.captures(msg)) {
                let name = c["name"].to_string();
                rt.lock().online_players.remove(&name);
                self.events.publish(DomainEvent::PlayerLeft {
                    server_id: rt.server_id,
                    player_name: name,
                });
            }
        }
    }

    async fn on_exit(self: Arc<Self>, sup: Supervised, exit: ExitInfo) {
        let rt = sup.runtime;
        let (prev_state, intentional, forced, restart, diagnosis, stale) = {
            let mut i = rt.lock();
            let stale = i.generation != sup.generation;
            i.stdin = None;
            i.controller = None;
            i.pid = None;
            i.process_start_time = None;
            i.last_exit_code = exit.code;
            i.last_exit_at = Some(crate::time::Timestamp::now());
            i.online_players.clear();
            let restart = std::mem::take(&mut i.restart_after_exit);
            (
                i.state,
                i.stop_requested || i.seen_stopping_line,
                i.forced,
                restart,
                i.diagnosis.clone(),
                stale,
            )
        };
        rt.console.stop_capture();
        if stale {
            return;
        }
        let code_text = exit.code.map_or("unknown".to_string(), |c| c.to_string());
        let terminal = diagnosis
            .as_ref()
            .is_some_and(|d| TERMINAL_DIAGNOSES.contains(&d.kind.as_str()));

        let next = if terminal && !(intentional && prev_state == LifecycleState::Stopping) {
            LifecycleState::Error
        } else if restart {
            LifecycleState::Restarting
        } else if intentional {
            LifecycleState::Stopped
        } else {
            LifecycleState::Crashed
        };

        match next {
            LifecycleState::Stopped | LifecycleState::Restarting => {
                rt.console.push(
                    ConsoleStream::System,
                    &if forced {
                        format!("Server process was terminated (exit code {code_text}).")
                    } else {
                        format!("Server stopped (exit code {code_text}).")
                    },
                );
            }
            LifecycleState::Error => {
                rt.console.push(
                    ConsoleStream::System,
                    &format!(
                        "Server failed (exit code {code_text}): {}",
                        diagnosis
                            .as_ref()
                            .map_or("unknown error", |d| d.message.as_str())
                    ),
                );
            }
            _ => {
                rt.console.push(
                    ConsoleStream::System,
                    &format!("Server crashed (exit code {code_text})."),
                );
            }
        }

        if next == LifecycleState::Restarting {
            // Already in Restarting; nothing to transition.
            if prev_state != LifecycleState::Restarting {
                self.transition(&rt, LifecycleState::Restarting);
            }
        } else {
            self.transition(&rt, next);
        }
        {
            let mut i = rt.lock();
            i.stop_requested = false;
            i.forced = false;
            i.seen_stopping_line = false;
        }
        self.persist_runtime(&rt).await;

        match next {
            LifecycleState::Crashed | LifecycleState::Error => {
                self.events.publish(DomainEvent::ServerCrashed {
                    server_id: rt.server_id,
                    exit_code: exit.code,
                    diagnosis: diagnosis.as_ref().map(|d| d.message.clone()),
                });
                self.audit
                    .record(
                        "system",
                        if next == LifecycleState::Error {
                            "server.failed"
                        } else {
                            "server.crashed"
                        },
                        Some(rt.server_id),
                        None,
                        AuditResult::Failure,
                        serde_json::json!({
                            "exitCode": exit.code,
                            "diagnosis": diagnosis.as_ref().map(|d| d.kind.clone()),
                        }),
                    )
                    .await;
            }
            _ => {
                self.events.publish(DomainEvent::ServerStopped {
                    server_id: rt.server_id,
                    exit_code: exit.code,
                    forced,
                });
            }
        }
        rt.exited.notify_waiters();

        if next == LifecycleState::Restarting {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if let Err(e) = self.start_from_restart(rt.server_id).await {
                rt.console.push(
                    ConsoleStream::System,
                    &format!("Restart failed: {}", e.message),
                );
                self.transition(&rt, LifecycleState::Error);
                self.persist_runtime(&rt).await;
            }
        }
    }
}
