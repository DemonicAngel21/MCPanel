#![allow(clippy::unwrap_used)]
//! End-to-end lifecycle tests: real core + real Windows platform + real SQLite, with
//! `fake-mc` standing in for `java.exe` running a Minecraft server.

mod common;

use common::harness::*;
use mcpanel_core::console::ConsoleStream;
use mcpanel_core::error::ErrorCode;
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::paths::AppPaths;
use mcpanel_core::{Core, CoreDeps};
use mcpanel_db::Database;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// Two servers with the same port: the second must not start while the first is still
/// starting and not yet listening (this broke a real world on its first start).
#[tokio::test(flavor = "multi_thread")]
async fn a_second_server_on_the_same_port_is_refused_while_the_first_starts() {
    let h = harness().await;
    let (first, port) = add_server(&h, "first", r#"{"startup_ms": 20000}"#, true, 60).await;
    let (second, _) = add_server(&h, "second", "{}", true, 60).await;
    std::fs::write(
        h.servers.path().join("second").join("server.properties"),
        format!(
            "server-port={port}
motd=Test
"
        ),
    )
    .unwrap();
    h.core.servers.start(first, "test").await.unwrap();
    wait_state(&h, first, LifecycleState::Starting, Duration::from_secs(10)).await;
    let result = h.core.servers.start(second, "test").await;
    if result.is_ok() {
        // Clean up before failing so a regression fails instead of hanging.
        let _ = h.core.servers.stop(second, true, "test").await;
        let _ = h.core.servers.stop(first, true, "test").await;
        wait_state(&h, second, LifecycleState::Stopped, Duration::from_secs(20)).await;
        wait_state(&h, first, LifecycleState::Stopped, Duration::from_secs(20)).await;
        panic!("the second server started on the port of a starting server");
    }
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::PortInUse);
    assert!(err.message.contains("\"first\""), "{}", err.message);
    assert_eq!(
        h.core.servers.view(second).await.unwrap().runtime.state,
        LifecycleState::Stopped
    );
    h.core.servers.stop(first, true, "test").await.unwrap();
    wait_state(&h, first, LifecycleState::Stopped, Duration::from_secs(20)).await;
    // Once the first server is stopped, the port is free for the second.
    h.core.servers.start(second, "test").await.unwrap();
    wait_state(&h, second, LifecycleState::Running, Duration::from_secs(20)).await;
    h.core.servers.stop(second, false, "test").await.unwrap();
    wait_state(&h, second, LifecycleState::Stopped, Duration::from_secs(20)).await;
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
        profiles: Arc::new(TestProfiles),
        secrets: Arc::new(mcpanel_core::crypto::MemorySecretStore::default()),
        repos: db2.repositories(),
        ai_client: None,
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
