//! Composition root of the headless core.

use crate::audit::AuditLog;
use crate::error::CoreResult;
use crate::events::EventBus;
use crate::java::JavaManager;
use crate::jobs::JobManager;
use crate::monitoring::Monitor;
use crate::paths::AppPaths;
use crate::ports::{
    AuditRepository, Downloader, JavaRuntimeRepository, JobRepository, Platform, ServerRepository,
    SettingsRepository,
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
}

pub struct CoreDeps {
    pub paths: AppPaths,
    pub platform: Arc<dyn Platform>,
    pub downloader: Arc<dyn Downloader>,
    pub registry: ProviderRegistry,
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
            executor: PlanExecutor::new(deps.downloader),
            console_capacity: app_settings.console_buffer_lines as usize,
        });
        servers.initialize().await?;
        let files = Arc::new(ServerFiles::new(Arc::clone(&servers)));
        let monitor = Monitor::new(Arc::clone(&deps.platform));
        monitor.spawn(Arc::clone(&servers));

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
        }))
    }
}
