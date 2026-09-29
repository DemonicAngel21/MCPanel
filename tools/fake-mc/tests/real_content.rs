#![allow(clippy::unwrap_used)]
//! Real plugin installs: plugins from Modrinth and Hangar are installed onto a real
//! Paper server through MCPanel's pipeline, and Paper must load them. Requires the
//! Minecraft EULA acceptance flag and network access:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_content -- --nocapture
//!
//! Paper 26.x only has experimental builds, so this uses Paper 1.21.11 (override with
//! MCPANEL_E2E_VERSION).

mod common;

use common::real::{KillOnPanic, RealServer, eula_accepted, wait_job};
use mcpanel_core::content::InstallRequest;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn req(provider: &str, project: &str) -> InstallRequest {
    InstallRequest {
        provider: provider.into(),
        project_id: project.into(),
        version_id: None,
        with_dependencies: true,
    }
}

async fn wait_console(s: &RealServer, needle: &str) {
    let start = Instant::now();
    while !s.console_tail(400).contains(needle) {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "console never showed {needle:?}\n{}",
            s.console_tail(60)
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn plugins_from_modrinth_and_hangar_load_on_paper() {
    if !eula_accepted() {
        return;
    }
    let version = std::env::var("MCPANEL_E2E_VERSION").unwrap_or_else(|_| "1.21.11".into());
    let s = RealServer::create_with("paper", Some(&version), &[("server-port", "25613")]).await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    let c = &s.core.content;

    // Search both providers the way the UI does.
    let chunky = c
        .search(s.id, "modrinth", "chunky", Default::default(), 0, 10)
        .await
        .unwrap()
        .hits
        .into_iter()
        .find(|h| h.slug == "chunky")
        .expect("Chunky on Modrinth");
    let via = c
        .search(s.id, "hangar", "ViaVersion", Default::default(), 0, 10)
        .await
        .unwrap()
        .hits
        .into_iter()
        .find(|h| h.slug == "ViaVersion")
        .expect("ViaVersion on Hangar");

    // Installed while stopped: placed directly.
    let plan = c.plan(s.id, &req("modrinth", &chunky.id)).await.unwrap();
    eprintln!(
        "plan: {:?}",
        plan.items
            .iter()
            .map(|i| (&i.project.name, &i.version.version_number))
            .collect::<Vec<_>>()
    );
    wait_job(
        &s.core,
        c.install(s.id, req("modrinth", &chunky.id), "e2e")
            .await
            .unwrap(),
    )
    .await;
    s.start_and_wait().await;
    wait_console(&s, "Enabling Chunky").await;

    // Installed while running: queued, applied on stop, loaded on the next start.
    let r = wait_job(
        &s.core,
        c.install(s.id, req("hangar", &via.id), "e2e")
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(r["deferred"], true);
    assert!(!s.console_tail(400).contains("Enabling ViaVersion"));
    // Disabling a loaded plugin is queued too (its jar is locked by the JVM).
    assert!(
        c.set_enabled(
            s.id,
            &c.list(s.id).await.unwrap().entries[0].file_name,
            false,
            "e2e"
        )
        .await
        .unwrap()
    );
    s.stop_and_wait().await;
    let start = Instant::now();
    while !c.pending(s.id).await.unwrap().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let list = c.list(s.id).await.unwrap();
    eprintln!(
        "after stop: {:?}",
        list.entries
            .iter()
            .map(|e| (&e.file_name, e.enabled, e.record.as_ref().map(|r| &r.name)))
            .collect::<Vec<_>>()
    );
    assert!(
        list.entries
            .iter()
            .any(|e| e.file_name.starts_with("ViaVersion") && e.enabled)
    );
    assert!(
        list.entries
            .iter()
            .any(|e| e.file_name.starts_with("Chunky") && !e.enabled)
    );

    s.start_and_wait().await;
    wait_console(&s, "Enabling ViaVersion").await;
    let tail = s.console_tail(600);
    let after_restart = &tail[tail.rfind("Starting Real E2E").unwrap_or(0)..];
    assert!(
        !after_restart.contains("Enabling Chunky"),
        "disabled plugin is not loaded"
    );

    // The update check identifies nothing new and reports no errors.
    let updates = c.check_updates(s.id).await.unwrap();
    eprintln!("updates: {updates:?}");
    s.stop_and_wait().await;
    s.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_from_spiget_loads_on_paper() {
    if !eula_accepted() {
        return;
    }
    let s = RealServer::create_with("paper", Some("1.21.11"), &[("server-port", "25614")]).await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    let c = &s.core.content;
    let plan = c.plan(s.id, &req("spiget", "28140")).await.unwrap();
    assert!(
        plan.warnings.iter().any(|w| w.contains("unverified")),
        "{:?}",
        plan.warnings
    );
    wait_job(
        &s.core,
        c.install(s.id, req("spiget", "28140"), "e2e")
            .await
            .unwrap(),
    )
    .await;
    let list = c.list(s.id).await.unwrap();
    let e = &list.entries[0];
    eprintln!("{} {:?}", e.file_name, e.descriptor);
    assert_eq!(
        e.descriptor.as_ref().and_then(|d| d.name.as_deref()),
        Some("LuckPerms")
    );
    s.start_and_wait().await;
    wait_console(&s, "Enabling LuckPerms").await;
    s.stop_and_wait().await;
    s.finish().await;
}
