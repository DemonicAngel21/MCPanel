#![allow(clippy::unwrap_used)]
//! End-to-end lifecycle tests: real core + real Windows platform + real SQLite, with
//! `fake-mc` standing in for `java.exe` running a Minecraft server.

mod common;

use async_trait::async_trait;
use mcpanel_core::console::ConsoleStream;
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
use std::path::PathBuf;
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

fn registry() -> ProviderRegistry {
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

struct Harness {
    core: Arc<Core>,
    db: Database,
    data: tempfile::TempDir,
    servers: tempfile::TempDir,
    java_id: mcpanel_core::ids::JavaRuntimeId,
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn harness_at(data: tempfile::TempDir, servers: tempfile::TempDir) -> Harness {
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
    async fn finish(self) {
        let Harness {
            core,
            db,
            data,
            servers,
            ..
        } = self;
        drop(core);
        common::cleanup(&db, vec![data, servers]).await;
    }
}

async fn harness() -> Harness {
    harness_at(tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()).await
}

async fn add_server(
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

async fn wait_state(h: &Harness, id: ServerId, want: LifecycleState, timeout: Duration) {
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

async fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

const T: Duration = Duration::from_secs(20);

#[tokio::test(flavor = "multi_thread")]
async fn start_ready_players_and_graceful_stop() {
    let h = harness().await;
    let (id, _) = add_server(&h, "basic", "{}", true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;

    h.core.servers.send_command(id, "join Alex").await.unwrap();
    let servers = Arc::clone(&h.core.servers);
    assert!(
        wait_until(T, || futures_lite_block(servers.view(id))
            .unwrap()
            .runtime
            .online_players
            == vec!["Alex".to_string()])
        .await,
        "player tracked"
    );
    h.core.servers.send_command(id, "leave Alex").await.unwrap();

    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    let v = h.core.servers.view(id).await.unwrap();
    assert_eq!(v.runtime.last_exit_code, Some(0));
    assert!(v.runtime.online_players.is_empty());

    // The audit log recorded the actions.
    let audit = h
        .core
        .audit
        .query(&mcpanel_core::model::AuditQuery {
            server_id: Some(id),
            before: None,
            limit: 50,
        })
        .await
        .unwrap();
    let actions: Vec<_> = audit.iter().map(|a| a.action.as_str()).collect();
    assert!(
        actions.contains(&"server.start") && actions.contains(&"server.stop"),
        "{actions:?}"
    );
    h.finish().await;
}

/// Block on a future from a sync closure (tests only).
fn futures_lite_block<F: std::future::Future>(f: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_is_detected() {
    let h = harness().await;
    let (id, _) = add_server(&h, "crash", "{}", true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core.servers.send_command(id, "crash").await.unwrap();
    wait_state(&h, id, LifecycleState::Crashed, T).await;
    assert_eq!(
        h.core
            .servers
            .view(id)
            .await
            .unwrap()
            .runtime
            .last_exit_code,
        Some(1)
    );
    // A crashed server can be started again.
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core.servers.stop(id, true, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn out_of_memory_is_a_crash_with_diagnosis() {
    let h = harness().await;
    let (id, _) = add_server(&h, "oom", "{}", true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core.servers.send_command(id, "oom").await.unwrap();
    wait_state(&h, id, LifecycleState::Crashed, T).await;
    let d = h
        .core
        .servers
        .view(id)
        .await
        .unwrap()
        .runtime
        .diagnosis
        .unwrap();
    assert_eq!(d.kind, "out_of_memory");
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unresponsive_server_is_terminated_after_grace_period() {
    let h = harness().await;
    let (id, _) = add_server(&h, "stubborn", r#"{"ignore_stop": true}"#, true, 5).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    let started = Instant::now();
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    assert!(
        started.elapsed() >= Duration::from_secs(5),
        "waited for the grace period"
    );
    let lines = h.core.servers.console(id).snapshot(None, 100);
    assert!(
        lines
            .iter()
            .any(|l| l.text.contains("did not stop within 5s"))
    );
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn port_bind_failure_is_an_error_with_diagnosis() {
    let h = harness().await;
    let (id, _) = add_server(&h, "bind", r#"{"fail_bind": true}"#, true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Error, T).await;
    let d = h
        .core
        .servers
        .view(id)
        .await
        .unwrap()
        .runtime
        .diagnosis
        .unwrap();
    assert_eq!(d.kind, "port_in_use");
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn preflight_rejects_missing_eula_and_busy_port() {
    let h = harness().await;
    let (id, _) = add_server(&h, "noeula", "{}", false, 60).await;
    let err = h.core.servers.start(id, "test").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::EulaNotAccepted);
    h.core.servers.accept_eula(id, "test").await.unwrap();

    let (id2, port) = add_server(&h, "busy", "{}", true, 60).await;
    let _blocker = std::net::TcpListener::bind(("0.0.0.0", port)).unwrap();
    let err = h.core.servers.start(id2, "test").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::PortInUse);
    assert_eq!(
        h.core.servers.view(id2).await.unwrap().runtime.state,
        LifecycleState::Stopped
    );
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn console_flood_does_not_block_or_lose_the_tail() {
    let h = harness().await;
    let (id, _) = add_server(&h, "flood", "{}", true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    let hub = h.core.servers.console(id);
    let mut sub = hub.subscribe(None, 0);
    let started = Instant::now();
    h.core
        .servers
        .send_command(id, "flood 200000")
        .await
        .unwrap();
    let mut seen_done = false;
    let mut delivered = 0usize;
    while started.elapsed() < Duration::from_secs(60) && !seen_done {
        if let Some(batch) = tokio::time::timeout(
            Duration::from_secs(5),
            sub.next_batch(500, Duration::from_millis(50)),
        )
        .await
        .ok()
        .flatten()
        {
            delivered += batch.lines.len();
            seen_done = batch.lines.iter().any(|l| l.text.ends_with("flood done"));
        }
    }
    assert!(
        seen_done,
        "tail of the flood was delivered ({delivered} lines)"
    );
    // Server stayed responsive.
    h.core
        .servers
        .send_command(id, "say still alive")
        .await
        .unwrap();
    let hub2 = Arc::clone(&hub);
    assert!(
        wait_until(T, || hub2
            .snapshot(None, 10)
            .iter()
            .any(|l| l.text.contains("still alive")))
        .await
    );
    assert_eq!(
        hub.snapshot(None, usize::MAX).len(),
        20_000,
        "ring buffer is bounded"
    );
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_brings_the_server_back() {
    let h = harness().await;
    let (id, _) = add_server(&h, "restart", "{}", true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    let pid1 = h.core.servers.view(id).await.unwrap().runtime.pid.unwrap();
    h.core.servers.restart(id, "test").await.unwrap();
    let servers = Arc::clone(&h.core.servers);
    assert!(
        wait_until(T, || {
            let v = futures_lite_block(servers.view(id)).unwrap();
            v.runtime.state == LifecycleState::Running && v.runtime.pid.is_some_and(|p| p != pid1)
        })
        .await,
        "server restarted with a new process"
    );
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn orphaned_server_is_detected_by_a_new_session_and_can_be_force_stopped() {
    let data = tempfile::tempdir().unwrap();
    let servers = tempfile::tempdir().unwrap();
    let data_path: PathBuf = data.path().to_path_buf();
    let servers_path: PathBuf = servers.path().to_path_buf();
    let h1 = harness_at(data, servers).await;
    let (id, _) = add_server(&h1, "orphan", "{}", true, 60).await;
    h1.core.servers.start(id, "test").await.unwrap();
    wait_state(&h1, id, LifecycleState::Running, T).await;

    // A second MCPanel session on the same database (as after a crash/restart).
    let (db2, _) = Database::open(&data_path.join("mcpanel.db")).await.unwrap();
    let core2 = Core::start(CoreDeps {
        paths: AppPaths::new(data_path.clone(), servers_path.clone()),
        platform: Arc::new(mcpanel_platform::NativePlatform::new()),
        downloader: Arc::new(mcpanel_providers::HttpDownloader::new(
            mcpanel_providers::http_client().unwrap(),
        )),
        registry: registry(),
        repos: db2.repositories(),
    })
    .await
    .unwrap();
    let v = core2.servers.view(id).await.unwrap();
    assert_eq!(v.runtime.state, LifecycleState::Detached);
    assert!(!v.runtime.console_attached);
    // Graceful stop is impossible without a console; force stop works.
    assert_eq!(
        core2
            .servers
            .stop(id, false, "test")
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    core2.servers.stop(id, true, "test").await.unwrap();
    let core2b = Arc::clone(&core2);
    assert!(
        wait_until(T, || futures_lite_block(core2b.servers.view(id))
            .unwrap()
            .runtime
            .state
            == LifecycleState::Stopped)
        .await,
        "detached server observed as stopped"
    );
    let lines = core2.servers.console(id).snapshot(None, 20);
    assert!(
        lines
            .iter()
            .any(|l| l.stream == ConsoleStream::System && l.text.contains("kept running"))
    );
    drop((core2, core2b));
    db2.close().await;
    h1.finish().await;
}
