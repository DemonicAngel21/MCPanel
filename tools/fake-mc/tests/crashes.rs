#![allow(clippy::unwrap_used)]
//! Crash auto-restart end-to-end tests: real core, SQLite and processes (`fake-mc`).

mod common;

use common::harness::*;
use mcpanel_core::crash::{CrashAction, RestartPolicy};
use mcpanel_core::ids::ServerId;
use mcpanel_core::lifecycle::LifecycleState;
use std::time::{Duration, Instant};

async fn policy(h: &Harness, id: ServerId, f: impl FnOnce(&mut RestartPolicy)) {
    let mut p = RestartPolicy::default_for(id);
    p.delay_secs = 0;
    f(&mut p);
    h.core.crashes.set_policy(p, "test").await.unwrap();
}

async fn crashes(h: &Harness, id: ServerId, n: usize) -> Vec<mcpanel_core::crash::CrashEvent> {
    let start = Instant::now();
    loop {
        let c = h.core.crashes.history(id, 20).await.unwrap();
        if c.len() >= n {
            return c;
        }
        assert!(
            start.elapsed() < T,
            "expected {n} crash records, have {}",
            c.len()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn pid(h: &Harness, id: ServerId) -> Option<u32> {
    h.core.servers.view(id).await.unwrap().runtime.pid
}

async fn wait_new_process(h: &Harness, id: ServerId, old: Option<u32>) {
    let start = Instant::now();
    loop {
        let v = h.core.servers.view(id).await.unwrap();
        if v.runtime.state == LifecycleState::Running
            && v.runtime.pid.is_some()
            && v.runtime.pid != old
        {
            return;
        }
        if start.elapsed() > T {
            let console: Vec<String> = h
                .core
                .servers
                .console(id)
                .snapshot(None, 30)
                .into_iter()
                .map(|l| l.text)
                .collect();
            let crashes = h.core.crashes.history(id, 5).await.unwrap();
            panic!(
                "no restart; state {:?}
crashes: {:?}
console:
{}",
                v.runtime.state,
                crashes
                    .iter()
                    .map(|c| (&c.kind, c.action, c.attempt))
                    .collect::<Vec<_>>(),
                console.join(
                    "
"
                )
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn crashed_server_restarts_until_the_attempt_limit() {
    let h = harness().await;
    let (id, _) = add_server(&h, "loop", r#"{"crash_report": true}"#, true, 60).await;
    policy(&h, id, |p| p.max_attempts = 2).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;

    for attempt in 1..=2u32 {
        let before = pid(&h, id).await;
        h.core.servers.send_command(id, "crash").await.unwrap();
        wait_new_process(&h, id, before).await;
        let c = crashes(&h, id, attempt as usize).await;
        assert_eq!((c[0].attempt, c[0].action), (attempt, CrashAction::Restart));
        assert_eq!(c[0].kind, "crash_report");
        assert_eq!(c[0].message, "Exception in server tick loop");
        assert_eq!(
            c[0].crash_report.as_deref(),
            Some("crash-reports/crash-2026-09-28_12.00.00-server.txt")
        );
        assert!(
            c[0].console_tail
                .iter()
                .any(|l| l.contains("IllegalStateException"))
        );
    }
    // Third crash within the window: give up.
    h.core.servers.send_command(id, "crash").await.unwrap();
    let c = crashes(&h, id, 3).await;
    assert_eq!((c[0].attempt, c[0].action), (3, CrashAction::GaveUp));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        h.core.servers.view(id).await.unwrap().runtime.state,
        LifecycleState::Crashed
    );
    let console: Vec<String> = h
        .core
        .servers
        .console(id)
        .snapshot(None, 100)
        .into_iter()
        .map(|l| l.text)
        .collect();
    assert!(
        console
            .iter()
            .any(|l| l.contains("automatic restarts stopped"))
    );
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnosed_crashes_restart_but_unfixable_failures_do_not() {
    let h = harness().await;
    // Out of memory and the watchdog are restarted.
    let (oom, _) = add_server(&h, "oom", "{}", true, 60).await;
    policy(&h, oom, |_| {}).await;
    h.core.servers.start(oom, "test").await.unwrap();
    wait_state(&h, oom, LifecycleState::Running, T).await;
    for (cmd, kind) in [("oom", "out_of_memory"), ("watchdog", "watchdog")] {
        let before = pid(&h, oom).await;
        h.core.servers.send_command(oom, cmd).await.unwrap();
        wait_new_process(&h, oom, before).await;
        let c = h.core.crashes.history(oom, 1).await.unwrap();
        assert_eq!(c[0].kind, kind);
        assert_eq!(c[0].action, CrashAction::Restart);
    }
    h.core.servers.stop(oom, false, "test").await.unwrap();
    wait_state(&h, oom, LifecycleState::Stopped, T).await;

    // A port bind failure cannot be fixed by restarting.
    let (bind, _) = add_server(&h, "bind", r#"{"fail_bind": true}"#, true, 60).await;
    h.core.servers.start(bind, "test").await.unwrap();
    wait_state(&h, bind, LifecycleState::Error, T).await;
    let c = crashes(&h, bind, 1).await;
    assert_eq!(
        (c[0].kind.as_str(), c[0].action),
        ("port_in_use", CrashAction::NotRestartable)
    );

    // Turned off: the server stays crashed.
    let (off, _) = add_server(&h, "off", "{}", true, 60).await;
    policy(&h, off, |p| p.enabled = false).await;
    h.core.servers.start(off, "test").await.unwrap();
    wait_state(&h, off, LifecycleState::Running, T).await;
    h.core.servers.send_command(off, "crash").await.unwrap();
    let c = crashes(&h, off, 1).await;
    assert_eq!(c[0].action, CrashAction::Disabled);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        h.core.servers.view(off).await.unwrap().runtime.state,
        LifecycleState::Crashed
    );
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_start_during_the_delay_wins() {
    let h = harness().await;
    let (id, _) = add_server(&h, "manual", "{}", true, 60).await;
    policy(&h, id, |p| p.delay_secs = 3).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core.servers.send_command(id, "crash").await.unwrap();
    let c = crashes(&h, id, 1).await;
    assert_eq!(c[0].action, CrashAction::Restart);
    assert!(c[0].restart_at.is_some());
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    let manual_pid = pid(&h, id).await;
    tokio::time::sleep(Duration::from_millis(3500)).await;
    let v = h.core.servers.view(id).await.unwrap();
    assert_eq!(v.runtime.state, LifecycleState::Running);
    assert_eq!(
        v.runtime.pid, manual_pid,
        "the scheduled restart did not start a second process"
    );
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_policies_are_refused() {
    let h = harness().await;
    let (id, _) = add_server(&h, "policy", "{}", true, 60).await;
    let mut p = h.core.crashes.policy(id).await.unwrap();
    assert!(p.enabled && p.max_attempts == 3 && p.window_secs == 600 && p.delay_secs == 10);
    p.max_attempts = 0;
    assert!(h.core.crashes.set_policy(p.clone(), "test").await.is_err());
    p.max_attempts = 5;
    p.stable_secs = 1;
    assert!(h.core.crashes.set_policy(p, "test").await.is_err());
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_waits_for_an_operation_that_holds_the_server() {
    let h = harness().await;
    let (id, _) = add_server(&h, "busy", "{}", true, 60).await;
    // A large file so the backup takes a moment.
    let world = h.servers.path().join("busy").join("world");
    std::fs::create_dir_all(&world).unwrap();
    let data: Vec<u8> = (0..48 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    std::fs::write(world.join("big.mca"), data).unwrap();
    policy(&h, id, |p| p.delay_secs = 1).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    let before = pid(&h, id).await;
    h.core.servers.send_command(id, "crash").await.unwrap();
    wait_state(&h, id, LifecycleState::Crashed, T).await;
    // The backup holds the server while the restart becomes due.
    let job = h
        .core
        .backups
        .create(
            mcpanel_core::backup::CreateBackupRequest {
                server_id: id,
                note: None,
            },
            "test",
        )
        .await
        .unwrap();
    wait_new_process(&h, id, before).await;
    let j = h.core.jobs.get(job).await.unwrap().unwrap();
    assert_eq!(
        j.status,
        mcpanel_core::jobs::JobStatus::Succeeded,
        "backup finished first"
    );
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}

/// Regression: Paper's watchdog logs "Stopping server" while shutting a hung server
/// down; that must still count as a crash (it was recorded as a normal stop).
#[tokio::test(flavor = "multi_thread")]
async fn paper_watchdog_shutdown_is_a_crash() {
    let h = harness().await;
    let (id, _) = add_server(&h, "hung", "{}", true, 60).await;
    policy(&h, id, |p| p.enabled = false).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core
        .servers
        .send_command(id, "paper-watchdog")
        .await
        .unwrap();
    let c = crashes(&h, id, 1).await;
    assert_eq!(c[0].kind, "watchdog");
    assert_eq!(c[0].exit_code, Some(70));
    assert_eq!(c[0].action, CrashAction::Disabled);
    assert_eq!(
        h.core.servers.view(id).await.unwrap().runtime.state,
        LifecycleState::Crashed
    );
    h.finish().await;
}

/// An in-game /stop (stopping line, exit code 0) is still a normal stop.
#[tokio::test(flavor = "multi_thread")]
async fn in_game_stop_is_not_a_crash() {
    let h = harness().await;
    let (id, _) = add_server(&h, "calm", "{}", true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    // Written to stdin directly, as a player's /stop would reach the server.
    h.core.servers.send_command(id, "say bye").await.unwrap();
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    assert!(h.core.crashes.history(id, 5).await.unwrap().is_empty());
    h.finish().await;
}
