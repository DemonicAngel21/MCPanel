#![allow(clippy::unwrap_used)]
//! Multihost service tests: local host introspection, account gating, remote host
//! enrollment, latency pinging, and lifecycle management.

mod common;

use common::harness::*;
use mcpanel_core::account::AccountProfile;
use mcpanel_core::error::ErrorCode;
use mcpanel_core::events::DomainEvent;
use mcpanel_core::multihost::HostStatus;
use mcpanel_core::ports::SecretStore;
use secrecy::SecretString;
use std::time::Duration;

fn sign_in(h: &Harness, uid: &str, email: &str) {
    h.secrets
        .set("account-refresh-token", &SecretString::from("fake-token"))
        .unwrap();
    let profile = AccountProfile {
        uid: uid.to_string(),
        email: Some(email.to_string()),
        email_verified: true,
        display_name: Some("Cluster Admin".to_string()),
        photo_url: None,
        provider: "password".to_string(),
    };
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            h.db.repositories()
                .settings
                .set("account.profile", &serde_json::to_value(profile).unwrap())
                .await
                .unwrap();
        });
    });
}

fn sign_out(h: &Harness) {
    let _ = h.secrets.delete("account-refresh-token");
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let _ =
                h.db.repositories()
                    .settings
                    .set("account.profile", &serde_json::Value::Null)
                    .await;
        });
    });
}

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
async fn multihost_requires_account_when_signed_out() {
    let h = harness().await;
    sign_out(&h);

    // Status is always accessible: reports account_required and local host.
    let status = h.core.multihost.status().await.unwrap();
    assert!(status.account_required);
    assert!(!status.signed_in);
    assert_eq!(status.user_email, None);
    assert_eq!(status.hosts.len(), 1);

    let local = &status.hosts[0];
    assert_eq!(local.id, "local");
    assert!(local.is_local);
    assert_eq!(local.status, HostStatus::Online);

    // Protected actions must fail with PermissionDenied
    let err = h.core.multihost.list_hosts().await.unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);

    let err = h
        .core
        .multihost
        .add_host(
            "Remote Node".into(),
            "https://node.example.com:8443".into(),
            None,
            vec![],
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);

    let err = h
        .core
        .multihost
        .remove_host("any-id", "test")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);

    let err = h.core.multihost.ping_host("local").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);

    let err = h
        .core
        .multihost
        .generate_enrollment_token()
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);

    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn multihost_operations_when_signed_in() {
    let h = harness().await;
    sign_in(&h, "user_abc123", "admin@mcpanel.net");

    // Check signed-in status
    let status = h.core.multihost.status().await.unwrap();
    assert!(status.signed_in);
    assert_eq!(status.user_email.as_deref(), Some("admin@mcpanel.net"));
    assert_eq!(status.hosts.len(), 1);

    // Generate enrollment token
    let token = h.core.multihost.generate_enrollment_token().await.unwrap();
    assert!(token.token.starts_with("mcp_"));
    assert_eq!(token.account_uid, "user_abc123");
    assert!(token.pairing_command.contains(&token.token));
    assert!(token.pairing_command.contains("user_abc123"));

    // Ping local host
    let ping_local = h.core.multihost.ping_host("local").await.unwrap();
    assert!(ping_local.online);
    assert_eq!(ping_local.latency_ms, Some(0));

    // Input validation
    let err = h
        .core
        .multihost
        .add_host(
            "".into(),
            "https://node.example.com:8443".into(),
            None,
            vec![],
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);

    let err = h
        .core
        .multihost
        .add_host("Valid Name".into(), "".into(), None, vec![], "test")
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
            Some("node-secret".into()),
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
            None,
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
            None,
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

    // Ping remote host
    let ping_remote = h.core.multihost.ping_host(&remote.id).await.unwrap();
    assert!(ping_remote.online);
    assert!(ping_remote.latency_ms.is_some());

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

    // Pinging removed host returns NotFound
    let err = h.core.multihost.ping_host(&remote.id).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    h.finish().await;
}
