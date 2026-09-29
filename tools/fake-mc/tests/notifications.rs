#![allow(clippy::unwrap_used)]
//! Notification rules end-to-end: real core, SQLite and processes (`fake-mc`).

mod common;

use common::harness::*;
use mcpanel_core::events::DomainEvent;
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::notify::{Category, Channels, Notification};
use std::time::{Duration, Instant};

async fn wait_for(h: &Harness, f: impl Fn(&Notification) -> bool) -> Notification {
    let start = Instant::now();
    loop {
        if let Some(n) = h
            .core
            .notifications
            .list(50)
            .await
            .unwrap()
            .into_iter()
            .find(&f)
        {
            return n;
        }
        assert!(start.elapsed() < T, "notification not produced");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_lands_in_the_inbox_and_on_the_desktop() {
    let h = harness().await;
    let mut rx = h.core.events.subscribe();
    let (id, _) = add_server(&h, "boom", r#"{"crash_report": true}"#, true, 60).await;
    let mut policy = mcpanel_core::crash::RestartPolicy::default_for(id);
    policy.delay_secs = 0;
    h.core.crashes.set_policy(policy, "test").await.unwrap();
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core.servers.send_command(id, "crash").await.unwrap();

    let n = wait_for(&h, |n| n.category == Category::Crash).await;
    assert_eq!(n.title, "boom crashed");
    assert!(
        n.body.starts_with("Exception in server tick loop"),
        "{}",
        n.body
    );
    assert!(n.body.contains("restarts it"), "{}", n.body);
    assert_eq!(n.server_id, Some(id));
    assert!(!n.read);
    assert_eq!(h.core.notifications.unread_count().await.unwrap(), 1);

    // The host is told to show it on the desktop (default rule for crashes).
    let start = Instant::now();
    loop {
        let env = tokio::time::timeout(T, rx.recv()).await.unwrap().unwrap();
        if let DomainEvent::NotificationCreated {
            desktop, inbox, id, ..
        } = env.event
        {
            assert!(desktop && inbox);
            assert_eq!(id, n.id);
            break;
        }
        assert!(start.elapsed() < T);
    }

    h.core.notifications.mark_read(None).await.unwrap();
    assert_eq!(h.core.notifications.unread_count().await.unwrap(), 0);
    // The automatic restart brings it back; stop it for good.
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.core.notifications.clear().await.unwrap();
    assert!(h.core.notifications.list(50).await.unwrap().is_empty());
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rules_decide_what_is_kept() {
    let h = harness().await;
    let (id, _) = add_server(&h, "hub", "{}", true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;

    // Player joins are off by default.
    h.core.servers.send_command(id, "join Alex").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(h.core.notifications.list(50).await.unwrap().is_empty());

    let mut prefs = h.core.notifications.prefs().await.unwrap();
    prefs.player_joined = Channels {
        inbox: true,
        desktop: false,
    };
    h.core.notifications.set_prefs(prefs).await.unwrap();
    assert_eq!(h.core.notifications.prefs().await.unwrap(), prefs);
    h.core.servers.send_command(id, "join Steve").await.unwrap();
    let n = wait_for(&h, |n| n.category == Category::PlayerJoined).await;
    assert_eq!(n.title, "Steve joined hub");

    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}
