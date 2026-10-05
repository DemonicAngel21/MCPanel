//! The Application API facade. Transport adapters (Tauri IPC in v1; HTTP/WebSocket
//! later) call these methods; each validates input, authorizes the principal, calls the
//! core and maps results to DTOs.

use crate::dto::*;
use crate::error::{ApiError, ApiResult};
use crate::grants::{GrantKind, GrantRegistry};
use crate::principal::{Permission, Principal};
use mcpanel_core::Core;
use mcpanel_core::backup::{BackupPolicy, CreateBackupRequest, Retention};
use mcpanel_core::bedrock::EnableRequest;
use mcpanel_core::console::ConsoleSubscription;
use mcpanel_core::content::{InstallRequest, SearchSort};
use mcpanel_core::events::EventEnvelope;
use mcpanel_core::files::service::WriteText;
use mcpanel_core::files::text::TextEncoding;
use mcpanel_core::ids::{BackupId, JavaRuntimeId, JobId, ServerId};
use mcpanel_core::model::{AuditQuery, LaunchConfig};
use mcpanel_core::server::{
    CreateServerRequest, ImportServerRequest, PropertyChange, UpdateServerRequest,
};
use mcpanel_core::settings::AppSettingsPatch;
use mcpanel_core::time::Timestamp;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

pub struct Api {
    core: Arc<Core>,
    pub grants: GrantRegistry,
    version: String,
}

fn server_id(s: &str) -> ApiResult<ServerId> {
    ServerId::from_str(s).map_err(|_| ApiError::invalid("Invalid server id"))
}

fn install_request(r: InstallRequestDto) -> InstallRequest {
    InstallRequest {
        provider: r.provider,
        project_id: r.project_id,
        version_id: r.version_id,
        with_dependencies: r.with_dependencies,
    }
}

fn backup_id(s: &str) -> ApiResult<BackupId> {
    BackupId::from_str(s).map_err(|_| ApiError::invalid("Invalid backup id"))
}

fn java_id(s: &str) -> ApiResult<JavaRuntimeId> {
    JavaRuntimeId::from_str(s).map_err(|_| ApiError::invalid("Invalid Java runtime id"))
}

fn launch_from(d: LaunchConfigDto) -> ApiResult<LaunchConfig> {
    Ok(LaunchConfig {
        java_runtime_id: d.java_runtime_id.as_deref().map(java_id).transpose()?,
        min_memory_mb: d.min_memory_mb,
        max_memory_mb: d.max_memory_mb,
        jvm_args: d.jvm_args,
        server_args: d.server_args,
        stop_timeout_secs: d.stop_timeout_secs,
    })
}

impl Api {
    pub fn new(core: Arc<Core>, version: impl Into<String>) -> Self {
        Self {
            core,
            grants: GrantRegistry::default(),
            version: version.into(),
        }
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }

    fn software_name(&self, id: &str) -> String {
        self.core
            .servers
            .registry()
            .get_software(id)
            .map(|p| p.descriptor.display_name.clone())
            .unwrap_or_else(|_| id.to_string())
    }

    // ───────────────────────────── system ─────────────────────────────

    pub fn app_info(&self, p: &Principal) -> ApiResult<AppInfoDto> {
        p.authorize(Permission::SystemRead)?;
        let paths = &self.core.paths;
        Ok(AppInfoDto {
            version: self.version.clone(),
            platform: self.core.platform.name().into(),
            data_dir: paths.data_dir.to_string_lossy().into(),
            default_servers_dir: paths.default_servers_dir.to_string_lossy().into(),
            logs_dir: paths.logs_dir().to_string_lossy().into(),
        })
    }

    pub fn system_metrics(&self, p: &Principal) -> ApiResult<SystemMetricsDto> {
        p.authorize(Permission::SystemRead)?;
        Ok(self.core.monitor.system().into())
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<EventEnvelope> {
        self.core.events.subscribe()
    }

    pub async fn settings_get(&self, p: &Principal) -> ApiResult<SettingsDto> {
        p.authorize(Permission::SystemRead)?;
        Ok(self.core.settings.get().await?.into())
    }

    pub async fn settings_update(
        &self,
        p: &Principal,
        patch: SettingsPatchDto,
    ) -> ApiResult<SettingsDto> {
        p.authorize(Permission::SettingsWrite)?;
        let theme = match patch.theme.as_deref() {
            Some(t) => Some(parse_theme(t).ok_or_else(|| ApiError::invalid("Unknown theme"))?),
            None => None,
        };
        if let Some(a) = &patch.accent
            && !mcpanel_core::settings::valid_accent(a)
        {
            return Err(ApiError::invalid("Unknown accent color"));
        }
        Ok(self
            .core
            .settings
            .update(AppSettingsPatch {
                theme,
                tray_notice_shown: patch.tray_notice_shown,
                console_buffer_lines: patch.console_buffer_lines,
                quit_stop_timeout_secs: patch.quit_stop_timeout_secs,
                tick_sampling: patch.tick_sampling,
                backups_dir: None,
                accent: patch.accent,
                onboarding_completed: patch.onboarding_completed,
            })
            .await?
            .into())
    }

    // ────────────────────────────── java ──────────────────────────────

    pub async fn java_list(&self, p: &Principal) -> ApiResult<Vec<JavaRuntimeDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .java
            .list()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn java_detect(&self, p: &Principal) -> ApiResult<Vec<JavaRuntimeDto>> {
        p.authorize(Permission::JavaManage)?;
        Ok(self
            .core
            .java
            .detect()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn java_add(&self, p: &Principal, grant: &str) -> ApiResult<JavaRuntimeDto> {
        p.authorize(Permission::JavaManage)?;
        let path = self.grants.take(grant, GrantKind::Source)?;
        Ok(self.core.java.add_manual(path).await?.into())
    }

    pub async fn java_revalidate(&self, p: &Principal, id: &str) -> ApiResult<JavaRuntimeDto> {
        p.authorize(Permission::JavaManage)?;
        Ok(self.core.java.revalidate(java_id(id)?).await?.into())
    }

    pub async fn java_remove(&self, p: &Principal, id: &str) -> ApiResult<()> {
        p.authorize(Permission::JavaManage)?;
        Ok(self.core.java.remove(java_id(id)?).await?)
    }

    // ──────────────────────────── software ────────────────────────────

    pub fn software_list(&self, p: &Principal) -> ApiResult<Vec<SoftwareDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .servers
            .registry()
            .software()
            .iter()
            .map(|s| (&s.descriptor).into())
            .collect())
    }

    pub async fn software_versions(
        &self,
        p: &Principal,
        software_id: &str,
        include_snapshots: bool,
    ) -> ApiResult<Vec<GameVersionDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .servers
            .game_versions(software_id, include_snapshots)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn software_builds(
        &self,
        p: &Principal,
        software_id: &str,
        game_version: &str,
    ) -> ApiResult<Vec<SoftwareBuildDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .servers
            .builds(software_id, game_version)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn software_preview(
        &self,
        p: &Principal,
        software_id: &str,
        game_version: &str,
        build: Option<String>,
    ) -> ApiResult<InstallPreviewDto> {
        p.authorize(Permission::ServersRead)?;
        let preview = self
            .core
            .servers
            .preview_install(software_id, game_version, build)
            .await?;
        let plan = preview.plan;
        // Summarise every download: the weakest hash, strong only if all are, total size.
        let downloads: Vec<_> = plan
            .steps
            .iter()
            .filter_map(|s| match s {
                mcpanel_core::software::InstallStep::Download {
                    expected_hash,
                    size,
                    ..
                } => Some((expected_hash, size)),
                _ => None,
            })
            .collect();
        let rank = |a: mcpanel_core::ports::HashAlgorithm| {
            use mcpanel_core::ports::HashAlgorithm as H;
            match a {
                H::Md5 => 0,
                H::Sha1 => 1,
                H::Sha256 => 2,
                H::Sha512 => 3,
            }
        };
        let all_hashed = !downloads.is_empty() && downloads.iter().all(|(h, _)| h.is_some());
        let weakest = downloads
            .iter()
            .filter_map(|(h, _)| h.as_ref().map(|h| h.algorithm))
            .min_by_key(|a| rank(*a));
        let hash_algorithm = weakest.filter(|_| all_hashed).map(|a| {
            serde_json::to_value(a)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default()
        });
        let hash_strong = all_hashed && weakest.is_some_and(|a| a.is_strong());
        let download_bytes = downloads.iter().map(|(_, s)| **s).sum::<Option<u64>>();
        Ok(InstallPreviewDto {
            software_id: plan.software_id,
            game_version: plan.game_version,
            build: plan.build,
            build_channel: plan.build_channel.map(|c| {
                serde_json::to_value(c)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default()
            }),
            java_min_major: plan.java.min_major,
            java_recommended_major: plan.java.recommended_major,
            recommended_jvm_flags: plan.java.recommended_flags,
            notes: plan.notes,
            hash_algorithm,
            hash_strong,
            download_bytes,
            java: preview
                .java
                .into_iter()
                .map(|(id, c)| JavaCompatibilityEntryDto {
                    java_runtime_id: id.to_string(),
                    compatibility: c.into(),
                })
                .collect(),
        })
    }

    // ───────────────────────────── servers ────────────────────────────

    pub async fn servers_list(&self, p: &Principal) -> ApiResult<Vec<ServerDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .servers
            .list()
            .await?
            .into_iter()
            .map(|v| {
                let name = self.software_name(&v.server.software.software_id);
                ServerDto::from_view(v, name)
            })
            .collect())
    }

    pub async fn servers_get(&self, p: &Principal, id: &str) -> ApiResult<ServerDto> {
        p.authorize(Permission::ServersRead)?;
        let v = self.core.servers.view(server_id(id)?).await?;
        let name = self.software_name(&v.server.software.software_id);
        Ok(ServerDto::from_view(v, name))
    }

    pub async fn servers_check_location(
        &self,
        p: &Principal,
        name: &str,
        parent_grant: Option<&str>,
    ) -> ApiResult<LocationCheckDto> {
        p.authorize(Permission::ServersRead)?;
        let parent = parent_grant
            .map(|g| self.grants.peek(g, GrantKind::Directory))
            .transpose()?;
        let c = self.core.servers.check_new_location(name, parent).await?;
        Ok(LocationCheckDto {
            directory: c.directory.to_string_lossy().into(),
            warnings: warnings(&c.warnings),
        })
    }

    /// Returns the id of the provisioning job.
    pub async fn servers_create(&self, p: &Principal, req: CreateServerDto) -> ApiResult<String> {
        p.authorize(Permission::ServersManage)?;
        let parent = req
            .parent_directory_grant
            .as_deref()
            .map(|g| self.grants.take(g, GrantKind::Directory))
            .transpose()?;
        let job = self
            .core
            .servers
            .create(
                CreateServerRequest {
                    name: req.name,
                    parent_directory: parent,
                    software_id: req.software_id,
                    game_version: req.game_version,
                    build: req.build,
                    java_runtime_id: java_id(&req.java_runtime_id)?,
                    min_memory_mb: req.min_memory_mb,
                    max_memory_mb: req.max_memory_mb,
                    jvm_args: req.jvm_args,
                    properties: req
                        .properties
                        .into_iter()
                        .map(|p| (p.key, p.value))
                        .collect(),
                    accept_eula: req.accept_eula,
                },
                p.actor(),
            )
            .await?;
        Ok(job.to_string())
    }

    pub async fn servers_detect_import(
        &self,
        p: &Principal,
        grant: &str,
    ) -> ApiResult<ImportDetectionDto> {
        p.authorize(Permission::ServersRead)?;
        let dir = self.grants.peek(grant, GrantKind::Directory)?;
        let d = self.core.servers.detect_import(dir.clone()).await?;
        Ok(ImportDetectionDto {
            detected: d.detected.map(|s| DetectedSoftwareDto {
                software_id: s.software_id,
                game_version: s.game_version,
                build: s.build,
                jar: s.jar,
                confidence: s.confidence,
            }),
            has_eula: d.has_eula,
            has_properties: d.has_properties,
            jars: d.jars,
            warnings: warnings(&d.warnings),
            directory: mcpanel_core::files::fsx::simplify(&dir)
                .to_string_lossy()
                .into(),
        })
    }

    pub async fn servers_import(
        &self,
        p: &Principal,
        req: ImportServerDto,
    ) -> ApiResult<ServerDto> {
        p.authorize(Permission::ServersManage)?;
        let dir = self
            .grants
            .take(&req.directory_grant, GrantKind::Directory)?;
        let server = self
            .core
            .servers
            .import(
                ImportServerRequest {
                    name: req.name,
                    directory: dir,
                    software_id: req.software_id,
                    game_version: req.game_version,
                    jar: req.jar,
                    java_runtime_id: req.java_runtime_id.as_deref().map(java_id).transpose()?,
                    min_memory_mb: req.min_memory_mb,
                    max_memory_mb: req.max_memory_mb,
                },
                p.actor(),
            )
            .await?;
        self.servers_get(p, &server.id.to_string()).await
    }

    pub async fn servers_update(
        &self,
        p: &Principal,
        id: &str,
        req: UpdateServerDto,
    ) -> ApiResult<ServerDto> {
        p.authorize(Permission::ServersManage)?;
        let sid = server_id(id)?;
        self.core
            .servers
            .update(
                sid,
                UpdateServerRequest {
                    name: req.name,
                    launch: req.launch.map(launch_from).transpose()?,
                },
                p.actor(),
            )
            .await?;
        self.servers_get(p, id).await
    }

    pub async fn servers_delete(
        &self,
        p: &Principal,
        id: &str,
        delete_files: bool,
    ) -> ApiResult<()> {
        p.authorize(Permission::ServersManage)?;
        Ok(self
            .core
            .servers
            .delete(server_id(id)?, delete_files, p.actor())
            .await?)
    }

    pub async fn servers_accept_eula(&self, p: &Principal, id: &str) -> ApiResult<()> {
        p.authorize(Permission::ServersManage)?;
        Ok(self
            .core
            .servers
            .accept_eula(server_id(id)?, p.actor())
            .await?)
    }

    pub async fn servers_start(&self, p: &Principal, id: &str) -> ApiResult<()> {
        p.authorize(Permission::ServersControl)?;
        Ok(self.core.servers.start(server_id(id)?, p.actor()).await?)
    }

    pub async fn servers_stop(&self, p: &Principal, id: &str, force: bool) -> ApiResult<()> {
        p.authorize(Permission::ServersControl)?;
        Ok(self
            .core
            .servers
            .stop(server_id(id)?, force, p.actor())
            .await?)
    }

    pub async fn servers_restart(&self, p: &Principal, id: &str) -> ApiResult<()> {
        p.authorize(Permission::ServersControl)?;
        Ok(self.core.servers.restart(server_id(id)?, p.actor()).await?)
    }

    pub async fn servers_command(&self, p: &Principal, id: &str, command: &str) -> ApiResult<()> {
        p.authorize(Permission::ServersControl)?;
        Ok(self
            .core
            .servers
            .send_command(server_id(id)?, command)
            .await?)
    }

    pub async fn servers_metrics(&self, p: &Principal, id: &str) -> ApiResult<ServerMetricsDto> {
        p.authorize(Permission::ServersRead)?;
        let sid = server_id(id)?;
        let v = self.core.servers.view(sid).await?;
        let uptime = if v.runtime.state.has_process() {
            v.runtime
                .started_at
                .map(|s| Timestamp::now().millis() - s.millis())
        } else {
            None
        };
        Ok(self.core.monitor.server(sid, uptime).into())
    }

    /// `server.properties` keys applicable to a new server of `game_version`.
    pub async fn software_property_schema(
        &self,
        p: &Principal,
        game_version: &str,
    ) -> ApiResult<Vec<PropertyDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .servers
            .property_schema_for(game_version)
            .await
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Write the buffered console output to a file chosen in a save dialog.
    pub async fn console_export(
        &self,
        p: &Principal,
        id: &str,
        save_grant: &str,
    ) -> ApiResult<u64> {
        p.authorize(Permission::ServersRead)?;
        let hub = self.core.servers.console(server_id(id)?);
        let dest = self.grants.take(save_grant, GrantKind::SaveTarget)?;
        let lines = hub.snapshot(None, usize::MAX);
        let count = lines.len() as u64;
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            use std::io::Write;
            let mut w = std::io::BufWriter::new(std::fs::File::create(&dest)?);
            for l in lines {
                match l.stream {
                    mcpanel_core::console::ConsoleStream::Stdout => writeln!(w, "{}", l.text)?,
                    mcpanel_core::console::ConsoleStream::Stderr => {
                        writeln!(w, "[stderr] {}", l.text)?
                    }
                    mcpanel_core::console::ConsoleStream::System => {
                        writeln!(w, "[mcpanel] {}", l.text)?
                    }
                    mcpanel_core::console::ConsoleStream::Command => writeln!(w, "> {}", l.text)?,
                }
            }
            w.flush()
        })
        .await
        .map_err(|e| ApiError::new("INTERNAL", e.to_string()))?
        .map_err(|e| ApiError::new("IO", format!("Cannot write log file: {e}")))?;
        Ok(count)
    }

    pub fn servers_running_count(&self, p: &Principal) -> ApiResult<u32> {
        p.authorize(Permission::ServersRead)?;
        Ok(self.core.servers.running_servers().len() as u32)
    }

    /// Stop all attached servers gracefully (used when quitting MCPanel).
    pub async fn servers_stop_all(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::ServersControl)?;
        let timeout = self.core.settings.get().await?.quit_stop_timeout_secs;
        self.core
            .servers
            .stop_all(Duration::from_secs(timeout as u64))
            .await;
        Ok(())
    }

    pub async fn servers_properties(
        &self,
        p: &Principal,
        id: &str,
    ) -> ApiResult<ServerPropertiesDto> {
        p.authorize(Permission::ServersRead)?;
        let props = self.core.servers.properties(server_id(id)?).await?;
        Ok(ServerPropertiesDto {
            file_exists: props.file_exists,
            game_version: props.game_version,
            restart_required: props.restart_required,
            properties: props.properties.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn servers_properties_update(
        &self,
        p: &Principal,
        id: &str,
        changes: Vec<PropertyChangeDto>,
    ) -> ApiResult<ServerPropertiesDto> {
        p.authorize(Permission::ServersManage)?;
        self.core
            .servers
            .update_properties(
                server_id(id)?,
                changes
                    .into_iter()
                    .map(|c| PropertyChange {
                        key: c.key,
                        value: c.value,
                    })
                    .collect(),
                p.actor(),
            )
            .await?;
        self.servers_properties(p, id).await
    }

    // ───────────────────────────── console ────────────────────────────

    pub fn console_history(
        &self,
        p: &Principal,
        id: &str,
        from_seq: Option<u64>,
        limit: u32,
    ) -> ApiResult<Vec<ConsoleLineDto>> {
        p.authorize(Permission::ServersRead)?;
        let hub = self.core.servers.console(server_id(id)?);
        Ok(hub
            .snapshot(from_seq, limit.clamp(1, 200_000) as usize)
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn console_search(
        &self,
        p: &Principal,
        id: &str,
        query: &str,
        limit: u32,
    ) -> ApiResult<Vec<ConsoleLineDto>> {
        p.authorize(Permission::ServersRead)?;
        let hub = self.core.servers.console(server_id(id)?);
        Ok(hub
            .search(query, limit.clamp(1, 5_000) as usize)
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Stream subscription; the transport adapter drives `next_batch` and forwards DTOs.
    pub fn console_subscribe(
        &self,
        p: &Principal,
        id: &str,
        after_seq: Option<u64>,
        backlog: u32,
    ) -> ApiResult<ConsoleSubscription> {
        p.authorize(Permission::ServersRead)?;
        let hub = self.core.servers.console(server_id(id)?);
        Ok(hub.subscribe(after_seq, backlog.min(20_000) as usize))
    }

    // ────────────────────────────── files ─────────────────────────────

    pub async fn files_list(
        &self,
        p: &Principal,
        id: &str,
        path: &str,
    ) -> ApiResult<Vec<FileEntryDto>> {
        p.authorize(Permission::FilesRead)?;
        Ok(self
            .core
            .files
            .list(server_id(id)?, path.to_string())
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn files_read(
        &self,
        p: &Principal,
        id: &str,
        path: &str,
    ) -> ApiResult<TextDocumentDto> {
        p.authorize(Permission::FilesRead)?;
        Ok(self
            .core
            .files
            .read_text(server_id(id)?, path.to_string())
            .await?
            .into())
    }

    pub async fn files_write(
        &self,
        p: &Principal,
        id: &str,
        path: &str,
        doc: WriteTextDto,
    ) -> ApiResult<TextDocumentDto> {
        p.authorize(Permission::FilesWrite)?;
        Ok(self
            .core
            .files
            .write_text(
                server_id(id)?,
                path.to_string(),
                WriteText {
                    content: doc.content,
                    encoding: TextEncoding {
                        name: doc.encoding,
                        bom: doc.bom,
                    },
                    expected_sha256: doc.expected_sha256,
                },
                p.actor(),
            )
            .await?
            .into())
    }

    pub async fn files_mkdir(
        &self,
        p: &Principal,
        id: &str,
        path: &str,
    ) -> ApiResult<FileEntryDto> {
        p.authorize(Permission::FilesWrite)?;
        Ok(self
            .core
            .files
            .create_dir(server_id(id)?, path.into(), p.actor())
            .await?
            .into())
    }

    pub async fn files_create(
        &self,
        p: &Principal,
        id: &str,
        path: &str,
    ) -> ApiResult<FileEntryDto> {
        p.authorize(Permission::FilesWrite)?;
        Ok(self
            .core
            .files
            .create_file(server_id(id)?, path.into(), p.actor())
            .await?
            .into())
    }

    pub async fn files_rename(
        &self,
        p: &Principal,
        id: &str,
        path: &str,
        new_name: &str,
    ) -> ApiResult<FileEntryDto> {
        p.authorize(Permission::FilesWrite)?;
        Ok(self
            .core
            .files
            .rename(server_id(id)?, path.into(), new_name.into(), p.actor())
            .await?
            .into())
    }

    pub async fn files_move(
        &self,
        p: &Principal,
        id: &str,
        paths: Vec<String>,
        dest: &str,
    ) -> ApiResult<()> {
        p.authorize(Permission::FilesWrite)?;
        Ok(self
            .core
            .files
            .move_to(server_id(id)?, paths, dest.into(), p.actor())
            .await?)
    }

    pub async fn files_copy(
        &self,
        p: &Principal,
        id: &str,
        paths: Vec<String>,
        dest: &str,
    ) -> ApiResult<FileOpResultDto> {
        p.authorize(Permission::FilesWrite)?;
        let s = self
            .core
            .files
            .copy_to(server_id(id)?, paths, dest.into(), p.actor())
            .await?;
        Ok(FileOpResultDto {
            files: s.files,
            bytes: s.bytes,
            skipped_links: s.skipped_links,
            skipped_sensitive: 0,
            entry: None,
        })
    }

    pub async fn files_delete(
        &self,
        p: &Principal,
        id: &str,
        paths: Vec<String>,
        permanent: bool,
    ) -> ApiResult<()> {
        p.authorize(Permission::FilesWrite)?;
        Ok(self
            .core
            .files
            .delete(server_id(id)?, paths, permanent, p.actor())
            .await?)
    }

    pub async fn files_zip(
        &self,
        p: &Principal,
        id: &str,
        paths: Vec<String>,
        archive_name: &str,
    ) -> ApiResult<FileOpResultDto> {
        p.authorize(Permission::FilesWrite)?;
        let (entry, r) = self
            .core
            .files
            .zip(server_id(id)?, paths, archive_name.into(), p.actor())
            .await?;
        Ok(FileOpResultDto {
            files: r.files,
            bytes: r.bytes,
            skipped_links: r.skipped_links,
            skipped_sensitive: r.skipped_sensitive,
            entry: Some(entry.into()),
        })
    }

    pub async fn files_unzip(
        &self,
        p: &Principal,
        id: &str,
        archive: &str,
        dest: &str,
        overwrite: bool,
    ) -> ApiResult<FileOpResultDto> {
        p.authorize(Permission::FilesWrite)?;
        let r = self
            .core
            .files
            .unzip(
                server_id(id)?,
                archive.into(),
                dest.into(),
                overwrite,
                p.actor(),
            )
            .await?;
        Ok(FileOpResultDto {
            files: r.files,
            bytes: r.bytes,
            skipped_links: 0,
            skipped_sensitive: 0,
            entry: None,
        })
    }

    pub async fn files_import(
        &self,
        p: &Principal,
        id: &str,
        grants: Vec<String>,
        dest: &str,
    ) -> ApiResult<Vec<FileEntryDto>> {
        p.authorize(Permission::FilesWrite)?;
        let sid = server_id(id)?;
        let mut out = Vec::new();
        for g in grants {
            let src = self.grants.take(&g, GrantKind::Source)?;
            out.push(
                self.core
                    .files
                    .import(sid, src, dest.into(), p.actor())
                    .await?
                    .into(),
            );
        }
        Ok(out)
    }

    pub async fn files_export(
        &self,
        p: &Principal,
        id: &str,
        path: &str,
        save_grant: &str,
    ) -> ApiResult<u64> {
        p.authorize(Permission::FilesRead)?;
        let dest = self.grants.take(save_grant, GrantKind::SaveTarget)?;
        Ok(self
            .core
            .files
            .export(server_id(id)?, path.into(), dest, p.actor())
            .await?)
    }

    pub async fn files_search(
        &self,
        p: &Principal,
        id: &str,
        path: &str,
        query: &str,
        limit: u32,
    ) -> ApiResult<Vec<FileEntryDto>> {
        p.authorize(Permission::FilesRead)?;
        Ok(self
            .core
            .files
            .search(server_id(id)?, path.into(), query.into(), limit as usize)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    // ──────────────────────────── templates ────────────────────────────

    pub fn templates_list(&self, p: &Principal) -> ApiResult<Vec<TemplateDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self.core.templates.list().iter().map(Into::into).collect())
    }

    pub async fn templates_resolve(
        &self,
        p: &Principal,
        id: &str,
        software_id: &str,
        game_version: &str,
    ) -> ApiResult<ResolvedTemplateDto> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .templates
            .resolve(id, software_id, game_version)
            .await?
            .into())
    }

    /// Apply a template's policies and chosen plugins to a created server; returns the
    /// plugin install job ids.
    pub async fn templates_apply(
        &self,
        p: &Principal,
        server: &str,
        id: &str,
        plugins: Vec<String>,
    ) -> ApiResult<Vec<String>> {
        p.authorize(Permission::ServersManage)?;
        Ok(self
            .core
            .templates
            .apply(server_id(server)?, id, &plugins, p.actor())
            .await?
            .into_iter()
            .map(|j| j.to_string())
            .collect())
    }

    // ───────────────────────────── crashes ─────────────────────────────

    pub async fn restart_policy(&self, p: &Principal, server: &str) -> ApiResult<RestartPolicyDto> {
        p.authorize(Permission::ServersRead)?;
        Ok(self.core.crashes.policy(server_id(server)?).await?.into())
    }

    pub async fn restart_policy_update(
        &self,
        p: &Principal,
        server: &str,
        d: RestartPolicyDto,
    ) -> ApiResult<RestartPolicyDto> {
        p.authorize(Permission::ServersManage)?;
        Ok(self
            .core
            .crashes
            .set_policy(
                mcpanel_core::crash::RestartPolicy {
                    server_id: server_id(server)?,
                    enabled: d.enabled,
                    max_attempts: d.max_attempts,
                    window_secs: d.window_secs,
                    delay_secs: d.delay_secs,
                    stable_secs: d.stable_secs,
                    crash_backup: d.crash_backup,
                },
                p.actor(),
            )
            .await?
            .into())
    }

    pub async fn crash_history(
        &self,
        p: &Principal,
        server: &str,
        limit: u32,
    ) -> ApiResult<Vec<CrashEventDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .crashes
            .history(server_id(server)?, limit)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    // ──────────────────────────────── disk ────────────────────────────────

    pub async fn server_disk_usage(&self, p: &Principal, server: &str) -> ApiResult<DiskUsageDto> {
        p.authorize(Permission::ServersRead)?;
        let id = server_id(server)?;
        let s = self.core.servers.get(id).await?;
        let dir = s.directory.clone();
        let level = std::fs::read(dir.join("server.properties"))
            .ok()
            .and_then(|b| {
                mcpanel_core::config::PropertiesDocument::parse(
                    &mcpanel_core::config::properties::decode_bytes(&b),
                )
                .get("level-name")
                .map(|v| v.trim().to_string())
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "world".into());
        let d2 = dir.clone();
        let u = tokio::task::spawn_blocking(move || mcpanel_core::disk::usage(&d2, &level))
            .await
            .map_err(|e| ApiError::new("INTERNAL", e.to_string()))?;
        let backups_bytes = self
            .core
            .backups
            .list(Some(id))
            .await?
            .iter()
            .filter(|b| b.file_present)
            .map(|b| b.backup.size_bytes)
            .sum();
        let space = self.core.platform.disk_space(&dir).ok();
        Ok(DiskUsageDto {
            total_bytes: u.total_bytes,
            worlds_bytes: u.worlds_bytes,
            content_bytes: u.content_bytes,
            logs_bytes: u.logs_bytes,
            other_bytes: u.other_bytes,
            backups_bytes,
            drive_free_bytes: space.map(|s| s.available_bytes),
            drive_total_bytes: space.map(|s| s.total_bytes),
            truncated: u.truncated,
        })
    }

    // ───────────────────────────── diagnostics ─────────────────────────────

    /// Write a support ZIP for a server to the save target; returns the entry count.
    pub async fn diagnostics_export(
        &self,
        p: &Principal,
        server: &str,
        save_grant: &str,
    ) -> ApiResult<u32> {
        p.authorize(Permission::ServersRead)?;
        let id = server_id(server)?;
        let s = self.core.servers.get(id).await?;
        let dest = self.grants.take(save_grant, GrantKind::SaveTarget)?;
        let java = match s.launch.java_runtime_id {
            Some(j) => self.core.java.get(j).await.ok().map(|r| {
                serde_json::json!({
                    "major": r.major, "version": r.version, "vendor": r.vendor,
                    "arch": r.arch, "path": r.path,
                })
            }),
            None => None,
        };
        let content = match self.core.content.list(id).await {
            Ok(list) => list
                .entries
                .into_iter()
                .map(|e| {
                    serde_json::json!({
                        "file": e.file_name,
                        "enabled": e.enabled,
                        "name": e.descriptor.as_ref().and_then(|d| d.name.clone()),
                        "version": e.descriptor.as_ref().and_then(|d| d.version.clone()),
                        "source": e.record.and_then(|r| r.source).map(|s| s.provider),
                    })
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        let crashes = self
            .core
            .crashes
            .history(id, 5)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "at": c.occurred_at.millis(), "kind": c.kind, "message": c.message,
                    "exitCode": c.exit_code, "action": c.action.as_str(),
                    "crashReport": c.crash_report, "analysis": c.analysis,
                })
            })
            .collect();
        let info = mcpanel_core::diagnostics::BundleInfo {
            generator: format!("MCPanel {}", self.version),
            created_at: Timestamp::now(),
            os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            server: serde_json::json!({
                "name": s.name,
                "software": s.software,
                "memoryMb": { "min": s.launch.min_memory_mb, "max": s.launch.max_memory_mb },
                "jvmArgs": s.launch.jvm_args,
                "serverArgs": s.launch.server_args,
            }),
            java: java.unwrap_or(serde_json::Value::Null),
            content,
            crashes,
        };
        let sources = mcpanel_core::diagnostics::BundleSources {
            server_dir: s.directory.clone(),
            console_dir: self.core.paths.console_dir(&id),
            app_logs_dir: self.core.paths.logs_dir(),
            sensitive_properties: mcpanel_core::config::schema::all_schemas()
                .iter()
                .filter(|p| p.sensitive)
                .map(|p| p.key.clone())
                .collect(),
        };
        let n = tokio::task::spawn_blocking(move || {
            mcpanel_core::diagnostics::write_bundle(&dest, &info, &sources)
        })
        .await
        .map_err(|e| ApiError::new("INTERNAL", e.to_string()))??;
        self.core
            .audit
            .record(
                p.actor(),
                "server.diagnostics_export",
                Some(id),
                Some(s.name.clone()),
                mcpanel_core::model::AuditResult::Success,
                serde_json::json!({ "entries": n }),
            )
            .await;
        Ok(n as u32)
    }

    // ─────────────────────────────── cloud ────────────────────────────────

    pub async fn cloud_list(&self, p: &Principal) -> ApiResult<Vec<CloudStatusDto>> {
        p.authorize(Permission::BackupsRead)?;
        Ok(self
            .core
            .cloud
            .list()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Start a sign-in; returns (flow id, authorization URL for the system browser).
    pub async fn cloud_begin_connect(
        &self,
        p: &Principal,
        provider: &str,
    ) -> ApiResult<(String, String)> {
        p.authorize(Permission::BackupsManage)?;
        let s = self.core.cloud.begin_connect(provider).await?;
        Ok((s.flow_id, s.authorize_url))
    }

    pub fn cloud_flow(&self, p: &Principal, flow_id: &str) -> ApiResult<CloudFlowDto> {
        p.authorize(Permission::BackupsRead)?;
        Ok(self.core.cloud.flow(flow_id)?.into())
    }

    pub fn cloud_cancel(&self, p: &Principal, flow_id: &str) -> ApiResult<()> {
        p.authorize(Permission::BackupsManage)?;
        self.core.cloud.cancel_connect(flow_id);
        Ok(())
    }

    pub async fn cloud_check(&self, p: &Principal, provider: &str) -> ApiResult<CloudStatusDto> {
        p.authorize(Permission::BackupsRead)?;
        Ok(self.core.cloud.check(provider).await?.into())
    }

    /// Returns "revoked" or "not_supported" (remove access in the provider's settings).
    pub async fn cloud_disconnect(&self, p: &Principal, provider: &str) -> ApiResult<String> {
        p.authorize(Permission::BackupsManage)?;
        let outcome = self.core.cloud.disconnect(provider).await?;
        self.core
            .audit
            .record(
                p.actor(),
                "cloud.disconnect",
                None,
                Some(provider.to_string()),
                mcpanel_core::model::AuditResult::Success,
                serde_json::json!({}),
            )
            .await;
        Ok(match outcome {
            mcpanel_core::cloud::RevokeOutcome::Revoked => "revoked".into(),
            mcpanel_core::cloud::RevokeOutcome::NotSupported => "not_supported".into(),
        })
    }

    pub async fn cloud_files_list(
        &self,
        p: &Principal,
        provider: Option<String>,
    ) -> ApiResult<Vec<CloudFileDto>> {
        p.authorize(Permission::BackupsRead)?;
        let files = self.core.cloud.list_files(provider.as_deref()).await?;
        Ok(files.into_iter().map(Into::into).collect())
    }

    pub async fn cloud_backup_upload(
        &self,
        p: &Principal,
        backup_id_str: &str,
        provider: &str,
    ) -> ApiResult<CloudOperationDto> {
        p.authorize(Permission::BackupsManage)?;
        let bid = backup_id(backup_id_str)?;
        let backup = self.core.backups.get(bid).await?;
        let file_name = backup
            .path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("backup.zip")
            .to_string();
        let file_meta = tokio::fs::metadata(&backup.path).await.map_err(|e| {
            mcpanel_core::error::CoreError::io("Cannot access backup file to upload", &e)
        })?;
        let file_size = file_meta.len();

        let op_id = uuid::Uuid::new_v4().to_string();
        let (snapshot, cancel_token) = self.core.cloud.start_operation(
            op_id.clone(),
            provider.to_string(),
            Some(backup.id.to_string()),
            file_name.clone(),
            mcpanel_core::cloud::CloudOperationType::Upload,
            Some(file_size),
        );

        let cloud_clone = self.core.cloud.clone();
        let op_id_prog = op_id.clone();
        let on_progress: mcpanel_core::cloud::CloudProgressFn =
            std::sync::Arc::new(move |completed, total| {
                cloud_clone.update_operation_progress(&op_id_prog, completed, total);
            });
        let transfer_opts = mcpanel_core::cloud::CloudTransferOptions {
            on_progress: Some(on_progress),
            cancel: cancel_token,
        };

        let core = self.core.clone();
        let provider_str = provider.to_string();
        let actor = p.actor().to_string();
        let backup_path = backup.path.clone();
        let server_id = backup.server_id;
        let op_id_clone = op_id.clone();

        tokio::spawn(async move {
            core.cloud.set_operation_state(
                &op_id_clone,
                mcpanel_core::cloud::CloudOperationState::Uploading,
                None,
            );

            match core
                .cloud
                .upload_file(&provider_str, &file_name, &backup_path, Some(transfer_opts))
                .await
            {
                Ok(meta) => {
                    core.cloud.set_operation_state(
                        &op_id_clone,
                        mcpanel_core::cloud::CloudOperationState::Completed,
                        None,
                    );
                    core.audit
                        .record(
                            &actor,
                            "cloud.backup_upload",
                            server_id,
                            Some(bid.to_string()),
                            mcpanel_core::model::AuditResult::Success,
                            serde_json::json!({ "provider": provider_str, "fileId": meta.id }),
                        )
                        .await;
                }
                Err(e) => {
                    if e.code == mcpanel_core::error::ErrorCode::Cancelled {
                        core.cloud.set_operation_state(
                            &op_id_clone,
                            mcpanel_core::cloud::CloudOperationState::Cancelled,
                            None,
                        );
                    } else {
                        let msg = e.to_string();
                        core.cloud.set_operation_state(
                            &op_id_clone,
                            mcpanel_core::cloud::CloudOperationState::Failed,
                            Some(msg.clone()),
                        );
                        core.audit
                            .record(
                                &actor,
                                "cloud.backup_upload",
                                server_id,
                                Some(bid.to_string()),
                                mcpanel_core::model::AuditResult::Failure,
                                serde_json::json!({ "provider": provider_str, "error": msg }),
                            )
                            .await;
                    }
                }
            }
        });

        Ok(CloudOperationDto::from(&snapshot))
    }

    pub async fn cloud_backup_download(
        &self,
        p: &Principal,
        file_id: &str,
        provider: &str,
        file_name: &str,
        server: Option<String>,
    ) -> ApiResult<CloudOperationDto> {
        p.authorize(Permission::BackupsManage)?;
        let backups_dir = self.core.backups.backups_dir().await?;
        let sid = server.as_deref().map(server_id).transpose()?;
        let safe_name = std::path::Path::new(file_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("downloaded-backup.zip")
            .to_string();
        let dest_path = backups_dir.join(&safe_name);

        if dest_path.exists() {
            return Err(ApiError::new(
                "CONFLICT",
                format!("Backup file '{safe_name}' already exists locally"),
            ));
        }

        let op_id = uuid::Uuid::new_v4().to_string();
        let (snapshot, cancel_token) = self.core.cloud.start_operation(
            op_id.clone(),
            provider.to_string(),
            Some(file_id.to_string()),
            safe_name.clone(),
            mcpanel_core::cloud::CloudOperationType::Download,
            None,
        );

        let cloud_clone = self.core.cloud.clone();
        let op_id_prog = op_id.clone();
        let on_progress: mcpanel_core::cloud::CloudProgressFn =
            std::sync::Arc::new(move |completed, total| {
                cloud_clone.update_operation_progress(&op_id_prog, completed, total);
            });
        let transfer_opts = mcpanel_core::cloud::CloudTransferOptions {
            on_progress: Some(on_progress),
            cancel: cancel_token,
        };

        let core = self.core.clone();
        let provider_str = provider.to_string();
        let file_id_str = file_id.to_string();
        let actor = p.actor().to_string();
        let op_id_clone = op_id.clone();

        tokio::spawn(async move {
            core.cloud.set_operation_state(
                &op_id_clone,
                mcpanel_core::cloud::CloudOperationState::Downloading,
                None,
            );

            match core
                .cloud
                .download_file(&provider_str, &file_id_str, &dest_path, Some(transfer_opts))
                .await
            {
                Ok(_) => {
                    match core
                        .backups
                        .register_downloaded_backup(&dest_path, sid)
                        .await
                    {
                        Ok(record) => {
                            core.cloud.set_operation_state(
                                &op_id_clone,
                                mcpanel_core::cloud::CloudOperationState::Completed,
                                None,
                            );
                            core.audit
                                .record(
                                    &actor,
                                    "cloud.backup_download",
                                    record.server_id,
                                    Some(record.id.to_string()),
                                    mcpanel_core::model::AuditResult::Success,
                                    serde_json::json!({ "provider": provider_str, "fileId": file_id_str }),
                                )
                                .await;
                        }
                        Err(e) => {
                            let msg = format!("Failed to register downloaded backup: {e}");
                            core.cloud.set_operation_state(
                                &op_id_clone,
                                mcpanel_core::cloud::CloudOperationState::Failed,
                                Some(msg),
                            );
                        }
                    }
                }
                Err(e) => {
                    if e.code == mcpanel_core::error::ErrorCode::Cancelled {
                        core.cloud.set_operation_state(
                            &op_id_clone,
                            mcpanel_core::cloud::CloudOperationState::Cancelled,
                            None,
                        );
                    } else {
                        let msg = e.to_string();
                        core.cloud.set_operation_state(
                            &op_id_clone,
                            mcpanel_core::cloud::CloudOperationState::Failed,
                            Some(msg.clone()),
                        );
                        core.audit
                            .record(
                                &actor,
                                "cloud.backup_download",
                                sid,
                                None,
                                mcpanel_core::model::AuditResult::Failure,
                                serde_json::json!({ "provider": provider_str, "error": msg }),
                            )
                            .await;
                    }
                }
            }
        });

        Ok(CloudOperationDto::from(&snapshot))
    }

    pub fn cloud_operations_list(&self, p: &Principal) -> ApiResult<Vec<CloudOperationDto>> {
        p.authorize(Permission::BackupsRead)?;
        let ops = self.core.cloud.list_operations();
        Ok(ops.iter().map(CloudOperationDto::from).collect())
    }

    pub fn cloud_operation_get(&self, p: &Principal, op_id: &str) -> ApiResult<CloudOperationDto> {
        p.authorize(Permission::BackupsRead)?;
        let op = self
            .core
            .cloud
            .get_operation(op_id)
            .ok_or_else(|| ApiError::new("NOT_FOUND", "Cloud operation not found"))?;
        Ok(CloudOperationDto::from(&op))
    }

    pub fn cloud_operation_cancel(&self, p: &Principal, op_id: &str) -> ApiResult<()> {
        p.authorize(Permission::BackupsManage)?;
        self.core.cloud.cancel_operation(op_id);
        Ok(())
    }

    pub async fn cloud_file_delete(
        &self,
        p: &Principal,
        file_id: &str,
        provider: &str,
    ) -> ApiResult<()> {
        p.authorize(Permission::BackupsManage)?;
        self.core.cloud.delete_file(provider, file_id).await?;
        self.core
            .audit
            .record(
                p.actor(),
                "cloud.file_delete",
                None,
                Some(file_id.to_string()),
                mcpanel_core::model::AuditResult::Success,
                serde_json::json!({ "provider": provider }),
            )
            .await;
        Ok(())
    }

    // ───────────────────────────── encryption ─────────────────────────────

    pub async fn encryption_status(&self, p: &Principal) -> ApiResult<EncryptionStatusDto> {
        p.authorize(Permission::BackupsRead)?;
        Ok(self.core.encryption.status().await?.into())
    }

    /// Create the Backup Master Key; the Recovery Kit is written to the save target.
    pub async fn encryption_setup(
        &self,
        p: &Principal,
        passphrase: String,
        kit_grant: &str,
    ) -> ApiResult<EncryptionStatusDto> {
        p.authorize(Permission::BackupsManage)?;
        let passphrase = secrecy::SecretString::from(passphrase);
        let dest = self.grants.take(kit_grant, GrantKind::SaveTarget)?;
        let status = self.core.encryption.setup(passphrase, &dest).await?;
        self.core
            .audit
            .record(
                p.actor(),
                "encryption.setup",
                None,
                status.recipient.clone(),
                mcpanel_core::model::AuditResult::Success,
                serde_json::json!({}),
            )
            .await;
        Ok(status.into())
    }

    pub async fn encryption_import(
        &self,
        p: &Principal,
        kit_grant: &str,
        passphrase: String,
    ) -> ApiResult<EncryptionStatusDto> {
        p.authorize(Permission::BackupsManage)?;
        let passphrase = secrecy::SecretString::from(passphrase);
        let path = self.grants.take(kit_grant, GrantKind::Source)?;
        let status = self.core.encryption.import(&path, passphrase).await?;
        self.core
            .audit
            .record(
                p.actor(),
                "encryption.import",
                None,
                status.recipient.clone(),
                mcpanel_core::model::AuditResult::Success,
                serde_json::json!({}),
            )
            .await;
        Ok(status.into())
    }

    pub async fn encryption_set_enabled(
        &self,
        p: &Principal,
        enabled: bool,
    ) -> ApiResult<EncryptionStatusDto> {
        p.authorize(Permission::BackupsManage)?;
        Ok(self
            .core
            .encryption
            .set_encrypt_backups(enabled)
            .await?
            .into())
    }

    // ──────────────────────────── notifications ────────────────────────────

    pub async fn notifications_list(
        &self,
        p: &Principal,
        limit: u32,
    ) -> ApiResult<Vec<NotificationDto>> {
        p.authorize(Permission::ActivityRead)?;
        Ok(self
            .core
            .notifications
            .list(limit)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn notifications_unread(&self, p: &Principal) -> ApiResult<u32> {
        p.authorize(Permission::ActivityRead)?;
        Ok(self.core.notifications.unread_count().await?)
    }

    /// `ids = None` marks everything read.
    pub async fn notifications_mark_read(
        &self,
        p: &Principal,
        ids: Option<Vec<String>>,
    ) -> ApiResult<()> {
        p.authorize(Permission::ActivityRead)?;
        Ok(self.core.notifications.mark_read(ids).await?)
    }

    pub async fn notifications_clear(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::ActivityRead)?;
        Ok(self.core.notifications.clear().await?)
    }

    pub async fn notification_prefs(&self, p: &Principal) -> ApiResult<NotificationPrefsDto> {
        p.authorize(Permission::ActivityRead)?;
        Ok(self.core.notifications.prefs().await?.into())
    }

    pub async fn notification_prefs_update(
        &self,
        p: &Principal,
        prefs: NotificationPrefsDto,
    ) -> ApiResult<NotificationPrefsDto> {
        p.authorize(Permission::SettingsWrite)?;
        Ok(self
            .core
            .notifications
            .set_prefs(prefs.into())
            .await?
            .into())
    }

    // ───────────────────────────── tunnels ─────────────────────────────

    pub async fn tunnel_status(&self, p: &Principal) -> ApiResult<TunnelStatusDto> {
        p.authorize(Permission::SystemRead)?;
        Ok(self.core.tunnels.status().await?.into())
    }

    pub async fn tunnel_install_agent(&self, p: &Principal) -> ApiResult<TunnelStatusDto> {
        p.authorize(Permission::SettingsWrite)?;
        let status = self.core.tunnels.install_agent().await?;
        self.audit_tunnel(p, "tunnel.install_agent", None).await;
        Ok(status.into())
    }

    async fn audit_tunnel(
        &self,
        p: &Principal,
        action: &str,
        server: Option<mcpanel_core::ids::ServerId>,
    ) {
        self.core
            .audit
            .record(
                p.actor(),
                action,
                server,
                Some("playit".to_string()),
                mcpanel_core::model::AuditResult::Success,
                serde_json::json!({}),
            )
            .await;
    }

    // ── accounts ──

    async fn audit_account(&self, p: &Principal, action: &str) {
        self.core
            .audit
            .record(
                p.actor(),
                action,
                None,
                None,
                mcpanel_core::model::AuditResult::Success,
                serde_json::json!({}),
            )
            .await;
    }

    pub async fn account_status(&self, p: &Principal) -> ApiResult<AccountDto> {
        p.authorize(Permission::SystemRead)?;
        Ok(self.core.account.status().await?.into())
    }

    pub async fn account_sign_up(
        &self,
        p: &Principal,
        email: &str,
        password: &str,
        display_name: Option<&str>,
    ) -> ApiResult<AccountDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core
            .account
            .sign_up(email, password, display_name)
            .await?;
        self.audit_account(p, "account.sign_up").await;
        Ok(self.core.account.status().await?.into())
    }

    pub async fn account_sign_in(
        &self,
        p: &Principal,
        email: &str,
        password: &str,
    ) -> ApiResult<AccountDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.account.sign_in(email, password).await?;
        self.audit_account(p, "account.sign_in").await;
        Ok(self.core.account.status().await?.into())
    }

    /// Start "Continue with Google"; returns Google's consent page.
    pub async fn account_google(&self, p: &Principal) -> ApiResult<String> {
        p.authorize(Permission::SettingsWrite)?;
        Ok(self.core.account.begin_google().await?)
    }

    pub fn account_cancel_google(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.account.cancel_google();
        Ok(())
    }

    /// Start "Continue with Microsoft"; returns Microsoft's consent page.
    pub async fn account_microsoft(&self, p: &Principal) -> ApiResult<String> {
        p.authorize(Permission::SettingsWrite)?;
        Ok(self.core.account.begin_microsoft().await?)
    }

    pub fn account_cancel_microsoft(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.account.cancel_microsoft();
        Ok(())
    }

    pub async fn account_guest_sign_in(&self, p: &Principal) -> ApiResult<AccountDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.account.enter_guest_mode().await?;
        self.audit_account(p, "account.guest_sign_in").await;
        Ok(self.core.account.status().await?.into())
    }

    pub async fn account_sync_settings(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.account.sync_settings().await?;
        self.audit_account(p, "account.sync_settings").await;
        Ok(())
    }

    pub async fn account_sign_out(&self, p: &Principal) -> ApiResult<AccountDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.account.sign_out().await?;
        self.audit_account(p, "account.sign_out").await;
        Ok(self.core.account.status().await?.into())
    }

    pub async fn account_refresh(&self, p: &Principal) -> ApiResult<AccountDto> {
        p.authorize(Permission::SystemRead)?;
        self.core.account.refresh_profile().await?;
        Ok(self.core.account.status().await?.into())
    }

    pub async fn account_resend_verification(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        Ok(self.core.account.resend_verification().await?)
    }

    pub async fn account_reset_password(&self, p: &Principal, email: &str) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        Ok(self.core.account.send_password_reset(email).await?)
    }

    pub async fn account_set_name(&self, p: &Principal, name: &str) -> ApiResult<AccountDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.account.set_display_name(name).await?;
        Ok(self.core.account.status().await?.into())
    }

    pub async fn account_configure_oauth(
        &self,
        p: &Principal,
        req: ConfigureOauthDto,
    ) -> ApiResult<AccountDto> {
        p.authorize(Permission::SettingsWrite)?;
        let google_secret = req.google_client_secret.map(secrecy::SecretString::from);
        let microsoft_secret = req.microsoft_client_secret.map(secrecy::SecretString::from);
        self.core
            .account
            .configure_credentials(
                req.firebase_api_key,
                req.google_client_id,
                google_secret,
                req.microsoft_client_id,
                microsoft_secret,
            )
            .await?;
        self.audit_account(p, "account.configure_oauth").await;
        Ok(self.core.account.status().await?.into())
    }

    /// Host hook: the Firebase backend.
    pub fn account_attach_backend(&self, backend: Arc<dyn mcpanel_core::account::AuthBackend>) {
        self.core.account.set_backend(backend);
    }

    /// Host hook: refresh the cached profile at start.
    pub async fn account_on_startup(&self) {
        self.core.account.on_startup().await;
    }

    // ── MCPanel's own playit agent ──

    pub async fn playit_status(&self, p: &Principal) -> ApiResult<PlayitAgentDto> {
        p.authorize(Permission::SystemRead)?;
        Ok(self.core.playit.status().await?.into())
    }

    pub async fn playit_install_agent(&self, p: &Principal) -> ApiResult<PlayitAgentDto> {
        p.authorize(Permission::SettingsWrite)?;
        let status = self.core.playit.install_agent().await?;
        self.audit_tunnel(p, "playit.install_agent", None).await;
        Ok(status.into())
    }

    /// Start linking MCPanel's agent; returns the playit.gg approval page.
    pub async fn playit_link(&self, p: &Principal) -> ApiResult<String> {
        p.authorize(Permission::SettingsWrite)?;
        let url = self.core.playit.begin_link().await?;
        self.audit_tunnel(p, "playit.link_start", None).await;
        Ok(url)
    }

    /// Forget MCPanel's current agent key and start a replacement approval flow.
    pub async fn playit_relink(&self, p: &Principal) -> ApiResult<String> {
        p.authorize(Permission::SettingsWrite)?;
        let url = self.core.playit.relink().await?;
        self.audit_tunnel(p, "playit.relink_start", None).await;
        Ok(url)
    }

    pub fn playit_cancel_link(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.cancel_link();
        Ok(())
    }

    pub async fn playit_unlink(&self, p: &Principal) -> ApiResult<PlayitAgentDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.unlink().await?;
        self.audit_tunnel(p, "playit.unlink", None).await;
        Ok(self.core.playit.status().await?.into())
    }

    pub async fn playit_start(&self, p: &Principal) -> ApiResult<PlayitAgentDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.start().await?;
        self.audit_tunnel(p, "playit.start", None).await;
        Ok(self.core.playit.status().await?.into())
    }

    pub async fn playit_stop(&self, p: &Principal) -> ApiResult<PlayitAgentDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.stop().await?;
        self.audit_tunnel(p, "playit.stop", None).await;
        Ok(self.core.playit.status().await?.into())
    }

    pub async fn playit_set_autostart(&self, p: &Principal, on: bool) -> ApiResult<PlayitAgentDto> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.set_autostart(on).await?;
        Ok(self.core.playit.status().await?.into())
    }

    pub async fn playit_tunnels(&self, p: &Principal) -> ApiResult<PlayitTunnelsDto> {
        p.authorize(Permission::SystemRead)?;
        Ok(self.core.playit.tunnels().await?.into())
    }

    pub async fn playit_create_tunnel(
        &self,
        p: &Principal,
        name: &str,
        kind: &str,
        port: u16,
    ) -> ApiResult<String> {
        p.authorize(Permission::SettingsWrite)?;
        let kind = match kind {
            "minecraft-java" => mcpanel_core::playit_api::PlayitTunnelKind::MinecraftJava,
            "minecraft-bedrock" => mcpanel_core::playit_api::PlayitTunnelKind::MinecraftBedrock,
            _ => return Err(ApiError::invalid("Unknown tunnel type")),
        };
        let id = self.core.playit.create_tunnel(name, kind, port).await?;
        self.audit_tunnel(p, "playit.tunnel_create", None).await;
        Ok(id)
    }

    pub async fn playit_rename_tunnel(&self, p: &Principal, id: &str, name: &str) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.rename_tunnel(id, name).await?;
        self.audit_tunnel(p, "playit.tunnel_rename", None).await;
        Ok(())
    }

    pub async fn playit_set_tunnel_port(
        &self,
        p: &Principal,
        id: &str,
        port: u16,
    ) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.set_tunnel_port(id, port).await?;
        self.audit_tunnel(p, "playit.tunnel_port", None).await;
        Ok(())
    }

    pub async fn playit_set_tunnel_enabled(
        &self,
        p: &Principal,
        id: &str,
        enabled: bool,
    ) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.set_tunnel_enabled(id, enabled).await?;
        let action = if enabled {
            "playit.tunnel_enable"
        } else {
            "playit.tunnel_disable"
        };
        self.audit_tunnel(p, action, None).await;
        Ok(())
    }

    pub async fn playit_delete_tunnel(&self, p: &Principal, id: &str) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.playit.delete_tunnel(id).await?;
        self.audit_tunnel(p, "playit.tunnel_delete", None).await;
        Ok(())
    }

    /// Host hook: the playit.gg web API client.
    pub fn playit_attach_api(&self, api: Arc<dyn mcpanel_core::playit_api::PlayitApi>) {
        self.core.playit.set_api(api);
    }

    /// Host hook: start the agent when it is linked and set to start with MCPanel.
    pub async fn playit_on_startup(&self) {
        self.core.playit.on_startup().await;
    }

    /// Host hook: stop the agent when MCPanel exits.
    pub async fn playit_shutdown(&self) {
        let _ = self.core.playit.stop().await;
    }

    /// Start the playit agent (publishes all of its tunnels).
    pub async fn tunnel_start_agent(&self, p: &Principal) -> ApiResult<TunnelStatusDto> {
        p.authorize(Permission::SettingsWrite)?;
        let status = self.core.tunnels.start_agent().await?;
        self.audit_tunnel(p, "tunnel.agent_start", None).await;
        Ok(status.into())
    }

    pub async fn tunnel_stop_agent(&self, p: &Principal) -> ApiResult<TunnelStatusDto> {
        p.authorize(Permission::SettingsWrite)?;
        let status = self.core.tunnels.stop_agent().await?;
        self.audit_tunnel(p, "tunnel.agent_stop", None).await;
        Ok(status.into())
    }

    /// Begin linking the agent to a playit.gg account; returns the approval URL.
    pub async fn tunnel_begin_link(&self, p: &Principal) -> ApiResult<String> {
        p.authorize(Permission::SettingsWrite)?;
        let url = self.core.tunnels.begin_link().await?;
        self.audit_tunnel(p, "tunnel.link_start", None).await;
        Ok(url)
    }

    pub fn tunnel_cancel_link(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::SettingsWrite)?;
        self.core.tunnels.cancel_link();
        Ok(())
    }

    pub async fn tunnel_server_address(
        &self,
        p: &Principal,
        server: &str,
    ) -> ApiResult<Option<String>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self.core.tunnels.server_address(server_id(server)?).await?)
    }

    pub async fn tunnel_set_server_address(
        &self,
        p: &Principal,
        server: &str,
        address: &str,
    ) -> ApiResult<Option<String>> {
        p.authorize(Permission::ServersManage)?;
        let id = server_id(server)?;
        let saved = self.core.tunnels.set_server_address(id, address).await?;
        self.audit_tunnel(p, "tunnel.address_set", Some(id)).await;
        Ok(saved)
    }

    // ───────────────────────────── bedrock ─────────────────────────────

    pub async fn bedrock_status(&self, p: &Principal, server: &str) -> ApiResult<BedrockStatusDto> {
        p.authorize(Permission::ServersRead)?;
        Ok(self.core.bedrock.status(server_id(server)?).await?.into())
    }

    /// Returns the id of the setup job.
    pub async fn bedrock_enable(
        &self,
        p: &Principal,
        server: &str,
        req: BedrockEnableDto,
    ) -> ApiResult<String> {
        p.authorize(Permission::ContentManage)?;
        Ok(self
            .core
            .bedrock
            .enable(
                server_id(server)?,
                EnableRequest {
                    floodgate: req.floodgate,
                    via_version: req.via_version,
                    port: req.port,
                },
                p.actor(),
            )
            .await?
            .to_string())
    }

    pub async fn bedrock_configure(
        &self,
        p: &Principal,
        server: &str,
        settings: BedrockSettingsDto,
    ) -> ApiResult<BedrockStatusDto> {
        p.authorize(Permission::ContentManage)?;
        Ok(self
            .core
            .bedrock
            .configure(server_id(server)?, settings.try_into()?, p.actor())
            .await?
            .into())
    }

    pub async fn bedrock_ping(&self, p: &Principal, server: &str) -> ApiResult<BedrockPongDto> {
        p.authorize(Permission::ServersRead)?;
        Ok(self.core.bedrock.ping(server_id(server)?).await?.into())
    }

    // ───────────────────────────── content ─────────────────────────────

    pub async fn content_list(&self, p: &Principal, server: &str) -> ApiResult<ContentListDto> {
        p.authorize(Permission::ServersRead)?;
        let id = server_id(server)?;
        let list = self.core.content.list(id).await?;
        let pending = self.core.content.pending(id).await?;
        let s = self.core.servers.get(id).await?;
        let target = self.core.content.target(&s)?;
        let providers = self
            .core
            .content
            .providers(&target)
            .iter()
            .map(|p| p.info().clone())
            .collect();
        Ok(ContentListDto::new(list, pending, providers))
    }

    pub async fn content_search(
        &self,
        p: &Principal,
        server: &str,
        q: SearchRequestDto,
    ) -> ApiResult<SearchPageDto> {
        p.authorize(Permission::ServersRead)?;
        let sort = match q.sort.as_str() {
            "downloads" => SearchSort::Downloads,
            "updated" => SearchSort::Updated,
            _ => SearchSort::Relevance,
        };
        let page = self
            .core
            .content
            .search(
                server_id(server)?,
                &q.provider,
                &q.text,
                sort,
                q.offset,
                q.limit,
            )
            .await?;
        Ok(SearchPageDto {
            total: page.total,
            hits: page.hits.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn content_versions(
        &self,
        p: &Principal,
        server: &str,
        provider: &str,
        project_id: &str,
    ) -> ApiResult<Vec<ContentVersionDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .content
            .versions(server_id(server)?, provider, project_id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn content_recommendations(
        &self,
        p: &Principal,
        server: &str,
    ) -> ApiResult<Vec<PluginRecommendationDto>> {
        p.authorize(Permission::ServersRead)?;
        let id = server_id(server)?;
        let recs = self.core.content.recommendations(id).await?;
        Ok(recs.into_iter().map(Into::into).collect())
    }

    pub async fn content_identify(
        &self,
        p: &Principal,
        server: &str,
        req: IdentifyContentRequestDto,
    ) -> ApiResult<()> {
        p.authorize(Permission::ContentManage)?;
        let id = server_id(server)?;
        self.core
            .content
            .identify(
                id,
                &req.file_name,
                &req.provider,
                &req.project_id,
                req.version_number,
                req.name,
            )
            .await?;
        Ok(())
    }

    pub async fn content_plan(
        &self,
        p: &Principal,
        server: &str,
        req: InstallRequestDto,
    ) -> ApiResult<InstallPlanDto> {
        p.authorize(Permission::ContentManage)?;
        let plan = self
            .core
            .content
            .plan(server_id(server)?, &install_request(req))
            .await?;
        Ok(InstallPlanDto {
            items: plan
                .items
                .into_iter()
                .map(|i| PlannedInstallDto {
                    project: i.project.into(),
                    version: i.version.into(),
                    replaces: i.replaces,
                    required_by: i.required_by,
                })
                .collect(),
            unresolved: plan.unresolved,
            warnings: plan.warnings,
            deferred: plan.deferred,
        })
    }

    /// Returns the id of the install job.
    pub async fn content_install(
        &self,
        p: &Principal,
        server: &str,
        req: InstallRequestDto,
    ) -> ApiResult<String> {
        p.authorize(Permission::ContentManage)?;
        Ok(self
            .core
            .content
            .install(server_id(server)?, install_request(req), p.actor())
            .await?
            .to_string())
    }

    /// Returns whether the change was queued until the server stops.
    pub async fn content_remove(
        &self,
        p: &Principal,
        server: &str,
        file_name: &str,
    ) -> ApiResult<bool> {
        p.authorize(Permission::ContentManage)?;
        Ok(self
            .core
            .content
            .remove(server_id(server)?, file_name, p.actor())
            .await?)
    }

    pub async fn content_set_enabled(
        &self,
        p: &Principal,
        server: &str,
        file_name: &str,
        enabled: bool,
    ) -> ApiResult<bool> {
        p.authorize(Permission::ContentManage)?;
        Ok(self
            .core
            .content
            .set_enabled(server_id(server)?, file_name, enabled, p.actor())
            .await?)
    }

    pub async fn content_discard_pending(
        &self,
        p: &Principal,
        server: &str,
        id: &str,
    ) -> ApiResult<()> {
        p.authorize(Permission::ContentManage)?;
        Ok(self
            .core
            .content
            .discard_pending(server_id(server)?, id, p.actor())
            .await?)
    }

    pub async fn content_check_updates(
        &self,
        p: &Principal,
        server: &str,
    ) -> ApiResult<Vec<UpdateInfoDto>> {
        p.authorize(Permission::ServersRead)?;
        Ok(self
            .core
            .content
            .check_updates(server_id(server)?)
            .await?
            .into_iter()
            .map(|u| UpdateInfoDto {
                file_name: u.file_name,
                name: u.name,
                current_version: u.current_version,
                latest: u.latest.into(),
            })
            .collect())
    }

    // ───────────────────────────── players ─────────────────────────────

    pub async fn players_get(&self, p: &Principal, server: &str) -> ApiResult<ServerPlayersDto> {
        p.authorize(Permission::ServersRead)?;
        Ok(self.core.players.view(server_id(server)?).await?.into())
    }

    pub async fn players_action(
        &self,
        p: &Principal,
        server: &str,
        action: PlayerActionDto,
    ) -> ApiResult<PlayerActionOutcomeDto> {
        p.authorize(Permission::PlayersManage)?;
        Ok(self
            .core
            .players
            .apply(server_id(server)?, action.into(), p.actor())
            .await?
            .into())
    }

    // ───────────────────────────── backups ─────────────────────────────

    pub async fn backups_list(
        &self,
        p: &Principal,
        server: Option<&str>,
    ) -> ApiResult<Vec<BackupDto>> {
        p.authorize(Permission::BackupsRead)?;
        let server = server.map(server_id).transpose()?;
        Ok(self
            .core
            .backups
            .list(server)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Returns the id of the backup job.
    pub async fn backups_create(
        &self,
        p: &Principal,
        server: &str,
        note: Option<String>,
    ) -> ApiResult<String> {
        p.authorize(Permission::BackupsManage)?;
        let job = self
            .core
            .backups
            .create(
                CreateBackupRequest {
                    server_id: server_id(server)?,
                    note,
                },
                p.actor(),
            )
            .await?;
        Ok(job.to_string())
    }

    pub async fn backups_delete(&self, p: &Principal, id: &str) -> ApiResult<()> {
        p.authorize(Permission::BackupsManage)?;
        Ok(self.core.backups.delete(backup_id(id)?, p.actor()).await?)
    }

    /// Returns the id of the verification job (its result is a verify report).
    pub async fn backups_verify(&self, p: &Principal, id: &str) -> ApiResult<String> {
        p.authorize(Permission::BackupsRead)?;
        Ok(self
            .core
            .backups
            .verify(backup_id(id)?, p.actor())
            .await?
            .to_string())
    }

    pub async fn backups_restore_preview(
        &self,
        p: &Principal,
        id: &str,
    ) -> ApiResult<RestorePreviewDto> {
        p.authorize(Permission::BackupsRestore)?;
        Ok(self
            .core
            .backups
            .restore_preview(backup_id(id)?)
            .await?
            .into())
    }

    /// Returns the id of the restore job.
    pub async fn backups_restore(&self, p: &Principal, id: &str) -> ApiResult<String> {
        p.authorize(Permission::BackupsRestore)?;
        Ok(self
            .core
            .backups
            .restore(backup_id(id)?, p.actor())
            .await?
            .to_string())
    }

    pub async fn backups_policy(&self, p: &Principal, server: &str) -> ApiResult<BackupPolicyDto> {
        p.authorize(Permission::BackupsRead)?;
        Ok(self.core.backups.policy(server_id(server)?).await?.into())
    }

    pub async fn backups_policy_update(
        &self,
        p: &Principal,
        server: &str,
        u: BackupPolicyUpdateDto,
    ) -> ApiResult<BackupPolicyDto> {
        p.authorize(Permission::BackupsManage)?;
        let server_id = server_id(server)?;
        let current = self.core.backups.policy(server_id).await?;
        Ok(self
            .core
            .backups
            .set_policy(
                BackupPolicy {
                    server_id,
                    enabled: u.enabled,
                    interval_minutes: u.interval_minutes,
                    skip_if_idle: u.skip_if_idle,
                    retention: Retention {
                        keep_last: u.keep_last,
                        keep_daily: u.keep_daily,
                        keep_weekly: u.keep_weekly,
                        keep_monthly: u.keep_monthly,
                    },
                    max_total_gb: u.max_total_gb,
                    last_run_at: current.last_run_at,
                },
                p.actor(),
            )
            .await?
            .into())
    }

    pub async fn backups_location(&self, p: &Principal) -> ApiResult<BackupLocationDto> {
        p.authorize(Permission::BackupsRead)?;
        let dir = self.core.backups.backups_dir().await?;
        let default = &self.core.paths.default_backups_dir;
        Ok(BackupLocationDto {
            is_default: mcpanel_core::files::fsx::path_starts_with_ci(&dir, default)
                && mcpanel_core::files::fsx::path_starts_with_ci(default, &dir),
            available_bytes: self
                .core
                .platform
                .disk_space(&dir)
                .ok()
                .map(|d| d.available_bytes),
            warnings: warnings(&self.core.platform.location_warnings(&dir)),
            directory: dir.to_string_lossy().into(),
            default_directory: default.to_string_lossy().into(),
        })
    }

    /// Use the folder the user picked (`None` = back to the default).
    pub async fn backups_set_location(
        &self,
        p: &Principal,
        grant: Option<&str>,
    ) -> ApiResult<BackupLocationDto> {
        p.authorize(Permission::SettingsWrite)?;
        let dir = grant
            .map(|g| self.grants.take(g, GrantKind::Directory))
            .transpose()?;
        self.core.backups.set_backups_dir(dir, p.actor()).await?;
        self.backups_location(p).await
    }

    /// The archive path of a backup (for revealing it in Explorer).
    pub async fn backups_path(&self, p: &Principal, id: &str) -> ApiResult<std::path::PathBuf> {
        p.authorize(Permission::BackupsRead)?;
        Ok(self.core.backups.get(backup_id(id)?).await?.path)
    }

    // ───────────────────────────── jobs / audit ───────────────────────

    pub async fn jobs_list(&self, p: &Principal, limit: u32) -> ApiResult<Vec<JobDto>> {
        p.authorize(Permission::ActivityRead)?;
        Ok(self
            .core
            .jobs
            .recent(limit)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn jobs_get(&self, p: &Principal, id: &str) -> ApiResult<JobDto> {
        p.authorize(Permission::ActivityRead)?;
        let jid = JobId::from_str(id).map_err(|_| ApiError::invalid("Invalid job id"))?;
        self.core
            .jobs
            .get(jid)
            .await?
            .map(Into::into)
            .ok_or_else(|| ApiError::new("NOT_FOUND", "Job not found"))
    }

    pub fn jobs_cancel(&self, p: &Principal, id: &str) -> ApiResult<bool> {
        p.authorize(Permission::ServersManage)?;
        let jid = JobId::from_str(id).map_err(|_| ApiError::invalid("Invalid job id"))?;
        Ok(self.core.jobs.cancel(jid))
    }

    pub async fn audit_query(
        &self,
        p: &Principal,
        server: Option<&str>,
        before: Option<i64>,
        limit: u32,
    ) -> ApiResult<Vec<AuditEntryDto>> {
        p.authorize(Permission::ActivityRead)?;
        Ok(self
            .core
            .audit
            .query(&AuditQuery {
                server_id: server.map(server_id).transpose()?,
                before: before.map(Timestamp),
                limit,
            })
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    // ──────────────────────────── multihost ───────────────────────────

    pub async fn multihost_status(&self, p: &Principal) -> ApiResult<MultihostStatusDto> {
        p.authorize(Permission::MultihostRead)?;
        let status = self.core.multihost.status().await?;
        Ok(MultihostStatusDto {
            account_required: status.account_required,
            signed_in: status.signed_in,
            user_email: status.user_email,
            hosts: status.hosts.iter().map(Into::into).collect(),
        })
    }

    pub async fn multihost_list_hosts(&self, p: &Principal) -> ApiResult<Vec<HostDto>> {
        p.authorize(Permission::MultihostRead)?;
        let hosts = self.core.multihost.list_hosts().await?;
        Ok(hosts.iter().map(Into::into).collect())
    }

    pub async fn multihost_add_host(&self, p: &Principal, body: AddHostDto) -> ApiResult<HostDto> {
        p.authorize(Permission::MultihostManage)?;
        let host = self
            .core
            .multihost
            .add_host(
                body.name,
                body.endpoint,
                body.auth_token,
                body.tags,
                p.actor(),
            )
            .await?;
        Ok((&host).into())
    }

    pub async fn multihost_remove_host(&self, p: &Principal, host_id: &str) -> ApiResult<()> {
        p.authorize(Permission::MultihostManage)?;
        self.core.multihost.remove_host(host_id, p.actor()).await?;
        Ok(())
    }

    pub async fn multihost_ping_host(
        &self,
        p: &Principal,
        host_id: &str,
    ) -> ApiResult<HostPingResultDto> {
        p.authorize(Permission::MultihostRead)?;
        let res = self.core.multihost.ping_host(host_id).await?;
        Ok(HostPingResultDto {
            host_id: res.host_id,
            online: res.online,
            latency_ms: res.latency_ms,
            message: res.message,
        })
    }

    pub async fn multihost_generate_token(
        &self,
        p: &Principal,
    ) -> ApiResult<HostEnrollmentTokenDto> {
        p.authorize(Permission::MultihostManage)?;
        let token = self.core.multihost.generate_enrollment_token().await?;
        Ok(HostEnrollmentTokenDto {
            token: token.token,
            account_uid: token.account_uid,
            expires_at: token.expires_at,
            pairing_command: token.pairing_command,
        })
    }

    // ───────────────────────────── AI Assistant ───────────────────────────

    pub async fn ai_get_config(&self, p: &Principal) -> ApiResult<AiConfigDto> {
        p.authorize(Permission::AiUse)?;
        let c = self.core.ai.get_config().await?;
        Ok(AiConfigDto {
            configured: c.configured,
            provider: c.provider,
            model: c.model,
            base_url: c.base_url,
        })
    }

    pub async fn ai_save_config(
        &self,
        p: &Principal,
        patch: AiConfigPatchDto,
    ) -> ApiResult<AiConfigDto> {
        p.authorize(Permission::AiManage)?;
        let c = self
            .core
            .ai
            .save_config(mcpanel_core::ai::AiConfigPatch {
                api_key: patch.api_key,
                provider: patch.provider,
                model: patch.model,
                base_url: patch.base_url,
            })
            .await?;
        self.core
            .audit
            .record(
                p.actor(),
                "ai.configure",
                None,
                Some(c.provider.clone()),
                mcpanel_core::model::AuditResult::Success,
                serde_json::json!({ "provider": c.provider, "model": c.model }),
            )
            .await;
        Ok(AiConfigDto {
            configured: c.configured,
            provider: c.provider,
            model: c.model,
            base_url: c.base_url,
        })
    }

    pub async fn ai_test_connection(&self, p: &Principal) -> ApiResult<()> {
        p.authorize(Permission::AiManage)?;
        self.core.ai.test_connection().await?;
        Ok(())
    }

    pub async fn ai_chat(
        &self,
        p: &Principal,
        req: AiChatRequestDto,
    ) -> ApiResult<AiChatResponseDto> {
        p.authorize(Permission::AiUse)?;
        let msgs = req
            .messages
            .into_iter()
            .map(|m| mcpanel_core::ai::AiChatMessage {
                id: m.id,
                role: m.role,
                content: m.content,
                timestamp: m.timestamp,
                pending_confirmation: m.pending_confirmation.map(|c| {
                    mcpanel_core::ai::AiPendingConfirmation {
                        confirmation_id: c.confirmation_id,
                        tool: c.tool,
                        title: c.title,
                        description: c.description,
                        params: c.params,
                    }
                }),
                tool_executions: m.tool_executions.map(|te| {
                    te.into_iter()
                        .map(|e| mcpanel_core::ai::AiToolExecution {
                            tool: e.tool,
                            description: e.description,
                            is_major: e.is_major,
                            result: e.result,
                            error: e.error,
                        })
                        .collect()
                }),
            })
            .collect();

        let resp = self.core.ai.chat(msgs, req.server_id).await?;

        Ok(AiChatResponseDto {
            message: AiChatMessageDto {
                id: resp.id,
                role: resp.role,
                content: resp.content,
                timestamp: resp.timestamp,
                pending_confirmation: resp.pending_confirmation.map(|c| AiPendingConfirmationDto {
                    confirmation_id: c.confirmation_id,
                    tool: c.tool,
                    title: c.title,
                    description: c.description,
                    params: c.params,
                }),
                tool_executions: resp.tool_executions.map(|te| {
                    te.into_iter()
                        .map(|e| AiToolExecutionDto {
                            tool: e.tool,
                            description: e.description,
                            is_major: e.is_major,
                            result: e.result,
                            error: e.error,
                        })
                        .collect()
                }),
            },
        })
    }

    pub async fn ai_confirm_action(
        &self,
        p: &Principal,
        req: AiConfirmRequestDto,
    ) -> ApiResult<AiChatResponseDto> {
        p.authorize(Permission::AiUse)?;
        let msgs = req
            .messages
            .into_iter()
            .map(|m| mcpanel_core::ai::AiChatMessage {
                id: m.id,
                role: m.role,
                content: m.content,
                timestamp: m.timestamp,
                pending_confirmation: m.pending_confirmation.map(|c| {
                    mcpanel_core::ai::AiPendingConfirmation {
                        confirmation_id: c.confirmation_id,
                        tool: c.tool,
                        title: c.title,
                        description: c.description,
                        params: c.params,
                    }
                }),
                tool_executions: m.tool_executions.map(|te| {
                    te.into_iter()
                        .map(|e| mcpanel_core::ai::AiToolExecution {
                            tool: e.tool,
                            description: e.description,
                            is_major: e.is_major,
                            result: e.result,
                            error: e.error,
                        })
                        .collect()
                }),
            })
            .collect();

        let resp = self
            .core
            .ai
            .confirm_action(&req.confirmation_id, req.approved, msgs, req.server_id)
            .await?;

        Ok(AiChatResponseDto {
            message: AiChatMessageDto {
                id: resp.id,
                role: resp.role,
                content: resp.content,
                timestamp: resp.timestamp,
                pending_confirmation: resp.pending_confirmation.map(|c| AiPendingConfirmationDto {
                    confirmation_id: c.confirmation_id,
                    tool: c.tool,
                    title: c.title,
                    description: c.description,
                    params: c.params,
                }),
                tool_executions: resp.tool_executions.map(|te| {
                    te.into_iter()
                        .map(|e| AiToolExecutionDto {
                            tool: e.tool,
                            description: e.description,
                            is_major: e.is_major,
                            result: e.result,
                            error: e.error,
                        })
                        .collect()
                }),
            },
        })
    }
}
