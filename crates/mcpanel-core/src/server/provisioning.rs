//! Creating, importing, updating and deleting servers.

use super::manager::{ServerManager, eula_accepted_text, read_root_file};
use super::runtime::Operation;
use crate::config::properties::{decode_bytes, validate_key};
use crate::config::{PropertiesDocument, schema};
use crate::console::ConsoleStream;
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::DomainEvent;
use crate::files::{SafeRoot, fsx};
use crate::ids::{JavaRuntimeId, ServerId};
use crate::java::{JavaCompatibility, check_compatibility};
use crate::lifecycle::LifecycleState;
use crate::model::{AuditResult, InstalledSoftware, LaunchConfig, Server};
use crate::ports::LocationWarning;
use crate::software::executor::PlanExecutor;
use crate::software::{DetectedSoftware, GameVersion, InstallPlan, InstallRequest, SoftwareBuild};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateServerRequest {
    pub name: String,
    /// Parent directory (from a native-dialog grant). `None` = default servers folder.
    pub parent_directory: Option<PathBuf>,
    pub software_id: String,
    pub game_version: String,
    pub build: Option<String>,
    pub java_runtime_id: JavaRuntimeId,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
    /// Initial `server.properties` values.
    pub properties: Vec<(String, String)>,
    /// The user explicitly accepted the Minecraft EULA in the wizard.
    pub accept_eula: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportServerRequest {
    pub name: String,
    pub directory: PathBuf,
    pub software_id: Option<String>,
    pub game_version: Option<String>,
    pub jar: Option<String>,
    pub java_runtime_id: Option<JavaRuntimeId>,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateServerRequest {
    pub name: Option<String>,
    pub launch: Option<LaunchConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPreview {
    pub plan: InstallPlan,
    pub java: Vec<(JavaRuntimeId, JavaCompatibility)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationCheck {
    /// Final server directory that will be used.
    pub directory: PathBuf,
    pub warnings: Vec<LocationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportDetection {
    pub detected: Option<DetectedSoftware>,
    pub has_eula: bool,
    pub has_properties: bool,
    pub jars: Vec<String>,
    pub warnings: Vec<LocationWarning>,
}

pub fn validate_server_name(name: &str) -> CoreResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 64 {
        return Err(CoreError::invalid("Server name must be 1–64 characters"));
    }
    if n.chars().any(|c| c.is_control()) {
        return Err(CoreError::invalid(
            "Server name contains invalid characters",
        ));
    }
    Ok(n.to_string())
}

/// Folder name derived from a server name ("My SMP!" → "my-smp").
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out
        .trim_end_matches('-')
        .chars()
        .take(48)
        .collect::<String>();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() || crate::files::safepath::validate_name(&out).is_err() {
        "server".to_string()
    } else {
        out
    }
}

pub fn eula_file_contents(at: Timestamp) -> String {
    format!(
        "#By changing the setting below to TRUE you are indicating your agreement to our EULA (https://aka.ms/MinecraftEULA).\n#Accepted by the user in MCPanel at {} (unix ms)\neula=true\n",
        at.millis()
    )
}

impl ServerManager {
    fn check_location(
        &self,
        dir: &Path,
        existing: &[Server],
        exclude: Option<ServerId>,
    ) -> CoreResult<Vec<LocationWarning>> {
        if self.paths.is_inside_data_dir(dir) || fsx::path_starts_with_ci(&self.paths.data_dir, dir)
        {
            return Err(CoreError::new(
                ErrorCode::DirectoryNotAllowed,
                "Servers cannot be stored inside MCPanel's application data folder",
            ));
        }
        let warnings = self.platform.location_warnings(dir);
        if let Some(w) = warnings.iter().find(|w| {
            matches!(
                w,
                LocationWarning::ProgramFiles
                    | LocationWarning::SystemDirectory
                    | LocationWarning::DriveRoot
            )
        }) {
            let reason = match w {
                LocationWarning::ProgramFiles => "Program Files is not writable for normal users",
                LocationWarning::SystemDirectory => "System folders cannot be used",
                _ => "A drive root cannot be used as a server folder",
            };
            return Err(CoreError::new(ErrorCode::DirectoryNotAllowed, reason));
        }
        for s in existing {
            if Some(s.id) == exclude {
                continue;
            }
            if fsx::path_starts_with_ci(dir, &s.directory)
                || fsx::path_starts_with_ci(&s.directory, dir)
            {
                return Err(CoreError::new(
                    ErrorCode::DirectoryNotAllowed,
                    format!("This folder overlaps the folder of server '{}'", s.name),
                ));
            }
        }
        Ok(warnings)
    }

    /// Compute the directory a new server would get and any warnings about it.
    pub async fn check_new_location(
        &self,
        name: &str,
        parent: Option<PathBuf>,
    ) -> CoreResult<LocationCheck> {
        let name = validate_server_name(name)?;
        let parent = parent.unwrap_or_else(|| self.paths.default_servers_dir.clone());
        let slug = slugify(&name);
        let mut dir = parent.join(&slug);
        let mut n = 2;
        while dir.exists() && !is_empty_dir(&dir) {
            dir = parent.join(format!("{slug}-{n}"));
            n += 1;
            if n > 999 {
                return Err(CoreError::new(
                    ErrorCode::DirectoryNotEmpty,
                    "Could not find a free folder name",
                ));
            }
        }
        let existing = self.repo.list().await?;
        let warnings = self.check_location(&dir, &existing, None)?;
        Ok(LocationCheck {
            directory: dir,
            warnings,
        })
    }

    pub async fn game_versions(
        &self,
        software_id: &str,
        include_snapshots: bool,
    ) -> CoreResult<Vec<GameVersion>> {
        let provider = self.registry.get_software(software_id)?;
        let mut versions = provider.catalog.game_versions().await?;
        // Enrich with Mojang metadata (kind + release time) for consistent ordering.
        for v in versions.iter_mut() {
            if let Some(m) = self.versions.get(&v.id).await {
                v.kind = m.kind;
                v.release_time = m.release_time;
            }
        }
        versions.sort_by_key(|v| std::cmp::Reverse(v.release_time));
        if !include_snapshots {
            versions.retain(|v| v.kind == crate::software::GameVersionKind::Release);
        }
        Ok(versions)
    }

    pub async fn builds(
        &self,
        software_id: &str,
        game_version: &str,
    ) -> CoreResult<Vec<SoftwareBuild>> {
        self.registry
            .get_software(software_id)?
            .catalog
            .builds(game_version)
            .await
    }

    pub async fn preview_install(
        &self,
        software_id: &str,
        game_version: &str,
        build: Option<String>,
    ) -> CoreResult<InstallPreview> {
        let provider = self.registry.get_software(software_id)?;
        let plan = provider
            .installer
            .plan_install(&InstallRequest {
                game_version: game_version.to_string(),
                build,
            })
            .await?;
        PlanExecutor::validate(&plan, &provider.descriptor)?;
        let java = self
            .java
            .list()
            .await?
            .into_iter()
            .map(|rt| {
                let c = check_compatibility(&rt, &plan.java);
                (rt.id, c)
            })
            .collect();
        Ok(InstallPreview { plan, java })
    }

    async fn validate_properties(
        &self,
        version: &str,
        props: &[(String, String)],
    ) -> CoreResult<()> {
        for (k, v) in props {
            validate_key(k)?;
            let s = schema::schema_for(k, version, &self.versions)
                .await
                .map(|(s, _)| s);
            schema::validate_value(s.as_ref(), k, v)?;
        }
        Ok(())
    }

    /// Validate synchronously, then provision in a background job. Returns the job id.
    pub async fn create(
        self: &Arc<Self>,
        req: CreateServerRequest,
        actor: &str,
    ) -> CoreResult<crate::ids::JobId> {
        let name = validate_server_name(&req.name)?;
        if !req.accept_eula {
            return Err(CoreError::new(
                ErrorCode::EulaNotAccepted,
                "You must accept the Minecraft EULA to create a server",
            ));
        }
        let launch = LaunchConfig {
            java_runtime_id: Some(req.java_runtime_id),
            min_memory_mb: req.min_memory_mb,
            max_memory_mb: req.max_memory_mb,
            jvm_args: req.jvm_args.clone(),
            server_args: Vec::new(),
            stop_timeout_secs: LaunchConfig::DEFAULT_STOP_TIMEOUT_SECS,
        };
        launch.validate()?;
        let java = self.java.get(req.java_runtime_id).await?;
        if !java.valid {
            return Err(CoreError::new(
                ErrorCode::JavaInvalid,
                "The selected Java runtime is not valid",
            ));
        }
        let provider = self.registry.get_software(&req.software_id)?.clone();
        if provider.descriptor.caps.requires_build_step {
            return Err(CoreError::new(
                ErrorCode::Unsupported,
                "This server software is not supported yet",
            ));
        }
        self.validate_properties(&req.game_version, &req.properties)
            .await?;
        let location = self
            .check_new_location(&name, req.parent_directory.clone())
            .await?;

        let this = Arc::clone(self);
        let actor = actor.to_string();
        let job = self
            .jobs
            .spawn("server.create", None, move |ctx| async move {
                let dir = location.directory.clone();
                let created_dir = !dir.exists();
                let result: CoreResult<Server> = async {
                    ctx.progress(Some(0.02), "Preparing server folder");
                    tokio::fs::create_dir_all(&dir)
                        .await
                        .map_err(|e| CoreError::io("Cannot create server folder", &e))?;
                    let root = SafeRoot::open(&dir)?;

                    ctx.progress(
                        Some(0.05),
                        format!(
                            "Resolving {} {}",
                            provider.descriptor.display_name, req.game_version
                        ),
                    );
                    let plan = provider
                        .installer
                        .plan_install(&InstallRequest {
                            game_version: req.game_version.clone(),
                            build: req.build.clone(),
                        })
                        .await?;
                    PlanExecutor::validate(&plan, &provider.descriptor)?;
                    if let JavaCompatibility::TooOld { required } =
                        check_compatibility(&java, &plan.java)
                    {
                        return Err(CoreError::new(
                            ErrorCode::JavaIncompatible,
                            format!(
                                "{} {} requires Java {required} or newer",
                                provider.descriptor.display_name, req.game_version
                            ),
                        ));
                    }
                    let staging = this.paths.staging_dir().join(ctx.id.to_string());
                    this.executor
                        .execute(&plan, &root, &staging, &ctx, (0.08, 0.9))
                        .await?;
                    ctx.check_cancelled()?;

                    ctx.progress(Some(0.92), "Writing configuration");
                    let mut doc = PropertiesDocument::default();
                    for (k, v) in &req.properties {
                        doc.set(k, v)?;
                    }
                    let props_path = root.resolve("server.properties")?;
                    let eula_path = root.resolve("eula.txt")?;
                    let props_text = doc.to_text();
                    let eula_text = eula_file_contents(Timestamp::now());
                    tokio::task::spawn_blocking(move || -> CoreResult<()> {
                        props_path.ensure_no_reparse_points()?;
                        eula_path.ensure_no_reparse_points()?;
                        fsx::atomic_write(&props_path.absolute(), props_text.as_bytes())?;
                        fsx::atomic_write(&eula_path.absolute(), eula_text.as_bytes())
                    })
                    .await
                    .map_err(|e| CoreError::internal(e.to_string()))??;

                    let now = Timestamp::now();
                    let server = Server {
                        id: ServerId::new(),
                        name: name.clone(),
                        directory: fsx::simplify(root.path()),
                        software: InstalledSoftware {
                            software_id: plan.software_id.clone(),
                            game_version: plan.game_version.clone(),
                            build: plan.build.clone(),
                            jar: plan.jar.clone(),
                            java_min_major: Some(plan.java.min_major),
                            java_recommended_major: plan.java.recommended_major,
                        },
                        launch,
                        created_at: now,
                        updated_at: now,
                    };
                    this.repo.insert(&server).await?;
                    Ok(server)
                }
                .await;

                match result {
                    Ok(server) => {
                        let rt = this.insert_runtime(server.id, LifecycleState::Created);
                        rt.console.push(
                            ConsoleStream::System,
                            "Server created. Press Start to run it for the first time.",
                        );
                        this.persist_runtime(&rt).await;
                        this.events.publish(DomainEvent::ServerCreated {
                            server_id: server.id,
                        });
                        this.audit
                            .record(
                                &actor,
                                "server.create",
                                Some(server.id),
                                Some(server.name.clone()),
                                AuditResult::Success,
                                serde_json::json!({
                                    "software": server.software.software_id,
                                    "gameVersion": server.software.game_version,
                                    "build": server.software.build,
                                    "eulaAccepted": true,
                                }),
                            )
                            .await;
                        Ok(Some(serde_json::json!({ "serverId": server.id })))
                    }
                    Err(e) => {
                        if created_dir {
                            let d = dir.clone();
                            let _ =
                                tokio::task::spawn_blocking(move || fsx::remove_tree_no_follow(&d))
                                    .await;
                        }
                        this.audit
                            .record(
                                &actor,
                                "server.create",
                                None,
                                Some(name.clone()),
                                AuditResult::Failure,
                                serde_json::json!({ "error": e.code }),
                            )
                            .await;
                        Err(e)
                    }
                }
            })
            .await?;
        Ok(job)
    }

    /// Inspect a folder before importing it.
    pub async fn detect_import(&self, dir: PathBuf) -> CoreResult<ImportDetection> {
        let existing = self.repo.list().await?;
        let warnings = self.check_location(&dir, &existing, None)?;
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            if !dir.is_dir() {
                return Err(CoreError::new(ErrorCode::PathNotFound, "Folder not found"));
            }
            Ok(ImportDetection {
                detected: registry.detect(&dir),
                has_eula: read_root_file(&dir, "eula.txt")
                    .is_some_and(|b| eula_accepted_text(&decode_bytes(&b))),
                has_properties: read_root_file(&dir, "server.properties").is_some(),
                jars: crate::software::jar::root_jars(&dir)
                    .into_iter()
                    .map(|(n, _)| n)
                    .collect(),
                warnings,
            })
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))?
    }

    pub async fn import(
        self: &Arc<Self>,
        req: ImportServerRequest,
        actor: &str,
    ) -> CoreResult<Server> {
        let name = validate_server_name(&req.name)?;
        let detection = self.detect_import(req.directory.clone()).await?;
        let software_id = req
            .software_id
            .clone()
            .or_else(|| detection.detected.as_ref().map(|d| d.software_id.clone()))
            .ok_or_else(|| {
                CoreError::invalid("Could not detect the server software; please choose it")
            })?;
        let provider = self.registry.get_software(&software_id)?.clone();
        let game_version = req
            .game_version
            .clone()
            .or_else(|| {
                detection
                    .detected
                    .as_ref()
                    .and_then(|d| d.game_version.clone())
            })
            .ok_or_else(|| {
                CoreError::invalid("Could not detect the Minecraft version; please choose it")
            })?;
        let jar = req
            .jar
            .clone()
            .or_else(|| detection.detected.as_ref().map(|d| d.jar.clone()))
            .ok_or_else(|| CoreError::invalid("Choose the server jar"))?;
        let root = SafeRoot::open(&req.directory)?;
        let jar_path = root.resolve(&jar)?;
        jar_path.ensure_no_reparse_points()?;
        if !jar_path.absolute().is_file() {
            return Err(CoreError::new(
                ErrorCode::PathNotFound,
                format!("'{jar}' was not found in the folder"),
            ));
        }
        let launch = LaunchConfig {
            java_runtime_id: req.java_runtime_id,
            min_memory_mb: req.min_memory_mb,
            max_memory_mb: req.max_memory_mb,
            jvm_args: Vec::new(),
            server_args: Vec::new(),
            stop_timeout_secs: LaunchConfig::DEFAULT_STOP_TIMEOUT_SECS,
        };
        launch.validate()?;
        if let Some(id) = req.java_runtime_id {
            self.java.get(id).await?;
        }
        // Java requirement from provider metadata (network); unknown if unavailable.
        let java_req = provider
            .installer
            .plan_install(&InstallRequest {
                game_version: game_version.clone(),
                build: detection.detected.as_ref().and_then(|d| d.build.clone()),
            })
            .await
            .ok()
            .map(|p| p.java);
        let now = Timestamp::now();
        let server = Server {
            id: ServerId::new(),
            name,
            directory: fsx::simplify(root.path()),
            software: InstalledSoftware {
                software_id: software_id.clone(),
                game_version,
                build: detection.detected.as_ref().and_then(|d| d.build.clone()),
                jar,
                java_min_major: java_req.as_ref().map(|j| j.min_major),
                java_recommended_major: java_req.and_then(|j| j.recommended_major),
            },
            launch,
            created_at: now,
            updated_at: now,
        };
        self.repo.insert(&server).await?;
        let rt = self.insert_runtime(server.id, LifecycleState::Stopped);
        rt.console
            .push(ConsoleStream::System, "Server imported into MCPanel.");
        self.persist_runtime(&rt).await;
        self.events.publish(DomainEvent::ServerCreated {
            server_id: server.id,
        });
        self.audit
            .record(
                actor,
                "server.import",
                Some(server.id),
                Some(server.name.clone()),
                AuditResult::Success,
                serde_json::json!({ "software": software_id, "gameVersion": server.software.game_version }),
            )
            .await;
        Ok(server)
    }

    pub async fn update(
        &self,
        id: ServerId,
        req: UpdateServerRequest,
        actor: &str,
    ) -> CoreResult<Server> {
        let mut server = self.get(id).await?;
        let mut changed = Vec::new();
        if let Some(name) = req.name {
            server.name = validate_server_name(&name)?;
            changed.push("name");
        }
        if let Some(launch) = req.launch {
            launch.validate()?;
            if let Some(jid) = launch.java_runtime_id {
                self.java.get(jid).await?;
            }
            server.launch = launch;
            changed.push("launch");
        }
        server.updated_at = Timestamp::now();
        self.repo.update(&server).await?;
        self.events
            .publish(DomainEvent::ServerUpdated { server_id: id });
        self.audit
            .record(
                actor,
                "server.update",
                Some(id),
                None,
                AuditResult::Success,
                serde_json::json!({ "fields": changed }),
            )
            .await;
        Ok(server)
    }

    pub async fn accept_eula(&self, id: ServerId, actor: &str) -> CoreResult<()> {
        let server = self.get(id).await?;
        let root = SafeRoot::open(&server.directory)?;
        let path = root.resolve("eula.txt")?;
        let text = eula_file_contents(Timestamp::now());
        tokio::task::spawn_blocking(move || -> CoreResult<()> {
            path.ensure_no_reparse_points()?;
            fsx::atomic_write(&path.absolute(), text.as_bytes())
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))??;
        self.events
            .publish(DomainEvent::ServerUpdated { server_id: id });
        self.audit
            .record(
                actor,
                "server.eula_accepted",
                Some(id),
                None,
                AuditResult::Success,
                serde_json::json!({}),
            )
            .await;
        Ok(())
    }

    /// Remove a server from MCPanel. Files are deleted only when explicitly requested.
    pub async fn delete(&self, id: ServerId, delete_files: bool, actor: &str) -> CoreResult<()> {
        let server = self.get(id).await?;
        let rt = self.runtime(id);
        let _guard = rt.begin(Operation::Deleting)?;
        if delete_files {
            let dir = server.directory.clone();
            tokio::task::spawn_blocking(move || {
                if dir.exists() {
                    fsx::remove_tree_no_follow(&dir)
                } else {
                    Ok(())
                }
            })
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        }
        let data = self.paths.server_data_dir(&id);
        let _ = tokio::task::spawn_blocking(move || {
            if data.exists() {
                let _ = fsx::remove_tree_no_follow(&data);
            }
        })
        .await;
        self.repo.delete(id).await?;
        drop(_guard);
        self.remove_runtime(id);
        self.events
            .publish(DomainEvent::ServerDeleted { server_id: id });
        self.audit
            .record(
                actor,
                "server.delete",
                None,
                Some(server.name.clone()),
                AuditResult::Success,
                serde_json::json!({ "serverId": id, "filesDeleted": delete_files }),
            )
            .await;
        Ok(())
    }
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut r| r.next().is_none())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("My SMP!"), "my-smp");
        assert_eq!(slugify("  Paper   1.21  "), "paper-1-21");
        assert_eq!(slugify("CON"), "server");
        assert_eq!(slugify("!!!"), "server");
        assert_eq!(slugify("Überserver"), "berserver");
    }

    #[test]
    fn names() {
        assert!(validate_server_name("  ").is_err());
        assert_eq!(validate_server_name(" SMP ").unwrap(), "SMP");
        assert!(validate_server_name(&"x".repeat(65)).is_err());
    }

    #[test]
    fn eula_text_is_accepted() {
        assert!(eula_accepted_text(&eula_file_contents(Timestamp(1))));
        assert!(!eula_accepted_text("eula=false\n"));
    }
}
