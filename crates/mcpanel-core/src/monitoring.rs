//! OS-level metrics sampling (labelled as such). JVM and Minecraft metrics (TPS/MSPT)
//! are separate sources added in later phases; nothing here estimates them.

use crate::ids::ServerId;
use crate::ports::{Platform, ProcessUsage, SystemSnapshot};
use crate::server::ServerManager;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// 30 minutes of history at the sample interval.
const HISTORY: usize = 900;

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
}

#[derive(Default)]
struct State {
    system: Option<SystemSnapshot>,
    system_history: VecDeque<SystemPoint>,
    process: HashMap<ServerId, (Option<ProcessUsage>, VecDeque<ProcessPoint>)>,
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
        ServerMetrics {
            current,
            history,
            uptime_ms,
        }
    }
}

impl ServerManager {
    /// PIDs of servers that currently have a process (including detached ones).
    pub fn list_pids(&self) -> Vec<(ServerId, u32)> {
        self.running_pids_internal()
    }
}
