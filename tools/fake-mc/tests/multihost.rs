#![allow(clippy::unwrap_used)]
//! Multihost service tests: local host introspection and saved remote host references.

mod common;

use common::harness::*;
use mcpanel_core::error::ErrorCode;
use mcpanel_core::events::DomainEvent;
use mcpanel_core::multihost::HostStatus;
use std::time::Duration;

async fn wait_for_hosts_changed(
    rx: &mut tokio::sync::broadcast::Receiver<mcpanel_core::events::EventEnvelope>,
) {
    let start = std::time::Instant::now();
    loop {
        let env = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        if matches!(env.event, DomainEvent::HostsChanged) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "timed out waiting for HostsChanged"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn multihost_is_available_without_mcpanel_account() {
    let h = harness().await;
    // Google Cloud uses the local gcloud credentials, not an MCPanel account.
    let status = h.core.multihost.status().await.unwrap();
    assert_eq!(status.hosts.len(), 1);

    let local = &status.hosts[0];
    assert_eq!(local.id, "local");
    assert!(local.is_local);
    assert_eq!(local.status, HostStatus::Online);

    let remote = h
        .core
        .multihost
        .add_host(
            "Remote Node".into(),
            "https://node.example.com:8443".into(),
            vec![],
            "test",
        )
        .await
        .unwrap();
    assert_eq!(remote.status, HostStatus::Unreachable);

    h.core
        .multihost
        .remove_host(&remote.id, "test")
        .await
        .unwrap();

    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn multihost_saved_hosts_are_not_reported_as_connected() {
    let h = harness().await;

    // Check signed-in status
    let status = h.core.multihost.status().await.unwrap();
    assert_eq!(status.hosts.len(), 1);

    // Input validation
    let err = h
        .core
        .multihost
        .add_host(
            "".into(),
            "https://node.example.com:8443".into(),
            vec![],
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);

    let err = h
        .core
        .multihost
        .add_host("Valid Name".into(), "".into(), vec![], "test")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);

    // Subscribe to domain events to verify HostsChanged
    let mut rx = h.core.events.subscribe();

    // Add a remote node
    let remote = h
        .core
        .multihost
        .add_host(
            "Frankfurt Node".into(),
            "https://de1.node.lan:8443".into(),
            vec!["europe".into(), "vps".into()],
            "test-user",
        )
        .await
        .unwrap();

    assert_eq!(remote.name, "Frankfurt Node");
    assert_eq!(
        remote.endpoint.as_deref(),
        Some("https://de1.node.lan:8443")
    );
    assert!(!remote.is_local);
    assert_eq!(remote.tags, vec!["europe", "vps"]);

    // Verify event received
    wait_for_hosts_changed(&mut rx).await;

    // Duplicate name rejected
    let err = h
        .core
        .multihost
        .add_host(
            "Frankfurt Node".into(),
            "https://another.node.lan:8443".into(),
            vec![],
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);

    // Duplicate endpoint rejected
    let err = h
        .core
        .multihost
        .add_host(
            "Second Node".into(),
            "https://de1.node.lan:8443".into(),
            vec![],
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);

    // List hosts returns both local and remote
    let hosts = h.core.multihost.list_hosts().await.unwrap();
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].id, "local");
    assert_eq!(hosts[1].id, remote.id);

    // Cannot remove local host
    let err = h
        .core
        .multihost
        .remove_host("local", "test")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);

    // Remove remote host
    h.core
        .multihost
        .remove_host(&remote.id, "test")
        .await
        .unwrap();

    // Verify event received
    wait_for_hosts_changed(&mut rx).await;

    // List hosts returns only local again
    let hosts = h.core.multihost.list_hosts().await.unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].id, "local");

    h.finish().await;
}
