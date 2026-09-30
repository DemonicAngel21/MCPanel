//! Port for playit.gg's web API (`https://api.playit.gg`), used for tunnel management.
//!
//! playit.gg does not publish this API for third-party programs ("We have an API, just
//! not public", playit-agent issue #150); its shapes come from playit's own open-source
//! agent (`packages/api_client/src/api.rs` @ 4c27794, BSD-2-Clause) and are the ones
//! playit's official Minecraft plugin uses. Every call is authenticated with the local
//! agent's secret key, which MCPanel only reads after the user turns tunnel management
//! on, keeps in memory for the call and never stores or logs (ADR-0007, amendment 2).

use crate::error::CoreResult;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayitAgentInfo {
    pub agent_id: String,
    /// Self-managed agents may only change their own tunnels.
    pub self_managed: bool,
    pub premium: bool,
    /// "guest" | "email-not-verified" | "verified"
    pub account_status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayitTunnelKind {
    /// TCP, for Minecraft Java Edition.
    MinecraftJava,
    /// UDP, for Minecraft Bedrock Edition (Geyser).
    MinecraftBedrock,
}

impl PlayitTunnelKind {
    pub fn api_name(self) -> &'static str {
        match self {
            Self::MinecraftJava => "minecraft-java",
            Self::MinecraftBedrock => "minecraft-bedrock",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayitTunnelInfo {
    pub id: String,
    pub name: Option<String>,
    /// playit's tunnel type (e.g. "minecraft-java"), `None` for custom port tunnels.
    pub tunnel_type: Option<String>,
    /// "tcp" | "udp" | "both"
    pub port_type: String,
    pub enabled: bool,
    /// playit's reasons why the tunnel is offline (e.g. "TunnelDisabled").
    pub offline_reasons: Vec<String>,
    pub agent_id: Option<String>,
    pub local_ip: Option<String>,
    pub local_port: Option<u16>,
    /// Addresses players connect to, best first.
    pub addresses: Vec<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewPlayitTunnel {
    pub name: String,
    pub kind: PlayitTunnelKind,
    pub local_ip: String,
    pub local_port: u16,
    pub enabled: bool,
}

#[async_trait::async_trait]
pub trait PlayitApi: Send + Sync {
    /// `/claim/setup` for a self-managed agent (no key needed). Returns playit's state:
    /// "WaitingForUserVisit" | "WaitingForUser" | "UserAccepted" | "UserRejected".
    async fn claim_setup(&self, code: &str, version: &str) -> CoreResult<String>;
    /// `/claim/exchange`: the new agent's secret key once the user accepted.
    async fn claim_exchange(&self, code: &str) -> CoreResult<SecretString>;
    /// `/v1/agents/rundata`: the agent the key belongs to.
    async fn agent(&self, key: &SecretString) -> CoreResult<PlayitAgentInfo>;
    /// `/v1/tunnels/list`: the account's tunnels.
    async fn tunnels(&self, key: &SecretString) -> CoreResult<Vec<PlayitTunnelInfo>>;
    /// `/v1/tunnels/create` (region "global", like playit's plugin). Returns the tunnel id.
    async fn create(
        &self,
        key: &SecretString,
        agent_id: &str,
        tunnel: &NewPlayitTunnel,
    ) -> CoreResult<String>;
    /// `/tunnels/rename`.
    async fn rename(&self, key: &SecretString, tunnel_id: &str, name: &str) -> CoreResult<()>;
    /// `/v1/tunnels/config`: the local address the agent forwards to.
    async fn set_local_address(
        &self,
        key: &SecretString,
        tunnel_id: &str,
        local_ip: &str,
        local_port: u16,
    ) -> CoreResult<()>;
    /// `/tunnels/enable`.
    async fn set_enabled(
        &self,
        key: &SecretString,
        tunnel_id: &str,
        enabled: bool,
    ) -> CoreResult<()>;
    /// `/tunnels/delete`.
    async fn delete(&self, key: &SecretString, tunnel_id: &str) -> CoreResult<()>;
}
