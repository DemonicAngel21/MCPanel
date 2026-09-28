//! Test harness: real core + real Windows platform + real SQLite, with `fake-mc`
//! standing in for `java.exe`.

use async_trait::async_trait;
use mcpanel_core::error::{CoreResult, ErrorCode};
use mcpanel_core::ids::ServerId;
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::model::{InstalledSoftware, LaunchConfig, Server};
use mcpanel_core::paths::AppPaths;
use mcpanel_core::software::jar::SingleJarLauncher;
use mcpanel_core::software::{
    GameVersion, InstallPlan, InstallRequest, ProviderRegistry, SoftwareBuild, SoftwareCaps,
    SoftwareCatalog, SoftwareDescriptor, SoftwareInstaller, SoftwareProvider,
};
use mcpanel_core::time::Timestamp;
use mcpanel_core::{Core, CoreDeps, CoreError};
use mcpanel_db::Database;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct NoCatalog;

#[async_trait]
impl SoftwareCatalog for NoCatalog {
    async fn game_versions(&self) -> CoreResult<Vec<GameVersion>> {
        Ok(Vec::new())
    }
    async fn builds(&self, _: &str) -> CoreResult<Vec<SoftwareBuild>> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl SoftwareInstaller for NoCatalog {
    async fn plan_install(&self, _: &InstallRequest) -> CoreResult<InstallPlan> {
        Err(CoreError::new(ErrorCode::Unsupported, "test provider"))
    }
}

pub fn registry() -> ProviderRegistry {
    let mut r = ProviderRegistry::new();
    r.register_software(SoftwareProvider {
        descriptor: SoftwareDescriptor {
            id: "fake".into(),
            display_name: "Fake".into(),
            description: "test".into(),
            caps: SoftwareCaps {
                content: vec![],
                is_proxy: false,
                log_dialect: "minecraft".into(),
                stop_command: "stop".into(),
                eula_required: true,
                requires_build_step: false,
                tps_source: None,
            },
            download_hosts: vec![],
        },
        catalog: Arc::new(NoCatalog),
        installer: Arc::new(NoCatalog),
        launcher: Arc::new(SingleJarLauncher::default()),
        detector: None,
    });
    r
}

pub struct Harness {
    pub core: Arc<Core>,
    pub db: Database,
    pub data: tempfile::TempDir,
    pub servers: tempfile::TempDir,
    pub java_id: mcpanel_core::ids::JavaRuntimeId,
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub async fn harness_at(data: tempfile::TempDir, servers: tempfile::TempDir) -> Harness {
    let (db, _) = Database::open(&data.path().join("mcpanel.db"))
        .await
        .unwrap();
    let http = mcpanel_providers::http_client().unwrap();
    let core = Core::start(CoreDeps {
        paths: AppPaths::new(data.path().to_path_buf(), servers.path().to_path_buf()),
        platform: Arc::new(mcpanel_platform::NativePlatform::new()),
        downloader: Arc::new(mcpanel_providers::HttpDownloader::new(http)),
        registry: registry(),
        repos: db.repositories(),
    })
    .await
    .unwrap();
    // fake-mc masquerades as java.exe.
    let java_dir = data.path().join("jdk").join("bin");
    std::fs::create_dir_all(&java_dir).unwrap();
    let java = java_dir.join("java.exe");
    if !java.exists() {
        std::fs::copy(env!("CARGO_BIN_EXE_fake-mc"), &java).unwrap();
    }
    let rt = core.java.add_manual(java).await.unwrap();
    assert!(rt.valid);
    assert_eq!(rt.major, 21);
    Harness {
        core,
        db,
        data,
        servers,
        java_id: rt.id,
    }
}

impl Harness {
    /// Release the database and delete the test directories (checked).
    pub async fn finish(self) {
        let Harness {
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

pub async fn harness() -> Harness {
    harness_at(tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()).await
}

pub async fn add_server(
    h: &Harness,
    name: &str,
    fake_cfg: &str,
    eula: bool,
    stop_timeout: u32,
) -> (ServerId, u16) {
    let dir = h.servers.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    std::fs::write(
        dir.join("server.properties"),
        format!("server-port={port}\nmotd=Test\n"),
    )
    .unwrap();
    if eula {
        std::fs::write(dir.join("eula.txt"), "eula=true\n").unwrap();
    }
    std::fs::write(dir.join("server.jar"), b"not a real jar").unwrap();
    std::fs::write(dir.join("fake-mc.json"), fake_cfg).unwrap();
    let server = Server {
        id: ServerId::new(),
        name: name.into(),
        directory: dir,
        software: InstalledSoftware {
            software_id: "fake".into(),
            game_version: "1.21.4".into(),
            build: None,
            jar: "server.jar".into(),
            java_min_major: Some(21),
            java_recommended_major: None,
        },
        launch: LaunchConfig {
            java_runtime_id: Some(h.java_id),
            min_memory_mb: 256,
            max_memory_mb: 512,
            jvm_args: vec![],
            server_args: vec![],
            stop_timeout_secs: stop_timeout,
        },
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    };
    h.db.repositories().servers.insert(&server).await.unwrap();
    (server.id, port)
}

pub async fn wait_state(h: &Harness, id: ServerId, want: LifecycleState, timeout: Duration) {
    let start = Instant::now();
    loop {
        let v = h.core.servers.view(id).await.unwrap();
        if v.runtime.state == want {
            return;
        }
        if start.elapsed() > timeout {
            let log: Vec<String> = h
                .core
                .servers
                .console(id)
                .snapshot(None, 50)
                .into_iter()
                .map(|l| l.text)
                .collect();
            panic!(
                "timed out waiting for {want:?}; state is {:?}\nconsole:\n{}",
                v.runtime.state,
                log.join("\n")
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub async fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

pub const T: Duration = Duration::from_secs(20);
