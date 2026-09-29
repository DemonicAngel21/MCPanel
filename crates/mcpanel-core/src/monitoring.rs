//! OS-level metrics sampling (labelled as such) and Minecraft TPS/MSPT from the
//! server's own commands (see [`crate::perf`]); nothing here estimates them.

use crate::ids::ServerId;
use crate::perf::{self, TickSample};
use crate::ports::{Platform, ProcessUsage, SystemSnapshot};
use crate::server::ServerManager;
use crate::settings::SettingsService;
use crate::software::TpsSource;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// 30 minutes of history at the sample interval.
const HISTORY: usize = 900;
/// TPS/MSPT query interval and history (30 minutes).
pub const TICK_INTERVAL: Duration = Duration::from_secs(15);
const TICK_HISTORY: usize = 120;
/// How long to wait for a query's reply.
const TICK_REPLY: Duration = Duration::from_secs(3);
/// Unanswered queries in a row after which a session is no longer queried (a server
/// that does not understand the command would print an error every interval).
const TICK_MAX_MISSES: u32 = 3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SystemPoint {
    pub at: Timestamp,
    pub cpu_percent: f32,
    pub memory_used_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProcessPoint {
    pub at: Timestamp,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub current: Option<SystemSnapshot>,
    pub history: Vec<SystemPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerMetrics {
    pub current: Option<ProcessUsage>,
    pub history: Vec<ProcessPoint>,
    pub uptime_ms: Option<i64>,
    /// Latest TPS/MSPT sample of the current session.
    pub tick: Option<TickSample>,
    pub tick_history: Vec<TickSample>,
    /// Where TPS/MSPT come from for this server (`None` = not available).
    pub tick_source: Option<TpsSource>,
}

#[derive(Default)]
struct State {
    system: Option<SystemSnapshot>,
    system_history: VecDeque<SystemPoint>,
    process: HashMap<ServerId, (Option<ProcessUsage>, VecDeque<ProcessPoint>)>,
    ticks: HashMap<ServerId, (Option<TickSample>, VecDeque<TickSample>)>,
    tick_sources: HashMap<ServerId, TpsSource>,
    /// Consecutive unanswered queries per server session (keyed by its start time).
    tick_misses: HashMap<ServerId, (Option<Timestamp>, u32)>,
}

pub struct Monitor {
    platform: Arc<dyn Platform>,
    state: Mutex<State>,
}

impl Monitor {
    pub fn new(platform: Arc<dyn Platform>) -> Arc<Self> {
        Arc::new(Self {
            platform,
            state: Mutex::new(State::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Start the background sampler.
    pub fn spawn(self: &Arc<Self>, servers: Arc<ServerManager>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(SAMPLE_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let pids: Vec<(ServerId, u32)> = servers.list_pids().into_iter().collect();
                let platform = Arc::clone(&this.platform);
                let sample = tokio::task::spawn_blocking(move || {
                    let sys = platform.system_snapshot();
                    let procs: Vec<(ServerId, Option<ProcessUsage>)> = pids
                        .into_iter()
                        .map(|(id, pid)| (id, platform.process_tree_usage(pid)))
                        .collect();
                    (sys, procs)
                })
                .await;
                let Ok((sys, procs)) = sample else { continue };
                let now = Timestamp::now();
                let mut st = this.lock();
                st.system_history.push_back(SystemPoint {
                    at: now,
                    cpu_percent: sys.cpu_percent,
                    memory_used_bytes: sys.memory_used_bytes,
                });
                if st.system_history.len() > HISTORY {
                    st.system_history.pop_front();
                }
                st.system = Some(sys);
                let live: Vec<ServerId> = procs.iter().map(|(id, _)| *id).collect();
                for (id, usage) in procs {
                    let entry = st.process.entry(id).or_default();
                    entry.0 = usage;
                    if let Some(u) = usage {
                        entry.1.push_back(ProcessPoint {
                            at: now,
                            cpu_percent: u.cpu_percent,
                            memory_bytes: u.memory_bytes,
                        });
                        if entry.1.len() > HISTORY {
                            entry.1.pop_front();
                        }
                    }
                }
                // Servers without a process have no current sample (history is kept).
                for (id, entry) in st.process.iter_mut() {
                    if !live.contains(id) {
                        entry.0 = None;
                    }
                }
            }
        });
    }

    /// Start the TPS/MSPT sampler (running servers whose software has a source).
    pub fn spawn_tick_sampler(
        self: &Arc<Self>,
        servers: Arc<ServerManager>,
        settings: Arc<SettingsService>,
    ) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(TICK_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let enabled = settings
                    .get()
                    .await
                    .map(|s| s.tick_sampling)
                    .unwrap_or(true);
                let Ok(views) = servers.list().await else {
                    continue;
                };
                for v in views {
                    let id = v.server.id;
                    if v.runtime.state != crate::lifecycle::LifecycleState::Running || !enabled {
                        this.lock().ticks.entry(id).or_default().0 = None;
                        continue;
                    }
                    let Some(source) = tick_source(&servers, &v.server).await else {
                        this.lock().tick_sources.remove(&id);
                        continue;
                    };
                    this.lock().tick_sources.insert(id, source);
                    let session = v.runtime.started_at;
                    {
                        let mut st = this.lock();
                        let misses = st.tick_misses.entry(id).or_insert((session, 0));
                        if misses.0 != session {
                            *misses = (session, 0);
                        }
                        if misses.1 >= TICK_MAX_MISSES {
                            continue;
                        }
                    }
                    let this = Arc::clone(&this);
                    let servers = Arc::clone(&servers);
                    tokio::spawn(async move {
                        let sample = query(&servers, id, source).await;
                        let mut st = this.lock();
                        let misses = st.tick_misses.entry(id).or_insert((session, 0));
                        if sample.is_some() {
                            misses.1 = 0;
                        } else {
                            misses.1 += 1;
                            if misses.1 == TICK_MAX_MISSES {
                                tracing::info!(target: "mcpanel::server", %id, "server does not answer TPS/MSPT queries; not querying it again until it restarts");
                            }
                        }
                        let entry = st.ticks.entry(id).or_default();
                        entry.0 = sample.clone();
                        if let Some(s) = sample {
                            entry.1.push_back(s);
                            if entry.1.len() > TICK_HISTORY {
                                entry.1.pop_front();
                            }
                        }
                    });
                }
            }
        });
    }

    pub fn system(&self) -> SystemMetrics {
        let st = self.lock();
        SystemMetrics {
            current: st.system.clone(),
            history: st.system_history.iter().copied().collect(),
        }
    }

    pub fn server(&self, id: ServerId, uptime_ms: Option<i64>) -> ServerMetrics {
        let st = self.lock();
        let (current, history) = st
            .process
            .get(&id)
            .map(|(c, h)| (*c, h.iter().copied().collect()))
            .unwrap_or((None, Vec::new()));
        let (tick, tick_history) = st
            .ticks
            .get(&id)
            .map(|(c, h)| (c.clone(), h.iter().cloned().collect()))
            .unwrap_or((None, Vec::new()));
        ServerMetrics {
            current,
            history,
            uptime_ms,
            tick,
            tick_history,
            tick_source: st.tick_sources.get(&id).copied(),
        }
    }
}

/// The TPS/MSPT source for a server, if its software and version have one.
async fn tick_source(servers: &ServerManager, server: &crate::model::Server) -> Option<TpsSource> {
    let caps = servers
        .registry()
        .get_software(&server.software.software_id)
        .ok()?
        .descriptor
        .caps
        .clone();
    match caps.tps_source? {
        TpsSource::PaperCommands => Some(TpsSource::PaperCommands),
        TpsSource::VanillaTickQuery => {
            let v = &server.software.game_version;
            let known = match servers
                .versions
                .is_at_least(v, perf::TICK_QUERY_SINCE)
                .await
            {
                Some(b) => Some(b),
                None => perf::release_at_least(v, perf::TICK_QUERY_SINCE),
            };
            (known == Some(true)).then_some(TpsSource::VanillaTickQuery)
        }
    }
}

/// Send the query and collect its diverted reply.
async fn query(servers: &ServerManager, id: ServerId, source: TpsSource) -> Option<TickSample> {
    let console = servers.console(id);
    let (guard, mut rx) = console.tap(
        Arc::new(move |raw: &str| perf::is_reply(source, &perf::message(raw))),
        TICK_REPLY + Duration::from_secs(1),
    );
    for c in perf::commands(source) {
        servers.send_probe(id, c).await.ok()?;
    }
    let deadline = tokio::time::Instant::now() + TICK_REPLY;
    let mut msgs = Vec::new();
    while !perf::is_complete(source, &msgs) {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(line)) => msgs.push(perf::message(&line)),
            _ => break,
        }
    }
    drop(guard);
    perf::parse(source, &msgs, Timestamp::now())
}

impl ServerManager {
    /// PIDs of servers that currently have a process (including detached ones).
    pub fn list_pids(&self) -> Vec<(ServerId, u32)> {
        self.running_pids_internal()
    }
}
