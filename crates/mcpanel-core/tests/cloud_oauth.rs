#![allow(clippy::unwrap_used)]
//! OAuth connect flow end to end with a fake provider: PKCE S256, state checking, the
//! loopback redirect (a real HTTP request stands in for the browser), token storage in
//! the secret store, refresh with rotation, and disconnect.

use async_trait::async_trait;
use mcpanel_core::cloud::pkce::challenge_s256;
use mcpanel_core::cloud::{
    CloudAccount, CloudProviderInfo, CloudService, CloudStorageProvider, FlowState, RedirectSpec,
    RevokeOutcome, TokenSet,
};
use mcpanel_core::crypto::MemorySecretStore;
use mcpanel_core::error::{CoreResult, ErrorCode};
use mcpanel_core::events::EventBus;
use mcpanel_core::ports::{SecretStore, SettingsRepository};
use mcpanel_core::time::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
struct Calls {
    exchanges: Vec<(String, String, String)>,
    refreshes: Vec<String>,
    revoked: Vec<String>,
}

struct Fake {
    info: CloudProviderInfo,
    calls: Arc<Mutex<Calls>>,
    access_ttl_ms: i64,
}

#[async_trait]
impl CloudStorageProvider for Fake {
    fn info(&self) -> &CloudProviderInfo {
        &self.info
    }
    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect: &str,
    ) -> CoreResult<TokenSet> {
        self.calls
            .lock()
            .unwrap()
            .exchanges
            .push((code.into(), verifier.into(), redirect.into()));
        Ok(TokenSet {
            access_token: SecretString::from("access-1".to_string()),
            refresh_token: Some(SecretString::from("refresh-1".to_string())),
            expires_at: Some(Timestamp(Timestamp::now().millis() + self.access_ttl_ms)),
        })
    }
    async fn refresh(&self, rt: &SecretString) -> CoreResult<TokenSet> {
        let n = {
            let mut c = self.calls.lock().unwrap();
            c.refreshes.push(rt.expose_secret().to_string());
            c.refreshes.len()
        };
        Ok(TokenSet {
            access_token: SecretString::from(format!("access-r{n}")),
            refresh_token: Some(SecretString::from(format!("refresh-r{n}"))),
            expires_at: Some(Timestamp(Timestamp::now().millis() + 3_600_000)),
        })
    }
    async fn account(&self, at: &SecretString) -> CoreResult<CloudAccount> {
        assert!(at.expose_secret().starts_with("access-"));
        Ok(CloudAccount {
            display_name: Some("Alex".into()),
            email: Some("alex@example.com".into()),
        })
    }
    async fn revoke(&self, rt: &SecretString) -> CoreResult<RevokeOutcome> {
        self.calls
            .lock()
            .unwrap()
            .revoked
            .push(rt.expose_secret().to_string());
        Ok(RevokeOutcome::Revoked)
    }
}

fn info(id: &'static str, client_id: Option<&str>, redirect: RedirectSpec) -> CloudProviderInfo {
    CloudProviderInfo {
        id,
        display_name: "Fake Cloud",
        client_id: client_id.map(String::from),
        client_id_variable: "MCPANEL_FAKE_CLIENT_ID",
        authorize_url: "https://auth.example/authorize",
        scopes: &["files.write", "offline_access"],
        extra_authorize_params: &[("token_access_type", "offline")],
        redirect,
        manage_access_url: "https://example/apps",
    }
}

fn query(url: &str) -> HashMap<String, String> {
    let q = url.split_once('?').unwrap().1;
    q.split('&')
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap();
            let v = v
                .replace("%3A", ":")
                .replace("%2F", "/")
                .replace("%20", " ");
            (k.to_string(), v)
        })
        .collect()
}

/// Act as the browser returning to the loopback redirect.
async fn browser(redirect_uri: &str, query: &str) -> String {
    let rest = redirect_uri.strip_prefix("http://").unwrap();
    let (hostport, path) = rest.split_once('/').map_or((rest, "/"), |(h, p)| (h, p));
    let port: u16 = hostport.rsplit(':').next().unwrap().parse().unwrap();
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let path = if path == "/" {
        String::from("/")
    } else {
        format!("/{path}")
    };
    s.write_all(format!("GET {path}?{query} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

async fn settle(svc: &CloudService, flow: &str) -> FlowState {
    for _ in 0..200 {
        let s = svc.flow(flow).unwrap();
        if s != FlowState::Waiting {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("flow did not finish");
}

fn service(
    providers: Vec<Arc<dyn CloudStorageProvider>>,
) -> (Arc<CloudService>, Arc<MemorySecretStore>) {
    let secrets = Arc::new(MemorySecretStore::default());
    let svc = CloudService::new(
        providers,
        Arc::clone(&secrets) as Arc<dyn SecretStore>,
        Arc::new(MemSettings::default()),
        EventBus::default(),
    );
    (svc, secrets)
}

#[tokio::test]
async fn connect_refresh_and_disconnect() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let fake = Arc::new(Fake {
        info: info(
            "fake",
            Some("client-123"),
            RedirectSpec::AnyPort {
                host: "127.0.0.1",
                path: "/cb",
            },
        ),
        calls: Arc::clone(&calls),
        access_ttl_ms: 30_000, // inside the refresh margin: the next use refreshes
    });
    let (svc, secrets) = service(vec![fake]);
    let st = svc.status("fake").await.unwrap();
    assert!(st.configured && !st.connected);
    assert_eq!(st.redirect_uris, vec!["http://127.0.0.1/cb".to_string()]);

    let start = svc.begin_connect("fake").await.unwrap();
    assert!(
        start
            .authorize_url
            .starts_with("https://auth.example/authorize?")
    );
    let q = query(&start.authorize_url);
    assert_eq!(q["client_id"], "client-123");
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["scope"], "files.write offline_access");
    assert_eq!(q["token_access_type"], "offline");
    assert!(!start.authorize_url.contains("client_secret"));
    let redirect = q["redirect_uri"].clone();
    assert!(
        redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/cb"),
        "{redirect}"
    );
    // A second sign-in while one is open is refused.
    assert_eq!(
        svc.begin_connect("fake").await.unwrap_err().code,
        ErrorCode::Conflict
    );

    let page = browser(&redirect, &format!("code=the-code&state={}", q["state"])).await;
    assert!(page.contains("connected"));
    assert_eq!(settle(&svc, &start.flow_id).await, FlowState::Connected);
    {
        let c = calls.lock().unwrap();
        let (code, verifier, redir) = &c.exchanges[0];
        assert_eq!(code, "the-code");
        assert_eq!(redir, &redirect);
        assert_eq!(
            challenge_s256(verifier),
            q["code_challenge"],
            "PKCE verifier matches the challenge"
        );
    }
    let st = svc.status("fake").await.unwrap();
    assert!(st.connected);
    assert_eq!(
        st.account.unwrap().email.as_deref(),
        Some("alex@example.com")
    );
    assert_eq!(
        secrets
            .get("cloud-fake-refresh-token")
            .unwrap()
            .unwrap()
            .expose_secret(),
        "refresh-1"
    );

    // The access token is close to expiry: it is refreshed and the rotated refresh token
    // replaces the stored one; the new access token is then reused.
    assert_eq!(
        svc.access_token("fake").await.unwrap().expose_secret(),
        "access-r1"
    );
    assert_eq!(
        calls.lock().unwrap().refreshes,
        vec!["refresh-1".to_string()]
    );
    assert_eq!(
        secrets
            .get("cloud-fake-refresh-token")
            .unwrap()
            .unwrap()
            .expose_secret(),
        "refresh-r1"
    );
    assert_eq!(
        svc.access_token("fake").await.unwrap().expose_secret(),
        "access-r1"
    );
    assert_eq!(calls.lock().unwrap().refreshes.len(), 1);
    svc.check("fake").await.unwrap();

    assert_eq!(
        svc.disconnect("fake").await.unwrap(),
        RevokeOutcome::Revoked
    );
    assert_eq!(
        calls.lock().unwrap().revoked,
        vec!["refresh-r1".to_string()]
    );
    assert!(secrets.get("cloud-fake-refresh-token").unwrap().is_none());
    let st = svc.status("fake").await.unwrap();
    assert!(!st.connected && st.account.is_none());
    assert_eq!(
        svc.access_token("fake").await.unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn state_mismatch_denial_and_missing_configuration_fail_clearly() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let spec = RedirectSpec::AnyPort {
        host: "127.0.0.1",
        path: "/cb",
    };
    let (svc, _) = service(vec![
        Arc::new(Fake {
            info: info("fake", Some("c"), spec.clone()),
            calls: Arc::clone(&calls),
            access_ttl_ms: 3_600_000,
        }),
        Arc::new(Fake {
            info: info("nocfg", None, spec),
            calls: Arc::clone(&calls),
            access_ttl_ms: 3_600_000,
        }),
    ]);
    let e = svc.begin_connect("nocfg").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported);
    assert!(
        e.message.contains("MCPANEL_FAKE_CLIENT_ID"),
        "{}",
        e.message
    );

    let start = svc.begin_connect("fake").await.unwrap();
    let q = query(&start.authorize_url);
    browser(&q["redirect_uri"], "code=x&state=forged").await;
    match settle(&svc, &start.flow_id).await {
        FlowState::Failed { message } => assert!(message.contains("state mismatch"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert!(
        calls.lock().unwrap().exchanges.is_empty(),
        "no code exchange on a forged state"
    );

    let start = svc.begin_connect("fake").await.unwrap();
    let q = query(&start.authorize_url);
    browser(
        &q["redirect_uri"],
        &format!("error=access_denied&state={}", q["state"]),
    )
    .await;
    match settle(&svc, &start.flow_id).await {
        FlowState::Failed { message } => assert!(message.contains("not granted"), "{message}"),
        other => panic!("{other:?}"),
    }

    let start = svc.begin_connect("fake").await.unwrap();
    svc.cancel_connect(&start.flow_id);
    assert_eq!(settle(&svc, &start.flow_id).await, FlowState::Cancelled);
}

#[tokio::test]
async fn fixed_ports_fall_back_to_the_next_registered_port() {
    // Reserve two free ports, keep the first busy.
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let p1 = busy.local_addr().unwrap().port();
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let p2 = free.local_addr().unwrap().port();
    drop(free);
    let ports: &'static [u16] = Box::leak(vec![p1, p2].into_boxed_slice());
    let calls = Arc::new(Mutex::new(Calls::default()));
    let (svc, _) = service(vec![Arc::new(Fake {
        info: info(
            "fixed",
            Some("c"),
            RedirectSpec::FixedPorts {
                host: "localhost",
                path: "/mcpanel/oauth",
                ports,
            },
        ),
        calls,
        access_ttl_ms: 3_600_000,
    })]);
    let st = svc.status("fixed").await.unwrap();
    assert_eq!(st.redirect_uris.len(), 2);
    let start = svc.begin_connect("fixed").await.unwrap();
    let q = query(&start.authorize_url);
    assert_eq!(
        q["redirect_uri"],
        format!("http://localhost:{p2}/mcpanel/oauth")
    );
    svc.cancel_connect(&start.flow_id);
    drop(busy);
}
