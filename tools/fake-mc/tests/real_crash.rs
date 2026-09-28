#![allow(clippy::unwrap_used)]
//! Real-server crash recovery: the server's Java process is killed from outside
//! MCPanel (as a crash or a killed process looks to MCPanel) and must be restarted
//! automatically by the restart policy. Requires the Minecraft EULA acceptance flag:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_crash -- --nocapture

mod common;

use common::real::{KillOnPanic, RealServer, eula_accepted};
use mcpanel_core::crash::{CrashAction, RestartPolicy};
use mcpanel_core::lifecycle::LifecycleState;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn killed_server_is_restarted_by_policy() {
    if !eula_accepted() {
        return;
    }
    let s = RealServer::create(&[("server-port", "25614")]).await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    let mut p = RestartPolicy::default_for(s.id);
    p.delay_secs = 2;
    s.core.crashes.set_policy(p, "e2e").await.unwrap();
    s.start_and_wait().await;

    let pid = s
        .core
        .servers
        .view(s.id)
        .await
        .unwrap()
        .runtime
        .pid
        .unwrap();
    let out = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let start = Instant::now();
    loop {
        let v = s.core.servers.view(s.id).await.unwrap();
        if v.runtime.state == LifecycleState::Running && v.runtime.pid.is_some_and(|p| p != pid) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "not restarted; state {:?}\n{}",
            v.runtime.state,
            s.console_tail(40)
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    eprintln!("restarted after {:?}", start.elapsed());
    let c = s.core.crashes.history(s.id, 5).await.unwrap();
    eprintln!(
        "crash record: {} / {} / exit {:?}",
        c[0].kind, c[0].message, c[0].exit_code
    );
    assert_eq!(
        (c.len(), c[0].attempt, c[0].action),
        (1, 1, CrashAction::Restart)
    );
    s.stop_and_wait().await;
    s.finish().await;
}
