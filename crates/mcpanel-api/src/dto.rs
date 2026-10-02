//! Data transfer objects: the stable, UI-facing shape of the API. Generated into
//! TypeScript (`apps/desktop/src/bindings`) by ts-rs. Domain types never cross the
//! boundary directly.

use crate::error::ApiError;
use mcpanel_core::backup::BackupPolicy;
use mcpanel_core::backup::restore::RestorePreview;
use mcpanel_core::backup::service::BackupView;
use mcpanel_core::bedrock::{AuthType, BedrockPiece, BedrockPong, BedrockSettings, BedrockStatus};
use mcpanel_core::config::schema::{Applicability, PropertySchema, PropertyView};
use mcpanel_core::console::{ConsoleBatch, ConsoleLine};
use mcpanel_core::content::service::{ContentList, PendingChange, PendingKind};
use mcpanel_core::content::{ContentProviderInfo, ContentVersion, ProjectSummary};
use mcpanel_core::crash::{CrashEvent, RestartPolicy};
use mcpanel_core::events::{DomainEvent, EventEnvelope};
use mcpanel_core::files::service::{FileEntry, TextDocument};
use mcpanel_core::java::JavaCompatibility;
use mcpanel_core::jobs::JobRecord;
use mcpanel_core::model::{AuditEntry, JavaRuntime, LaunchConfig};
use mcpanel_core::monitoring::{ServerMetrics, SystemMetrics};
use mcpanel_core::notify::{Channels, Notification, NotificationPrefs};
use mcpanel_core::players::lists::Ban;
use mcpanel_core::players::{ActionOutcome, PlayerAction, ServerPlayers};
use mcpanel_core::ports::{LocationWarning, PortStatus, SystemSnapshot};
use mcpanel_core::server::ServerView;
use mcpanel_core::settings::{AppSettings, ThemePreference};
use mcpanel_core::software::{GameVersion, SoftwareBuild, SoftwareDescriptor};
use mcpanel_core::templates::{ResolvedTemplate, Template, TemplatePlugin};
use mcpanel_core::tunnels::{LinkProgress, TunnelStatus};
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
    /// Source: the server's own `tick query` or Paper `tps`/`mspt` commands.
    pub tick: Option<TickSampleDto>,
    pub tick_history: Vec<TickSampleDto>,
    /// "vanilla_tick_query" | "paper_commands"; `None` = not available for this server.
    pub tick_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TickSampleDto {
    #[ts(type = "number")]
    pub at: i64,
    pub tps: Option<f32>,
    /// TPS was calculated from MSPT (vanilla reports no TPS).
    pub tps_calculated: bool,
    pub mspt: Option<f32>,
    /// Vanilla P95 or Paper's 5-second maximum.
    pub mspt_high: Option<f32>,
    pub status: Option<String>,
}

impl From<mcpanel_core::perf::TickSample> for TickSampleDto {
    fn from(s: mcpanel_core::perf::TickSample) -> Self {
        Self {
            at: s.at.millis(),
            tps: s.tps,
            tps_calculated: s.tps_calculated,
            mspt: s.mspt,
            mspt_high: s.mspt_high,
            status: s.status,
        }
    }
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
            tick: m.tick.map(Into::into),
            tick_history: m.tick_history.into_iter().map(Into::into).collect(),
            tick_source: m.tick_source.map(|s| enum_str(&s)),
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
    pub tick_sampling: bool,
    /// Preset name, "system" or "#rrggbb".
    pub accent: String,
    pub onboarding_completed: bool,
}

impl From<AppSettings> for SettingsDto {
    fn from(s: AppSettings) -> Self {
        Self {
            theme: enum_str(&s.theme),
            tray_notice_shown: s.tray_notice_shown,
            console_buffer_lines: s.console_buffer_lines,
            quit_stop_timeout_secs: s.quit_stop_timeout_secs,
            tick_sampling: s.tick_sampling,
            accent: s.accent,
            onboarding_completed: s.onboarding_completed,
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
    pub tick_sampling: Option<bool>,
    pub accent: Option<String>,
    pub onboarding_completed: Option<bool>,
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
    ContentChanged {
        server_id: String,
    },
    BedrockChanged {
        server_id: String,
    },
    CrashRecorded {
        server_id: String,
        action: String,
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
    NotificationCreated {
        id: String,
        server_id: Option<String>,
        severity: String,
        title: String,
        body: String,
        desktop: bool,
        inbox: bool,
    },
    NotificationsChanged,
    CloudChanged {
        provider: String,
    },
    BackupsChanged {
        server_id: Option<String>,
        backup_id: String,
    },
    HostsChanged,
    CloudOperationUpdated {
        operation: CloudOperationDto,
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
            D::ContentChanged { server_id } => Self::ContentChanged {
                server_id: server_id.to_string(),
            },
            D::BedrockChanged { server_id } => Self::BedrockChanged {
                server_id: server_id.to_string(),
            },
            D::CrashRecorded { server_id, action } => Self::CrashRecorded {
                server_id: server_id.to_string(),
                action: action.clone(),
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
            D::NotificationCreated {
                id,
                server_id,
                severity,
                title,
                body,
                desktop,
                inbox,
            } => Self::NotificationCreated {
                id: id.clone(),
                server_id: server_id.map(|s| s.to_string()),
                severity: severity.clone(),
                title: title.clone(),
                body: body.clone(),
                desktop: *desktop,
                inbox: *inbox,
            },
            D::NotificationsChanged => Self::NotificationsChanged,
            D::CloudChanged { provider } => Self::CloudChanged {
                provider: provider.clone(),
            },
            D::BackupsChanged {
                server_id,
                backup_id,
            } => Self::BackupsChanged {
                server_id: server_id.map(|s| s.to_string()),
                backup_id: backup_id.to_string(),
            },
            D::HostsChanged => Self::HostsChanged,
            D::CloudOperationUpdated { operation } => Self::CloudOperationUpdated {
                operation: CloudOperationDto::from(operation),
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
    /// Age-encrypted with the Backup Master Key.
    pub encrypted: bool,
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
            encrypted: b.encrypted,
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
    /// GiB; 0 = no cap.
    pub max_total_gb: u32,
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
            max_total_gb: p.max_total_gb,
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
    #[serde(default)]
    pub max_total_gb: u32,
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

// ─────────────────────────────── content ──────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentProviderDto {
    pub id: String,
    pub display_name: String,
    pub website: String,
    pub hash_lookup: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DetectedPluginDto {
    pub file_name: String,
    pub name: String,
    pub version: Option<String>,
    pub provider: Option<String>,
    pub project_id: Option<String>,
    pub platform: Option<String>,
    /// "exact" | "high" | "medium" | "low" | "unknown"
    pub confidence: String,
    pub description: Option<String>,
}

impl From<mcpanel_core::content::DetectedPlugin> for DetectedPluginDto {
    fn from(d: mcpanel_core::content::DetectedPlugin) -> Self {
        Self {
            file_name: d.file_name,
            name: d.name,
            version: d.version,
            provider: d.provider,
            project_id: d.project_id,
            platform: d.platform,
            confidence: d.confidence.as_str().to_string(),
            description: d.description,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginRecommendationDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub provider: String,
    pub project_id: String,
    pub icon_url: Option<String>,
    pub reason: String,
}

impl From<mcpanel_core::content::PluginRecommendation> for PluginRecommendationDto {
    fn from(r: mcpanel_core::content::PluginRecommendation) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            category: r.category.as_str().to_string(),
            provider: r.provider,
            project_id: r.project_id,
            icon_url: r.icon_url,
            reason: r.reason,
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IdentifyContentRequestDto {
    pub file_name: String,
    pub provider: String,
    pub project_id: String,
    pub version_number: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentEntryDto {
    pub file_name: String,
    pub enabled: bool,
    pub size_bytes: u64,
    /// Name from MCPanel's record or the jar's descriptor.
    pub name: String,
    pub version: Option<String>,
    /// e.g. `plugin.yml`; `null` = no descriptor found.
    pub descriptor_format: Option<String>,
    pub provider: Option<String>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    /// Queued change: "install" | "remove" | "disable" | "enable".
    pub pending: Option<String>,
    pub detection: Option<DetectedPluginDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PendingChangeDto {
    pub id: String,
    /// "install" | "remove" | "disable" | "enable"
    pub action: String,
    pub file_name: String,
    pub name: String,
    pub version: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentListDto {
    /// "plugin" | "mod"
    pub kind: String,
    pub folder: String,
    pub running: bool,
    pub entries: Vec<ContentEntryDto>,
    pub pending: Vec<PendingChangeDto>,
    pub providers: Vec<ContentProviderDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectDto {
    pub provider: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub author: Option<String>,
    pub downloads: u64,
    pub icon_url: Option<String>,
    pub page_url: String,
    pub updated: Option<i64>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchRequestDto {
    pub provider: String,
    pub text: String,
    /// "relevance" | "downloads" | "updated"
    pub sort: String,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchPageDto {
    pub hits: Vec<ProjectDto>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentDependencyDto {
    pub project_id: Option<String>,
    pub name: Option<String>,
    /// "required" | "optional" | "incompatible" | "embedded"
    pub kind: String,
    pub external_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentVersionDto {
    pub provider: String,
    pub project_id: String,
    pub id: String,
    pub name: String,
    pub version_number: String,
    /// "release" | "beta" | "alpha"
    pub channel: String,
    pub game_versions: Vec<String>,
    pub published: Option<i64>,
    pub file_name: Option<String>,
    pub size_bytes: Option<u64>,
    /// "sha512" | "sha256" | "sha1"; `null` = unverified.
    pub hash_algorithm: Option<String>,
    pub external_url: Option<String>,
    pub dependencies: Vec<ContentDependencyDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlannedInstallDto {
    pub project: ProjectDto,
    pub version: ContentVersionDto,
    pub replaces: Option<String>,
    pub required_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallPlanDto {
    pub items: Vec<PlannedInstallDto>,
    pub unresolved: Vec<String>,
    pub warnings: Vec<String>,
    pub deferred: bool,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallRequestDto {
    pub provider: String,
    pub project_id: String,
    pub version_id: Option<String>,
    pub with_dependencies: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateInfoDto {
    pub file_name: String,
    pub name: String,
    pub current_version: Option<String>,
    pub latest: ContentVersionDto,
}

impl From<ProjectSummary> for ProjectDto {
    fn from(p: ProjectSummary) -> Self {
        Self {
            provider: p.provider,
            id: p.id,
            slug: p.slug,
            name: p.name,
            description: p.description,
            author: p.author,
            downloads: p.downloads,
            icon_url: p.icon_url,
            page_url: p.page_url,
            updated: p.updated.map(|t| t.millis()),
            license: p.license,
        }
    }
}

impl From<ContentVersion> for ContentVersionDto {
    fn from(v: ContentVersion) -> Self {
        Self {
            provider: v.provider,
            project_id: v.project_id,
            id: v.id,
            name: v.name,
            version_number: v.version_number,
            channel: enum_str(&v.channel),
            game_versions: v.game_versions,
            published: v.published.map(|t| t.millis()),
            file_name: v.file.as_ref().map(|f| f.file_name.clone()),
            size_bytes: v.file.as_ref().and_then(|f| f.size),
            hash_algorithm: v
                .file
                .as_ref()
                .and_then(|f| f.hash.as_ref())
                .map(|h| enum_str(&h.algorithm)),
            external_url: v.external_url,
            dependencies: v
                .dependencies
                .into_iter()
                .map(|d| ContentDependencyDto {
                    project_id: d.project_id,
                    name: d.name,
                    kind: enum_str(&d.kind),
                    external_url: d.external_url,
                })
                .collect(),
        }
    }
}

fn pending_action(k: &PendingKind) -> &'static str {
    match k {
        PendingKind::Install { .. } => "install",
        PendingKind::Remove { .. } => "remove",
        PendingKind::Disable { .. } => "disable",
        PendingKind::Enable { .. } => "enable",
    }
}

impl ContentListDto {
    pub fn new(
        l: ContentList,
        pending: Vec<PendingChange>,
        providers: Vec<ContentProviderInfo>,
    ) -> Self {
        Self {
            kind: l.kind.as_str().into(),
            folder: l.folder,
            running: l.running,
            entries: l
                .entries
                .into_iter()
                .map(|e| {
                    let src = e.record.as_ref().and_then(|r| r.source.clone());
                    ContentEntryDto {
                        name: e
                            .record
                            .as_ref()
                            .map(|r| r.name.clone())
                            .or_else(|| e.descriptor.as_ref().and_then(|d| d.name.clone()))
                            .unwrap_or_else(|| e.file_name.trim_end_matches(".jar").to_string()),
                        version: e
                            .record
                            .as_ref()
                            .and_then(|r| r.version_number.clone())
                            .or_else(|| e.descriptor.as_ref().and_then(|d| d.version.clone())),
                        descriptor_format: e.descriptor.as_ref().map(|d| d.format.clone()),
                        provider: src.as_ref().map(|s| s.provider.clone()),
                        project_id: src.as_ref().map(|s| s.project_id.clone()),
                        version_id: src.as_ref().map(|s| s.version_id.clone()),
                        pending: e.pending.as_ref().map(|p| pending_action(p).to_string()),
                        file_name: e.file_name,
                        enabled: e.enabled,
                        size_bytes: e.size,
                        detection: e.detection.map(Into::into),
                    }
                })
                .collect(),
            pending: pending
                .into_iter()
                .map(|p| {
                    let (name, version) = match &p.change {
                        PendingKind::Install { record, .. } => {
                            (record.name.clone(), record.version_number.clone())
                        }
                        other => (other_file(other), None),
                    };
                    PendingChangeDto {
                        id: p.id,
                        action: pending_action(&p.change).into(),
                        file_name: other_file(&p.change),
                        name,
                        version,
                        created_at: p.created_at.millis(),
                    }
                })
                .collect(),
            providers: providers
                .into_iter()
                .map(|p| ContentProviderDto {
                    id: p.id,
                    display_name: p.display_name,
                    website: p.website,
                    hash_lookup: p.hash_lookup,
                })
                .collect(),
        }
    }
}

fn other_file(k: &PendingKind) -> String {
    match k {
        PendingKind::Install { record, .. } => record.file_name.clone(),
        PendingKind::Remove { file_name }
        | PendingKind::Disable { file_name }
        | PendingKind::Enable { file_name } => file_name.clone(),
    }
}

// ──────────────────────────────── disk ────────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiskUsageDto {
    #[ts(type = "number")]
    pub total_bytes: u64,
    #[ts(type = "number")]
    pub worlds_bytes: u64,
    #[ts(type = "number")]
    pub content_bytes: u64,
    #[ts(type = "number")]
    pub logs_bytes: u64,
    #[ts(type = "number")]
    pub other_bytes: u64,
    /// This server's backups (all kinds).
    #[ts(type = "number")]
    pub backups_bytes: u64,
    /// Free space on the server's drive.
    #[ts(type = "number | null")]
    pub drive_free_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub drive_total_bytes: Option<u64>,
    pub truncated: bool,
}

// ─────────────────────────────── cloud ────────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CloudStatusDto {
    /// "google_drive" | "dropbox"
    pub id: String,
    pub display_name: String,
    pub configured: bool,
    pub client_id_variable: String,
    pub connected: bool,
    pub needs_reconnect: bool,
    pub account_name: Option<String>,
    pub account_email: Option<String>,
    #[ts(type = "number | null")]
    pub connected_at: Option<i64>,
    pub scopes: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub manage_access_url: String,
    pub is_guest: bool,
    pub auto_linked: bool,
}

impl From<mcpanel_core::cloud::CloudStatus> for CloudStatusDto {
    fn from(s: mcpanel_core::cloud::CloudStatus) -> Self {
        Self {
            id: s.id,
            display_name: s.display_name,
            configured: s.configured,
            client_id_variable: s.client_id_variable,
            connected: s.connected,
            needs_reconnect: s.needs_reconnect,
            account_name: s.account.as_ref().and_then(|a| a.display_name.clone()),
            account_email: s.account.as_ref().and_then(|a| a.email.clone()),
            connected_at: s.connected_at.map(|t| t.millis()),
            scopes: s.scopes,
            redirect_uris: s.redirect_uris,
            manage_access_url: s.manage_access_url,
            is_guest: s.is_guest,
            auto_linked: s.auto_linked,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CloudFileDto {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub size_bytes: u64,
    #[ts(type = "number | null")]
    pub created_at: Option<i64>,
    #[ts(type = "number | null")]
    pub modified_at: Option<i64>,
}

impl From<mcpanel_core::cloud::CloudFileMetadata> for CloudFileDto {
    fn from(f: mcpanel_core::cloud::CloudFileMetadata) -> Self {
        Self {
            id: f.id,
            provider: f.provider,
            name: f.name,
            size_bytes: f.size_bytes,
            created_at: f.created_at.map(|t| t.millis()),
            modified_at: f.modified_at.map(|t| t.millis()),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CloudFlowDto {
    /// "waiting" | "connected" | "failed" | "cancelled"
    pub state: String,
    pub message: Option<String>,
}

impl From<mcpanel_core::cloud::FlowState> for CloudFlowDto {
    fn from(f: mcpanel_core::cloud::FlowState) -> Self {
        use mcpanel_core::cloud::FlowState as F;
        match f {
            F::Waiting => Self {
                state: "waiting".into(),
                message: None,
            },
            F::Connected => Self {
                state: "connected".into(),
                message: None,
            },
            F::Failed { message } => Self {
                state: "failed".into(),
                message: Some(message),
            },
            F::Cancelled => Self {
                state: "cancelled".into(),
                message: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CloudOperationDto {
    pub id: String,
    pub provider: String,
    pub backup_id: Option<String>,
    pub backup_name: String,
    /// "upload" | "download" | "restore"
    pub op_type: String,
    /// "idle" | "starting" | "uploading" | "downloading" | "processing" | "cancelling" | "cancelled" | "completed" | "failed"
    pub state: String,
    pub bytes_completed: u64,
    pub total_bytes: Option<u64>,
    pub percentage: Option<f32>,
    pub started_at_ms: i64,
    pub bytes_per_sec: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub error_message: Option<String>,
}

impl From<&mcpanel_core::cloud::CloudOperationSnapshot> for CloudOperationDto {
    fn from(op: &mcpanel_core::cloud::CloudOperationSnapshot) -> Self {
        Self {
            id: op.id.clone(),
            provider: op.provider.clone(),
            backup_id: op.backup_id.clone(),
            backup_name: op.backup_name.clone(),
            op_type: match op.operation_type {
                mcpanel_core::cloud::CloudOperationType::Upload => "upload".into(),
                mcpanel_core::cloud::CloudOperationType::Download => "download".into(),
                mcpanel_core::cloud::CloudOperationType::Restore => "restore".into(),
            },
            state: match op.state {
                mcpanel_core::cloud::CloudOperationState::Idle => "idle".into(),
                mcpanel_core::cloud::CloudOperationState::Starting => "starting".into(),
                mcpanel_core::cloud::CloudOperationState::Uploading => "uploading".into(),
                mcpanel_core::cloud::CloudOperationState::Downloading => "downloading".into(),
                mcpanel_core::cloud::CloudOperationState::Processing => "processing".into(),
                mcpanel_core::cloud::CloudOperationState::Cancelling => "cancelling".into(),
                mcpanel_core::cloud::CloudOperationState::Cancelled => "cancelled".into(),
                mcpanel_core::cloud::CloudOperationState::Completed => "completed".into(),
                mcpanel_core::cloud::CloudOperationState::Failed => "failed".into(),
            },
            bytes_completed: op.bytes_completed,
            total_bytes: op.total_bytes,
            percentage: op.progress_percentage,
            started_at_ms: op.start_time.millis(),
            bytes_per_sec: op.speed_bytes_per_sec,
            eta_seconds: op.eta_seconds,
            error_message: op.error_message.clone(),
        }
    }
}

// ───────────────────────────── encryption ─────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EncryptionStatusDto {
    pub configured: bool,
    /// Public key (`age1…`); identifies which Recovery Kit belongs to it.
    pub recipient: Option<String>,
    #[ts(type = "number | null")]
    pub created_at: Option<i64>,
    pub encrypt_backups: bool,
    pub key_available: bool,
    pub min_passphrase_chars: u32,
}

impl From<mcpanel_core::crypto::EncryptionStatus> for EncryptionStatusDto {
    fn from(s: mcpanel_core::crypto::EncryptionStatus) -> Self {
        Self {
            configured: s.configured,
            recipient: s.recipient,
            created_at: s.created_at.map(|t| t.millis()),
            encrypt_backups: s.encrypt_backups,
            key_available: s.key_available,
            min_passphrase_chars: mcpanel_core::crypto::MIN_PASSPHRASE_CHARS as u32,
        }
    }
}

// ──────────────────────────── notifications ───────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NotificationDto {
    pub id: String,
    #[ts(type = "number")]
    pub created_at: i64,
    pub server_id: Option<String>,
    /// "crash" | "backup_failed" | "task_failed" | "player_joined"
    pub category: String,
    /// "info" | "success" | "warning" | "error"
    pub severity: String,
    pub title: String,
    pub body: String,
    pub read: bool,
}

impl From<Notification> for NotificationDto {
    fn from(n: Notification) -> Self {
        Self {
            id: n.id,
            created_at: n.created_at.millis(),
            server_id: n.server_id.map(|s| s.to_string()),
            category: n.category.as_str().into(),
            severity: n.severity.as_str().into(),
            title: n.title,
            body: n.body,
            read: n.read,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChannelsDto {
    pub inbox: bool,
    pub desktop: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NotificationPrefsDto {
    pub crash: ChannelsDto,
    pub backup_failed: ChannelsDto,
    pub task_failed: ChannelsDto,
    pub player_joined: ChannelsDto,
    pub disk_low: ChannelsDto,
}

impl From<NotificationPrefs> for NotificationPrefsDto {
    fn from(p: NotificationPrefs) -> Self {
        let c = |c: Channels| ChannelsDto {
            inbox: c.inbox,
            desktop: c.desktop,
        };
        Self {
            crash: c(p.crash),
            backup_failed: c(p.backup_failed),
            task_failed: c(p.task_failed),
            player_joined: c(p.player_joined),
            disk_low: c(p.disk_low),
        }
    }
}

impl From<NotificationPrefsDto> for NotificationPrefs {
    fn from(p: NotificationPrefsDto) -> Self {
        let c = |c: ChannelsDto| Channels {
            inbox: c.inbox,
            desktop: c.desktop,
        };
        Self {
            crash: c(p.crash),
            backup_failed: c(p.backup_failed),
            task_failed: c(p.task_failed),
            player_joined: c(p.player_joined),
            disk_low: c(p.disk_low),
        }
    }
}

// ─────────────────────────────── tunnels ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TunnelLinkDto {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TunnelStatusDto {
    pub provider: String,
    pub display_name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub agent_running: Option<bool>,
    pub phase: Option<String>,
    pub secret_configured: Option<bool>,
    /// "waiting" | "linked" | "failed" while or after a link started in MCPanel.
    pub link_state: Option<String>,
    pub link_claim_url: Option<String>,
    pub link_error: Option<String>,
    pub can_control_agent: bool,
    pub can_link: bool,
    pub can_manage_tunnels: bool,
    pub links: Vec<TunnelLinkDto>,
}

impl From<TunnelStatus> for TunnelStatusDto {
    fn from(s: TunnelStatus) -> Self {
        Self {
            provider: s.provider,
            display_name: s.display_name,
            installed: s.installed,
            version: s.version,
            agent_running: s.agent_running,
            phase: s.phase,
            secret_configured: s.secret_configured,
            link_state: s.link.as_ref().map(|l| {
                match l {
                    LinkProgress::Waiting { .. } => "waiting",
                    LinkProgress::Linked => "linked",
                    LinkProgress::Failed { .. } => "failed",
                }
                .to_string()
            }),
            link_claim_url: match &s.link {
                Some(LinkProgress::Waiting { claim_url }) => Some(claim_url.clone()),
                _ => None,
            },
            link_error: match &s.link {
                Some(LinkProgress::Failed { message }) => Some(message.clone()),
                _ => None,
            },
            can_control_agent: s.caps.can_control_agent,
            can_link: s.caps.can_link,
            can_manage_tunnels: s.caps.can_manage_tunnels,
            links: s
                .links
                .into_iter()
                .map(|l| TunnelLinkDto {
                    label: l.label,
                    url: l.url,
                })
                .collect(),
        }
    }
}

// ─────────────────────────────── account ───────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountProfileDto {
    pub account_id: String,
    pub account_type: String,
    pub uid: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub display_name: Option<String>,
    pub photo_url: Option<String>,
    /// "password" | "google.com" | "microsoft.com" | "guest"
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountDto {
    /// A Firebase project is configured in this build.
    pub configured: bool,
    pub google_available: bool,
    pub microsoft_available: bool,
    pub signed_in: bool,
    pub is_guest: bool,
    pub profile: Option<AccountProfileDto>,
    /// "waiting" | "done" | "failed" for a Google sign-in started in MCPanel.
    pub google_state: Option<String>,
    pub google_error: Option<String>,
    /// "waiting" | "done" | "failed" for a Microsoft sign-in started in MCPanel.
    pub microsoft_state: Option<String>,
    pub microsoft_error: Option<String>,
    pub custom_oauth_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConfigureOauthDto {
    pub firebase_api_key: Option<String>,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub microsoft_client_id: Option<String>,
    pub microsoft_client_secret: Option<String>,
}

impl From<mcpanel_core::account::AccountProfile> for AccountProfileDto {
    fn from(p: mcpanel_core::account::AccountProfile) -> Self {
        Self {
            account_id: p.account_id,
            account_type: p.account_type.to_string(),
            uid: p.uid,
            email: p.email,
            email_verified: p.email_verified,
            display_name: p.display_name,
            photo_url: p.photo_url,
            provider: p.provider,
        }
    }
}

impl From<mcpanel_core::account::AccountStatus> for AccountDto {
    fn from(s: mcpanel_core::account::AccountStatus) -> Self {
        use mcpanel_core::account::{GoogleSignIn, MicrosoftSignIn};
        let (google_state, google_error) = match s.google {
            Some(GoogleSignIn::Waiting { .. }) => (Some("waiting".into()), None),
            Some(GoogleSignIn::Done) => (Some("done".into()), None),
            Some(GoogleSignIn::Failed { message }) => (Some("failed".into()), Some(message)),
            None => (None, None),
        };
        let (microsoft_state, microsoft_error) = match s.microsoft {
            Some(MicrosoftSignIn::Waiting { .. }) => (Some("waiting".into()), None),
            Some(MicrosoftSignIn::Done) => (Some("done".into()), None),
            Some(MicrosoftSignIn::Failed { message }) => (Some("failed".into()), Some(message)),
            None => (None, None),
        };
        Self {
            configured: s.configured,
            google_available: s.google_available,
            microsoft_available: s.microsoft_available,
            signed_in: s.signed_in,
            is_guest: s.is_guest,
            profile: s.profile.map(Into::into),
            google_state,
            google_error,
            microsoft_state,
            microsoft_error,
            custom_oauth_configured: s.custom_oauth_configured,
        }
    }
}

// ──────────────────────────── playit agent ────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayitAgentDto {
    /// The playit program (with playitd.exe) is installed.
    pub installed: bool,
    pub linked: bool,
    /// "waiting" | "linked" | "failed" for a link started in MCPanel.
    pub link_state: Option<String>,
    pub link_url: Option<String>,
    pub link_error: Option<String>,
    pub running: bool,
    pub phase: Option<String>,
    pub autostart: bool,
}

impl From<mcpanel_core::playit_agent::AgentStatus> for PlayitAgentDto {
    fn from(s: mcpanel_core::playit_agent::AgentStatus) -> Self {
        use mcpanel_core::playit_agent::AgentLink;
        let (link_state, link_url, link_error) = match s.link {
            Some(AgentLink::Waiting { url }) => (Some("waiting".into()), Some(url), None),
            Some(AgentLink::Linked) => (Some("linked".into()), None, None),
            Some(AgentLink::Failed { message }) => (Some("failed".into()), None, Some(message)),
            None => (None, None, None),
        };
        Self {
            installed: s.installed,
            linked: s.linked,
            link_state,
            link_url,
            link_error,
            running: s.running,
            phase: s.phase,
            autostart: s.autostart,
        }
    }
}

#[cfg(test)]
mod playit_agent_dto_tests {
    use super::*;
    use mcpanel_core::playit_agent::{AgentLink, AgentStatus};

    #[test]
    fn pending_agent_approval_url_reaches_the_frontend_dto() {
        let dto = PlayitAgentDto::from(AgentStatus {
            installed: true,
            linked: false,
            link: Some(AgentLink::Waiting {
                url: "https://playit.gg/claim/0123abcdef".into(),
            }),
            running: false,
            phase: None,
            autostart: true,
        });
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(value["linkState"], "waiting");
        assert_eq!(value["linkUrl"], "https://playit.gg/claim/0123abcdef");
        assert!(value["linkError"].is_null());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayitTunnelDto {
    pub id: String,
    pub name: Option<String>,
    /// "minecraft-java" | "minecraft-bedrock" | other playit types; null for custom ports.
    pub tunnel_type: Option<String>,
    pub port_type: String,
    pub enabled: bool,
    pub offline_reasons: Vec<String>,
    pub local_ip: Option<String>,
    pub local_port: Option<u16>,
    pub addresses: Vec<String>,
    /// Belongs to MCPanel's agent, so MCPanel may change it.
    pub editable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayitTunnelsDto {
    pub agent_id: String,
    /// "guest" | "email-not-verified" | "verified"
    pub account_status: String,
    pub premium: bool,
    pub tunnels: Vec<PlayitTunnelDto>,
}

impl From<mcpanel_core::playit_agent::TunnelList> for PlayitTunnelsDto {
    fn from(l: mcpanel_core::playit_agent::TunnelList) -> Self {
        Self {
            agent_id: l.agent.agent_id,
            account_status: l.agent.account_status,
            premium: l.agent.premium,
            tunnels: l
                .tunnels
                .into_iter()
                .map(|m| PlayitTunnelDto {
                    id: m.tunnel.id,
                    name: m.tunnel.name,
                    tunnel_type: m.tunnel.tunnel_type,
                    port_type: m.tunnel.port_type,
                    enabled: m.tunnel.enabled,
                    offline_reasons: m.tunnel.offline_reasons,
                    local_ip: m.tunnel.local_ip,
                    local_port: m.tunnel.local_port,
                    addresses: m.tunnel.addresses,
                    editable: m.editable,
                })
                .collect(),
        }
    }
}

// ─────────────────────────────── bedrock ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BedrockPieceDto {
    pub file_name: String,
    pub version: Option<String>,
    pub enabled: bool,
    pub pending: bool,
}

impl From<BedrockPiece> for BedrockPieceDto {
    fn from(p: BedrockPiece) -> Self {
        Self {
            file_name: p.file_name,
            version: p.version,
            enabled: p.enabled,
            pending: p.pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BedrockSettingsDto {
    pub port: u16,
    /// "online" | "offline" | "floodgate"
    pub auth_type: String,
}

impl From<BedrockSettings> for BedrockSettingsDto {
    fn from(s: BedrockSettings) -> Self {
        Self {
            port: s.port,
            auth_type: serde_json::to_value(s.auth_type)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
        }
    }
}

impl TryFrom<BedrockSettingsDto> for BedrockSettings {
    type Error = ApiError;
    fn try_from(d: BedrockSettingsDto) -> Result<Self, ApiError> {
        let auth_type: AuthType = serde_json::from_value(serde_json::Value::String(d.auth_type))
            .map_err(|_| ApiError::invalid("Unknown authentication type"))?;
        Ok(Self {
            port: d.port,
            auth_type,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BedrockStatusDto {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub geyser: Option<BedrockPieceDto>,
    pub floodgate: Option<BedrockPieceDto>,
    pub via_version: Option<BedrockPieceDto>,
    pub via_version_available: bool,
    pub via_version_suggested: bool,
    pub via_version_required: bool,
    pub config_path: Option<String>,
    pub config_exists: bool,
    pub settings: BedrockSettingsDto,
    pub active_port: Option<u16>,
    pub running: bool,
    pub restart_required: bool,
    pub floodgate_key_present: bool,
    /// Another program holds the Bedrock UDP port (checked while stopped).
    pub port_in_use: bool,
    pub port_in_use_by: Option<String>,
    pub default_port: u16,
}

impl From<BedrockStatus> for BedrockStatusDto {
    fn from(s: BedrockStatus) -> Self {
        let (port_in_use, port_in_use_by) = match s.port_status {
            Some(PortStatus::InUse { pid, process_name }) => (
                true,
                match (process_name, pid) {
                    (Some(n), Some(p)) => Some(format!("{n} (PID {p})")),
                    (None, Some(p)) => Some(format!("PID {p}")),
                    (n, None) => n,
                },
            ),
            _ => (false, None),
        };
        Self {
            supported: s.supported,
            unsupported_reason: s.unsupported_reason,
            geyser: s.geyser.map(Into::into),
            floodgate: s.floodgate.map(Into::into),
            via_version: s.via_version.map(Into::into),
            via_version_available: s.via_version_available,
            via_version_suggested: s.via_version_suggested,
            via_version_required: s.via_version_required,
            config_path: s.config_path,
            config_exists: s.config_exists,
            settings: s.settings.into(),
            active_port: s.active_port,
            running: s.running,
            restart_required: s.restart_required,
            floodgate_key_present: s.floodgate_key_present,
            port_in_use,
            port_in_use_by,
            default_port: s.default_port,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BedrockEnableDto {
    pub floodgate: bool,
    pub via_version: bool,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BedrockPongDto {
    pub motd: String,
    pub sub_motd: Option<String>,
    pub version: String,
    pub protocol: Option<u32>,
    pub players: Option<u32>,
    pub max_players: Option<u32>,
    pub game_mode: Option<String>,
    pub latency_ms: u64,
}

impl From<BedrockPong> for BedrockPongDto {
    fn from(p: BedrockPong) -> Self {
        Self {
            motd: p.motd,
            sub_motd: p.sub_motd,
            version: p.version,
            protocol: p.protocol,
            players: p.players,
            max_players: p.max_players,
            game_mode: p.game_mode,
            latency_ms: p.latency_ms,
        }
    }
}

// ─────────────────────────────── crashes ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RestartPolicyDto {
    pub enabled: bool,
    pub max_attempts: u32,
    pub window_secs: u32,
    pub delay_secs: u32,
    pub stable_secs: u32,
    pub crash_backup: bool,
}

impl From<RestartPolicy> for RestartPolicyDto {
    fn from(p: RestartPolicy) -> Self {
        Self {
            enabled: p.enabled,
            max_attempts: p.max_attempts,
            window_secs: p.window_secs,
            delay_secs: p.delay_secs,
            stable_secs: p.stable_secs,
            crash_backup: p.crash_backup,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CrashEventDto {
    pub id: String,
    pub occurred_at: i64,
    pub exit_code: Option<i32>,
    pub kind: String,
    pub message: String,
    pub attempt: u32,
    /// "restart" | "gave_up" | "not_restartable" | "disabled"
    pub action: String,
    pub restart_at: Option<i64>,
    pub crash_report: Option<String>,
    pub console_tail: Vec<String>,
    /// Root-cause exception, if one was found.
    pub exception: Option<String>,
    /// Plugins/mods in the crashing code path, most likely first.
    pub suspects: Vec<SuspectDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SuspectDto {
    pub name: String,
    pub file_name: String,
    /// "loader" (the stack frame names the jar) | "package"
    pub evidence: String,
}

impl From<CrashEvent> for CrashEventDto {
    fn from(e: CrashEvent) -> Self {
        Self {
            id: e.id,
            occurred_at: e.occurred_at.millis(),
            exit_code: e.exit_code,
            kind: e.kind,
            message: e.message,
            attempt: e.attempt,
            action: e.action.as_str().into(),
            restart_at: e.restart_at.map(|t| t.millis()),
            crash_report: e.crash_report,
            console_tail: e.console_tail,
            exception: e.analysis.exception,
            suspects: e
                .analysis
                .suspects
                .into_iter()
                .map(|s| SuspectDto {
                    name: s.name,
                    file_name: s.file_name,
                    evidence: s.evidence,
                })
                .collect(),
        }
    }
}

// ────────────────────────────── templates ─────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemoryRangeDto {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplatePluginDto {
    pub provider: String,
    pub project: String,
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub software: Vec<String>,
    pub memory_mb: Option<MemoryRangeDto>,
    /// Keys the template sets (values depend on the Minecraft version).
    pub property_keys: Vec<String>,
    /// Resolved default properties for modern versions (where until is None).
    pub default_properties: Vec<TemplatePropertyDto>,
    pub backup_interval_minutes: Option<u32>,
    pub auto_restart: bool,
    pub plugins: Vec<TemplatePluginDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplatePropertyDto {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResolvedTemplateDto {
    pub template_id: String,
    pub properties: Vec<TemplatePropertyDto>,
    pub plugins: Vec<TemplatePluginDto>,
    pub memory_mb: Option<MemoryRangeDto>,
    pub notes: Vec<String>,
}

fn plugin_dto(p: &TemplatePlugin) -> TemplatePluginDto {
    TemplatePluginDto {
        provider: p.provider.clone(),
        project: p.project.clone(),
        name: p.name.clone(),
        reason: p.reason.clone(),
    }
}

impl From<&Template> for TemplateDto {
    fn from(t: &Template) -> Self {
        let mut keys: Vec<String> = t.properties.iter().map(|p| p.key.clone()).collect();
        keys.dedup();
        let default_properties = t
            .properties
            .iter()
            .filter(|p| p.until.is_none())
            .map(|p| TemplatePropertyDto {
                key: p.key.clone(),
                value: p.value.clone(),
            })
            .collect();
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            description: t.description.clone(),
            icon: t.icon.clone(),
            software: t.software.clone(),
            memory_mb: t.memory_mb.as_ref().map(|m| MemoryRangeDto {
                min: m.min,
                max: m.max,
            }),
            property_keys: keys,
            default_properties,
            backup_interval_minutes: t.backups.as_ref().map(|b| b.interval_minutes),
            auto_restart: t.auto_restart,
            plugins: t.plugins.iter().map(plugin_dto).collect(),
        }
    }
}

impl From<ResolvedTemplate> for ResolvedTemplateDto {
    fn from(r: ResolvedTemplate) -> Self {
        Self {
            template_id: r.template_id,
            properties: r
                .properties
                .into_iter()
                .map(|(key, value)| TemplatePropertyDto { key, value })
                .collect(),
            plugins: r.plugins.iter().map(plugin_dto).collect(),
            memory_mb: r.memory_mb.map(|m| MemoryRangeDto {
                min: m.min,
                max: m.max,
            }),
            notes: r.notes,
        }
    }
}

// ────────────────────────────── multihost ─────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HostDto {
    pub id: String,
    pub name: String,
    pub endpoint: Option<String>,
    pub is_local: bool,
    pub status: String,
    pub tags: Vec<String>,
    pub os_info: Option<String>,
    pub cpu_count: Option<u32>,
    pub total_memory_bytes: Option<u64>,
    pub used_memory_bytes: Option<u64>,
    pub servers_count: u32,
    pub running_servers_count: u32,
    pub latency_ms: Option<u32>,
    pub last_seen: Option<i64>,
    pub created_at: i64,
}

impl From<&mcpanel_core::multihost::HostNode> for HostDto {
    fn from(h: &mcpanel_core::multihost::HostNode) -> Self {
        Self {
            id: h.id.clone(),
            name: h.name.clone(),
            endpoint: h.endpoint.clone(),
            is_local: h.is_local,
            status: h.status.as_str().to_string(),
            tags: h.tags.clone(),
            os_info: h.os_info.clone(),
            cpu_count: h.cpu_count,
            total_memory_bytes: h.total_memory_bytes,
            used_memory_bytes: h.used_memory_bytes,
            servers_count: h.servers_count,
            running_servers_count: h.running_servers_count,
            latency_ms: h.latency_ms,
            last_seen: h.last_seen,
            created_at: h.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MultihostStatusDto {
    pub account_required: bool,
    pub signed_in: bool,
    pub user_email: Option<String>,
    pub hosts: Vec<HostDto>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AddHostDto {
    pub name: String,
    pub endpoint: String,
    pub auth_token: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HostPingResultDto {
    pub host_id: String,
    pub online: bool,
    pub latency_ms: Option<u32>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HostEnrollmentTokenDto {
    pub token: String,
    pub account_uid: String,
    pub expires_at: i64,
    pub pairing_command: String,
}
