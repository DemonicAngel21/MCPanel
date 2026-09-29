#![allow(clippy::unwrap_used)]
//! Real Forge/NeoForge servers: the official installer (SHA-512 verified) runs through
//! MCPanel's plan executor and the server starts from its argument file; a mod from
//! Modrinth loads. Requires the Minecraft EULA acceptance flag and network access:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_forge -- --nocapture --test-threads 1

mod common;

use common::real::{KillOnPanic, RealServer, eula_accepted, wait_job};
use mcpanel_core::content::InstallRequest;
use std::sync::Arc;
use std::time::Instant;

async fn boots(software: &str, version: &str, port: &str) -> (RealServer, KillOnPanic) {
    let t = Instant::now();
    let s = RealServer::create_with(software, Some(version), &[("server-port", port)]).await;
    eprintln!("{software} {version}: installed in {:?}", t.elapsed());
    let guard = KillOnPanic(Arc::clone(&s.core), s.id);
    let ready = s.start_and_wait().await;
    eprintln!("{software} {version}: ready after {ready:?}");
    let dir = s.core.servers.get(s.id).await.unwrap().directory;
    assert!(!dir.join("installer.jar").exists(), "installer removed");
    assert!(
        !dir.join("installer.jar.log").exists(),
        "installer log removed"
    );
    (s, guard)
}

#[tokio::test(flavor = "multi_thread")]
async fn neoforge_installs_boots_and_loads_a_mod() {
    if !eula_accepted() {
        return;
    }
    let (s, _guard) = boots("neoforge", "1.21.4", "25617").await;
    s.stop_and_wait().await;
    let c = &s.core.content;
    let job = c
        .install(
            s.id,
            InstallRequest {
                provider: "modrinth".into(),
                project_id: "ferrite-core".into(),
                version_id: None,
                with_dependencies: true,
            },
            "e2e",
        )
        .await
        .unwrap();
    wait_job(&s.core, job).await;
    let list = c.list(s.id).await.unwrap();
    eprintln!(
        "mods: {:?}",
        list.entries
            .iter()
            .map(|e| &e.file_name)
            .collect::<Vec<_>>()
    );
    assert_eq!(list.folder, "mods");
    s.start_and_wait().await;
    assert!(
        s.console_tail(800).to_lowercase().contains("ferritecore"),
        "mod loaded:\n{}",
        s.console_tail(60)
    );
    s.stop_and_wait().await;
    s.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn forge_installs_and_boots() {
    if !eula_accepted() {
        return;
    }
    let (s, _guard) = boots("forge", "1.20.1", "25618").await;
    s.stop_and_wait().await;
    s.finish().await;
}
