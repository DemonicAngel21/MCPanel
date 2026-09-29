#![allow(clippy::unwrap_used)]
//! Real Quilt server: MCPanel's native Knot install (vanilla jar + SHA-512-verified
//! libraries from Quilt's and Fabric's Maven + launch jar) must boot, and a Fabric mod
//! from Modrinth must load on it. Requires the Minecraft EULA acceptance flag and
//! network access:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_quilt -- --nocapture

mod common;

use common::real::{KillOnPanic, RealServer, eula_accepted, wait_job};
use mcpanel_core::content::InstallRequest;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn quilt_boots_and_loads_a_fabric_mod() {
    if !eula_accepted() {
        return;
    }
    let s = RealServer::create_with("quilt", Some("1.21.4"), &[("server-port", "25619")]).await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    let ready = s.start_and_wait().await;
    let line = s
        .console_tail(400)
        .lines()
        .find(|l| l.contains("with Quilt Loader"))
        .map(str::to_string);
    eprintln!("quilt 1.21.4: ready after {ready:?}; {line:?}");
    assert!(
        line.is_some(),
        "Quilt Loader did not start:\n{}",
        s.console_tail(60)
    );
    s.stop_and_wait().await;

    let c = &s.core.content;
    let job = c
        .install(
            s.id,
            InstallRequest {
                provider: "modrinth".into(),
                project_id: "lithium".into(),
                version_id: None,
                with_dependencies: true,
            },
            "e2e",
        )
        .await
        .unwrap();
    wait_job(&s.core, job).await;
    let list = c.list(s.id).await.unwrap();
    assert_eq!(list.folder, "mods");
    assert_eq!(list.entries.len(), 1, "{:?}", list.entries);

    s.start_and_wait().await;
    assert!(
        s.console_tail(800).to_lowercase().contains("lithium"),
        "mod loaded:\n{}",
        s.console_tail(80)
    );
    s.stop_and_wait().await;
    s.finish().await;
}
