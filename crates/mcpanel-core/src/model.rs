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
            if lower.starts_with("-xmx") || lower.starts_with("-xms") {
                return Err(CoreError::invalid(
                    "Set memory with the memory fields, not -Xmx/-Xms JVM arguments",
                ));
            }
        }
        Ok(())
    }
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
