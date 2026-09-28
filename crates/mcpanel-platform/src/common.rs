//! Cross-platform pieces shared by OS implementations (sysinfo-based metrics, paths).

use mcpanel_core::error::{CoreError, CoreResult};
use mcpanel_core::paths::AppPaths;
use mcpanel_core::ports::{DiskInfo, ProcessUsage, SystemSnapshot};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

/// `%LOCALAPPDATA%\MCPanel` for data and `%USERPROFILE%\MCPanel\Servers` for servers
/// (deliberately not Documents/Desktop, which are often redirected into OneDrive).
///
/// `MCPANEL_DATA_DIR` / `MCPANEL_SERVERS_DIR` override both locations (development,
/// testing and portable setups).
pub fn default_paths() -> CoreResult<AppPaths> {
    let base = platform_default_paths()?;
    let data = std::env::var_os("MCPANEL_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(base.data_dir);
    let servers = std::env::var_os("MCPANEL_SERVERS_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(base.default_servers_dir);
    Ok(AppPaths::new(data, servers))
}

fn platform_default_paths() -> CoreResult<AppPaths> {
    #[cfg(windows)]
    {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| CoreError::internal("LOCALAPPDATA is not set"))?;
        let profile = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .ok_or_else(|| CoreError::internal("USERPROFILE is not set"))?;
        Ok(AppPaths::new(
            local.join("MCPanel"),
            profile.join("MCPanel").join("Servers"),
        ))
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| CoreError::internal("HOME is not set"))?;
        Ok(AppPaths::new(
            home.join(".local/share/mcpanel"),
            home.join("MCPanel/Servers"),
        ))
    }
}

pub struct Metrics {
    system: Mutex<System>,
    disks: Mutex<Disks>,
}

impl Metrics {
    pub fn new() -> Self {
        let system = System::new_with_specifics(RefreshKind::nothing());
        Self {
            system: Mutex::new(system),
            disks: Mutex::new(Disks::new_with_refreshed_list()),
        }
    }

    pub fn snapshot(&self) -> SystemSnapshot {
        let mut sys = self.system.lock().unwrap_or_else(|p| p.into_inner());
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let mut disks = self.disks.lock().unwrap_or_else(|p| p.into_inner());
        disks.refresh(true);
        SystemSnapshot {
            cpu_percent: sys.global_cpu_usage(),
            cpu_count: sys.cpus().len() as u32,
            memory_total_bytes: sys.total_memory(),
            memory_used_bytes: sys.used_memory(),
            disks: disks
                .list()
                .iter()
                .map(|d| DiskInfo {
                    mount_point: d.mount_point().to_string_lossy().to_string(),
                    name: d.name().to_string_lossy().to_string(),
                    total_bytes: d.total_space(),
                    available_bytes: d.available_space(),
                    removable: d.is_removable(),
                })
                .collect(),
            os_name: System::name().unwrap_or_else(|| "Unknown".into()),
            os_version: System::os_version().unwrap_or_default(),
            host_name: System::host_name(),
        }
    }

    fn refresh_processes(sys: &mut System) {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
    }

    /// Pid and all descendants (children first order not guaranteed).
    fn tree(sys: &System, root: u32) -> Vec<Pid> {
        let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for (pid, p) in sys.processes() {
            if let Some(parent) = p.parent() {
                children.entry(parent).or_default().push(*pid);
            }
        }
        let mut out = Vec::new();
        let mut stack = vec![Pid::from_u32(root)];
        while let Some(p) = stack.pop() {
            if out.contains(&p) {
                continue;
            }
            out.push(p);
            if let Some(c) = children.get(&p) {
                stack.extend(c.iter().copied());
            }
        }
        out
    }

    pub fn process_tree_usage(&self, pid: u32) -> Option<ProcessUsage> {
        let mut sys = self.system.lock().unwrap_or_else(|p| p.into_inner());
        if sys.cpus().is_empty() {
            sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
        }
        Self::refresh_processes(&mut sys);
        sys.process(Pid::from_u32(pid))?;
        let cpus = sys.cpus().len().max(1) as f32;
        let mut usage = ProcessUsage {
            cpu_percent: 0.0,
            memory_bytes: 0,
            process_count: 0,
        };
        for p in Self::tree(&sys, pid) {
            if let Some(proc_) = sys.process(p) {
                usage.cpu_percent += proc_.cpu_usage() / cpus;
                usage.memory_bytes += proc_.memory();
                usage.process_count += 1;
            }
        }
        usage.cpu_percent = usage.cpu_percent.clamp(0.0, 100.0);
        Some(usage)
    }

    pub fn process_start_time(&self, pid: u32) -> Option<u64> {
        let mut sys = self.system.lock().unwrap_or_else(|p| p.into_inner());
        let p = Pid::from_u32(pid);
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[p]),
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.process(p).map(|p| p.start_time())
    }

    pub fn process_name(&self, pid: u32) -> Option<String> {
        let mut sys = self.system.lock().unwrap_or_else(|p| p.into_inner());
        let p = Pid::from_u32(pid);
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[p]),
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.process(p)
            .map(|p| p.name().to_string_lossy().to_string())
    }

    /// Terminate a process tree (children first). Used for orphans without a job handle.
    pub fn kill_tree(&self, pid: u32) -> CoreResult<()> {
        let mut sys = self.system.lock().unwrap_or_else(|p| p.into_inner());
        Self::refresh_processes(&mut sys);
        if sys.process(Pid::from_u32(pid)).is_none() {
            return Ok(());
        }
        let tree = Self::tree(&sys, pid);
        let mut failed = false;
        for p in tree.iter().rev() {
            if let Some(proc_) = sys.process(*p)
                && !proc_.kill()
            {
                failed = true;
            }
        }
        if failed {
            return Err(CoreError::internal(
                "Some processes could not be terminated",
            ));
        }
        Ok(())
    }
}
