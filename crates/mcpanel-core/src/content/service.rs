//! Content service: listing, search, install planning (with dependencies), install with
//! rollback, enable/disable/remove, update checks and pending changes.

use super::descriptor::{self, Descriptor};
use super::{
    ContentKind, ContentProvider, ContentSource, ContentTarget, ContentVersion, DependencyKind,
    InstalledContent, ProjectSummary, ReleaseChannel, SearchPage, SearchQuery, SearchSort,
};
use crate::audit::AuditLog;
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::files::fsx::is_reparse_point;
use crate::files::safepath::parse_relative;
use crate::ids::{JobId, ServerId};
use crate::jobs::{JobContext, JobManager};
use crate::model::{AuditResult, Server};
use crate::ports::{ContentRepository, DownloadRequest, Downloader};
use crate::server::runtime::{Operation, ServerRuntime};
use crate::server::{LaunchHook, ServerManager};
use crate::software::executor::host_of;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

const RESERVED: &str = ".mcpanel";
const MAX_DEPENDENCY_DEPTH: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PendingKind {
    /// A verified file waiting in `.mcpanel/pending/`; replaces `replaces` when applied.
    Install {
        staged: String,
        record: Box<InstalledContent>,
        replaces: Option<String>,
    },
    Remove {
        file_name: String,
    },
    Disable {
        file_name: String,
    },
    Enable {
        file_name: String,
    },
}

impl PendingKind {
    fn file_name(&self) -> &str {
        match self {
            Self::Install { record, .. } => &record.file_name,
            Self::Remove { file_name }
            | Self::Disable { file_name }
            | Self::Enable { file_name } => file_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingChange {
    pub id: String,
    pub server_id: ServerId,
    pub kind: ContentKind,
    pub change: PendingKind,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentEntry {
    pub file_name: String,
    pub enabled: bool,
    pub size: u64,
    pub descriptor: Option<Descriptor>,
    pub record: Option<InstalledContent>,
    /// A queued change for this file (applies when the server stops or next starts).
    pub pending: Option<PendingKind>,
    pub detection: Option<super::DetectedPlugin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentList {
    pub kind: ContentKind,
    pub folder: String,
    pub running: bool,
    pub entries: Vec<ContentEntry>,
    /// Queued installs of files that do not exist yet.
    pub pending_installs: Vec<PendingChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallRequest {
    pub provider: String,
    pub project_id: String,
    /// `None` = newest compatible version (releases preferred).
    pub version_id: Option<String>,
    pub with_dependencies: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedInstall {
    pub provider: String,
    pub project: ProjectSummary,
    pub version: ContentVersion,
    /// Installed file this replaces (update), if any.
    pub replaces: Option<String>,
    /// `None` = requested by the user; otherwise the project that needs it.
    pub required_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    pub items: Vec<PlannedInstall>,
    /// Required dependencies MCPanel cannot install (external downloads, unknown).
    pub unresolved: Vec<String>,
    /// Problems that do not block the install (incompatibilities, unverified files).
    pub warnings: Vec<String>,
    /// The server is running: files are applied when it stops or restarts.
    pub deferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub file_name: String,
    pub name: String,
    pub current_version: Option<String>,
    pub latest: ContentVersion,
}

pub struct ContentServiceDeps {
    pub servers: Arc<ServerManager>,
    pub repo: Arc<dyn ContentRepository>,
    pub downloader: Arc<dyn Downloader>,
    pub jobs: Arc<JobManager>,
    pub audit: Arc<AuditLog>,
    pub events: EventBus,
}

pub struct ContentService {
    servers: Arc<ServerManager>,
    repo: Arc<dyn ContentRepository>,
    downloader: Arc<dyn Downloader>,
    jobs: Arc<JobManager>,
    audit: Arc<AuditLog>,
    events: EventBus,
    /// Serialises file changes (install/apply) across all servers.
    apply_lock: AsyncMutex<()>,
    detection_cache: std::sync::RwLock<
        std::collections::HashMap<(ServerId, String, u64), super::DetectedPlugin>,
    >,
}

fn safe_file_name(name: &str) -> CoreResult<String> {
    let parts = parse_relative(name)?;
    let ok =
        parts.len() == 1 && parts[0].to_lowercase().ends_with(".jar") && !parts[0].starts_with('.');
    if !ok {
        return Err(CoreError::new(
            ErrorCode::PathRejected,
            format!("'{name}' is not an acceptable plugin/mod file name"),
        ));
    }
    Ok(parts[0].clone())
}

fn sha512_file(path: &Path) -> CoreResult<String> {
    let mut f = std::fs::File::open(path).map_err(|e| CoreError::io("Cannot read file", &e))?;
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| CoreError::io("Cannot read file", &e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

struct Dirs {
    active: PathBuf,
    disabled: PathBuf,
    pending: PathBuf,
    incoming: PathBuf,
    trash: PathBuf,
}

fn dirs(root: &Path, kind: ContentKind) -> Dirs {
    let reserved = root.join(RESERVED);
    Dirs {
        active: root.join(kind.folder()),
        disabled: reserved.join("disabled").join(kind.folder()),
        pending: reserved.join("pending"),
        incoming: reserved.join("incoming"),
        trash: reserved.join("trash"),
    }
}

/// Refuse to work through links (the folder must be a real directory or absent).
fn ensure_real_dir(dir: &Path) -> CoreResult<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(md) if is_reparse_point(&md) || !md.is_dir() => Err(CoreError::new(
            ErrorCode::PathRejected,
            format!("'{}' is a link or not a folder", dir.display()),
        )),
        Ok(_) => Ok(()),
        Err(_) => {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io("Cannot create folder", &e))
        }
    }
}

fn jars_in(dir: &Path) -> Vec<(String, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, u64)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let md = std::fs::symlink_metadata(e.path()).ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            (md.is_file() && !is_reparse_point(&md) && name.to_lowercase().ends_with(".jar"))
                .then_some((name, md.len()))
        })
        .collect();
    out.sort_by_key(|(n, _)| n.to_lowercase());
    out
}

/// Move a file to the server's trash; returns where it went (for rollback).
fn to_trash(trash: &Path, from: &Path, folder: &str) -> CoreResult<PathBuf> {
    let dir = trash.join(Timestamp::now().millis().to_string());
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::io("Cannot create trash", &e))?;
    let name = from
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut dest = dir.join(format!("{folder}__{name}"));
    let mut n = 1;
    while dest.exists() {
        n += 1;
        dest = dir.join(format!("{folder}__{n}__{name}"));
    }
    std::fs::rename(from, &dest).map_err(|e| {
        CoreError::io(
            format!("Cannot move '{name}' (is the server still using it?)"),
            &e,
        )
    })?;
    Ok(dest)
}

fn newer(a: &ContentVersion, b: &ContentVersion) -> bool {
    match (a.published, b.published) {
        (Some(x), Some(y)) => x > y,
        _ => a.id != b.id,
    }
}

/// Newest release, else newest pre-release.
fn pick(versions: &[ContentVersion]) -> Option<&ContentVersion> {
    versions
        .iter()
        .find(|v| v.channel == ReleaseChannel::Release)
        .or_else(|| versions.first())
}

impl ContentService {
    pub fn new(deps: ContentServiceDeps) -> Arc<Self> {
        Arc::new(Self {
            servers: deps.servers,
            repo: deps.repo,
            downloader: deps.downloader,
            jobs: deps.jobs,
            audit: deps.audit,
            events: deps.events,
            apply_lock: AsyncMutex::new(()),
            detection_cache: std::sync::RwLock::new(std::collections::HashMap::new()),
        })
    }

    pub fn target(&self, server: &Server) -> CoreResult<ContentTarget> {
        let caps = &self
            .servers
            .registry()
            .get_software(&server.software.software_id)?
            .descriptor
            .caps;
        let kind = ContentKind::for_ecosystems(&caps.content).ok_or_else(|| {
            CoreError::new(
                ErrorCode::Unsupported,
                "This server software does not support plugins or mods",
            )
        })?;
        Ok(ContentTarget {
            kind,
            software_id: server.software.software_id.clone(),
            game_version: server.software.game_version.clone(),
            ecosystems: caps.content.clone(),
        })
    }

    pub fn providers(&self, target: &ContentTarget) -> Vec<Arc<dyn ContentProvider>> {
        self.servers
            .registry()
            .content_providers()
            .iter()
            .filter(|p| p.info().kinds.contains(&target.kind) && p.supports(target))
            .cloned()
            .collect()
    }

    fn provider(&self, target: &ContentTarget, id: &str) -> CoreResult<Arc<dyn ContentProvider>> {
        self.providers(target)
            .into_iter()
            .find(|p| p.info().id == id)
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::Unsupported,
                    format!("'{id}' has no content for this server"),
                )
            })
    }

    fn running(&self, server_id: ServerId) -> bool {
        self.servers.runtime(server_id).state().has_process()
    }

    fn changed(&self, server_id: ServerId) {
        self.events
            .publish(DomainEvent::ContentChanged { server_id });
    }

    // ───────────────────────────── queries ─────────────────────────────

    pub async fn list(&self, server_id: ServerId) -> CoreResult<ContentList> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        let records = self.repo.installed(server_id).await?;
        let pending = self.repo.pending(server_id).await?;
        let root = server.directory.clone();
        let kind = target.kind;
        let scanned = tokio::task::spawn_blocking(move || {
            let d = dirs(&root, kind);
            let mut out = Vec::new();
            for (enabled, dir) in [(true, &d.active), (false, &d.disabled)] {
                for (name, size) in jars_in(dir) {
                    let desc = descriptor::read(&dir.join(&name)).ok().flatten();
                    out.push((name, enabled, size, desc));
                }
            }
            out
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))?;
        let entries = scanned
            .into_iter()
            .map(|(file_name, enabled, size, descriptor)| {
                let record = records
                    .iter()
                    .find(|r| r.kind == kind && r.file_name.eq_ignore_ascii_case(&file_name))
                    .cloned();

                let detection = {
                    let cache_key = (server_id, file_name.clone(), size);
                    if let Ok(c) = self.detection_cache.read() {
                        c.get(&cache_key).cloned()
                    } else {
                        None
                    }
                }
                .or_else(|| {
                    let det =
                        super::detect_plugin(&file_name, descriptor.as_ref(), record.as_ref());
                    if let Ok(mut c) = self.detection_cache.write() {
                        c.insert((server_id, file_name.clone(), size), det.clone());
                    }
                    Some(det)
                });

                ContentEntry {
                    record,
                    pending: pending
                        .iter()
                        .rev()
                        .find(|p| p.change.file_name().eq_ignore_ascii_case(&file_name))
                        .map(|p| p.change.clone()),
                    file_name,
                    enabled,
                    size,
                    descriptor,
                    detection,
                }
            })
            .collect::<Vec<_>>();
        let pending_installs = pending
            .into_iter()
            .filter(|p| {
                matches!(p.change, PendingKind::Install { .. })
                    && !entries
                        .iter()
                        .any(|e| e.file_name.eq_ignore_ascii_case(p.change.file_name()))
            })
            .collect();
        Ok(ContentList {
            kind,
            folder: kind.folder().into(),
            running: self.running(server_id),
            entries,
            pending_installs,
        })
    }

    pub async fn search(
        &self,
        server_id: ServerId,
        provider: &str,
        text: &str,
        sort: SearchSort,
        offset: u32,
        limit: u32,
    ) -> CoreResult<SearchPage> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        let p = self.provider(&target, provider)?;
        let text: String = text.trim().chars().take(100).collect();
        p.search(&SearchQuery {
            text,
            target,
            sort,
            offset,
            limit: limit.clamp(1, 50),
        })
        .await
    }

    pub async fn versions(
        &self,
        server_id: ServerId,
        provider: &str,
        project_id: &str,
    ) -> CoreResult<Vec<ContentVersion>> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        self.provider(&target, provider)?
            .versions(project_id, &target)
            .await
    }

    pub async fn recommendations(
        &self,
        server_id: ServerId,
    ) -> CoreResult<Vec<super::PluginRecommendation>> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        let props = self.servers.properties(server_id).await?;
        let online_mode = props
            .properties
            .iter()
            .find(|p| p.key == "online-mode")
            .and_then(|p| p.value.as_deref())
            .is_none_or(|v| v.trim().eq_ignore_ascii_case("true"));

        let list = self.list(server_id).await?;
        let mut installed = Vec::new();
        for e in &list.entries {
            installed.push(e.file_name.clone());
            if let Some(d) = &e.descriptor {
                if let Some(n) = &d.name {
                    installed.push(n.clone());
                }
                if let Some(id) = &d.id {
                    installed.push(id.clone());
                }
            }
            if let Some(r) = &e.record {
                installed.push(r.name.clone());
                if let Some(src) = &r.source {
                    installed.push(src.project_id.clone());
                }
            }
            if let Some(det) = &e.detection {
                installed.push(det.name.clone());
                if let Some(pid) = &det.project_id {
                    installed.push(pid.clone());
                }
            }
        }

        let has_bedrock = list.entries.iter().any(|e| {
            e.file_name.to_ascii_lowercase().contains("geyser")
                || e.descriptor
                    .as_ref()
                    .and_then(|d| d.name.as_deref())
                    .is_some_and(|n| n.to_ascii_lowercase().contains("geyser"))
                || e.detection
                    .as_ref()
                    .is_some_and(|d| d.name.to_ascii_lowercase().contains("geyser"))
        });

        let ctx = super::RecommendationContext {
            software_id: target.software_id,
            game_version: target.game_version,
            ecosystems: target.ecosystems,
            online_mode,
            has_bedrock,
            installed,
        };

        Ok(super::filter_recommendations(&ctx))
    }

    pub async fn identify(
        &self,
        server_id: ServerId,
        file_name: &str,
        provider: &str,
        project_id: &str,
        version_number: Option<String>,
        name: Option<String>,
    ) -> CoreResult<()> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        let clean_file = safe_file_name(file_name)?;
        let display_name = name.unwrap_or_else(|| clean_file.trim_end_matches(".jar").to_string());

        let record = InstalledContent {
            server_id,
            kind: target.kind,
            file_name: clean_file.clone(),
            name: display_name,
            version_number,
            source: Some(super::ContentSource {
                provider: provider.to_string(),
                project_id: project_id.to_string(),
                version_id: "".to_string(),
            }),
            sha512: None,
            installed_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };

        self.repo.upsert(&record).await?;
        if let Ok(mut c) = self.detection_cache.write() {
            c.retain(|(sid, f, _), _| *sid != server_id || *f != clean_file);
        }
        self.events
            .publish(DomainEvent::ContentChanged { server_id });
        Ok(())
    }

    // ───────────────────────────── planning ─────────────────────────────

    pub async fn plan(&self, server_id: ServerId, req: &InstallRequest) -> CoreResult<InstallPlan> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        let provider = self.provider(&target, &req.provider)?;
        let list = self.list(server_id).await?;
        let mut plan = InstallPlan {
            items: Vec::new(),
            unresolved: Vec::new(),
            warnings: Vec::new(),
            deferred: list.running,
        };
        let mut seen = HashSet::new();
        let mut queue: Vec<(String, Option<String>, Option<String>, usize)> =
            vec![(req.project_id.clone(), req.version_id.clone(), None, 0)];
        while let Some((project_id, version_id, required_by, depth)) = queue.pop() {
            if !seen.insert(project_id.clone()) {
                continue;
            }
            let installed = list.entries.iter().find(|e| {
                e.record
                    .as_ref()
                    .and_then(|r| r.source.as_ref())
                    .is_some_and(|s| s.provider == req.provider && s.project_id == project_id)
            });
            let project = provider.project(&project_id).await?;
            // A dependency that is already present (managed, or by descriptor name).
            if required_by.is_some() {
                let by_name = list.entries.iter().any(|e| {
                    e.descriptor
                        .as_ref()
                        .and_then(|d| d.name.as_deref())
                        .is_some_and(|n| {
                            n.eq_ignore_ascii_case(&project.name)
                                || n.eq_ignore_ascii_case(&project.slug)
                        })
                });
                if installed.is_some() || by_name {
                    continue;
                }
            }
            let version = match &version_id {
                Some(v) => provider.version(&project_id, v).await?,
                None => {
                    let versions = provider.versions(&project_id, &target).await?;
                    match pick(&versions) {
                        Some(v) => v.clone(),
                        None => {
                            let msg = format!(
                                "No version of {} is compatible with this server ({} {})",
                                project.name, target.software_id, target.game_version
                            );
                            if required_by.is_none() {
                                return Err(CoreError::new(ErrorCode::VersionNotFound, msg));
                            }
                            plan.unresolved.push(msg);
                            continue;
                        }
                    }
                }
            };
            if let Some(i) = installed
                && i.record
                    .as_ref()
                    .and_then(|r| r.source.as_ref())
                    .is_some_and(|s| s.version_id == version.id)
            {
                if required_by.is_none() {
                    return Err(CoreError::new(
                        ErrorCode::Conflict,
                        format!(
                            "{} {} is already installed",
                            project.name, version.version_number
                        ),
                    ));
                }
                continue;
            }
            let Some(file) = &version.file else {
                let msg = format!(
                    "{} is only available from an external site{}",
                    project.name,
                    version
                        .external_url
                        .as_deref()
                        .map(|u| format!(" ({u})"))
                        .unwrap_or_default()
                );
                if required_by.is_none() {
                    return Err(CoreError::new(ErrorCode::Unsupported, msg));
                }
                plan.unresolved.push(msg);
                continue;
            };
            safe_file_name(&file.file_name)?;
            let host = host_of(&file.url).unwrap_or_default();
            if !provider
                .info()
                .download_hosts
                .iter()
                .any(|h| h.eq_ignore_ascii_case(host))
            {
                return Err(CoreError::new(
                    ErrorCode::ProviderError,
                    format!(
                        "Download host '{host}' is not trusted for {}",
                        provider.info().display_name
                    ),
                ));
            }
            if file.hash.is_none() {
                plan.warnings.push(format!(
                    "{} has no published hash (unverified download)",
                    project.name
                ));
            }
            if req.with_dependencies && depth < MAX_DEPENDENCY_DEPTH {
                for d in &version.dependencies {
                    match (d.kind, &d.project_id) {
                        (DependencyKind::Required, Some(pid)) => {
                            queue.push((
                                pid.clone(),
                                d.version_id.clone(),
                                Some(project.name.clone()),
                                depth + 1,
                            ));
                        }
                        (DependencyKind::Required, None) => plan.unresolved.push(format!(
                            "{} needs {}{}",
                            project.name,
                            d.name.as_deref().unwrap_or("another plugin"),
                            d.external_url
                                .as_deref()
                                .map(|u| format!(" ({u})"))
                                .unwrap_or_default()
                        )),
                        (DependencyKind::Incompatible, Some(pid)) => {
                            let clash = list.entries.iter().any(|e| {
                                e.record
                                    .as_ref()
                                    .and_then(|r| r.source.as_ref())
                                    .is_some_and(|s| &s.project_id == pid)
                            });
                            if clash {
                                plan.warnings.push(format!(
                                    "{} is incompatible with something already installed",
                                    project.name
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
            plan.items.push(PlannedInstall {
                provider: req.provider.clone(),
                replaces: installed.map(|e| e.file_name.clone()),
                project,
                version,
                required_by,
            });
        }
        // Requested item first, then its dependencies.
        plan.items.sort_by_key(|i| i.required_by.is_some());
        Ok(plan)
    }

    // ───────────────────────────── install ─────────────────────────────

    /// Start an install job for the request (the plan is recomputed in the job).
    pub async fn install(
        self: &Arc<Self>,
        server_id: ServerId,
        req: InstallRequest,
        actor: &str,
    ) -> CoreResult<JobId> {
        let plan = self.plan(server_id, &req).await?;
        let this = Arc::clone(self);
        let actor = actor.to_string();
        let name = plan
            .items
            .first()
            .map(|i| i.project.name.clone())
            .unwrap_or_default();
        self.jobs
            .spawn("content.install", Some(server_id), move |ctx| async move {
                let result = this.run_install(server_id, &req, &ctx).await;
                this.audit
                    .record(
                        &actor,
                        "content.install",
                        Some(server_id),
                        Some(name),
                        if result.is_ok() {
                            AuditResult::Success
                        } else {
                            AuditResult::Failure
                        },
                        serde_json::json!({ "provider": req.provider, "project": req.project_id }),
                    )
                    .await;
                this.changed(server_id);
                result.map(Some)
            })
            .await
    }

    /// Run an install inside another job (e.g. Bedrock setup); same pipeline as
    /// [`Self::install`] without a job of its own.
    pub async fn install_in_job(
        &self,
        server_id: ServerId,
        req: &InstallRequest,
        ctx: &JobContext,
    ) -> CoreResult<serde_json::Value> {
        let r = self.run_install(server_id, req, ctx).await;
        self.changed(server_id);
        r
    }

    async fn run_install(
        &self,
        server_id: ServerId,
        req: &InstallRequest,
        ctx: &JobContext,
    ) -> CoreResult<serde_json::Value> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        let plan = self.plan(server_id, req).await?;
        let d = dirs(&server.directory, target.kind);
        for dir in [&d.incoming, &d.pending] {
            ensure_real_dir(&server.directory.join(RESERVED))?;
            ensure_real_dir(dir)?;
        }

        // Download and verify everything first (nothing on the server changes yet).
        let mut staged: Vec<(PlannedInstall, PathBuf, String, Descriptor)> = Vec::new();
        let total = plan.items.len().max(1) as f32;
        let cleanup = |staged: &[(PlannedInstall, PathBuf, String, Descriptor)]| {
            for (_, p, _, _) in staged {
                let _ = std::fs::remove_file(p);
            }
        };
        for (i, item) in plan.items.iter().enumerate() {
            let file = item
                .version
                .file
                .as_ref()
                .ok_or_else(|| CoreError::internal("planned item without a file"))?;
            let tmp = d
                .incoming
                .join(format!("{}.part", uuid::Uuid::new_v4().simple()));
            let base = i as f32 / total;
            let label = format!("Downloading {}", item.project.name);
            let job = ctx.clone();
            let progress = move |done: u64, size: Option<u64>| {
                let frac = size
                    .filter(|s| *s > 0)
                    .map_or(0.0, |s| done as f32 / s as f32);
                job.progress(Some(base + frac / total), label.clone());
            };
            let r = self
                .downloader
                .download(
                    &DownloadRequest {
                        url: file.url.clone(),
                        expected_hash: file.hash.clone(),
                        expected_size: file.size,
                    },
                    &tmp,
                    &progress,
                    ctx.cancellation(),
                )
                .await;
            if let Err(e) = r {
                let _ = std::fs::remove_file(&tmp);
                cleanup(&staged);
                return Err(e);
            }
            let kind = target.kind;
            let tmp2 = tmp.clone();
            let checked =
                tokio::task::spawn_blocking(move || -> CoreResult<(Descriptor, String)> {
                    Ok((descriptor::validate(&tmp2, kind)?, sha512_file(&tmp2)?))
                })
                .await
                .map_err(|e| CoreError::internal(e.to_string()))
                .and_then(|r| r);
            match checked {
                Ok((desc, sha)) => staged.push((item.clone(), tmp, sha, desc)),
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    cleanup(&staged);
                    return Err(CoreError::new(
                        e.code,
                        format!(
                            "{} {}: {}",
                            item.project.name, item.version.version_number, e.message
                        ),
                    ));
                }
            }
        }

        let now = Timestamp::now();
        let record =
            |item: &PlannedInstall, sha: &str, desc: &Descriptor| -> CoreResult<InstalledContent> {
                let file = item
                    .version
                    .file
                    .as_ref()
                    .ok_or_else(|| CoreError::internal("no file"))?;
                Ok(InstalledContent {
                    server_id,
                    kind: target.kind,
                    file_name: safe_file_name(&file.file_name)?,
                    name: desc
                        .name
                        .clone()
                        .unwrap_or_else(|| item.project.name.clone()),
                    version_number: Some(item.version.version_number.clone()),
                    source: Some(ContentSource {
                        provider: item.provider.clone(),
                        project_id: item.project.id.clone(),
                        version_id: item.version.id.clone(),
                    }),
                    sha512: Some(sha.to_string()),
                    installed_at: now,
                    updated_at: now,
                })
            };

        let _apply = self.apply_lock.lock().await;
        if self.running(server_id) {
            // Queue: keep the verified files in .mcpanel/pending until the server stops.
            for (item, tmp, sha, desc) in &staged {
                let rec = record(item, sha, desc)?;
                let id = uuid::Uuid::new_v4().simple().to_string();
                let dest = d.pending.join(format!("{id}.jar"));
                std::fs::rename(tmp, &dest).map_err(|e| CoreError::io("Cannot stage file", &e))?;
                self.repo
                    .add_pending(&PendingChange {
                        id,
                        server_id,
                        kind: target.kind,
                        change: PendingKind::Install {
                            staged: dest
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default(),
                            record: Box::new(rec),
                            replaces: item.replaces.clone(),
                        },
                        created_at: now,
                    })
                    .await?;
            }
            return Ok(serde_json::json!({ "installed": staged.len(), "deferred": true }));
        }

        let rt = self.servers.runtime(server_id);
        let _op = rt.begin(Operation::InstallingContent)?;
        let items: Vec<(PathBuf, InstalledContent, Option<String>)> = staged
            .iter()
            .map(|(item, tmp, sha, desc)| {
                Ok((tmp.clone(), record(item, sha, desc)?, item.replaces.clone()))
            })
            .collect::<CoreResult<_>>()?;
        let root = server.directory.clone();
        let kind = target.kind;
        let placed = tokio::task::spawn_blocking(move || place_all(&root, kind, &items))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))?;
        match placed {
            Ok(records) => {
                for (old, rec) in &records {
                    if let Some(old) = old
                        && !old.eq_ignore_ascii_case(&rec.file_name)
                    {
                        self.repo.remove(server_id, kind, old).await?;
                    }
                    self.repo.upsert(rec).await?;
                }
                Ok(serde_json::json!({ "installed": records.len(), "deferred": false }))
            }
            Err(e) => {
                cleanup(&staged);
                Err(e)
            }
        }
    }

    // ─────────────────────── remove / enable / disable ───────────────────────

    pub async fn remove(
        &self,
        server_id: ServerId,
        file_name: &str,
        actor: &str,
    ) -> CoreResult<bool> {
        self.change(
            server_id,
            PendingKind::Remove {
                file_name: safe_file_name(file_name)?,
            },
            actor,
        )
        .await
    }

    pub async fn set_enabled(
        &self,
        server_id: ServerId,
        file_name: &str,
        enabled: bool,
        actor: &str,
    ) -> CoreResult<bool> {
        let file_name = safe_file_name(file_name)?;
        let change = if enabled {
            PendingKind::Enable { file_name }
        } else {
            PendingKind::Disable { file_name }
        };
        self.change(server_id, change, actor).await
    }

    /// Apply (stopped) or queue (running) a change. Returns whether it was deferred.
    async fn change(
        &self,
        server_id: ServerId,
        change: PendingKind,
        actor: &str,
    ) -> CoreResult<bool> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        let action = match &change {
            PendingKind::Remove { .. } => "content.remove",
            PendingKind::Disable { .. } => "content.disable",
            PendingKind::Enable { .. } => "content.enable",
            PendingKind::Install { .. } => "content.install",
        };
        let file = change.file_name().to_string();
        let _apply = self.apply_lock.lock().await;
        let deferred = self.running(server_id);
        let result = if deferred {
            self.repo
                .add_pending(&PendingChange {
                    id: uuid::Uuid::new_v4().simple().to_string(),
                    server_id,
                    kind: target.kind,
                    change,
                    created_at: Timestamp::now(),
                })
                .await
        } else {
            let rt = self.servers.runtime(server_id);
            let _op = rt.begin(Operation::InstallingContent)?;
            self.apply_one(&server, target.kind, &change).await
        };
        self.audit
            .record(
                actor,
                action,
                Some(server_id),
                Some(file),
                if result.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
                serde_json::json!({ "deferred": deferred }),
            )
            .await;
        result?;
        self.changed(server_id);
        Ok(deferred)
    }

    async fn apply_one(
        &self,
        server: &Server,
        kind: ContentKind,
        change: &PendingKind,
    ) -> CoreResult<()> {
        let root = server.directory.clone();
        let c = change.clone();
        tokio::task::spawn_blocking(move || apply_files(&root, kind, &c))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        match change {
            PendingKind::Remove { file_name } => self.repo.remove(server.id, kind, file_name).await,
            PendingKind::Install {
                record, replaces, ..
            } => {
                if let Some(old) = replaces
                    && !old.eq_ignore_ascii_case(&record.file_name)
                {
                    self.repo.remove(server.id, kind, old).await?;
                }
                self.repo.upsert(record).await
            }
            _ => Ok(()),
        }
    }

    /// Apply queued changes of a server that has no process. Failed changes stay queued
    /// only if they may succeed later; the rest are dropped with a console note.
    pub async fn apply_pending(&self, server: &Server, rt: &ServerRuntime) -> CoreResult<usize> {
        let pending = self.repo.pending(server.id).await?;
        if pending.is_empty() {
            return Ok(0);
        }
        let _apply = self.apply_lock.lock().await;
        let mut applied = 0;
        for p in self.repo.pending(server.id).await? {
            match self.apply_one(server, p.kind, &p.change).await {
                Ok(()) => {
                    applied += 1;
                    rt.console.push(
                        crate::console::ConsoleStream::System,
                        &format!(
                            "Applied pending change: {} {}",
                            pending_verb(&p.change),
                            p.change.file_name()
                        ),
                    );
                }
                Err(e) => {
                    rt.console.push(
                        crate::console::ConsoleStream::System,
                        &format!(
                            "Could not apply pending change to {}: {}",
                            p.change.file_name(),
                            e.message
                        ),
                    );
                }
            }
            self.repo.remove_pending(&p.id).await?;
        }
        self.changed(server.id);
        Ok(applied)
    }

    pub async fn pending(&self, server_id: ServerId) -> CoreResult<Vec<PendingChange>> {
        self.servers.get(server_id).await?;
        self.repo.pending(server_id).await
    }

    pub async fn discard_pending(
        &self,
        server_id: ServerId,
        id: &str,
        actor: &str,
    ) -> CoreResult<()> {
        let server = self.servers.get(server_id).await?;
        let _apply = self.apply_lock.lock().await;
        let p = self
            .repo
            .pending(server_id)
            .await?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| CoreError::not_found("Pending change not found"))?;
        if let PendingKind::Install { staged, .. } = &p.change {
            let path = dirs(&server.directory, p.kind)
                .pending
                .join(safe_file_name(staged).unwrap_or_default());
            let _ = std::fs::remove_file(path);
        }
        self.repo.remove_pending(id).await?;
        self.audit
            .record(
                actor,
                "content.discard_pending",
                Some(server_id),
                Some(p.change.file_name().to_string()),
                AuditResult::Success,
                serde_json::json!({}),
            )
            .await;
        self.changed(server_id);
        Ok(())
    }

    /// Apply pending changes whenever a server stops.
    pub fn spawn_pending_applier(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let mut rx = self.events.subscribe();
        tokio::spawn(async move {
            // Changes queued before MCPanel was closed, for servers that are not running.
            if let Ok(ids) = this.repo.servers_with_pending().await {
                for id in ids {
                    this.apply_if_stopped(id).await;
                }
            }
            loop {
                match rx.recv().await {
                    Ok(env) => {
                        if let DomainEvent::ServerStateChanged {
                            server_id, state, ..
                        } = env.event
                            && !state.has_process()
                        {
                            this.apply_if_stopped(server_id).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    async fn apply_if_stopped(&self, server_id: ServerId) {
        let Ok(server) = self.servers.get(server_id).await else {
            return;
        };
        let rt = self.servers.runtime(server_id);
        let Ok(_op) = rt.begin(Operation::InstallingContent) else {
            return; // starting or busy: the launch hook applies them.
        };
        if let Err(e) = self.apply_pending(&server, &rt).await {
            tracing::warn!(target: "mcpanel::content", server = %server_id, "applying pending changes failed: {}", e.message);
        }
    }

    // ───────────────────────────── updates ─────────────────────────────

    /// Identify unmanaged files by hash and find newer compatible versions.
    pub async fn check_updates(&self, server_id: ServerId) -> CoreResult<Vec<UpdateInfo>> {
        let server = self.servers.get(server_id).await?;
        let target = self.target(&server)?;
        let list = self.list(server_id).await?;
        let d = dirs(&server.directory, target.kind);

        // Identify unmanaged jars with providers that support hash lookup.
        let unmanaged: Vec<(String, PathBuf, bool)> = list
            .entries
            .iter()
            .filter(|e| e.record.as_ref().and_then(|r| r.source.as_ref()).is_none())
            .map(|e| {
                let dir = if e.enabled { &d.active } else { &d.disabled };
                (e.file_name.clone(), dir.join(&e.file_name), e.enabled)
            })
            .collect();
        if !unmanaged.is_empty() {
            let paths: Vec<PathBuf> = unmanaged.iter().map(|(_, p, _)| p.clone()).collect();
            let hashes = tokio::task::spawn_blocking(move || {
                paths
                    .iter()
                    .map(|p| sha512_file(p).ok())
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(|e| CoreError::internal(e.to_string()))?;
            for p in self
                .providers(&target)
                .into_iter()
                .filter(|p| p.info().hash_lookup)
            {
                let wanted: Vec<String> = hashes.iter().flatten().cloned().collect();
                let found = p.identify(&wanted).await?;
                for ((file_name, _, _), hash) in unmanaged.iter().zip(&hashes) {
                    let Some(v) = hash.as_ref().and_then(|h| found.get(h)) else {
                        continue;
                    };
                    let name = p
                        .project(&v.project_id)
                        .await
                        .map(|s| s.name)
                        .unwrap_or_else(|_| file_name.clone());
                    let now = Timestamp::now();
                    self.repo
                        .upsert(&InstalledContent {
                            server_id,
                            kind: target.kind,
                            file_name: file_name.clone(),
                            name,
                            version_number: Some(v.version_number.clone()),
                            source: Some(ContentSource {
                                provider: p.info().id.clone(),
                                project_id: v.project_id.clone(),
                                version_id: v.id.clone(),
                            }),
                            sha512: hash.clone(),
                            installed_at: now,
                            updated_at: now,
                        })
                        .await?;
                }
            }
        }

        let mut updates = Vec::new();
        for rec in self.repo.installed(server_id).await? {
            let Some(src) = &rec.source else { continue };
            let Ok(p) = self.provider(&target, &src.provider) else {
                continue;
            };
            let Ok(versions) = p.versions(&src.project_id, &target).await else {
                continue;
            };
            let Some(latest) = pick(&versions) else {
                continue;
            };
            if latest.id == src.version_id || latest.file.is_none() {
                continue;
            }
            let current = versions.iter().find(|v| v.id == src.version_id);
            if current.is_none_or(|c| newer(latest, c)) {
                updates.push(UpdateInfo {
                    file_name: rec.file_name.clone(),
                    name: rec.name.clone(),
                    current_version: rec.version_number.clone(),
                    latest: latest.clone(),
                });
            }
        }
        self.changed(server_id);
        Ok(updates)
    }
}

fn pending_verb(c: &PendingKind) -> &'static str {
    match c {
        PendingKind::Install { .. } => "install",
        PendingKind::Remove { .. } => "remove",
        PendingKind::Disable { .. } => "disable",
        PendingKind::Enable { .. } => "enable",
    }
}

/// Place verified files into the content folder; replaced files go to the trash. All or
/// nothing: on failure every move is undone.
fn place_all(
    root: &Path,
    kind: ContentKind,
    items: &[(PathBuf, InstalledContent, Option<String>)],
) -> CoreResult<Vec<(Option<String>, InstalledContent)>> {
    let d = dirs(root, kind);
    ensure_real_dir(&d.active)?;
    let mut undo: Vec<(PathBuf, PathBuf)> = Vec::new(); // (current location, original location)
    let result = (|| -> CoreResult<Vec<(Option<String>, InstalledContent)>> {
        let mut out = Vec::new();
        for (staged, rec, replaces) in items {
            let dest = d.active.join(&rec.file_name);
            // The old version (if any) and a same-named file both go to the trash.
            let mut olds: Vec<PathBuf> = Vec::new();
            if let Some(old) = replaces {
                for base in [&d.active, &d.disabled] {
                    let p = base.join(old);
                    if p.is_file() {
                        olds.push(p);
                    }
                }
            }
            if dest.is_file() && !olds.contains(&dest) {
                olds.push(dest.clone());
            }
            for old in olds {
                let t = to_trash(&d.trash, &old, kind.folder())?;
                undo.push((t, old));
            }
            std::fs::rename(staged, &dest).map_err(|e| CoreError::io("Cannot install file", &e))?;
            undo.push((dest.clone(), staged.clone()));
            out.push((replaces.clone(), rec.clone()));
        }
        Ok(out)
    })();
    if result.is_err() {
        for (now, orig) in undo.iter().rev() {
            if let Err(e) = std::fs::rename(now, orig) {
                tracing::error!(target: "mcpanel::content", "rollback failed for {}: {e}", orig.display());
            }
        }
    }
    result
}

/// Perform one change on the files (server stopped).
fn apply_files(root: &Path, kind: ContentKind, change: &PendingKind) -> CoreResult<()> {
    let d = dirs(root, kind);
    match change {
        PendingKind::Install {
            staged,
            record,
            replaces,
        } => {
            let staged = d.pending.join(safe_file_name(staged)?);
            if !staged.is_file() {
                return Err(CoreError::not_found("The staged file is missing"));
            }
            place_all(
                root,
                kind,
                &[(staged, (**record).clone(), replaces.clone())],
            )
            .map(|_| ())
        }
        PendingKind::Remove { file_name } => {
            let mut found = false;
            for base in [&d.active, &d.disabled] {
                let p = base.join(file_name);
                if p.is_file() {
                    to_trash(&d.trash, &p, kind.folder())?;
                    found = true;
                }
            }
            if found {
                Ok(())
            } else {
                Err(CoreError::not_found(format!("'{file_name}' was not found")))
            }
        }
        PendingKind::Disable { file_name } => {
            ensure_real_dir(&root.join(RESERVED))?;
            ensure_real_dir(&root.join(RESERVED).join("disabled"))?;
            ensure_real_dir(&d.disabled)?;
            move_between(&d.active.join(file_name), &d.disabled.join(file_name))
        }
        PendingKind::Enable { file_name } => {
            ensure_real_dir(&d.active)?;
            move_between(&d.disabled.join(file_name), &d.active.join(file_name))
        }
    }
}

fn move_between(from: &Path, to: &Path) -> CoreResult<()> {
    if !from.is_file() {
        return Err(CoreError::not_found(format!(
            "'{}' was not found",
            from.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        )));
    }
    if to.exists() {
        return Err(CoreError::new(
            ErrorCode::PathExists,
            format!("'{}' already exists", to.display()),
        ));
    }
    std::fs::rename(from, to)
        .map_err(|e| CoreError::io("Cannot move file (is the server still using it?)", &e))
}

#[async_trait::async_trait]
impl LaunchHook for ContentService {
    async fn before_launch(&self, server: &Server, runtime: &ServerRuntime) -> CoreResult<()> {
        self.apply_pending(server, runtime).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_single_jar_components() {
        assert_eq!(
            safe_file_name("LuckPerms-5.5.jar").unwrap(),
            "LuckPerms-5.5.jar"
        );
        for bad in [
            "../x.jar",
            "a/b.jar",
            "x.zip",
            ".hidden.jar",
            "C:\\x.jar",
            "con.jar",
            "",
        ] {
            assert!(safe_file_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn place_all_rolls_back() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("plugins")).unwrap();
        std::fs::create_dir_all(root.join(".mcpanel/pending")).unwrap();
        std::fs::write(root.join("plugins/Old-1.jar"), b"old").unwrap();
        std::fs::write(root.join(".mcpanel/pending/a.jar"), b"new").unwrap();
        let rec = |f: &str| InstalledContent {
            server_id: ServerId::new(),
            kind: ContentKind::Plugin,
            file_name: f.into(),
            name: f.into(),
            version_number: None,
            source: None,
            sha512: None,
            installed_at: Timestamp(0),
            updated_at: Timestamp(0),
        };
        // The second item's staged file does not exist → everything is undone.
        let items = vec![
            (
                root.join(".mcpanel/pending/a.jar"),
                rec("Old-2.jar"),
                Some("Old-1.jar".to_string()),
            ),
            (
                root.join(".mcpanel/pending/missing.jar"),
                rec("B.jar"),
                None,
            ),
        ];
        assert!(place_all(root, ContentKind::Plugin, &items).is_err());
        assert_eq!(
            std::fs::read(root.join("plugins/Old-1.jar")).unwrap(),
            b"old"
        );
        assert!(!root.join("plugins/Old-2.jar").exists());
        assert!(root.join(".mcpanel/pending/a.jar").exists());

        // Success: the old version goes to the trash.
        let ok = vec![(
            root.join(".mcpanel/pending/a.jar"),
            rec("Old-2.jar"),
            Some("Old-1.jar".to_string()),
        )];
        place_all(root, ContentKind::Plugin, &ok).unwrap();
        assert_eq!(
            std::fs::read(root.join("plugins/Old-2.jar")).unwrap(),
            b"new"
        );
        assert!(!root.join("plugins/Old-1.jar").exists());
        assert!(
            std::fs::read_dir(root.join(".mcpanel/trash"))
                .unwrap()
                .next()
                .is_some()
        );
    }

    #[test]
    fn disable_and_enable_move_between_folders() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("plugins")).unwrap();
        std::fs::write(root.join("plugins/A.jar"), b"a").unwrap();
        apply_files(
            root,
            ContentKind::Plugin,
            &PendingKind::Disable {
                file_name: "A.jar".into(),
            },
        )
        .unwrap();
        assert!(root.join(".mcpanel/disabled/plugins/A.jar").is_file());
        assert!(
            apply_files(
                root,
                ContentKind::Plugin,
                &PendingKind::Disable {
                    file_name: "A.jar".into()
                }
            )
            .is_err()
        );
        apply_files(
            root,
            ContentKind::Plugin,
            &PendingKind::Enable {
                file_name: "A.jar".into(),
            },
        )
        .unwrap();
        assert!(root.join("plugins/A.jar").is_file());
        apply_files(
            root,
            ContentKind::Plugin,
            &PendingKind::Remove {
                file_name: "A.jar".into(),
            },
        )
        .unwrap();
        assert!(!root.join("plugins/A.jar").exists());
    }
}
