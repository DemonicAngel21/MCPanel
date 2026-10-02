//! Composition root of the headless core.

use crate::audit::AuditLog;
use crate::backup::{BackupService, BackupServiceDeps};
use crate::content::{ContentService, ContentServiceDeps};
use crate::crash::CrashService;
use crate::error::CoreResult;
use crate::events::EventBus;
use crate::java::JavaManager;
use crate::jobs::JobManager;
use crate::monitoring::Monitor;
use crate::paths::AppPaths;
use crate::players::PlayerService;
use crate::ports::{
    AuditRepository, BackupRepository, ContentRepository, CrashRepository, Downloader,
    JavaRuntimeRepository, JobRepository, Platform, PlayerRepository, ProfileLookup,
    ServerRepository, SettingsRepository,
};
use crate::server::{ServerManager, ServerManagerDeps};
use crate::server_files::ServerFiles;
use crate::settings::SettingsService;
use crate::software::executor::PlanExecutor;
use crate::software::{ProviderRegistry, VersionCatalog};
use crate::templates::TemplateService;
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
    pub crashes: Arc<dyn CrashRepository>,
    pub notifications: Arc<dyn crate::ports::NotificationRepository>,
}

pub struct CoreDeps {
    pub paths: AppPaths,
    pub platform: Arc<dyn Platform>,
    pub downloader: Arc<dyn Downloader>,
    pub registry: ProviderRegistry,
    pub profiles: Arc<dyn ProfileLookup>,
    pub secrets: Arc<dyn crate::ports::SecretStore>,
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
    pub crashes: Arc<CrashService>,
    pub templates: Arc<TemplateService>,
    pub bedrock: Arc<crate::bedrock::BedrockService>,
    pub tunnels: Arc<crate::tunnels::PlayitTunnel>,
    /// MCPanel's own playit agent and tunnel management.
    pub playit: Arc<crate::playit_agent::PlayitAgent>,
    /// MCPanel accounts (Firebase Authentication).
    pub account: Arc<crate::account::AccountService>,
    pub notifications: Arc<crate::notify::NotificationService>,
    pub encryption: Arc<crate::crypto::EncryptionService>,
    pub cloud: Arc<crate::cloud::CloudService>,
    pub multihost: Arc<crate::multihost::MultihostService>,
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
        let settings = Arc::new(SettingsService::new(
            Arc::clone(&deps.repos.settings),
            events.clone(),
        ));
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
            executor: PlanExecutor::new(Arc::clone(&deps.downloader))
                .with_platform(Arc::clone(&deps.platform)),
            console_capacity: app_settings.console_buffer_lines as usize,
        });
        servers.initialize().await?;
        let files = Arc::new(ServerFiles::new(Arc::clone(&servers)));
        let monitor = Monitor::new(Arc::clone(&deps.platform));
        monitor.spawn(Arc::clone(&servers));
        monitor.spawn_tick_sampler(Arc::clone(&servers), Arc::clone(&settings));
        let encryption = crate::crypto::EncryptionService::new(
            Arc::clone(&deps.secrets),
            Arc::clone(&deps.repos.settings),
            events.clone(),
        );
        let backups = BackupService::new(BackupServiceDeps {
            encryption: Arc::clone(&encryption),
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
        servers.add_launch_hook(Arc::clone(&content) as Arc<dyn crate::server::LaunchHook>);
        content.spawn_pending_applier();
        let crashes = CrashService::new(
            Arc::clone(&servers),
            deps.repos.crashes,
            Arc::clone(&backups),
            Arc::clone(&audit),
            events.clone(),
        );
        crashes.spawn_listener();
        let notifications = crate::notify::NotificationService::new(
            deps.repos.notifications,
            Arc::clone(&deps.repos.settings),
            Arc::clone(&servers),
            Arc::clone(&crashes),
            events.clone(),
        );
        notifications.spawn_listener();
        notifications.spawn_disk_watch(Arc::clone(&deps.platform), Arc::clone(&backups));
        let bedrock = crate::bedrock::BedrockService::new(
            Arc::clone(&servers),
            Arc::clone(&content),
            Arc::clone(&audit),
            events.clone(),
        );
        servers.add_launch_hook(Arc::clone(&bedrock) as Arc<dyn crate::server::LaunchHook>);
        let templates = TemplateService::new(
            Arc::clone(&servers),
            Arc::clone(&backups),
            Arc::clone(&crashes),
            Arc::clone(&content),
            Arc::clone(&audit),
            Arc::clone(&versions),
        );

        let cloud = crate::cloud::CloudService::new(
            servers.registry().cloud_providers().to_vec(),
            Arc::clone(&deps.secrets),
            Arc::clone(&deps.repos.settings),
            events.clone(),
        );
        let tunnels = Arc::new(crate::tunnels::PlayitTunnel::new(
            Arc::clone(&deps.platform),
            Arc::clone(&deps.repos.settings),
        ));
        // One pipe per data folder, so a development instance never talks to the
        // installed app's agent.
        let pipe = {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in deps.paths.data_dir.to_string_lossy().to_lowercase().bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            format!(r"\\.\pipe\mcpanel-playit-{h:016x}")
        };
        let account = Arc::new(crate::account::AccountService::new(
            Arc::clone(&deps.secrets),
            Arc::clone(&deps.repos.settings),
        ));
        account.set_cloud(Arc::clone(&cloud));
        cloud.set_account_service(Arc::clone(&account));
        let playit = Arc::new(crate::playit_agent::PlayitAgent::new(
            Arc::clone(&deps.platform),
            Arc::clone(&deps.secrets),
            Arc::clone(&deps.repos.settings),
            &deps.paths.data_dir,
            pipe,
        ));
        let multihost = Arc::new(crate::multihost::MultihostService::new(
            Arc::clone(&account),
            Arc::clone(&deps.repos.settings),
            Arc::clone(&servers),
            Arc::clone(&monitor),
            Arc::clone(&deps.platform),
            Arc::clone(&audit),
            events.clone(),
        ));
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
            crashes,
            templates,
            bedrock,
            tunnels,
            playit,
            account,
            notifications,
            encryption,
            cloud,
            multihost,
        }))
    }
}
