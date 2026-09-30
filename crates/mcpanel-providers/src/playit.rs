//! playit.gg web API client for tunnel management (see `mcpanel_core::playit_api` for why
//! and under which rules MCPanel uses it). Requests are `POST {base}{path}` with a JSON
//! body and `Authorization: Agent-Key <secret>`; responses are
//! `{"status":"success"|"fail"|"error","data":…}` (playit-agent `api_client`, 4c27794).
//! Responses are read as loose JSON so that an unexpected extra field never breaks the
//! parts MCPanel understands.

use crate::http::HttpClient;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::playit_api::{NewPlayitTunnel, PlayitAgentInfo, PlayitApi, PlayitTunnelInfo};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::time::Duration;

pub const PLAYIT_API_BASE: &str = "https://api.playit.gg";

pub struct PlayitWebApi {
    /// HTTPS-only in production (MCPanel's shared client).
    client: reqwest::Client,
    base: String,
}

impl PlayitWebApi {
    pub fn new(http: &HttpClient) -> Self {
        Self {
            client: http.inner().clone(),
            base: PLAYIT_API_BASE.to_string(),
        }
    }

    /// For tests against a local plain-HTTP stand-in.
    #[cfg(test)]
    fn for_test(base: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    async fn call(&self, key: &SecretString, path: &str, body: Value) -> CoreResult<Value> {
        self.send(Some(key), path, body).await
    }

    async fn send(&self, key: Option<&SecretString>, path: &str, body: Value) -> CoreResult<Value> {
        let mut req = self
            .client
            .post(format!("{}{path}", self.base))
            .timeout(Duration::from_secs(30));
        if let Some(key) = key {
            req = req.header(
                reqwest::header::AUTHORIZATION,
                format!("Agent-Key {}", key.expose_secret().trim()),
            );
        }
        let resp = req.json(&body).send().await.map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderUnavailable,
                format!("playit.gg could not be reached: {}", e.without_url()),
            )
            .retryable()
        })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        parse_envelope(status, &text)
    }
}

/// Turn a response into its `data`, or a user-facing error.
pub fn parse_envelope(http_status: u16, text: &str) -> CoreResult<Value> {
    if http_status == 429 {
        return Err(CoreError::new(
            ErrorCode::ProviderUnavailable,
            "playit.gg received too many requests. Try again in a minute.",
        )
        .retryable());
    }
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Err(CoreError::new(
            ErrorCode::ProviderError,
            format!("playit.gg sent an unexpected answer (HTTP {http_status})."),
        ));
    };
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    match v.get("status").and_then(Value::as_str) {
        Some("success") => Ok(data),
        Some("fail") => Err(fail_error(&data)),
        Some("error") => Err(api_error(&data)),
        _ => Err(CoreError::new(
            ErrorCode::ProviderError,
            format!("playit.gg sent an unexpected answer (HTTP {http_status})."),
        )),
    }
}

/// A request-specific failure (`"fail"`): a code string or `{"error": code}`.
fn fail_error(data: &Value) -> CoreError {
    let code = data
        .as_str()
        .or_else(|| data.get("error").and_then(Value::as_str))
        .unwrap_or("Unknown");
    let (error_code, message) = match code {
        "TunnelNotFound" => (
            ErrorCode::NotFound,
            "This tunnel no longer exists on playit.gg.".to_string(),
        ),
        "RequiresVerifiedAccount" => (
            ErrorCode::ProviderError,
            "playit.gg requires a verified account for this. Verify your email on playit.gg."
                .into(),
        ),
        "RegionRequiresPlayitPremium"
        | "RequiresPlayitPremium"
        | "PublicPortRequiresPlayitPremium" => (
            ErrorCode::ProviderError,
            "This needs a playit Premium subscription.".into(),
        ),
        "AgentVersionTooOld" => (
            ErrorCode::ProviderError,
            "The playit program is too old for this. Update it from playit.gg.".into(),
        ),
        "TunnelNameTooLong" | "NameTooLong" => (
            ErrorCode::InvalidInput,
            "The tunnel name is too long.".into(),
        ),
        "TunnelNameIsNotAscii" => (
            ErrorCode::InvalidInput,
            "Use only English letters, digits and punctuation in the tunnel name.".into(),
        ),
        "AgentNotFound" | "InvalidAgentId" => (
            ErrorCode::ProviderError,
            "playit.gg does not know this agent. Link the playit agent again.".into(),
        ),
        "CannotUpdateLocalAddressForUnassignedTunnel" | "CannotConfigTunnelWithoutAgent" => (
            ErrorCode::Conflict,
            "This tunnel is not assigned to an agent. Assign it on playit.gg first.".into(),
        ),
        "SelfManagedAgentCannotReassignTunnel" | "ChangingAgentIdNotAllowed" => (
            ErrorCode::Conflict,
            "This tunnel belongs to another agent and cannot be changed from here.".into(),
        ),
        "NothingToUpdate" => (ErrorCode::InvalidInput, "Nothing changed.".into()),
        "InvalidCode" | "CodeNotFound" | "NotSetup" => (
            ErrorCode::ProviderError,
            "playit.gg does not know this link code. Start linking again.".into(),
        ),
        "CodeExpired" => (
            ErrorCode::ProviderError,
            "The link expired. Start linking again.".into(),
        ),
        "UserRejected" => (
            ErrorCode::Cancelled,
            "The agent was declined on playit.gg.".into(),
        ),
        "NotAccepted" => (
            ErrorCode::Conflict,
            "The agent has not been approved on playit.gg yet.".into(),
        ),
        other => (
            ErrorCode::ProviderError,
            format!("playit.gg refused the request ({other})."),
        ),
    };
    CoreError::new(error_code, message).with_details(json!({ "playit": code }))
}

/// A general API error (`"error"`): `{"type": "auth"|"validation"|…, "message": …}`.
fn api_error(data: &Value) -> CoreError {
    let kind = data.get("type").and_then(Value::as_str).unwrap_or("");
    let detail = data
        .get("message")
        .map(|m| match m {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default();
    match kind {
        "auth" => {
            let message = match detail.as_str() {
                "InvalidAgentKey" | "NoLongerValid" => {
                    "playit.gg no longer accepts this agent's key. Link the playit agent again."
                        .to_string()
                }
                "AgentNotSelfManaged" | "DefaultAgentBlocked" | "SelfManagedAgentCanOnlyAffectSelf" => {
                    format!(
                        "playit.gg does not let this agent change tunnels through its API ({detail}). Manage the tunnel on playit.gg instead."
                    )
                }
                "GuestAccountNotAllowed" | "EmailMustBeVerified" => {
                    "playit.gg requires a verified account for this. Verify your email on playit.gg."
                        .into()
                }
                other => format!("playit.gg refused the agent's key ({other})."),
            };
            CoreError::new(ErrorCode::ProviderError, message)
        }
        "validation" => CoreError::new(
            ErrorCode::InvalidInput,
            format!("playit.gg rejected the request: {}", truncate(&detail, 200)),
        ),
        "internal" => CoreError::new(
            ErrorCode::ProviderUnavailable,
            "playit.gg had an internal error. Try again later.",
        )
        .retryable(),
        _ => CoreError::new(
            ErrorCode::ProviderError,
            format!(
                "playit.gg returned an error ({}).",
                truncate(&data.to_string(), 200)
            ),
        ),
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

pub fn parse_agent(data: &Value) -> CoreResult<PlayitAgentInfo> {
    let agent_id = str_field(data, "agent_id").ok_or_else(|| {
        CoreError::new(
            ErrorCode::ProviderError,
            "playit.gg did not name the agent.",
        )
    })?;
    let perms = data.get("permissions").cloned().unwrap_or(Value::Null);
    Ok(PlayitAgentInfo {
        agent_id,
        self_managed: perms
            .get("is_self_managed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        premium: perms
            .get("has_premium")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        account_status: str_field(&perms, "account_status").unwrap_or_else(|| "unknown".into()),
    })
}

/// The address players type, from one `connect_addresses` entry.
fn connect_address(a: &Value) -> Option<String> {
    let value = a.get("value")?;
    let address = value.get("address")?.as_str()?.to_string();
    match a.get("type")?.as_str()? {
        "ip4" | "ip6" => Some(match value.get("default_port").and_then(Value::as_u64) {
            Some(p) => format!("{address}:{p}"),
            None => address,
        }),
        _ => Some(address),
    }
}

pub fn parse_tunnels(data: &Value) -> Vec<PlayitTunnelInfo> {
    let Some(list) = data.get("tunnels").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|t| {
            let id = str_field(t, "id")?;
            let origin = t.get("origin").cloned().unwrap_or(Value::Null);
            let (agent_id, fields) = if origin.get("type").and_then(Value::as_str) == Some("agent")
            {
                let d = origin.get("details").cloned().unwrap_or(Value::Null);
                (
                    str_field(&d, "agent_id"),
                    d.pointer("/config_data/fields")
                        .cloned()
                        .unwrap_or(Value::Null),
                )
            } else {
                (None, Value::Null)
            };
            let field = |name: &str| {
                fields.as_array().and_then(|f| {
                    f.iter()
                        .find(|x| x.get("name").and_then(Value::as_str) == Some(name))
                        .and_then(|x| str_field(x, "value"))
                })
            };
            let mut addresses: Vec<String> = Vec::new();
            for a in t
                .get("connect_addresses")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(s) = connect_address(a)
                    && !addresses.contains(&s)
                {
                    addresses.push(s);
                }
            }
            Some(PlayitTunnelInfo {
                id,
                name: str_field(t, "name"),
                tunnel_type: str_field(t, "tunnel_type"),
                port_type: str_field(t, "port_type").unwrap_or_else(|| "tcp".into()),
                enabled: t
                    .get("user_enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                offline_reasons: t
                    .get("offline_reasons")
                    .and_then(Value::as_array)
                    .map(|r| {
                        r.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                agent_id,
                local_ip: field("local_ip"),
                local_port: field("local_port").and_then(|p| p.parse().ok()),
                addresses,
                created_at: str_field(t, "created_at"),
            })
        })
        .collect()
}

fn agent_config(local_ip: &str, local_port: u16) -> Value {
    json!({ "fields": [
        { "name": "local_ip", "value": local_ip },
        { "name": "local_port", "value": local_port.to_string() },
    ]})
}

pub fn create_body(agent_id: &str, t: &NewPlayitTunnel) -> Value {
    json!({
        "ports": { "type": "tunnel-type", "details": t.kind.api_name() },
        "origin": { "type": "agent", "data": {
            "agent_id": agent_id,
            "config": agent_config(&t.local_ip, t.local_port),
        }},
        "enabled": t.enabled,
        "alloc": { "type": "region", "details": { "region": "global", "port": null } },
        "name": t.name,
        "firewall_id": null,
    })
}

#[async_trait::async_trait]
impl PlayitApi for PlayitWebApi {
    async fn claim_setup(&self, code: &str, version: &str) -> CoreResult<String> {
        let data = self
            .send(
                None,
                "/claim/setup",
                json!({ "code": code, "agent_type": "self-managed", "version": version }),
            )
            .await?;
        data.as_str().map(str::to_string).ok_or_else(|| {
            CoreError::new(
                ErrorCode::ProviderError,
                "playit.gg sent an unexpected link state.",
            )
        })
    }

    async fn claim_exchange(&self, code: &str) -> CoreResult<SecretString> {
        let data = self
            .send(None, "/claim/exchange", json!({ "code": code }))
            .await?;
        let key = str_field(&data, "secret_key")
            .filter(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric()))
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::ProviderError,
                    "playit.gg did not return the agent's key.",
                )
            })?;
        Ok(SecretString::from(key))
    }

    async fn agent(&self, key: &SecretString) -> CoreResult<PlayitAgentInfo> {
        parse_agent(&self.call(key, "/v1/agents/rundata", json!({})).await?)
    }

    async fn tunnels(&self, key: &SecretString) -> CoreResult<Vec<PlayitTunnelInfo>> {
        Ok(parse_tunnels(
            &self.call(key, "/v1/tunnels/list", json!({})).await?,
        ))
    }

    async fn create(
        &self,
        key: &SecretString,
        agent_id: &str,
        tunnel: &NewPlayitTunnel,
    ) -> CoreResult<String> {
        let data = self
            .call(key, "/v1/tunnels/create", create_body(agent_id, tunnel))
            .await?;
        str_field(&data, "id").ok_or_else(|| {
            CoreError::new(
                ErrorCode::ProviderError,
                "playit.gg did not return the new tunnel's id.",
            )
        })
    }

    async fn rename(&self, key: &SecretString, tunnel_id: &str, name: &str) -> CoreResult<()> {
        self.call(
            key,
            "/tunnels/rename",
            json!({ "tunnel_id": tunnel_id, "name": name }),
        )
        .await
        .map(drop)
    }

    async fn set_local_address(
        &self,
        key: &SecretString,
        tunnel_id: &str,
        local_ip: &str,
        local_port: u16,
    ) -> CoreResult<()> {
        self.call(
            key,
            "/v1/tunnels/config",
            json!({
                "tunnel_id": tunnel_id,
                "new_agent_id": null,
                "new_config": agent_config(local_ip, local_port),
            }),
        )
        .await
        .map(drop)
    }

    async fn set_enabled(
        &self,
        key: &SecretString,
        tunnel_id: &str,
        enabled: bool,
    ) -> CoreResult<()> {
        self.call(
            key,
            "/tunnels/enable",
            json!({ "tunnel_id": tunnel_id, "enabled": enabled }),
        )
        .await
        .map(drop)
    }

    async fn delete(&self, key: &SecretString, tunnel_id: &str) -> CoreResult<()> {
        self.call(key, "/tunnels/delete", json!({ "tunnel_id": tunnel_id }))
            .await
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpanel_core::playit_api::PlayitTunnelKind;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Shape of a real `/v1/tunnels/list` answer (observed 2026-09-30; addresses changed).
    const LIST: &str = r#"{"status":"success","data":{"tunnels":[{
        "id":"3f1b2c4d-0000-4000-8000-000000000001","created_at":"2026-09-10T10:00:00Z",
        "name":"MC Create 1.21.1","user_enabled":true,"offline_reasons":null,
        "tunnel_type":"minecraft-java","port_type":"tcp","port_count":1,"firewall_id":null,
        "disabled_by_admin":null,"region_change":null,"props":{"hostname_verify_level":"none"},
        "origin":{"type":"agent","details":{"agent_id":"4d95c6dc-3144-4494-8f99-50acfdf92cab",
          "name":"My PC","config_schema_id":"00000000-0000-4000-8000-000000000000",
          "config_data":{"fields":[{"name":"local_ip","value":"127.0.0.1"},{"name":"local_port","value":"25565"}]},
          "config_invalid":null}},
        "port_allocation_requests":[],"public_allocations":[],
        "connect_addresses":[
          {"type":"auto","value":{"address":"example-name.gl.joinmc.link","source":"auto"}},
          {"type":"auto","value":{"address":"26.example.ply.gg:54298","source":"auto"}},
          {"type":"addr4","value":{"address":"147.185.221.26:54298","source":"auto"}},
          {"type":"ip4","value":{"address":"147.185.221.26","default_port":54298,"source":"auto"}}
        ]},
        {"id":"3f1b2c4d-0000-4000-8000-000000000002","name":null,"user_enabled":false,
         "offline_reasons":["TunnelDisabled"],"tunnel_type":null,"port_type":"udp",
         "origin":{"type":"not-set","details":{"agent_config":null}},"connect_addresses":[]}
    ]}}"#;

    #[test]
    fn parses_tunnels_like_the_real_api() {
        let data = parse_envelope(200, LIST).unwrap();
        let t = parse_tunnels(&data);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name.as_deref(), Some("MC Create 1.21.1"));
        assert_eq!(t[0].tunnel_type.as_deref(), Some("minecraft-java"));
        assert!(t[0].enabled);
        assert_eq!(t[0].local_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(t[0].local_port, Some(25565));
        assert_eq!(
            t[0].agent_id.as_deref(),
            Some("4d95c6dc-3144-4494-8f99-50acfdf92cab")
        );
        assert_eq!(
            t[0].addresses,
            vec![
                "example-name.gl.joinmc.link",
                "26.example.ply.gg:54298",
                "147.185.221.26:54298"
            ]
        );
        assert!(!t[1].enabled);
        assert_eq!(t[1].offline_reasons, vec!["TunnelDisabled"]);
        assert_eq!(t[1].local_port, None);
        assert!(t[1].addresses.is_empty());
    }

    #[test]
    fn parses_the_agent() {
        let data = parse_envelope(200, r#"{"status":"success","data":{"agent_id":"a-1","tunnels":[],"pending":[],"notices":[],
            "permissions":{"is_self_managed":false,"has_premium":false,"account_status":"verified"}}}"#).unwrap();
        let a = parse_agent(&data).unwrap();
        assert_eq!(a.agent_id, "a-1");
        assert!(!a.self_managed);
        assert_eq!(a.account_status, "verified");
    }

    #[test]
    fn errors_are_explained() {
        let e = parse_envelope(200, r#"{"status":"fail","data":"TunnelNotFound"}"#).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        let e = parse_envelope(
            200,
            r#"{"status":"fail","data":{"error":"NothingToUpdate"}}"#,
        )
        .unwrap_err();
        assert_eq!(e.message, "Nothing changed.");
        let e = parse_envelope(
            401,
            r#"{"status":"error","data":{"type":"auth","message":"AgentNotSelfManaged"}}"#,
        )
        .unwrap_err();
        assert!(e.message.contains("AgentNotSelfManaged"), "{}", e.message);
        assert!(e.message.contains("playit.gg instead"));
        let e = parse_envelope(429, "").unwrap_err();
        assert!(e.retryable);
        let e = parse_envelope(502, "<html>bad gateway</html>").unwrap_err();
        assert_eq!(e.code, ErrorCode::ProviderError);
    }

    #[test]
    fn create_body_matches_playits_own_client() {
        let b = create_body(
            "agent-1",
            &NewPlayitTunnel {
                name: "Survival".into(),
                kind: PlayitTunnelKind::MinecraftJava,
                local_ip: "127.0.0.1".into(),
                local_port: 25566,
                enabled: true,
            },
        );
        assert_eq!(
            b["ports"],
            json!({"type":"tunnel-type","details":"minecraft-java"})
        );
        assert_eq!(b["origin"]["type"], "agent");
        assert_eq!(b["origin"]["data"]["agent_id"], "agent-1");
        assert_eq!(
            b["origin"]["data"]["config"]["fields"][1],
            json!({"name":"local_port","value":"25566"})
        );
        assert_eq!(
            b["alloc"],
            json!({"type":"region","details":{"region":"global","port":null}})
        );
        assert_eq!(b["enabled"], true);
    }

    /// One recorded request: (path, authorization header, JSON body).
    type Seen = Arc<Mutex<Vec<(String, String, Value)>>>;

    /// A minimal HTTP stand-in that answers every request with `reply`.
    async fn stand_in(reply: &'static str) -> (String, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        let len: usize = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                            let auth = head
                                .lines()
                                .find_map(|l| {
                                    l.strip_prefix("authorization: ")
                                        .or_else(|| l.strip_prefix("Authorization: "))
                                })
                                .unwrap_or("")
                                .to_string();
                            let body = serde_json::from_slice(&buf[end + 4..end + 4 + len])
                                .unwrap_or(Value::Null);
                            log.lock().unwrap().push((path, auth, body));
                            break;
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        (base, seen)
    }

    #[tokio::test]
    async fn sends_agent_key_and_documented_paths() {
        let (base, seen) = stand_in(r#"{"status":"success","data":null}"#).await;
        let api = PlayitWebApi::for_test(base);
        let key = SecretString::from("0123abcd".to_string());
        api.rename(&key, "t-1", "New name").await.unwrap();
        api.set_local_address(&key, "t-1", "127.0.0.1", 25570)
            .await
            .unwrap();
        api.set_enabled(&key, "t-1", false).await.unwrap();
        api.delete(&key, "t-1").await.unwrap();
        let seen = seen.lock().unwrap().clone();
        let paths: Vec<&str> = seen.iter().map(|(p, _, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "/tunnels/rename",
                "/v1/tunnels/config",
                "/tunnels/enable",
                "/tunnels/delete"
            ]
        );
        assert!(seen.iter().all(|(_, a, _)| a == "Agent-Key 0123abcd"));
        assert_eq!(seen[0].2, json!({"tunnel_id":"t-1","name":"New name"}));
        assert_eq!(seen[1].2["new_config"]["fields"][1]["value"], "25570");
        assert_eq!(seen[2].2, json!({"tunnel_id":"t-1","enabled":false}));
        assert_eq!(seen[3].2, json!({"tunnel_id":"t-1"}));
    }

    #[tokio::test]
    async fn claim_setup_sends_no_key_and_asks_for_a_self_managed_agent() {
        let (base, seen) = stand_in(r#"{"status":"success","data":"WaitingForUserVisit"}"#).await;
        let api = PlayitWebApi::for_test(base);
        assert_eq!(
            api.claim_setup("0a1b2c3d4e", "MCPanel 0.1.0")
                .await
                .unwrap(),
            "WaitingForUserVisit"
        );
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].0, "/claim/setup");
        assert_eq!(seen[0].1, "", "no Authorization header");
        assert_eq!(
            seen[0].2,
            json!({"code":"0a1b2c3d4e","agent_type":"self-managed","version":"MCPanel 0.1.0"})
        );
        let e = parse_envelope(200, r#"{"status":"fail","data":"NotAccepted"}"#).unwrap_err();
        assert_eq!(e.details.unwrap()["playit"], "NotAccepted");
    }

    #[tokio::test]
    async fn claim_exchange_returns_the_key() {
        use secrecy::ExposeSecret;
        let (base, _) = stand_in(r#"{"status":"success","data":{"secret_key":"abc123DEF"}}"#).await;
        let key = PlayitWebApi::for_test(base)
            .claim_exchange("0a1b2c3d4e")
            .await
            .unwrap();
        assert_eq!(key.expose_secret(), "abc123DEF");
    }

    #[tokio::test]
    async fn create_returns_the_new_id() {
        let (base, seen) = stand_in(
            r#"{"status":"success","data":{"id":"3f1b2c4d-0000-4000-8000-00000000000a"}}"#,
        )
        .await;
        let api = PlayitWebApi::for_test(base);
        let id = api
            .create(
                &SecretString::from("k".to_string()),
                "agent-1",
                &NewPlayitTunnel {
                    name: "Bedrock".into(),
                    kind: PlayitTunnelKind::MinecraftBedrock,
                    local_ip: "127.0.0.1".into(),
                    local_port: 19132,
                    enabled: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(id, "3f1b2c4d-0000-4000-8000-00000000000a");
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].0, "/v1/tunnels/create");
        assert_eq!(seen[0].2["ports"]["details"], "minecraft-bedrock");
    }
}
