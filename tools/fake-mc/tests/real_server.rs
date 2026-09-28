//! Real Minecraft server end-to-end test (downloads server software and runs it).
//!
//! Running a Minecraft server requires accepting the Minecraft EULA
//! (https://aka.ms/MinecraftEULA). This test only runs when the person running it has
//! explicitly accepted it by setting `MCPANEL_E2E_ACCEPT_MINECRAFT_EULA=yes`.
//!
//! Optional: `MCPANEL_E2E_SOFTWARE` (vanilla|paper|purpur, default vanilla) and
//! `MCPANEL_E2E_VERSION` (default: newest release the software offers).
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_server -- --nocapture
#![allow(clippy::unwrap_used)]

mod common;

use mcpanel_core::ids::JobId;
use mcpanel_core::jobs::JobStatus;
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::paths::AppPaths;
use mcpanel_core::server::CreateServerRequest;
use mcpanel_core::{Core, CoreDeps};
use std::sync::Arc;
use std::time::{Duration, Instant};

async fn wait_job(core: &Core, id: JobId) -> serde_json::Value {
    let start = Instant::now();
    loop {
        let job = core.jobs.get(id).await.unwrap().unwrap();
        match job.status {
            JobStatus::Succeeded => return job.result.unwrap(),
            JobStatus::Running | JobStatus::Queued => {}
            other => panic!("job {other:?}: {:?}", job.error_message),
        }
        assert!(
            start.elapsed() < Duration::from_secs(900),
            "download timed out"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn create_start_and_stop_a_real_server() {
    if std::env::var("MCPANEL_E2E_ACCEPT_MINECRAFT_EULA").as_deref() != Ok("yes") {
        eprintln!(
            "skipped: set MCPANEL_E2E_ACCEPT_MINECRAFT_EULA=yes to accept the Minecraft EULA and run this test"
        );
        return;
    }
    let software = std::env::var("MCPANEL_E2E_SOFTWARE").unwrap_or_else(|_| "vanilla".into());
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
        repos: db.repositories(),
    })
    .await
    .unwrap();

    let version = match std::env::var("MCPANEL_E2E_VERSION") {
        Ok(v) => v,
        Err(_) => core.servers.game_versions(&software, false).await.unwrap()[0]
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
        .unwrap_or_else(|| panic!("no Java {}+ runtime installed", preview.plan.java.min_major));
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
                properties: vec![
                    ("server-port".into(), "25611".into()),
                    ("online-mode".into(), "true".into()),
                ],
                accept_eula: true,
            },
            "e2e",
        )
        .await
        .unwrap();
    let result = wait_job(&core, job).await;
    let id = result["serverId"].as_str().unwrap().parse().unwrap();

    core.servers.start(id, "e2e").await.unwrap();
    let start = Instant::now();
    loop {
        let state = core.servers.view(id).await.unwrap().runtime.state;
        if state == LifecycleState::Running {
            break;
        }
        assert!(
            state == LifecycleState::Starting,
            "server left Starting as {state:?}:\n{}",
            core.servers
                .console(id)
                .snapshot(None, 60)
                .into_iter()
                .map(|l| l.text)
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            start.elapsed() < Duration::from_secs(600),
            "server did not become ready"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    eprintln!("ready after {:?}", start.elapsed());
    core.servers.send_command(id, "list").await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    core.servers.stop(id, false, "e2e").await.unwrap();
    assert!(
        core.servers
            .wait_for_exit(id, Duration::from_secs(120))
            .await
    );
    let v = core.servers.view(id).await.unwrap();
    assert_eq!(v.runtime.state, LifecycleState::Stopped, "clean stop");
    assert_eq!(v.runtime.last_exit_code, Some(0));
    let props = core.servers.properties(id).await.unwrap();
    assert!(
        props.file_exists && props.properties.len() > 20,
        "server generated server.properties"
    );
    drop(core);
    common::cleanup(&db, vec![data, servers]).await;
}
