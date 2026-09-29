//! Persisted domain records owned by MCPanel (not Minecraft configuration — see ADR-0005).

use crate::ids::{JavaRuntimeId, ServerId};
use crate::lifecycle::LifecycleState;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A Minecraft server registered in MCPanel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub id: ServerId,
    pub name: String,
    /// Absolute server root directory.
    pub directory: PathBuf,
    pub software: InstalledSoftware,
    pub launch: LaunchConfig,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Which server software is installed and how it is launched.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledSoftware {
    /// Provider id, e.g. `vanilla`, `paper`, `purpur`.
    pub software_id: String,
    pub game_version: String,
    /// Provider-specific build identifier, if the software has builds.
    pub build: Option<String>,
    /// Server jar path relative to the server root.
    pub jar: String,
    /// Java requirement recorded at install time (from provider metadata).
    pub java_min_major: Option<u32>,
    pub java_recommended_major: Option<u32>,
}

impl InstalledSoftware {
    pub fn java_requirement(&self) -> crate::software::JavaRequirement {
        crate::software::JavaRequirement {
            min_major: self.java_min_major.unwrap_or(8),
            recommended_major: self.java_recommended_major,
            recommended_flags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchConfig {
    pub java_runtime_id: Option<JavaRuntimeId>,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    /// Additional JVM arguments (argv entries, never a shell string).
    pub jvm_args: Vec<String>,
    /// Additional server arguments after the jar.
    pub server_args: Vec<String>,
    /// Grace period for a graceful stop before the process tree is terminated.
    pub stop_timeout_secs: u32,
}

impl LaunchConfig {
    pub const MIN_MEMORY_MB: u32 = 256;
    pub const DEFAULT_STOP_TIMEOUT_SECS: u32 = 60;

    pub fn validate(&self) -> crate::error::CoreResult<()> {
        use crate::error::CoreError;
        if self.max_memory_mb < Self::MIN_MEMORY_MB {
            return Err(CoreError::invalid(format!(
                "Maximum memory must be at least {} MB",
                Self::MIN_MEMORY_MB
            )));
        }
        if self.min_memory_mb > self.max_memory_mb {
            return Err(CoreError::invalid(
                "Minimum memory cannot exceed maximum memory",
            ));
        }
        if !(5..=3600).contains(&self.stop_timeout_secs) {
            return Err(CoreError::invalid(
                "Stop timeout must be between 5 and 3600 seconds",
            ));
        }
        for arg in self.jvm_args.iter().chain(self.server_args.iter()) {
            if arg.contains('\0') || arg.contains('\n') || arg.contains('\r') {
                return Err(CoreError::invalid(
                    "Arguments must not contain NUL or newline characters",
                ));
            }
        }
        for arg in &self.jvm_args {
            let lower = arg.to_ascii_lowercase();
            if lower.starts_with("-xmx")
                || lower.starts_with("-xms")
                || lower.starts_with("-xx:maxheapsize")
                || lower.starts_with("-xx:initialheapsize")
                || lower.starts_with("-xx:minheapsize")
            {
                return Err(CoreError::invalid(
                    "Set memory with the memory fields, not -Xmx/-Xms JVM arguments",
                ));
            }
            check_jvm_arg(arg)?;
        }
        for arg in &self.server_args {
            check_server_arg(arg)?;
        }
        Ok(())
    }
}

/// `-XX` options that run programs, write or read files elsewhere, or load code.
const BLOCKED_XX: &[&str] = &[
    "onoutofmemoryerror",
    "onerror",
    "errorfile",
    "logfile",
    "heapdumppath",
    "vmoptionsfile",
    "flags",
    "compilecommandfile",
    "replaydatafile",
    "inlinedatafile",
    "startflightrecording",
    "flightrecorderoptions",
    "jvmcilibpath",
    "jvmcilibdumpjnconfig",
    "enablejvmci",
    "usejvmcicompiler",
    "sharedarchivefile",
    "archiveclassesatexit",
    "sharedclasslistfile",
];

/// System properties that make the JVM or the server load code or configuration from
/// another place.
const BLOCKED_PROPERTIES: &[&str] = &[
    "java.system.class.loader",
    "java.library.path",
    "java.class.path",
    "java.ext.dirs",
    "java.security.manager",
    "java.security.policy",
    "jdk.module.path",
    "log4j.configurationfile",
    "log4j2.configurationfile",
    "log4j.configuration",
    "fabric.addmods",
    "loader.addmods",
    "fabric.gamejarpath",
    "loader.gamejarpath",
    "fabric.classpathgroups",
    "java.home",
];

/// JVM options are allowlisted: `-D` properties, `-XX` flags and a few `-X`/module options.
/// Anything that could start other programs or load code from elsewhere is rejected.
fn check_jvm_arg(arg: &str) -> crate::error::CoreResult<()> {
    use crate::error::CoreError;
    let reject = |why: &str| {
        Err(CoreError::invalid(format!(
            "The JVM argument \"{arg}\" is not allowed: {why}"
        )))
    };
    let lower = arg.to_ascii_lowercase();
    if let Some(prop) = arg.strip_prefix("-D") {
        let key = prop.split('=').next().unwrap_or_default();
        let ok_key = !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !ok_key {
            return reject("invalid property name");
        }
        if BLOCKED_PROPERTIES.contains(&key.to_ascii_lowercase().as_str()) {
            return reject("this property loads code or files from elsewhere");
        }
        return Ok(());
    }
    if let Some(flag) = lower.strip_prefix("-xx:") {
        let name = flag
            .trim_start_matches(['+', '-'])
            .split('=')
            .next()
            .unwrap_or_default();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return reject("invalid -XX option");
        }
        if BLOCKED_XX.contains(&name) || name.starts_with("on") {
            return reject("this option can run programs or write files");
        }
        return Ok(());
    }
    let x_ok = ["-xss", "-xmn", "-xshare:", "-xnoclassgc", "-xincgc"]
        .iter()
        .any(|p| lower.starts_with(p));
    let module_ok = [
        "--add-opens=",
        "--add-exports=",
        "--add-modules=",
        "--enable-native-access=",
        "--sun-misc-unsafe-memory-access=",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
        || matches!(
            lower.as_str(),
            "--enable-preview" | "-server" | "-ea" | "-da" | "-esa" | "-dsa"
        );
    if x_ok || module_ok {
        return Ok(());
    }
    reject("only -D properties, -XX options, -Xss/-Xmn and module options are accepted")
}

/// Server arguments must not point at files or folders (they could make the server load
/// plugins or worlds from outside its folder).
fn check_server_arg(arg: &str) -> crate::error::CoreResult<()> {
    let bad = arg.starts_with('@')
        || arg.contains(['/', '\\'])
        || arg.contains("..")
        || arg.as_bytes().get(1) == Some(&b':');
    if bad {
        return Err(crate::error::CoreError::invalid(format!(
            "The server argument \"{arg}\" is not allowed: arguments cannot contain paths"
        )));
    }
    Ok(())
}

/// Persisted process bookkeeping, used for orphan detection after an MCPanel crash.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeStateRecord {
    pub server_id: Option<ServerId>,
    pub last_state: Option<LifecycleState>,
    pub pid: Option<u32>,
    /// Process start time (seconds since epoch, as reported by the OS).
    pub process_start_time: Option<u64>,
    pub last_started_at: Option<Timestamp>,
    pub last_ready_at: Option<Timestamp>,
    pub last_stopped_at: Option<Timestamp>,
    pub last_exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JavaSource {
    Detected,
    Manual,
    Managed,
}

impl JavaSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Manual => "manual",
            Self::Managed => "managed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "manual" => Self::Manual,
            "managed" => Self::Managed,
            _ => Self::Detected,
        }
    }
}

/// A Java runtime known to MCPanel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaRuntime {
    pub id: JavaRuntimeId,
    /// Absolute path to the `java` executable.
    pub path: PathBuf,
    pub major: u32,
    pub version: String,
    pub vendor: Option<String>,
    pub arch: Option<String>,
    pub is_64bit: bool,
    pub source: JavaSource,
    pub valid: bool,
    pub validation_error: Option<String>,
    pub validated_at: Timestamp,
}

/// One audit log entry. Metadata must never contain secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: crate::ids::AuditId,
    pub occurred_at: Timestamp,
    pub actor: String,
    pub action: String,
    pub server_id: Option<ServerId>,
    pub target: Option<String>,
    pub result: AuditResult,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Success,
    Failure,
}

impl AuditResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    pub server_id: Option<ServerId>,
    pub before: Option<Timestamp>,
    pub limit: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(jvm: &[&str], server: &[&str]) -> LaunchConfig {
        LaunchConfig {
            java_runtime_id: None,
            min_memory_mb: 512,
            max_memory_mb: 1024,
            jvm_args: jvm.iter().map(|s| s.to_string()).collect(),
            server_args: server.iter().map(|s| s.to_string()).collect(),
            stop_timeout_secs: 60,
        }
    }

    #[test]
    fn aikars_flags_are_accepted() {
        let flags = [
            "-XX:+UseG1GC",
            "-XX:+ParallelRefProcEnabled",
            "-XX:MaxGCPauseMillis=200",
            "-XX:+UnlockExperimentalVMOptions",
            "-XX:+DisableExplicitGC",
            "-XX:+AlwaysPreTouch",
            "-XX:G1NewSizePercent=30",
            "-XX:G1HeapRegionSize=8M",
            "-XX:InitiatingHeapOccupancyPercent=15",
            "-Dusing.aikars.flags=https://mcflags.emc.gs",
            "-Daikars.new.flags=true",
            "-Xss2M",
            "--add-opens=java.base/java.lang=ALL-UNNAMED",
            "-Dfile.encoding=UTF-8",
        ];
        launch(&flags, &["--nogui", "nogui", "--forceUpgrade"])
            .validate()
            .unwrap();
    }

    #[test]
    fn dangerous_jvm_arguments_are_rejected() {
        for bad in [
            "-XX:OnOutOfMemoryError=cmd /c calc",
            "-XX:OnError=calc",
            "-XX:ErrorFile=C:/x.log",
            "-javaagent:evil.jar",
            "-agentpath:C:/x.dll",
            "@C:/args.txt",
            "-jar",
            "other.jar",
            "-cp",
            "-classpath",
            "--class-path=x",
            "-XX:MaxHeapSize=64G",
            "-Xmx4G",
            "-Djava.system.class.loader=Evil",
            "-Dlog4j2.configurationFile=http://x",
            "-Dfabric.addMods=C:/mods",
            "-XX:+StartFlightRecording",
            "-verbose",
        ] {
            assert!(launch(&[bad], &[]).validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn server_arguments_cannot_contain_paths() {
        for bad in [
            "--plugins=C:/evil",
            "C:\\x",
            "../world",
            "@args",
            "--world-dir=/tmp",
        ] {
            assert!(launch(&[], &[bad]).validate().is_err(), "{bad}");
        }
        launch(&[], &["--port", "25570", "--safeMode"])
            .validate()
            .unwrap();
    }
}
