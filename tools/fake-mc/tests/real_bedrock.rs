#![allow(clippy::unwrap_used)]
//! Real Bedrock crossplay: MCPanel installs Geyser + Floodgate (+ ViaVersion) on Paper
//! and Fabric, seeds the Geyser config, and the Bedrock listener answers a RakNet ping
//! on the chosen port. Also checks the UDP port preflight, live settings changes and
//! that the Floodgate key stays unreadable. Requires the Minecraft EULA acceptance flag
//! and network access:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_bedrock -- --nocapture --test-threads 1

mod common;

use common::real::{KillOnPanic, RealServer, eula_accepted, wait_job};
use mcpanel_core::ErrorCode;
use mcpanel_core::bedrock::{AuthType, BedrockSettings, EnableRequest};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Wait until Geyser reports its UDP port in the console.
async fn geyser_port(s: &RealServer) -> u16 {
    let start = Instant::now();
    loop {
        if let Some(p) = s.core.bedrock.status(s.id).await.unwrap().active_port {
            return p;
        }
        assert!(
            start.elapsed() < Duration::from_secs(90),
            "Geyser did not start:\n{}",
            s.console_tail(80)
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn geyser_and_floodgate_on_paper() {
    if !eula_accepted() {
        return;
    }
    let s = RealServer::create_with("paper", Some("1.21.11"), &[("server-port", "25631")]).await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    let b = &s.core.bedrock;

    let st = b.status(s.id).await.unwrap();
    assert!(st.supported && st.geyser.is_none());
    assert!(st.via_version_available && st.via_version_suggested);

    let job = b
        .enable(
            s.id,
            EnableRequest {
                floodgate: true,
                via_version: true,
                port: Some(19150),
            },
            "e2e",
        )
        .await
        .unwrap();
    eprintln!("enable: {}", wait_job(&s.core, job).await);
    let st = b.status(s.id).await.unwrap();
    eprintln!(
        "geyser {:?} floodgate {:?} via {:?}",
        st.geyser, st.floodgate, st.via_version
    );
    assert!(st.geyser.is_some() && st.floodgate.is_some() && st.via_version.is_some());
    assert!(st.config_exists);
    assert_eq!(
        st.settings,
        BedrockSettings {
            port: 19150,
            auth_type: AuthType::Floodgate
        }
    );

    s.start_and_wait().await;
    assert_eq!(geyser_port(&s).await, 19150);
    let pong = b.ping(s.id).await.unwrap();
    eprintln!("pong: {pong:?}");
    assert_eq!(pong.edition, "MCPE");
    let st = b.status(s.id).await.unwrap();
    assert!(st.floodgate_key_present, "Floodgate created its key");
    assert!(!st.via_version_required, "no ViaVersion warning");
    assert!(!st.restart_required);
    let tail = s.console_tail(600);
    assert!(!tail.contains("Please install ViaVersion"), "{tail}");

    // The key is protected from the file API.
    let err = s
        .core
        .files
        .read_text(s.id, "plugins/floodgate/key.pem".into())
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::SensitiveFile);

    // Geyser completed the seeded config and kept MCPanel's values.
    let dir = s.core.servers.get(s.id).await.unwrap().directory;
    let cfg = std::fs::read_to_string(dir.join("plugins/Geyser-Spigot/config.yml")).unwrap();
    assert!(cfg.contains("config-version"), "full config written");
    assert!(cfg.contains("  port: 19150") && cfg.contains("  auth-type: floodgate"));

    // A change while running is saved and flagged until the restart.
    let st = b
        .configure(
            s.id,
            BedrockSettings {
                port: 19151,
                auth_type: AuthType::Floodgate,
            },
            "e2e",
        )
        .await
        .unwrap();
    assert!(st.restart_required);
    s.stop_and_wait().await;

    // Another program on the Bedrock port blocks the start.
    let blocker = std::net::UdpSocket::bind("0.0.0.0:19151").unwrap();
    let st = b.status(s.id).await.unwrap();
    assert!(matches!(
        st.port_status,
        Some(mcpanel_core::ports::PortStatus::InUse { .. })
    ));
    let err = s.core.servers.start(s.id, "e2e").await.unwrap_err();
    eprintln!("blocked start: {}", err.message);
    assert_eq!(err.code, ErrorCode::PortInUse);
    drop(blocker);

    s.start_and_wait().await;
    assert_eq!(geyser_port(&s).await, 19151);
    b.ping(s.id).await.unwrap();
    s.stop_and_wait().await;
    s.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn geyser_and_floodgate_on_fabric() {
    if !eula_accepted() {
        return;
    }
    let s = RealServer::create_with("fabric", Some("26.2"), &[("server-port", "25632")]).await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    let b = &s.core.bedrock;
    let st = b.status(s.id).await.unwrap();
    assert!(st.supported && !st.via_version_available);

    let job = b
        .enable(
            s.id,
            EnableRequest {
                floodgate: true,
                via_version: false,
                port: Some(19152),
            },
            "e2e",
        )
        .await
        .unwrap();
    eprintln!("enable: {}", wait_job(&s.core, job).await);
    let list = s.core.content.list(s.id).await.unwrap();
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
    let st = b.status(s.id).await.unwrap();
    assert!(st.geyser.is_some() && st.floodgate.is_some(), "{st:?}");

    s.start_and_wait().await;
    assert_eq!(geyser_port(&s).await, 19152);
    let pong = b.ping(s.id).await.unwrap();
    eprintln!("pong: {pong:?}");
    let st = b.status(s.id).await.unwrap();
    assert!(st.floodgate_key_present, "Floodgate created its key");
    let tail = s.console_tail(800);
    assert!(
        !tail.to_lowercase().contains("floodgate key"),
        "no key errors:\n{tail}"
    );
    s.stop_and_wait().await;
    s.finish().await;
}
