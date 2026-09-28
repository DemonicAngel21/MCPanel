//! Composition root of the headless core.

use crate::audit::AuditLog;
use crate::backup::{BackupService, BackupServiceDeps};
use crate::content::{ContentService, ContentServiceDeps};
use crate::error::CoreResult;
use crate::events::EventBus;
use crate::java::JavaManager;
use crate::jobs::JobManager;
use crate::monitoring::Monitor;
use crate::paths::AppPaths;
use crate::players::PlayerService;
use crate::ports::{
    AuditRepository, BackupRepository, ContentRepository, Downloader, JavaRuntimeRepository,
    JobRepository, Platform, PlayerRepository, ProfileLookup, ServerRepository, SettingsRepository,
};
use crate::server::{ServerManager, ServerManagerDeps};
use crate::server_files::ServerFiles;
use crate::settings::SettingsService;
use crate::software::executor::PlanExecutor;
use crate::software::{ProviderRegistry, VersionCatalog};
use std::sync::Arc;

pub struct Repositories {
    pub servers: Arc<dyn ServerRepository>,
    pub java: Arc<dyn JavaRuntimeRepository>,
    pub audit: Arc<dyn AuditRepository>,
    pub jobs: Arc<dyn JobRepository>,
    pub settings: Arc<dyn SettingsRepository>,
    pub backups: Arc<dyn BackupRepository>,
    pub players: Arc<dyn PlayerRepository>,
    pub content: Arc<dyn ContentRepository>,
}

pub struct CoreDeps {
    pub paths: AppPaths,
    pub platform: Arc<dyn Platform>,
    pub downloader: Arc<dyn Downloader>,
    pub registry: ProviderRegistry,
    pub profiles: Arc<dyn ProfileLookup>,
    pub repos: Repositories,
}

pub struct Core {
    pub paths: AppPaths,
    pub events: EventBus,
    pub platform: Arc<dyn Platform>,
    pub versions: Arc<VersionCatalog>,
    pub jobs: Arc<JobManager>,
    pub java: Arc<JavaManager>,
    pub servers: Arc<ServerManager>,
    pub files: Arc<ServerFiles>,
    pub audit: Arc<AuditLog>,
    pub settings: Arc<SettingsService>,
    pub monitor: Arc<Monitor>,
    pub backups: Arc<BackupService>,
    pub players: Arc<PlayerService>,
    pub content: Arc<ContentService>,
}

impl Core {
    /// Build and initialise the core: recover interrupted jobs, detect orphaned server
    /// processes, and start background samplers. Must be called inside a tokio runtime.
    pub async fn start(deps: CoreDeps) -> CoreResult<Arc<Core>> {
        let events = EventBus::default();
        let audit = Arc::new(AuditLog::new(deps.repos.audit, events.clone()));
        let jobs = Arc::new(JobManager::new(deps.repos.jobs, events.clone()));
        let interrupted = jobs.recover_interrupted().await?;
        if interrupted > 0 {
            tracing::warn!(target: "mcpanel::jobs", interrupted, "jobs were interrupted by the previous shutdown");
        }
        let versions = Arc::new(VersionCatalog::new(deps.registry.reference_catalog()));
        let java = Arc::new(JavaManager::new(
            Arc::clone(&deps.platform),
            deps.repos.java,
            events.clone(),
        ));
        let settings = Arc::new(SettingsService::new(deps.repos.settings, events.clone()));
        let app_settings = settings.get().await?;

        let servers = ServerManager::new(ServerManagerDeps {
            repo: deps.repos.servers,
            platform: Arc::clone(&deps.platform),
            registry: deps.registry,
            versions: Arc::clone(&versions),
            jobs: Arc::clone(&jobs),
            java: Arc::clone(&java),
            events: events.clone(),
            audit: Arc::clone(&audit),
            paths: deps.paths.clone(),
            executor: PlanExecutor::new(Arc::clone(&deps.downloader)),
            console_capacity: app_settings.console_buffer_lines as usize,
        });
        servers.initialize().await?;
        let files = Arc::new(ServerFiles::new(Arc::clone(&servers)));
        let monitor = Monitor::new(Arc::clone(&deps.platform));
        monitor.spawn(Arc::clone(&servers));
        let backups = BackupService::new(BackupServiceDeps {
            repo: deps.repos.backups,
            servers: Arc::clone(&servers),
            jobs: Arc::clone(&jobs),
            events: events.clone(),
            audit: Arc::clone(&audit),
            settings: Arc::clone(&settings),
            paths: deps.paths.clone(),
            platform: Arc::clone(&deps.platform),
        });
        let interrupted = backups.recover_interrupted().await?;
        if interrupted > 0 {
            tracing::warn!(target: "mcpanel::backup", interrupted, "backups were interrupted by the previous shutdown");
        }
        backups.spawn_scheduler();
        let players = PlayerService::new(
            Arc::clone(&servers),
            deps.repos.players,
            deps.profiles,
            Arc::clone(&audit),
            events.clone(),
        );
        players.spawn_session_tracker();
        let content = ContentService::new(ContentServiceDeps {
            servers: Arc::clone(&servers),
            repo: deps.repos.content,
            downloader: deps.downloader,
            jobs: Arc::clone(&jobs),
            audit: Arc::clone(&audit),
            events: events.clone(),
        });
        servers.set_launch_hook(Arc::clone(&content) as Arc<dyn crate::server::LaunchHook>);
        content.spawn_pending_applier();

        Ok(Arc::new(Core {
            paths: deps.paths,
            events,
            platform: deps.platform,
            versions,
            jobs,
            java,
            servers,
            files,
            audit,
            settings,
            monitor,
            backups,
            players,
            content,
        }))
    }
}
