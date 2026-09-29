#![allow(clippy::unwrap_used)]
//! Playit against the real installation, when one exists. Detection only runs
//! `playit version` and `playit status`. The link test runs its own playitd on a private
//! pipe with a throwaway secret file, so the installed service and its account are never
//! touched; it stops before the approval step (that needs a person in the browser).

use async_trait::async_trait;
use mcpanel_core::error::CoreResult;
use mcpanel_core::ids::ServerId;
use mcpanel_core::ports::SettingsRepository;
use mcpanel_core::tunnels::{LinkProgress, PlayitTunnel};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct MemSettings(Mutex<HashMap<String, serde_json::Value>>);

#[async_trait]
impl SettingsRepository for MemSettings {
    async fn get(&self, key: &str) -> CoreResult<Option<serde_json::Value>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    async fn set(&self, key: &str, value: &serde_json::Value) -> CoreResult<()> {
        self.0.lock().unwrap().insert(key.into(), value.clone());
        Ok(())
    }
    async fn all(&self) -> CoreResult<Vec<(String, serde_json::Value)>> {
        Ok(self.0.lock().unwrap().clone().into_iter().collect())
    }
}

fn tunnel() -> PlayitTunnel {
    PlayitTunnel::new(
        Arc::new(mcpanel_platform::NativePlatform::new()),
        Arc::new(MemSettings::default()),
    )
}

#[tokio::test]
async fn detects_an_installed_playit_agent() {
    let s = tunnel().status().await.unwrap();
    eprintln!("{s:?}");
    assert!(!s.caps.can_manage_tunnels);
    if PlayitTunnel::locate().is_none() {
        assert!(!s.installed);
        return;
    }
    assert!(s.installed);
    assert!(s.caps.can_control_agent);
    let v = s.version.expect("version");
    assert!(v.chars().next().unwrap().is_ascii_digit(), "{v}");
    assert!(s.agent_running.is_some(), "status parsed");
}

#[tokio::test]
async fn server_addresses_round_trip() {
    let t = tunnel();
    let id = ServerId::new();
    assert_eq!(t.server_address(id).await.unwrap(), None);
    t.set_server_address(id, " Abc.ply.gg:4321 ").await.unwrap();
    assert_eq!(
        t.server_address(id).await.unwrap().as_deref(),
        Some("abc.ply.gg:4321")
    );
    assert!(t.set_server_address(id, "not an address").await.is_err());
    t.set_server_address(id, "").await.unwrap();
    assert_eq!(t.server_address(id).await.unwrap(), None);
}

#[tokio::test]
async fn links_an_unclaimed_agent_up_to_the_approval_page() {
    let Some(exe) = PlayitTunnel::locate() else {
        eprintln!("playit is not installed; skipped");
        return;
    };
    let daemon_exe = exe.with_file_name("playitd.exe");
    if !daemon_exe.is_file() {
        eprintln!("playitd.exe not found; skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let pipe = format!(r"\\.\pipe\mcpanel-playit-test-{}", std::process::id());
    let mut daemon = std::process::Command::new(&daemon_exe)
        .arg("--secret-path")
        .arg(dir.path().join("playit.toml"))
        .arg("--socket-path")
        .arg(&pipe)
        .arg("-l")
        .arg(dir.path().join("playitd.log"))
        .spawn()
        .unwrap();
    let t = tunnel().with_socket_path(&pipe);
    let mut status = None;
    for _ in 0..40 {
        let s = t.status().await.unwrap();
        if s.agent_running == Some(true) {
            status = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let status = status.expect("isolated playitd came up");
    assert_eq!(status.secret_configured, Some(false));
    assert_eq!(status.phase.as_deref(), Some("waiting for secret"));
    assert!(status.caps.can_link);

    let url = t.begin_link().await.unwrap();
    assert!(url.starts_with("https://playit.gg/claim/"), "{url}");
    // A second request returns the same pending link instead of a new claim.
    assert_eq!(t.begin_link().await.unwrap(), url);
    let s = t.status().await.unwrap();
    assert_eq!(s.link, Some(LinkProgress::Waiting { claim_url: url }));

    t.cancel_link();
    assert!(t.status().await.unwrap().link.is_none());
    daemon.kill().unwrap();
    let _ = daemon.wait();
}
