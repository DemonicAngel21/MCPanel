#![allow(clippy::unwrap_used)]
//! Real Fabric servers: MCPanel's native Fabric install (vanilla jar + SHA-512-verified
//! libraries + launch jar) must boot on an unobfuscated (26.x) and an obfuscated
//! (1.21.x, with intermediary) Minecraft version, and mods from Modrinth must load.
//! Requires the Minecraft EULA acceptance flag and network access:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_fabric -- --nocapture --test-threads 1

mod common;

use common::real::{KillOnPanic, RealServer, eula_accepted, wait_job};
use mcpanel_core::content::InstallRequest;
use mcpanel_core::jobs::JobStatus;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn req(project: &str) -> InstallRequest {
    InstallRequest {
        provider: "modrinth".into(),
        project_id: project.into(),
        version_id: None,
        with_dependencies: true,
    }
}

async fn boots(version: &str, port: &str) -> (RealServer, KillOnPanic) {
    let s = RealServer::create_with("fabric", Some(version), &[("server-port", port)]).await;
    let guard = KillOnPanic(Arc::clone(&s.core), s.id);
    let ready = s.start_and_wait().await;
    let tail = s.console_tail(400);
    let line = tail
        .lines()
        .find(|l| l.contains("with Fabric Loader"))
        .map(str::to_string);
    eprintln!("fabric {version}: ready after {ready:?}; {line:?}");
    assert!(
        line.is_some(),
        "Fabric Loader did not start:\n{}",
        s.console_tail(60)
    );
    (s, guard)
}

#[tokio::test(flavor = "multi_thread")]
async fn fabric_boots_and_loads_mods_from_modrinth() {
    if !eula_accepted() {
        return;
    }
    let (s, _guard) = boots("26.3", "25615").await;
    s.stop_and_wait().await;

    let c = &s.core.content;
    let hits = c
        .search(s.id, "modrinth", "sodium", Default::default(), 0, 10)
        .await
        .unwrap()
        .hits;
    assert!(
        !hits.iter().any(|h| h.slug == "sodium"),
        "client-only mods are hidden"
    );

    for project in ["lithium", "fabric-api"] {
        wait_job(&s.core, c.install(s.id, req(project), "e2e").await.unwrap()).await;
    }
    let list = c.list(s.id).await.unwrap();
    eprintln!(
        "mods: {:?}",
        list.entries
            .iter()
            .map(|e| (
                &e.file_name,
                e.descriptor.as_ref().and_then(|d| d.name.clone())
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(list.folder, "mods");
    assert_eq!(list.entries.len(), 2);

    // A client-only mod is refused by the descriptor check even if requested directly.
    let job = c.install(s.id, req("sodium"), "e2e").await.unwrap();
    let start = Instant::now();
    let j = loop {
        let j = s.core.jobs.get(job).await.unwrap().unwrap();
        if j.status.is_terminal() {
            break j;
        }
        assert!(start.elapsed() < Duration::from_secs(120));
        tokio::time::sleep(Duration::from_millis(300)).await;
    };
    eprintln!("sodium: {:?} {:?}", j.status, j.error_message);
    assert_eq!(j.status, JobStatus::Failed);
    assert!(j.error_message.unwrap_or_default().contains("client"));

    s.start_and_wait().await;
    let tail = s.console_tail(600);
    let loaded = tail
        .lines()
        .find(|l| l.contains("Loading") && l.contains("mods"))
        .map(str::to_string);
    eprintln!("{loaded:?}");
    assert!(
        tail.contains("lithium") && tail.contains("fabric-api"),
        "mods listed at start:\n{}",
        s.console_tail(80)
    );
    s.stop_and_wait().await;
    s.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fabric_boots_on_an_obfuscated_version() {
    if !eula_accepted() {
        return;
    }
    let (s, _guard) = boots("1.21.4", "25616").await;
    s.stop_and_wait().await;
    s.finish().await;
}
