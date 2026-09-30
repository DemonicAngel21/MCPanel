#![allow(clippy::unwrap_used)]
//! MCPanel's own playit agent: linking and tunnel management against an in-memory
//! stand-in for playit's API, and the agent process with the real `playitd` (when the
//! playit program is installed) using an obviously fake key on a private pipe.

use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::playit_agent::{AgentLink, PlayitAgent};
use mcpanel_core::playit_api::{
    NewPlayitTunnel, PlayitAgentInfo, PlayitApi, PlayitTunnelInfo, PlayitTunnelKind,
};
use mcpanel_core::ports::{SecretStore, SettingsRepository};
use mcpanel_core::tunnels::PlayitTunnel;
use secrecy::{ExposeSecret, SecretString};
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

#[derive(Default)]
struct MemSecrets(Mutex<HashMap<String, String>>);

impl SecretStore for MemSecrets {
    fn get(&self, name: &str) -> CoreResult<Option<SecretString>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .map(SecretString::from))
    }
    fn set(&self, name: &str, value: &SecretString) -> CoreResult<()> {
        self.0
            .lock()
            .unwrap()
            .insert(name.into(), value.expose_secret().to_string());
        Ok(())
    }
    fn delete(&self, name: &str) -> CoreResult<()> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

const AGENT: &str = "11111111-1111-4111-8111-111111111111";
const OTHER_AGENT: &str = "22222222-2222-4222-8222-222222222222";
const KEY: &str = "fakekey0000test";

/// playit's API as the tests need it: the claim is accepted after two polls.
#[derive(Default)]
struct FakePlayit {
    polls: Mutex<u32>,
    tunnels: Mutex<Vec<PlayitTunnelInfo>>,
    calls: Mutex<Vec<String>>,
}

impl FakePlayit {
    fn check(&self, key: &SecretString) -> CoreResult<()> {
        if key.expose_secret() == KEY {
            Ok(())
        } else {
            Err(CoreError::new(ErrorCode::ProviderError, "bad key"))
        }
    }
}

#[async_trait]
impl PlayitApi for FakePlayit {
    async fn claim_setup(&self, code: &str, version: &str) -> CoreResult<String> {
        assert_eq!(code.len(), 10);
        assert!(version.starts_with("MCPanel "));
        let mut p = self.polls.lock().unwrap();
        *p += 1;
        Ok(if *p < 3 {
            "WaitingForUser"
        } else {
            "UserAccepted"
        }
        .into())
    }
    async fn claim_exchange(&self, _code: &str) -> CoreResult<SecretString> {
        Ok(SecretString::from(KEY.to_string()))
    }
    async fn agent(&self, key: &SecretString) -> CoreResult<PlayitAgentInfo> {
        self.check(key)?;
        Ok(PlayitAgentInfo {
            agent_id: AGENT.into(),
            self_managed: true,
            premium: false,
            account_status: "verified".into(),
        })
    }
    async fn tunnels(&self, key: &SecretString) -> CoreResult<Vec<PlayitTunnelInfo>> {
        self.check(key)?;
        Ok(self.tunnels.lock().unwrap().clone())
    }
    async fn create(
        &self,
        key: &SecretString,
        agent_id: &str,
        t: &NewPlayitTunnel,
    ) -> CoreResult<String> {
        self.check(key)?;
        let id = "33333333-3333-4333-8333-333333333333".to_string();
        self.tunnels.lock().unwrap().push(PlayitTunnelInfo {
            id: id.clone(),
            name: Some(t.name.clone()),
            tunnel_type: Some(t.kind.api_name().into()),
            port_type: "tcp".into(),
            enabled: t.enabled,
            offline_reasons: vec![],
            agent_id: Some(agent_id.into()),
            local_ip: Some(t.local_ip.clone()),
            local_port: Some(t.local_port),
            addresses: vec!["test.joinmc.link".into()],
            created_at: None,
        });
        Ok(id)
    }
    async fn rename(&self, key: &SecretString, id: &str, name: &str) -> CoreResult<()> {
        self.check(key)?;
        self.calls
            .lock()
            .unwrap()
            .push(format!("rename {id} {name}"));
        Ok(())
    }
    async fn set_local_address(
        &self,
        key: &SecretString,
        id: &str,
        ip: &str,
        port: u16,
    ) -> CoreResult<()> {
        self.check(key)?;
        self.calls
            .lock()
            .unwrap()
            .push(format!("port {id} {ip}:{port}"));
        Ok(())
    }
    async fn set_enabled(&self, key: &SecretString, id: &str, on: bool) -> CoreResult<()> {
        self.check(key)?;
        self.calls.lock().unwrap().push(format!("enable {id} {on}"));
        Ok(())
    }
    async fn delete(&self, key: &SecretString, id: &str) -> CoreResult<()> {
        self.check(key)?;
        self.tunnels.lock().unwrap().retain(|t| t.id != id);
        Ok(())
    }
}

fn agent(
    dir: &std::path::Path,
    daemon: Option<std::path::PathBuf>,
) -> (Arc<PlayitAgent>, Arc<MemSecrets>) {
    let secrets = Arc::new(MemSecrets::default());
    let a = PlayitAgent::new(
        Arc::new(mcpanel_platform::NativePlatform::new()),
        secrets.clone(),
        Arc::new(MemSettings::default()),
        dir,
        format!(
            r"\\.\pipe\mcpanel-playit-test-{}",
            uuid::Uuid::new_v4().simple()
        ),
    )
    .with_daemon(daemon);
    (Arc::new(a), secrets)
}

#[tokio::test]
async fn links_manages_tunnels_and_unlinks() {
    let dir = tempfile::tempdir().unwrap();
    let (a, secrets) = agent(dir.path(), None);
    let api = Arc::new(FakePlayit::default());
    a.set_api(api.clone());
    assert!(!a.status().await.unwrap().linked);
    assert!(a.tunnels().await.is_err(), "no key yet");

    let url = a.begin_link().await.unwrap();
    assert!(url.starts_with("https://playit.gg/claim/"), "{url}");
    assert_eq!(a.begin_link().await.unwrap(), url, "pending link is reused");
    let mut linked = false;
    for _ in 0..100 {
        if a.status().await.unwrap().link == Some(AgentLink::Linked) {
            linked = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(linked);
    assert!(a.status().await.unwrap().linked);
    assert_eq!(
        secrets
            .get("playit-agent-secret")
            .unwrap()
            .unwrap()
            .expose_secret(),
        KEY
    );

    // A tunnel of another agent is listed but not editable.
    api.tunnels.lock().unwrap().push(PlayitTunnelInfo {
        id: "44444444-4444-4444-8444-444444444444".into(),
        name: Some("Old".into()),
        tunnel_type: Some("minecraft-java".into()),
        port_type: "tcp".into(),
        enabled: true,
        offline_reasons: vec![],
        agent_id: Some(OTHER_AGENT.into()),
        local_ip: Some("127.0.0.1".into()),
        local_port: Some(25565),
        addresses: vec!["old.joinmc.link".into()],
        created_at: None,
    });
    let id = a
        .create_tunnel("Survival", PlayitTunnelKind::MinecraftJava, 25570)
        .await
        .unwrap();
    let list = a.tunnels().await.unwrap();
    assert_eq!(list.agent.agent_id, AGENT);
    let mine = list.tunnels.iter().find(|t| t.tunnel.id == id).unwrap();
    assert!(mine.editable);
    assert_eq!(mine.tunnel.local_port, Some(25570));
    assert!(
        !list
            .tunnels
            .iter()
            .find(|t| t.tunnel.name.as_deref() == Some("Old"))
            .unwrap()
            .editable
    );

    a.rename_tunnel(&id, "Survival Java").await.unwrap();
    a.set_tunnel_port(&id, 25571).await.unwrap();
    a.set_tunnel_enabled(&id, false).await.unwrap();
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec![
            format!("rename {id} Survival Java"),
            format!("port {id} 127.0.0.1:25571"),
            format!("enable {id} false"),
        ]
    );
    assert!(a.rename_tunnel(&id, "Überwelt").await.is_err());
    assert!(a.set_tunnel_port("not-a-uuid", 1).await.is_err());
    a.delete_tunnel(&id).await.unwrap();
    assert_eq!(a.tunnels().await.unwrap().tunnels.len(), 1);

    a.unlink().await.unwrap();
    assert!(!a.status().await.unwrap().linked);
    assert!(secrets.get("playit-agent-secret").unwrap().is_none());
}

#[tokio::test]
async fn runs_and_stops_its_own_playitd() {
    let Some(daemon) = PlayitTunnel::locate()
        .map(|p| p.with_file_name("playitd.exe"))
        .filter(|p| p.is_file())
    else {
        eprintln!("playit is not installed; skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (a, secrets) = agent(dir.path(), Some(daemon));
    assert!(a.start().await.is_err(), "no key yet");
    // An obviously fake key: playitd starts and reports an invalid secret.
    secrets
        .set("playit-agent-secret", &SecretString::from("0".repeat(64)))
        .unwrap();
    a.start().await.unwrap();
    let s = a.status().await.unwrap();
    assert!(s.running, "{s:?}");
    assert!(dir.path().join("playit").join("agent.toml").is_file());
    a.start().await.unwrap(); // already running: no second process
    a.stop().await.unwrap();
    assert!(!a.status().await.unwrap().running);
    // The key file is removed when the agent stops.
    let mut gone = false;
    for _ in 0..40 {
        if !dir.path().join("playit").join("agent.toml").exists() {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(gone);
}
