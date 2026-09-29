#![allow(clippy::unwrap_used)]
//! Playit detection against the real installation, when one exists (read-only: only
//! `playit version` and `playit status` run; the agent is never started or stopped).

use mcpanel_core::tunnels::PlayitTunnel;
use std::sync::Arc;

#[tokio::test]
async fn detects_an_installed_playit_agent() {
    let t = PlayitTunnel::new(Arc::new(mcpanel_platform::NativePlatform::new()));
    let s = t.status().await.unwrap();
    eprintln!("{s:?}");
    assert!(!s.caps.can_create_via_api);
    if PlayitTunnel::locate().is_none() {
        assert!(!s.installed);
        return;
    }
    assert!(s.installed);
    let v = s.version.expect("version");
    assert!(v.chars().next().unwrap().is_ascii_digit(), "{v}");
    assert!(s.agent_running.is_some(), "status parsed");
}
