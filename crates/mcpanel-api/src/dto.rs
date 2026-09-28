//! Data transfer objects: the stable, UI-facing shape of the API. Generated into
//! TypeScript (`apps/desktop/src/bindings`) by ts-rs. Domain types never cross the
//! boundary directly.

use mcpanel_core::backup::BackupPolicy;
use mcpanel_core::backup::restore::RestorePreview;
use mcpanel_core::backup::service::BackupView;
use mcpanel_core::config::schema::{Applicability, PropertySchema, PropertyView};
use mcpanel_core::console::{ConsoleBatch, ConsoleLine};
use mcpanel_core::events::{DomainEvent, EventEnvelope};
use mcpanel_core::files::service::{FileEntry, TextDocument};
use mcpanel_core::java::JavaCompatibility;
use mcpanel_core::jobs::JobRecord;
use mcpanel_core::model::{AuditEntry, JavaRuntime, LaunchConfig};
use mcpanel_core::monitoring::{ServerMetrics, SystemMetrics};
use mcpanel_core::players::lists::Ban;
use mcpanel_core::players::{ActionOutcome, PlayerAction, ServerPlayers};
use mcpanel_core::ports::{LocationWarning, SystemSnapshot};
use mcpanel_core::server::ServerView;
use mcpanel_core::settings::{AppSettings, ThemePreference};
use mcpanel_core::software::{GameVersion, SoftwareBuild, SoftwareDescriptor};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

fn enum_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

// ─────────────────────────────── system ───────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfoDto {
    pub version: String,
    pub platform: String,
    pub data_dir: String,
    pub default_servers_dir: String,
    pub logs_dir: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiskDto {
    pub mount_point: String,
    pub name: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SystemSnapshotDto {
    pub cpu_percent: f32,
    pub cpu_count: u32,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub disks: Vec<DiskDto>,
    pub os_name: String,
    pub os_version: String,
    pub host_name: Option<String>,
}

impl From<SystemSnapshot> for SystemSnapshotDto {
    fn from(s: SystemSnapshot) -> Self {
        Self {
            cpu_percent: s.cpu_percent,
            cpu_count: s.cpu_count,
            memory_total_bytes: s.memory_total_bytes,
            memory_used_bytes: s.memory_used_bytes,
            disks: s
                .disks
                .into_iter()
                .map(|d| DiskDto {
                    mount_point: d.mount_point,
                    name: d.name,
                    total_bytes: d.total_bytes,
                    available_bytes: d.available_bytes,
                    removable: d.removable,
                })
                .collect(),
            os_name: s.os_name,
            os_version: s.os_version,
            host_name: s.host_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MetricPointDto {
    pub at: i64,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SystemMetricsDto {
    /// Source: operating system (sysinfo).
    pub current: Option<SystemSnapshotDto>,
    pub history: Vec<MetricPointDto>,
}

impl From<SystemMetrics> for SystemMetricsDto {
    fn from(m: SystemMetrics) -> Self {
        Self {
            current: m.current.map(Into::into),
            history: m
                .history
                .into_iter()
                .map(|p| MetricPointDto {
                    at: p.at.millis(),
                    cpu_percent: p.cpu_percent,
                    memory_bytes: p.memory_used_bytes,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessUsageDto {
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub process_count: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerMetricsDto {
    /// Source: operating system process metrics of the server's process tree.
    pub current: Option<ProcessUsageDto>,
    pub history: Vec<MetricPointDto>,
    pub uptime_ms: Option<i64>,
}

impl From<ServerMetrics> for ServerMetricsDto {
    fn from(m: ServerMetrics) -> Self {
        Self {
            current: m.current.map(|u| ProcessUsageDto {
                cpu_percent: u.cpu_percent,
                memory_bytes: u.memory_bytes,
                process_count: u.process_count,
            }),
            history: m
                .history
                .into_iter()
                .map(|p| MetricPointDto {
                    at: p.at.millis(),
                    cpu_percent: p.cpu_percent,
                    memory_bytes: p.memory_bytes,
                })
                .collect(),
            uptime_ms: m.uptime_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingsDto {
    /// "system" | "dark" | "light"
    pub theme: String,
    pub tray_notice_shown: bool,
    pub console_buffer_lines: u32,
    pub quit_stop_timeout_secs: u32,
}

impl From<AppSettings> for SettingsDto {
    fn from(s: AppSettings) -> Self {
        Self {
            theme: enum_str(&s.theme),
            tray_notice_shown: s.tray_notice_shown,
            console_buffer_lines: s.console_buffer_lines,
            quit_stop_timeout_secs: s.quit_stop_timeout_secs,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct SettingsPatchDto {
    pub theme: Option<String>,
    pub tray_notice_shown: Option<bool>,
    pub console_buffer_lines: Option<u32>,
    pub quit_stop_timeout_secs: Option<u32>,
}

pub(crate) fn parse_theme(s: &str) -> Option<ThemePreference> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

// ──────────────────────────────── java ────────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JavaRuntimeDto {
    pub id: String,
    pub path: String,
    pub major: u32,
    pub version: String,
    pub vendor: Option<String>,
    pub arch: Option<String>,
    pub is_64bit: bool,
    /// "detected" | "manual" | "managed"
    pub source: String,
    pub valid: bool,
    pub validation_error: Option<String>,
    pub validated_at: i64,
}

impl From<JavaRuntime> for JavaRuntimeDto {
    fn from(j: JavaRuntime) -> Self {
        Self {
            id: j.id.to_string(),
            path: j.path.to_string_lossy().to_string(),
            major: j.major,
            version: j.version,
            vendor: j.vendor,
            arch: j.arch,
            is_64bit: j.is_64bit,
            source: j.source.as_str().to_string(),
            valid: j.valid,
            validation_error: j.validation_error,
            validated_at: j.validated_at.millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
#[ts(export)]
pub enum JavaCompatibilityDto {
    Compatible,
    TooOld { required: u32 },
    NewerThanRecommended { recommended: u32 },
    NotValidated,
}

impl From<JavaCompatibility> for JavaCompatibilityDto {
    fn from(c: JavaCompatibility) -> Self {
        match c {
            JavaCompatibility::Compatible => Self::Compatible,
            JavaCompatibility::TooOld { required } => Self::TooOld { required },
            JavaCompatibility::NewerThanRecommended { recommended } => {
                Self::NewerThanRecommended { recommended }
            }
            JavaCompatibility::NotValidated => Self::NotValidated,
        }
    }
}

// ────────────────────────────── software ──────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SoftwareDto {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub content: Vec<String>,
    pub eula_required: bool,
    pub supported: bool,
}

impl From<&SoftwareDescriptor> for SoftwareDto {
    fn from(d: &SoftwareDescriptor) -> Self {
        Self {
            id: d.id.clone(),
            display_name: d.display_name.clone(),
            description: d.description.clone(),
            content: d.caps.content.iter().map(enum_str).collect(),
            eula_required: d.caps.eula_required,
            supported: !d.caps.requires_build_step,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameVersionDto {
    pub id: String,
    /// "release" | "snapshot" | "old_beta" | "old_alpha"
    pub kind: String,
    pub release_time: Option<i64>,
}

impl From<GameVersion> for GameVersionDto {
    fn from(v: GameVersion) -> Self {
        Self {
            kind: enum_str(&v.kind),
            id: v.id,
            release_time: v.release_time.map(|t| t.millis()),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SoftwareBuildDto {
    pub id: String,
    /// "stable" | "beta" | "alpha"
    pub channel: String,
    pub published_at: Option<i64>,
}

impl From<SoftwareBuild> for SoftwareBuildDto {
    fn from(b: SoftwareBuild) -> Self {
        Self {
            channel: enum_str(&b.channel),
            id: b.id,
            published_at: b.published_at.map(|t| t.millis()),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JavaCompatibilityEntryDto {
    pub java_runtime_id: String,
    pub compatibility: JavaCompatibilityDto,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallPreviewDto {
    pub software_id: String,
    pub game_version: String,
    pub build: Option<String>,
    pub build_channel: Option<String>,
    pub java_min_major: u32,
    pub java_recommended_major: Option<u32>,
    pub recommended_jvm_flags: Vec<String>,
    pub notes: Vec<String>,
    /// e.g. "sha256"; `null` if the provider publishes no checksum.
    pub hash_algorithm: Option<String>,
    /// SHA-256/SHA-512 = strong; SHA-1/MD5 = integrity only.
    pub hash_strong: bool,
    pub download_bytes: Option<u64>,
    pub java: Vec<JavaCompatibilityEntryDto>,
}

// ─────────────────────────────── servers ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaunchConfigDto {
    pub java_runtime_id: Option<String>,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
    pub server_args: Vec<String>,
    pub stop_timeout_secs: u32,
}

impl From<&LaunchConfig> for LaunchConfigDto {
    fn from(l: &LaunchConfig) -> Self {
        Self {
            java_runtime_id: l.java_runtime_id.map(|j| j.to_string()),
            min_memory_mb: l.min_memory_mb,
            max_memory_mb: l.max_memory_mb,
            jvm_args: l.jvm_args.clone(),
            server_args: l.server_args.clone(),
            stop_timeout_secs: l.stop_timeout_secs,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagnosisDto {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerSoftwareDto {
    pub software_id: String,
    pub software_name: String,
    pub game_version: String,
    pub build: Option<String>,
    pub jar: String,
    pub java_min_major: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerDto {
    pub id: String,
    pub name: String,
    pub directory: String,
    pub software: ServerSoftwareDto,
    pub launch: LaunchConfigDto,
    /// Lifecycle state (see `LifecycleState`), e.g. "running".
    pub state: String,
    pub pid: Option<u32>,
    pub started_at: Option<i64>,
    pub ready_at: Option<i64>,
    /// Players seen joining in the console log since the last start.
    pub online_players: Vec<String>,
    pub diagnosis: Option<DiagnosisDto>,
    pub last_exit_code: Option<i32>,
    pub operations: Vec<String>,
    pub console_attached: bool,
    pub port: Option<u16>,
    pub eula_accepted: bool,
    pub directory_exists: bool,
    pub created_at: i64,
}

impl ServerDto {
    pub fn from_view(v: ServerView, software_name: String) -> Self {
        let s = v.server;
        let r = v.runtime;
        Self {
            id: s.id.to_string(),
            name: s.name,
            directory: s.directory.to_string_lossy().to_string(),
            software: ServerSoftwareDto {
                software_id: s.software.software_id,
                software_name,
                game_version: s.software.game_version,
                build: s.software.build,
                jar: s.software.jar,
                java_min_major: s.software.java_min_major,
            },
            launch: (&s.launch).into(),
            state: r.state.as_str().to_string(),
            pid: r.pid,
            started_at: r.started_at.map(|t| t.millis()),
            ready_at: r.ready_at.map(|t| t.millis()),
            online_players: r.online_players,
            diagnosis: r.diagnosis.map(|d| DiagnosisDto {
                kind: d.kind,
                message: d.message,
            }),
            last_exit_code: r.last_exit_code,
            operations: r.operations.iter().map(enum_str).collect(),
            console_attached: r.console_attached,
            port: v.port,
            eula_accepted: v.eula_accepted,
            directory_exists: v.directory_exists,
            created_at: s.created_at.millis(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PropertyValueDto {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CreateServerDto {
    pub name: String,
    /// Grant token for a parent folder; `null` = default servers folder.
    pub parent_directory_grant: Option<String>,
    pub software_id: String,
    pub game_version: String,
    pub build: Option<String>,
    pub java_runtime_id: String,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
    pub properties: Vec<PropertyValueDto>,
    /// The user ticked "I accept the Minecraft EULA".
    pub accept_eula: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LocationCheckDto {
    pub directory: String,
    /// "one_drive_synced" | "network_drive" | ...
    pub warnings: Vec<String>,
}

pub(crate) fn warnings(w: &[LocationWarning]) -> Vec<String> {
    w.iter().map(enum_str).collect()
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DetectedSoftwareDto {
    pub software_id: String,
    pub game_version: Option<String>,
    pub build: Option<String>,
    pub jar: String,
    pub confidence: u8,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportDetectionDto {
    pub detected: Option<DetectedSoftwareDto>,
    pub has_eula: bool,
    pub has_properties: bool,
    pub jars: Vec<String>,
    pub warnings: Vec<String>,
    pub directory: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportServerDto {
    pub name: String,
    pub directory_grant: String,
    pub software_id: Option<String>,
    pub game_version: Option<String>,
    pub jar: Option<String>,
    pub java_runtime_id: Option<String>,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct UpdateServerDto {
    pub name: Option<String>,
    pub launch: Option<LaunchConfigDto>,
}

// ───────────────────────────── properties ─────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PropertySchemaDto {
    /// "boolean" | "integer" | "string" | "text" | "enum"
    pub kind: String,
    pub values: Vec<String>,
    pub suggestions: Vec<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub default: Option<String>,
    /// "gameplay" | "network" | "world" | "performance" | "security" | "advanced"
    pub category: String,
    pub label: String,
    pub description: Option<String>,
    pub sensitive: bool,
}

impl From<PropertySchema> for PropertySchemaDto {
    fn from(s: PropertySchema) -> Self {
        Self {
            kind: enum_str(&s.kind),
            values: s.values,
            suggestions: s.suggestions,
            min: s.min,
            max: s.max,
            default: s.default,
            category: enum_str(&s.category),
            label: s.label,
            description: s.description,
            sensitive: s.sensitive,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PropertyDto {
    pub key: String,
    /// `null` for sensitive keys (write-only) or keys not in the file.
    pub value: Option<String>,
    pub present: bool,
    pub has_value: bool,
    /// "applies" | "unknown"
    pub applicability: String,
    pub schema: Option<PropertySchemaDto>,
}

impl From<PropertyView> for PropertyDto {
    fn from(p: PropertyView) -> Self {
        Self {
            key: p.key,
            value: p.value,
            present: p.present,
            has_value: p.has_value,
            applicability: match p.applicability {
                Applicability::Applies => "applies".into(),
                Applicability::Unknown => "unknown".into(),
            },
            schema: p.schema.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerPropertiesDto {
    pub file_exists: bool,
    pub game_version: String,
    pub restart_required: bool,
    pub properties: Vec<PropertyDto>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PropertyChangeDto {
    pub key: String,
    /// `null` removes the key.
    pub value: Option<String>,
}

// ─────────────────────────────── console ──────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleLineDto {
    pub seq: u64,
    pub at: i64,
    /// "stdout" | "stderr" | "system" | "command"
    pub stream: String,
    /// "trace" | "debug" | "info" | "warn" | "error" | "fatal"
    pub level: Option<String>,
    pub text: String,
}

impl From<ConsoleLine> for ConsoleLineDto {
    fn from(l: ConsoleLine) -> Self {
        Self {
            seq: l.seq,
            at: l.at.millis(),
            stream: enum_str(&l.stream),
            level: l.level.as_ref().map(enum_str),
            text: l.text,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleBatchDto {
    pub lines: Vec<ConsoleLineDto>,
    /// Inclusive range of sequence numbers that were dropped (buffer overflow).
    pub gap_from: Option<u64>,
    pub gap_to: Option<u64>,
}

impl From<ConsoleBatch> for ConsoleBatchDto {
    fn from(b: ConsoleBatch) -> Self {
        Self {
            lines: b.lines.into_iter().map(Into::into).collect(),
            gap_from: b.gap.map(|g| g.0),
            gap_to: b.gap.map(|g| g.1),
        }
    }
}

// ──────────────────────────────── files ───────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileEntryDto {
    pub name: String,
    pub path: String,
    /// "file" | "directory" | "link"
    pub kind: String,
    pub size: Option<u64>,
    pub modified: Option<i64>,
    /// Protected key material (e.g. Floodgate key): content is never exposed.
    pub sensitive: bool,
    pub readonly: bool,
}

impl From<FileEntry> for FileEntryDto {
    fn from(e: FileEntry) -> Self {
        Self {
            kind: enum_str(&e.kind),
            name: e.name,
            path: e.path,
            size: e.size,
            modified: e.modified.map(|t| t.millis()),
            sensitive: e.sensitive,
            readonly: e.readonly,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TextDocumentDto {
    pub path: String,
    pub content: String,
    pub encoding: String,
    pub bom: bool,
    /// "lf" | "cr_lf" | "mixed" | "none"
    pub line_ending: String,
    pub sha256: String,
    pub size: u64,
    pub modified: Option<i64>,
    pub lossy: bool,
    pub read_only_reason: Option<String>,
    pub truncated: bool,
}

impl From<TextDocument> for TextDocumentDto {
    fn from(d: TextDocument) -> Self {
        Self {
            line_ending: enum_str(&d.line_ending),
            path: d.path,
            content: d.content,
            encoding: d.encoding.name,
            bom: d.encoding.bom,
            sha256: d.sha256,
            size: d.size,
            modified: d.modified.map(|t| t.millis()),
            lossy: d.lossy,
            read_only_reason: d.read_only_reason,
            truncated: d.truncated,
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WriteTextDto {
    pub content: String,
    pub encoding: String,
    pub bom: bool,
    /// Hash from when the document was opened; `null` to overwrite unconditionally.
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileOpResultDto {
    pub files: u64,
    pub bytes: u64,
    pub skipped_links: u64,
    pub skipped_sensitive: u64,
    pub entry: Option<FileEntryDto>,
}

// ───────────────────────────── jobs / audit ───────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobDto {
    pub id: String,
    pub kind: String,
    pub server_id: Option<String>,
    /// "queued" | "running" | "succeeded" | "failed" | "cancelled"
    pub status: String,
    pub progress: Option<f32>,
    pub message: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub result: Option<serde_json::Value>,
}

impl From<JobRecord> for JobDto {
    fn from(j: JobRecord) -> Self {
        Self {
            id: j.id.to_string(),
            kind: j.kind,
            server_id: j.server_id.map(|s| s.to_string()),
            status: j.status.as_str().into(),
            progress: j.progress,
            message: j.message,
            created_at: j.created_at.millis(),
            finished_at: j.finished_at.map(|t| t.millis()),
            error_code: j.error_code.as_ref().map(enum_str),
            error_message: j.error_message,
            result: j.result,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AuditEntryDto {
    pub id: String,
    pub occurred_at: i64,
    pub actor: String,
    pub action: String,
    pub server_id: Option<String>,
    pub target: Option<String>,
    /// "success" | "failure"
    pub result: String,
    pub metadata: serde_json::Value,
}

impl From<AuditEntry> for AuditEntryDto {
    fn from(a: AuditEntry) -> Self {
        Self {
            id: a.id.to_string(),
            occurred_at: a.occurred_at.millis(),
            actor: a.actor,
            action: a.action,
            server_id: a.server_id.map(|s| s.to_string()),
            target: a.target,
            result: a.result.as_str().into(),
            metadata: a.metadata,
        }
    }
}

// ─────────────────────────────── events ───────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum EventDto {
    ServerCreated {
        server_id: String,
    },
    ServerUpdated {
        server_id: String,
    },
    ServerDeleted {
        server_id: String,
    },
    ServerStateChanged {
        server_id: String,
        state: String,
        previous: String,
    },
    ServerReady {
        server_id: String,
        startup_ms: i64,
    },
    ServerStopped {
        server_id: String,
        exit_code: Option<i32>,
        forced: bool,
    },
    ServerCrashed {
        server_id: String,
        exit_code: Option<i32>,
        diagnosis: Option<String>,
    },
    PlayerJoined {
        server_id: String,
        player_name: String,
    },
    PlayerLeft {
        server_id: String,
        player_name: String,
    },
    PlayersChanged {
        server_id: String,
    },
    JobUpdated {
        job_id: String,
        kind: String,
        server_id: Option<String>,
        status: String,
        progress: Option<f32>,
        message: Option<String>,
    },
    JavaRuntimesChanged,
    SettingsChanged,
    AuditRecorded,
    BackupsChanged {
        server_id: Option<String>,
        backup_id: String,
    },
}

impl From<&EventEnvelope> for EventDto {
    fn from(e: &EventEnvelope) -> Self {
        use DomainEvent as D;
        match &e.event {
            D::ServerCreated { server_id } => Self::ServerCreated {
                server_id: server_id.to_string(),
            },
            D::ServerUpdated { server_id } => Self::ServerUpdated {
                server_id: server_id.to_string(),
            },
            D::ServerDeleted { server_id } => Self::ServerDeleted {
                server_id: server_id.to_string(),
            },
            D::ServerStateChanged {
                server_id,
                state,
                previous,
            } => Self::ServerStateChanged {
                server_id: server_id.to_string(),
                state: state.as_str().into(),
                previous: previous.as_str().into(),
            },
            D::ServerReady {
                server_id,
                startup_ms,
            } => Self::ServerReady {
                server_id: server_id.to_string(),
                startup_ms: *startup_ms,
            },
            D::ServerStopped {
                server_id,
                exit_code,
                forced,
            } => Self::ServerStopped {
                server_id: server_id.to_string(),
                exit_code: *exit_code,
                forced: *forced,
            },
            D::ServerCrashed {
                server_id,
                exit_code,
                diagnosis,
            } => Self::ServerCrashed {
                server_id: server_id.to_string(),
                exit_code: *exit_code,
                diagnosis: diagnosis.clone(),
            },
            D::PlayerJoined {
                server_id,
                player_name,
            } => Self::PlayerJoined {
                server_id: server_id.to_string(),
                player_name: player_name.clone(),
            },
            D::PlayerLeft {
                server_id,
                player_name,
            } => Self::PlayerLeft {
                server_id: server_id.to_string(),
                player_name: player_name.clone(),
            },
            D::PlayersChanged { server_id } => Self::PlayersChanged {
                server_id: server_id.to_string(),
            },
            D::JobUpdated {
                job_id,
                kind,
                server_id,
                status,
                progress,
                message,
            } => Self::JobUpdated {
                job_id: job_id.to_string(),
                kind: kind.clone(),
                server_id: server_id.map(|s| s.to_string()),
                status: status.as_str().into(),
                progress: *progress,
                message: message.clone(),
            },
            D::JavaRuntimesChanged { .. } => Self::JavaRuntimesChanged,
            D::SettingsChanged { .. } => Self::SettingsChanged,
            D::AuditRecorded => Self::AuditRecorded,
            D::BackupsChanged {
                server_id,
                backup_id,
            } => Self::BackupsChanged {
                server_id: server_id.map(|s| s.to_string()),
                backup_id: backup_id.to_string(),
            },
        }
    }
}

// ─────────────────────────────── backups ──────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SkippedFileDto {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupDto {
    pub id: String,
    pub server_id: Option<String>,
    pub server_name: String,
    /// "manual" | "scheduled" | "pre_restore"
    pub kind: String,
    /// "creating" | "ready" | "failed"
    pub status: String,
    pub path: String,
    pub file_name: String,
    pub file_present: bool,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub size_bytes: u64,
    pub content_bytes: u64,
    pub file_count: u64,
    pub live: bool,
    pub contains_sensitive: bool,
    pub software_id: String,
    pub game_version: String,
    pub note: Option<String>,
    pub protected: bool,
    pub skipped: Vec<SkippedFileDto>,
    pub error_message: Option<String>,
}

impl From<BackupView> for BackupDto {
    fn from(v: BackupView) -> Self {
        let b = v.backup;
        Self {
            id: b.id.to_string(),
            server_id: b.server_id.map(|s| s.to_string()),
            server_name: b.server_name,
            kind: b.kind.as_str().into(),
            status: b.status.as_str().into(),
            file_name: b
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            path: b.path.to_string_lossy().into(),
            file_present: v.file_present,
            created_at: b.created_at.millis(),
            finished_at: b.finished_at.map(|t| t.millis()),
            size_bytes: b.size_bytes,
            content_bytes: b.content_bytes,
            file_count: b.file_count,
            live: b.live,
            contains_sensitive: b.contains_sensitive,
            software_id: b.software_id,
            game_version: b.game_version,
            note: b.note,
            protected: b.protected,
            skipped: b
                .skipped
                .into_iter()
                .map(|s| SkippedFileDto {
                    path: s.path,
                    reason: s.reason,
                })
                .collect(),
            error_message: b.error_message,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupPolicyDto {
    pub server_id: String,
    pub enabled: bool,
    pub interval_minutes: u32,
    pub skip_if_idle: bool,
    pub keep_last: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
    pub last_run_at: Option<i64>,
    /// When the scheduler next considers this server (if enabled).
    pub next_run_at: Option<i64>,
}

impl From<BackupPolicy> for BackupPolicyDto {
    fn from(p: BackupPolicy) -> Self {
        Self {
            server_id: p.server_id.to_string(),
            enabled: p.enabled,
            interval_minutes: p.interval_minutes,
            skip_if_idle: p.skip_if_idle,
            keep_last: p.retention.keep_last,
            keep_daily: p.retention.keep_daily,
            keep_weekly: p.retention.keep_weekly,
            keep_monthly: p.retention.keep_monthly,
            last_run_at: p.last_run_at.map(|t| t.millis()),
            next_run_at: p
                .enabled
                .then(|| {
                    p.last_run_at
                        .map(|t| t.millis() + p.interval_minutes as i64 * 60_000)
                })
                .flatten(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupPolicyUpdateDto {
    pub enabled: bool,
    pub interval_minutes: u32,
    pub skip_if_idle: bool,
    pub keep_last: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RestorePreviewDto {
    pub added: u64,
    pub removed: u64,
    pub changed: u64,
    pub unchanged: u64,
    pub changed_jars: Vec<String>,
    pub removed_sample: Vec<String>,
    pub total_bytes: u64,
    pub contains_sensitive: bool,
}

impl From<RestorePreview> for RestorePreviewDto {
    fn from(p: RestorePreview) -> Self {
        Self {
            added: p.added,
            removed: p.removed,
            changed: p.changed,
            unchanged: p.unchanged,
            changed_jars: p.changed_jars,
            removed_sample: p.removed_sample,
            total_bytes: p.total_bytes,
            contains_sensitive: p.contains_sensitive,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupLocationDto {
    pub directory: String,
    pub is_default: bool,
    pub default_directory: String,
    /// Free space on the drive, when the folder exists.
    pub available_bytes: Option<u64>,
    pub warnings: Vec<String>,
}

// ─────────────────────────────── players ──────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OperatorDto {
    pub name: String,
    pub uuid: Option<String>,
    pub level: u8,
    pub bypasses_player_limit: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ListedPlayerDto {
    pub name: String,
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BanDto {
    /// Player name, or the IP address for IP bans.
    pub target: String,
    pub uuid: Option<String>,
    pub reason: Option<String>,
    pub source: Option<String>,
    pub created: Option<String>,
    /// `null` = permanent.
    pub expires: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KnownPlayerDto {
    pub name: String,
    pub uuid: Option<String>,
    pub online: bool,
    pub op_level: Option<u8>,
    pub whitelisted: bool,
    pub banned: bool,
    pub first_seen: Option<i64>,
    pub last_seen: Option<i64>,
    pub total_play_ms: Option<i64>,
    pub sessions: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerPlayersDto {
    /// Changes are sent to the running server through its console.
    pub live: bool,
    pub read_only_reason: Option<String>,
    pub online_known: bool,
    pub online: Vec<String>,
    pub online_mode: bool,
    pub whitelist_enabled: bool,
    pub enforce_whitelist: bool,
    pub max_players: Option<u32>,
    pub operators: Vec<OperatorDto>,
    pub whitelist: Vec<ListedPlayerDto>,
    pub bans: Vec<BanDto>,
    pub ip_bans: Vec<BanDto>,
    pub known: Vec<KnownPlayerDto>,
}

fn ban_dto(b: Ban) -> BanDto {
    BanDto {
        target: b.target,
        uuid: b.uuid.map(|u| u.to_string()),
        reason: b.reason,
        source: b.source,
        created: b.created,
        expires: b.expires,
    }
}

impl From<ServerPlayers> for ServerPlayersDto {
    fn from(p: ServerPlayers) -> Self {
        Self {
            live: p.live,
            read_only_reason: p.read_only_reason,
            online_known: p.online_known,
            online: p.online,
            online_mode: p.online_mode,
            whitelist_enabled: p.whitelist_enabled,
            enforce_whitelist: p.enforce_whitelist,
            max_players: p.max_players,
            operators: p
                .operators
                .into_iter()
                .map(|o| OperatorDto {
                    name: o.name,
                    uuid: o.uuid.map(|u| u.to_string()),
                    level: o.level,
                    bypasses_player_limit: o.bypasses_player_limit,
                })
                .collect(),
            whitelist: p
                .whitelist
                .into_iter()
                .map(|w| ListedPlayerDto {
                    name: w.name,
                    uuid: w.uuid.map(|u| u.to_string()),
                })
                .collect(),
            bans: p.bans.into_iter().map(ban_dto).collect(),
            ip_bans: p.ip_bans.into_iter().map(ban_dto).collect(),
            known: p
                .known
                .into_iter()
                .map(|k| KnownPlayerDto {
                    name: k.name,
                    uuid: k.uuid.map(|u| u.to_string()),
                    online: k.online,
                    op_level: k.op_level,
                    whitelisted: k.whitelisted,
                    banned: k.banned,
                    first_seen: k.stats.as_ref().map(|s| s.first_seen.millis()),
                    last_seen: k.stats.as_ref().map(|s| s.last_seen.millis()),
                    total_play_ms: k.stats.as_ref().map(|s| s.total_play_ms),
                    sessions: k.stats.as_ref().map(|s| s.sessions),
                })
                .collect(),
        }
    }
}

/// A player-management action. Names and reasons are validated by the core.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum PlayerActionDto {
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

impl From<PlayerActionDto> for PlayerAction {
    fn from(a: PlayerActionDto) -> Self {
        use PlayerActionDto as D;
        match a {
            D::Op { name } => Self::Op { name },
            D::Deop { name } => Self::Deop { name },
            D::WhitelistAdd { name } => Self::WhitelistAdd { name },
            D::WhitelistRemove { name } => Self::WhitelistRemove { name },
            D::SetWhitelist { enabled } => Self::SetWhitelist { enabled },
            D::Ban { name, reason } => Self::Ban { name, reason },
            D::Pardon { name } => Self::Pardon { name },
            D::BanIp { ip, reason } => Self::BanIp { ip, reason },
            D::PardonIp { ip } => Self::PardonIp { ip },
            D::Kick { name, reason } => Self::Kick { name, reason },
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerActionOutcomeDto {
    /// "console" | "files"
    pub via: String,
    pub messages: Vec<String>,
}

impl From<ActionOutcome> for PlayerActionOutcomeDto {
    fn from(o: ActionOutcome) -> Self {
        Self {
            via: enum_str(&o.via),
            messages: o.messages,
        }
    }
}
