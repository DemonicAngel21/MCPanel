//! Real Minecraft servers for end-to-end tests (downloads server software, runs Java).
//! Only used when the person running the tests accepted the Minecraft EULA by setting
//! `MCPANEL_E2E_ACCEPT_MINECRAFT_EULA=yes`.

use mcpanel_core::ids::{JobId, ServerId};
use mcpanel_core::jobs::JobStatus;
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::paths::AppPaths;
use mcpanel_core::server::CreateServerRequest;
use mcpanel_core::{Core, CoreDeps};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn eula_accepted() -> bool {
    if std::env::var("MCPANEL_E2E_ACCEPT_MINECRAFT_EULA").as_deref() == Ok("yes") {
        return true;
    }
    eprintln!(
        "skipped: set MCPANEL_E2E_ACCEPT_MINECRAFT_EULA=yes to accept the Minecraft EULA and run this test"
    );
    false
}

pub async fn wait_job(core: &Core, id: JobId) -> serde_json::Value {
    let start = Instant::now();
    loop {
        let job = core.jobs.get(id).await.unwrap().unwrap();
        match job.status {
            JobStatus::Succeeded => return job.result.unwrap_or_default(),
            JobStatus::Running | JobStatus::Queued => {}
            other => panic!("job {other:?}: {:?}", job.error_message),
        }
        assert!(start.elapsed() < Duration::from_secs(900), "job timed out");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub struct RealServer {
    pub core: Arc<Core>,
    pub db: mcpanel_db::Database,
    pub data: tempfile::TempDir,
    pub servers: tempfile::TempDir,
    pub id: ServerId,
    pub software: String,
    pub version: String,
}

/// A failed assertion must not leave a real server running (it would also keep the
/// test process alive, since the runtime waits for the console readers).
pub struct KillOnPanic(pub Arc<Core>, pub ServerId);

impl Drop for KillOnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let (core, id) = (Arc::clone(&self.0), self.1);
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let _ = core.servers.stop(id, true, "e2e").await;
                    core.servers
                        .wait_for_exit(id, Duration::from_secs(30))
                        .await;
                })
            });
        }
    }
}

impl RealServer {
    /// Create a server of `MCPANEL_E2E_SOFTWARE` (default vanilla) at
    /// `MCPANEL_E2E_VERSION` (default: newest release) with extra properties.
    pub async fn create(properties: &[(&str, &str)]) -> RealServer {
        let software = std::env::var("MCPANEL_E2E_SOFTWARE").unwrap_or_else(|_| "vanilla".into());
        let version = std::env::var("MCPANEL_E2E_VERSION").ok();
        Self::create_with(&software, version.as_deref(), properties).await
    }

    /// Create a server of the given software (newest release when `version` is None).
    pub async fn create_with(
        software: &str,
        version: Option<&str>,
        properties: &[(&str, &str)],
    ) -> RealServer {
        let software = software.to_string();
        let data = tempfile::tempdir().unwrap();
        let servers = tempfile::tempdir().unwrap();
        let (db, _) = mcpanel_db::Database::open(&data.path().join("mcpanel.db"))
            .await
            .unwrap();
        let http = mcpanel_providers::http_client().unwrap();
        let core = Core::start(CoreDeps {
            paths: AppPaths::new(data.path().to_path_buf(), servers.path().to_path_buf()),
            platform: Arc::new(mcpanel_platform::NativePlatform::new()),
            downloader: Arc::new(mcpanel_providers::HttpDownloader::new(http.clone())),
            registry: mcpanel_providers::builtin_registry(&http),
            profiles: Arc::new(mcpanel_providers::MojangProfiles::new(
                mcpanel_providers::http_client().unwrap(),
            )),
            secrets: Arc::new(mcpanel_core::crypto::MemorySecretStore::default()),
            repos: db.repositories(),
        })
        .await
        .unwrap();
        let version = match version {
            Some(v) => v.to_string(),
            None => core.servers.game_versions(&software, false).await.unwrap()[0]
                .id
                .clone(),
        };
        let preview = core
            .servers
            .preview_install(&software, &version, None)
            .await
            .unwrap();
        let runtimes = core.java.detect().await.unwrap();
        let java = runtimes
            .iter()
            .filter(|j| j.valid && j.major >= preview.plan.java.min_major)
            .min_by_key(|j| j.major)
            .unwrap_or_else(|| {
                panic!("no Java {}+ runtime installed", preview.plan.java.min_major)
            });
        eprintln!(
            "{software} {version} with Java {} ({})",
            java.major,
            java.path.display()
        );
        let job = core
            .servers
            .create(
                CreateServerRequest {
                    name: "Real E2E".into(),
                    parent_directory: None,
                    software_id: software.clone(),
                    game_version: version.clone(),
                    build: None,
                    java_runtime_id: java.id,
                    min_memory_mb: 1024,
                    max_memory_mb: 2048,
                    jvm_args: vec![],
                    properties: properties
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    accept_eula: true,
                },
                "e2e",
            )
            .await
            .unwrap();
        let result = wait_job(&core, job).await;
        let id = result["serverId"].as_str().unwrap().parse().unwrap();
        RealServer {
            core,
            db,
            data,
            servers,
            id,
            software,
            version,
        }
    }

    pub fn console_tail(&self, n: usize) -> String {
        self.core
            .servers
            .console(self.id)
            .snapshot(None, n)
            .into_iter()
            .map(|l| l.text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub async fn start_and_wait(&self) -> Duration {
        self.core.servers.start(self.id, "e2e").await.unwrap();
        let start = Instant::now();
        loop {
            let state = self.core.servers.view(self.id).await.unwrap().runtime.state;
            if state == LifecycleState::Running {
                return start.elapsed();
            }
            assert!(
                state == LifecycleState::Starting,
                "server left Starting as {state:?}:\n{}",
                self.console_tail(60)
            );
            assert!(
                start.elapsed() < Duration::from_secs(600),
                "server did not become ready"
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub async fn stop_and_wait(&self) {
        self.core.servers.stop(self.id, false, "e2e").await.unwrap();
        assert!(
            self.core
                .servers
                .wait_for_exit(self.id, Duration::from_secs(120))
                .await
        );
        let v = self.core.servers.view(self.id).await.unwrap();
        assert_eq!(v.runtime.state, LifecycleState::Stopped, "clean stop");
        assert_eq!(v.runtime.last_exit_code, Some(0));
    }

    pub async fn finish(self) {
        let RealServer {
            core,
            db,
            data,
            servers,
            ..
        } = self;
        drop(core);
        super::cleanup(&db, vec![data, servers]).await;
    }
}
