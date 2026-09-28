//! Windows implementation of the platform port.
#![allow(unsafe_code)]

use crate::common::Metrics;
use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult};
use mcpanel_core::ports::{
    CommandOutput, DiskSpace, ExitInfo, LocationWarning, Platform, PortStatus, ProcessController,
    ProcessSpec, ProcessUsage, ProcessWaiter, SpawnedProcess, SystemSnapshot,
};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
};
use windows::core::PCWSTR;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// Environment variables passed to child processes. Everything else (including
/// `JAVA_TOOL_OPTIONS` / `_JAVA_OPTIONS`, which inject JVM flags, and any tokens in the
/// user's environment) is dropped.
const ENV_ALLOWLIST: &[&str] = &[
    "SystemRoot",
    "SystemDrive",
    "windir",
    "TEMP",
    "TMP",
    "PATH",
    "PATHEXT",
    "ComSpec",
    "USERPROFILE",
    "USERNAME",
    "USERDOMAIN",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
    "PUBLIC",
    "ALLUSERSPROFILE",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "PROCESSOR_IDENTIFIER",
    "PROCESSOR_LEVEL",
    "PROCESSOR_REVISION",
    "OS",
    "COMPUTERNAME",
];

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn minimal_env(cmd: &mut tokio::process::Command) {
    cmd.env_clear();
    for key in ENV_ALLOWLIST {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
}

/// Owned Job Object handle.
struct JobHandle(HANDLE);
// SAFETY: a job object HANDLE is a kernel handle usable from any thread.
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

impl Drop for JobHandle {
    fn drop(&mut self) {
        // SAFETY: we own the handle and close it exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct JobController {
    job: Option<JobHandle>,
    pid: u32,
    metrics: Arc<Metrics>,
}

impl ProcessController for JobController {
    fn terminate_tree(&self) -> CoreResult<()> {
        if let Some(job) = &self.job {
            // SAFETY: valid job handle owned by self.
            let r = unsafe { TerminateJobObject(job.0, 1) };
            if r.is_ok() {
                return Ok(());
            }
            tracing::warn!(target: "mcpanel::platform", "TerminateJobObject failed; falling back to process tree kill");
        }
        self.metrics.kill_tree(self.pid)
    }
}

struct PidTreeController {
    pid: u32,
    metrics: Arc<Metrics>,
}

impl ProcessController for PidTreeController {
    fn terminate_tree(&self) -> CoreResult<()> {
        self.metrics.kill_tree(self.pid)
    }
}

struct ChildWaiter(tokio::process::Child);

#[async_trait]
impl ProcessWaiter for ChildWaiter {
    async fn wait(mut self: Box<Self>) -> CoreResult<ExitInfo> {
        let status = self
            .0
            .wait()
            .await
            .map_err(|e| CoreError::io("Failed to wait for process", &e))?;
        Ok(ExitInfo {
            code: status.code(),
        })
    }
}

pub struct WindowsPlatform {
    metrics: Arc<Metrics>,
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Metrics::new()),
        }
    }

    fn create_job() -> Option<JobHandle> {
        // Unnamed job without JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: servers survive an
        // MCPanel crash (decision #3) and orphans are handled on next launch.
        // SAFETY: plain FFI call; the returned handle is owned by JobHandle.
        match unsafe { CreateJobObjectW(None, PCWSTR::null()) } {
            Ok(h) => Some(JobHandle(h)),
            Err(e) => {
                tracing::warn!(target: "mcpanel::platform", "CreateJobObjectW failed: {e}");
                None
            }
        }
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn starts_with_ci(path: &Path, prefix: &Path) -> bool {
    mcpanel_core::files::path_starts_with_ci(path, prefix)
}

fn push_java(out: &mut Vec<PathBuf>, home: &Path) {
    let exe = home.join("bin").join("java.exe");
    if exe.is_file() {
        out.push(exe);
    }
}

fn scan_vendor_dir(out: &mut Vec<PathBuf>, dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            push_java(out, &p);
            // Some layouts nest one more level (e.g. "jdk-21\jre").
            push_java(out, &p.join("jre"));
        }
    }
}

fn registry_java_homes() -> Vec<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    let mut out = Vec::new();
    let roots = [
        RegKey::predef(HKEY_LOCAL_MACHINE),
        RegKey::predef(HKEY_CURRENT_USER),
    ];
    // (key path, value name, optional sub-path below each version key)
    let layouts: &[(&str, &str, &str)] = &[
        (r"SOFTWARE\JavaSoft\JDK", "JavaHome", ""),
        (r"SOFTWARE\JavaSoft\JRE", "JavaHome", ""),
        (r"SOFTWARE\JavaSoft\Java Development Kit", "JavaHome", ""),
        (
            r"SOFTWARE\JavaSoft\Java Runtime Environment",
            "JavaHome",
            "",
        ),
        (r"SOFTWARE\Eclipse Adoptium\JDK", "Path", r"hotspot\MSI"),
        (r"SOFTWARE\Eclipse Adoptium\JRE", "Path", r"hotspot\MSI"),
        (r"SOFTWARE\Eclipse Foundation\JDK", "Path", r"hotspot\MSI"),
        (r"SOFTWARE\Microsoft\JDK", "Path", r"hotspot\MSI"),
        (r"SOFTWARE\Azul Systems\Zulu", "InstallationPath", ""),
        (r"SOFTWARE\BellSoft\Liberica", "InstallationPath", ""),
        (r"SOFTWARE\Amazon Corretto", "InstallationPath", ""),
    ];
    for root in &roots {
        for (key, value, sub) in layouts {
            let Ok(k) = root.open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY) else {
                continue;
            };
            for name in k.enum_keys().flatten() {
                let path = if sub.is_empty() {
                    name.clone()
                } else {
                    format!(r"{name}\{sub}")
                };
                if let Ok(vk) = k.open_subkey_with_flags(&path, KEY_READ | KEY_WOW64_64KEY)
                    && let Ok(home) = vk.get_value::<String, _>(value)
                {
                    out.push(PathBuf::from(home));
                }
            }
        }
    }
    out
}

#[async_trait]
impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn spawn(&self, spec: &ProcessSpec) -> CoreResult<SpawnedProcess> {
        let build = |flags: u32| {
            let mut cmd = tokio::process::Command::new(&spec.program);
            cmd.args(&spec.args)
                .current_dir(&spec.cwd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(false)
                .creation_flags(flags);
            minimal_env(&mut cmd);
            for (k, v) in &spec.env {
                cmd.env(k, v);
            }
            cmd
        };
        // Break away from any job MCPanel itself runs in (launchers and dev tools often
        // use kill-on-close jobs), so servers survive MCPanel exiting (decision #3). If
        // the parent job forbids breakaway, fall back to a normal child.
        let base = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
        let mut child = match build(base | CREATE_BREAKAWAY_FROM_JOB).spawn() {
            Ok(c) => c,
            Err(e) if e.raw_os_error() == Some(5) => {
                tracing::info!(target: "mcpanel::platform", "job breakaway not permitted; server will share MCPanel's job");
                build(base).spawn().map_err(|e| {
                    CoreError::io(format!("Failed to start {}", spec.program.display()), &e)
                })?
            }
            Err(e) => {
                return Err(CoreError::io(
                    format!("Failed to start {}", spec.program.display()),
                    &e,
                ));
            }
        };
        let pid = child
            .id()
            .ok_or_else(|| CoreError::internal("Process exited immediately"))?;

        let job = Self::create_job().and_then(|job| {
            let handle = child.raw_handle()?;
            // SAFETY: both handles are valid for the duration of the call.
            match unsafe { AssignProcessToJobObject(job.0, HANDLE(handle)) } {
                Ok(()) => Some(job),
                Err(e) => {
                    tracing::warn!(target: "mcpanel::platform", "AssignProcessToJobObject failed: {e}");
                    None
                }
            }
        });

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CoreError::internal("stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CoreError::internal("stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CoreError::internal("stderr unavailable"))?;
        let start_time = self.metrics.process_start_time(pid);
        Ok(SpawnedProcess {
            pid,
            start_time,
            stdin: Box::new(stdin),
            stdout: Box::new(stdout),
            stderr: Box::new(stderr),
            waiter: Box::new(ChildWaiter(child)),
            controller: Arc::new(JobController {
                job,
                pid,
                metrics: Arc::clone(&self.metrics),
            }),
        })
    }

    async fn run_capture(
        &self,
        program: &Path,
        args: &[String],
        timeout: Duration,
    ) -> CoreResult<CommandOutput> {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .creation_flags(CREATE_NO_WINDOW);
        minimal_env(&mut cmd);
        let child = cmd
            .spawn()
            .map_err(|e| CoreError::io(format!("Failed to run {}", program.display()), &e))?;
        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(out)) => Ok(CommandOutput {
                code: out.status.code(),
                stdout: out.stdout,
                stderr: out.stderr,
                timed_out: false,
            }),
            Ok(Err(e)) => Err(CoreError::io("Process failed", &e)),
            Err(_) => Ok(CommandOutput {
                code: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
                timed_out: true,
            }),
        }
    }

    fn process_start_time(&self, pid: u32) -> Option<u64> {
        self.metrics.process_start_time(pid)
    }

    fn attach_orphan(&self, pid: u32, _group_name: &str) -> Option<Arc<dyn ProcessController>> {
        // The original (unnamed) job handle died with the previous MCPanel process, so
        // the tree is terminated by walking parent/child relationships.
        self.metrics.process_start_time(pid)?;
        Some(Arc::new(PidTreeController {
            pid,
            metrics: Arc::clone(&self.metrics),
        }))
    }

    fn java_candidates(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(home) = env_path("JAVA_HOME") {
            push_java(&mut out, &home);
        }
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                let exe = dir.join("java.exe");
                if exe.is_file() {
                    // Resolve Oracle's javapath shim and similar links to the real runtime.
                    out.push(
                        std::fs::canonicalize(&exe)
                            .map(|p| mcpanel_core::files::fsx::simplify(&p))
                            .unwrap_or(exe),
                    );
                }
            }
        }
        for home in registry_java_homes() {
            push_java(&mut out, &home);
        }
        for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(pf) = env_path(var) {
                for vendor in [
                    "Java",
                    "Eclipse Adoptium",
                    "Eclipse Foundation",
                    "Microsoft",
                    "Zulu",
                    "Amazon Corretto",
                    "BellSoft",
                    "Semeru",
                    "OpenJDK",
                    "Oracle",
                    "GraalVM",
                ] {
                    scan_vendor_dir(&mut out, &pf.join(vendor));
                }
            }
        }
        if let Some(profile) = env_path("USERPROFILE") {
            scan_vendor_dir(&mut out, &profile.join(".jdks"));
            let scoop = profile.join("scoop").join("apps");
            if let Ok(rd) = std::fs::read_dir(&scoop) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    if n.contains("jdk") || n.contains("jre") || n.contains("java") {
                        push_java(&mut out, &e.path().join("current"));
                    }
                }
            }
        }
        // Minecraft Launcher bundled runtimes.
        let mut launcher_roots = Vec::new();
        if let Some(appdata) = env_path("APPDATA") {
            launcher_roots.push(appdata.join(".minecraft").join("runtime"));
        }
        if let Some(local) = env_path("LOCALAPPDATA") {
            launcher_roots.push(
                local
                    .join("Packages")
                    .join("Microsoft.4297127D64EC6_8wekyb3d8bbwe")
                    .join("LocalCache")
                    .join("Local")
                    .join("runtime"),
            );
        }
        for root in launcher_roots {
            let Ok(rd) = std::fs::read_dir(&root) else {
                continue;
            };
            for component in rd.flatten() {
                let name = component.file_name();
                let home = component.path().join("windows-x64").join(&name);
                push_java(&mut out, &home);
            }
        }

        // De-duplicate case-insensitively, preserving order.
        let mut seen = std::collections::HashSet::new();
        out.retain(|p| seen.insert(p.to_string_lossy().to_lowercase()));
        out
    }

    fn disk_space(&self, path: &Path) -> CoreResult<DiskSpace> {
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let mut probe = path.to_path_buf();
        while !probe.exists() {
            if !probe.pop() {
                break;
            }
        }
        let w = wide(probe.as_os_str());
        let mut available = 0u64;
        let mut total = 0u64;
        // SAFETY: `w` is NUL-terminated and outlives the call; out-params are valid.
        unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(w.as_ptr()),
                Some(&mut available),
                Some(&mut total),
                None,
            )
        }
        .map_err(|e| CoreError::internal(format!("Cannot query disk space: {e}")))?;
        Ok(DiskSpace {
            total_bytes: total,
            available_bytes: available,
        })
    }

    fn location_warnings(&self, path: &Path) -> Vec<LocationWarning> {
        use windows::Win32::Storage::FileSystem::GetDriveTypeW;
        const DRIVE_REMOTE: u32 = 4;
        let mut w = Vec::new();
        let simplified = mcpanel_core::files::fsx::simplify(path);
        let s = simplified.to_string_lossy().to_string();
        for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
            if let Some(od) = env_path(var)
                && starts_with_ci(&simplified, &od)
            {
                w.push(LocationWarning::OneDriveSynced);
                break;
            }
        }
        if s.starts_with(r"\\") {
            w.push(LocationWarning::NetworkDrive);
        } else if s.len() >= 2 && s.as_bytes()[1] == b':' {
            let root = wide(OsStr::new(&format!("{}\\", &s[..2])));
            // SAFETY: NUL-terminated wide string.
            if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } == DRIVE_REMOTE {
                w.push(LocationWarning::NetworkDrive);
            }
            let rest = s[2..].trim_matches(['\\', '/']);
            if rest.is_empty() {
                w.push(LocationWarning::DriveRoot);
            }
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(pf) = env_path(var)
                && starts_with_ci(&simplified, &pf)
            {
                w.push(LocationWarning::ProgramFiles);
                break;
            }
        }
        if let Some(sys) = env_path("SystemRoot")
            && starts_with_ci(&simplified, &sys)
        {
            w.push(LocationWarning::SystemDirectory);
        }
        w.dedup();
        w
    }

    fn system_snapshot(&self) -> SystemSnapshot {
        self.metrics.snapshot()
    }

    fn process_tree_usage(&self, pid: u32) -> Option<ProcessUsage> {
        self.metrics.process_tree_usage(pid)
    }

    fn tcp_port_status(&self, port: u16) -> PortStatus {
        match listening_pid(port) {
            Ok(None) => PortStatus::Free,
            Ok(Some(pid)) => PortStatus::InUse {
                pid: Some(pid),
                process_name: self.metrics.process_name(pid),
            },
            Err(e) => {
                tracing::warn!(target: "mcpanel::platform", "cannot query TCP table: {e}");
                PortStatus::Unknown
            }
        }
    }
}

/// Owner PID of a TCP listener on `port` (IPv4 or IPv6), via the IP Helper API.
fn listening_pid(port: u16) -> Result<Option<u32>, String> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    for family in [AF_INET.0 as u32, AF_INET6.0 as u32] {
        let mut size = 0u32;
        // SAFETY: size query with a null buffer.
        let rc = unsafe {
            GetExtendedTcpTable(
                None,
                &mut size,
                false,
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc != ERROR_INSUFFICIENT_BUFFER && rc != 0 {
            return Err(format!("GetExtendedTcpTable size query failed ({rc})"));
        }
        let mut buf = vec![0u8; size as usize + 1024];
        let mut size = buf.len() as u32;
        // SAFETY: buffer is large enough per the size query; the API fills it.
        let rc = unsafe {
            GetExtendedTcpTable(
                Some(buf.as_mut_ptr().cast()),
                &mut size,
                false,
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc != 0 {
            return Err(format!("GetExtendedTcpTable failed ({rc})"));
        }
        if buf.len() < 4 {
            continue;
        }
        let count = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        // Rows start after dwNumEntries, aligned to the row's alignment (4 bytes).
        let rows_offset = 4usize;
        if family == AF_INET.0 as u32 {
            let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
            for i in 0..count {
                let off = rows_offset + i * row_size;
                if off + row_size > buf.len() {
                    break;
                }
                // SAFETY: bounds checked above; read_unaligned tolerates alignment.
                let row: MIB_TCPROW_OWNER_PID =
                    unsafe { std::ptr::read_unaligned(buf[off..].as_ptr().cast()) };
                if u16::from_be((row.dwLocalPort & 0xFFFF) as u16) == port {
                    return Ok(Some(row.dwOwningPid));
                }
            }
        } else {
            let row_size = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
            for i in 0..count {
                let off = rows_offset + i * row_size;
                if off + row_size > buf.len() {
                    break;
                }
                // SAFETY: bounds checked above.
                let row: MIB_TCP6ROW_OWNER_PID =
                    unsafe { std::ptr::read_unaligned(buf[off..].as_ptr().cast()) };
                if u16::from_be((row.dwLocalPort & 0xFFFF) as u16) == port {
                    return Ok(Some(row.dwOwningPid));
                }
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_listening_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let p = WindowsPlatform::new();
        match p.tcp_port_status(port) {
            PortStatus::InUse { pid, .. } => assert_eq!(pid, Some(std::process::id())),
            other => panic!("expected in use, got {other:?}"),
        }
        drop(listener);
        assert_eq!(p.tcp_port_status(port), PortStatus::Free);
    }

    #[test]
    fn location_warnings_for_system_paths() {
        let p = WindowsPlatform::new();
        assert!(
            p.location_warnings(Path::new(r"C:\"))
                .contains(&LocationWarning::DriveRoot)
        );
        if let Some(pf) = env_path("ProgramFiles") {
            assert!(
                p.location_warnings(&pf.join("X"))
                    .contains(&LocationWarning::ProgramFiles)
            );
        }
        let home = env_path("USERPROFILE")
            .unwrap()
            .join("MCPanel")
            .join("Servers");
        let w = p.location_warnings(&home);
        assert!(!w.contains(&LocationWarning::ProgramFiles));
        assert!(!w.contains(&LocationWarning::DriveRoot));
    }

    #[test]
    fn disk_space_of_temp() {
        let p = WindowsPlatform::new();
        let d = p.disk_space(&std::env::temp_dir()).unwrap();
        assert!(d.total_bytes > 0);
    }

    #[tokio::test]
    async fn spawn_captures_output_and_terminates_tree() {
        use tokio::io::AsyncReadExt;
        let p = WindowsPlatform::new();
        let dir = tempfile::tempdir().unwrap();
        let spec = ProcessSpec {
            program: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            args: vec!["/C".into(), "echo hello & ping -n 30 127.0.0.1 >NUL".into()],
            cwd: dir.path().to_path_buf(),
            env: vec![],
            group_name: None,
        };
        let mut sp = p.spawn(&spec).unwrap();
        let mut buf = [0u8; 5];
        sp.stdout.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
        assert!(sp.start_time.is_some());
        sp.controller.terminate_tree().unwrap();
        let exit = tokio::time::timeout(Duration::from_secs(10), sp.waiter.wait())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(exit.code, Some(1));
    }

    #[tokio::test]
    async fn child_env_is_minimal() {
        let p = WindowsPlatform::new();
        // SAFETY: test-only env mutation before spawning.
        unsafe { std::env::set_var("MCPANEL_TEST_SECRET", "s3cret") };
        let out = p
            .run_capture(
                Path::new(r"C:\Windows\System32\cmd.exe"),
                &["/C".into(), "set".into()],
                Duration::from_secs(10),
            )
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("MCPANEL_TEST_SECRET"));
        assert!(text.to_uppercase().contains("SYSTEMROOT"));
    }
}
