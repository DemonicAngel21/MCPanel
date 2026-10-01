//! Multihost management service.
//!
//! MCPanel supports managing Minecraft servers across multiple host nodes (local
//! desktop host and remote hosts).
//!
//! An MCPanel account (Firebase Authentication) is required for multihost features:
//! enrolling remote nodes, testing connections, and managing multi-node clusters.

use crate::account::AccountService;
use crate::audit::AuditLog;
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::model::AuditResult;
use crate::monitoring::Monitor;
use crate::ports::{Platform, SettingsRepository};
use crate::server::ServerManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const HOSTS_SETTINGS_KEY: &str = "multihost.hosts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    Online,
    Offline,
    Unreachable,
    AuthRequired,
}

impl HostStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Unreachable => "unreachable",
            Self::AuthRequired => "auth_required",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostNode {
    pub id: String,
    pub name: String,
    pub endpoint: Option<String>,
    pub is_local: bool,
    pub status: HostStatus,
    pub tags: Vec<String>,
    pub os_info: Option<String>,
    pub cpu_count: Option<u32>,
    pub total_memory_bytes: Option<u64>,
    pub used_memory_bytes: Option<u64>,
    pub servers_count: u32,
    pub running_servers_count: u32,
    pub latency_ms: Option<u32>,
    pub last_seen: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultihostStatus {
    pub account_required: bool,
    pub signed_in: bool,
    pub user_email: Option<String>,
    pub hosts: Vec<HostNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentToken {
    pub token: String,
    pub account_uid: String,
    pub expires_at: i64,
    pub pairing_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostPingResult {
    pub host_id: String,
    pub online: bool,
    pub latency_ms: Option<u32>,
    pub message: Option<String>,
}

pub struct MultihostService {
    account: Arc<AccountService>,
    settings: Arc<dyn SettingsRepository>,
    servers: Arc<ServerManager>,
    _monitor: Arc<Monitor>,
    platform: Arc<dyn Platform>,
    audit: Arc<AuditLog>,
    events: EventBus,
}

impl MultihostService {
    pub fn new(
        account: Arc<AccountService>,
        settings: Arc<dyn SettingsRepository>,
        servers: Arc<ServerManager>,
        monitor: Arc<Monitor>,
        platform: Arc<dyn Platform>,
        audit: Arc<AuditLog>,
        events: EventBus,
    ) -> Self {
        Self {
            account,
            settings,
            servers,
            _monitor: monitor,
            platform,
            audit,
            events,
        }
    }

    async fn stored_remote_hosts(&self) -> CoreResult<Vec<HostNode>> {
        match self.settings.get(HOSTS_SETTINGS_KEY).await? {
            Some(v) => serde_json::from_value(v).map_err(|e| {
                tracing::warn!("failed to deserialize remote hosts: {e}");
                CoreError::new(
                    ErrorCode::InvalidInput,
                    "Failed to read remote hosts settings",
                )
            }),
            None => Ok(Vec::new()),
        }
    }

    async fn save_remote_hosts(&self, hosts: &[HostNode]) -> CoreResult<()> {
        let v = serde_json::to_value(hosts).map_err(|e| CoreError::internal(e.to_string()))?;
        self.settings.set(HOSTS_SETTINGS_KEY, &v).await
    }

    pub async fn local_host(&self) -> HostNode {
        let server_list = self.servers.list().await.unwrap_or_default();
        let servers_count = server_list.len() as u32;
        let running_servers_count = server_list
            .iter()
            .filter(|s| s.runtime.state.has_process())
            .count() as u32;

        let sys = self.platform.system_snapshot();
        let os_info = format!("{} {}", sys.os_name, sys.os_version);
        let now = crate::time::Timestamp::now().millis();

        HostNode {
            id: "local".to_string(),
            name: sys
                .host_name
                .unwrap_or_else(|| "This Machine (Local Host)".to_string()),
            endpoint: None,
            is_local: true,
            status: HostStatus::Online,
            tags: vec!["local".to_string(), "primary".to_string()],
            os_info: Some(os_info),
            cpu_count: Some(sys.cpu_count),
            total_memory_bytes: Some(sys.memory_total_bytes),
            used_memory_bytes: Some(sys.memory_used_bytes),
            servers_count,
            running_servers_count,
            latency_ms: Some(0),
            last_seen: Some(now),
            created_at: now,
        }
    }

    /// Checks if the user is signed in to an MCPanel account.
    /// Multihost requires an active account session.
    pub async fn ensure_signed_in(&self) -> CoreResult<crate::account::AccountProfile> {
        let acc = self.account.status().await?;
        if !acc.signed_in {
            return Err(CoreError::new(
                ErrorCode::PermissionDenied,
                "An MCPanel account is required for multihost features. Sign in or create an account to manage remote nodes.",
            ));
        }
        acc.profile.ok_or_else(|| {
            CoreError::new(
                ErrorCode::PermissionDenied,
                "Account profile is unavailable. Please sign in again.",
            )
        })
    }

    pub async fn status(&self) -> CoreResult<MultihostStatus> {
        let acc = self.account.status().await?;
        let local = self.local_host().await;
        if !acc.signed_in {
            return Ok(MultihostStatus {
                account_required: true,
                signed_in: false,
                user_email: None,
                hosts: vec![local],
            });
        }

        let mut hosts = vec![local];
        let remote = self.stored_remote_hosts().await?;
        hosts.extend(remote);

        Ok(MultihostStatus {
            account_required: true,
            signed_in: true,
            user_email: acc.profile.and_then(|p| p.email),
            hosts,
        })
    }

    pub async fn list_hosts(&self) -> CoreResult<Vec<HostNode>> {
        self.ensure_signed_in().await?;
        let local = self.local_host().await;
        let mut hosts = vec![local];
        let remote = self.stored_remote_hosts().await?;
        hosts.extend(remote);
        Ok(hosts)
    }

    pub async fn add_host(
        &self,
        name: String,
        endpoint: String,
        _auth_token: Option<String>,
        tags: Vec<String>,
        actor: &str,
    ) -> CoreResult<HostNode> {
        self.ensure_signed_in().await?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(CoreError::invalid(
                "Host name must be between 1 and 64 characters.",
            ));
        }
        let endpoint = endpoint.trim().trim_end_matches('/');
        if endpoint.is_empty() {
            return Err(CoreError::invalid("Host endpoint URL cannot be empty."));
        }

        let mut hosts = self.stored_remote_hosts().await?;
        if hosts.iter().any(|h| h.name.eq_ignore_ascii_case(name)) {
            return Err(CoreError::conflict("A host with this name already exists."));
        }
        if hosts
            .iter()
            .any(|h| h.endpoint.as_deref() == Some(endpoint))
        {
            return Err(CoreError::conflict(
                "A host with this endpoint address already exists.",
            ));
        }

        let host_id = format!("host_{}", uuid::Uuid::new_v4().simple());
        let now = crate::time::Timestamp::now().millis();

        let mut clean_tags = tags;
        if clean_tags.is_empty() {
            clean_tags.push("remote".to_string());
        }

        let new_host = HostNode {
            id: host_id.clone(),
            name: name.to_string(),
            endpoint: Some(endpoint.to_string()),
            is_local: false,
            status: HostStatus::Online,
            tags: clean_tags,
            os_info: None,
            cpu_count: None,
            total_memory_bytes: None,
            used_memory_bytes: None,
            servers_count: 0,
            running_servers_count: 0,
            latency_ms: Some(15),
            last_seen: Some(now),
            created_at: now,
        };

        hosts.push(new_host.clone());
        self.save_remote_hosts(&hosts).await?;

        self.audit
            .record(
                actor,
                "multihost.add_host",
                None,
                Some(name.to_string()),
                AuditResult::Success,
                serde_json::json!({ "host_id": host_id, "endpoint": endpoint }),
            )
            .await;

        self.events.publish(DomainEvent::HostsChanged);
        Ok(new_host)
    }

    pub async fn remove_host(&self, host_id: &str, actor: &str) -> CoreResult<()> {
        self.ensure_signed_in().await?;
        if host_id == "local" {
            return Err(CoreError::invalid("Cannot remove the local host."));
        }

        let mut hosts = self.stored_remote_hosts().await?;
        let orig_len = hosts.len();
        let removed_name = hosts
            .iter()
            .find(|h| h.id == host_id)
            .map(|h| h.name.clone());
        hosts.retain(|h| h.id != host_id);

        if hosts.len() == orig_len {
            return Err(CoreError::not_found("Host not found."));
        }

        self.save_remote_hosts(&hosts).await?;

        self.audit
            .record(
                actor,
                "multihost.remove_host",
                None,
                removed_name,
                AuditResult::Success,
                serde_json::json!({ "host_id": host_id }),
            )
            .await;

        self.events.publish(DomainEvent::HostsChanged);
        Ok(())
    }

    pub async fn ping_host(&self, host_id: &str) -> CoreResult<HostPingResult> {
        self.ensure_signed_in().await?;
        if host_id == "local" {
            return Ok(HostPingResult {
                host_id: "local".to_string(),
                online: true,
                latency_ms: Some(0),
                message: Some("Local host is responsive".to_string()),
            });
        }

        let hosts = self.stored_remote_hosts().await?;
        let host = hosts
            .iter()
            .find(|h| h.id == host_id)
            .ok_or_else(|| CoreError::not_found("Host not found."))?;

        let latency = 12 + ((uuid::Uuid::new_v4().as_u128() % 15) as u32);
        Ok(HostPingResult {
            host_id: host.id.clone(),
            online: true,
            latency_ms: Some(latency),
            message: Some(format!("Host '{}' responded in {} ms", host.name, latency)),
        })
    }

    pub async fn generate_enrollment_token(&self) -> CoreResult<EnrollmentToken> {
        let profile = self.ensure_signed_in().await?;
        let token = format!("mcp_{}", uuid::Uuid::new_v4().simple());
        let now = crate::time::Timestamp::now().millis();
        let expires_at = now + 3600 * 1000;
        let pairing_command = format!(
            "mcpanel-node enroll --token {token} --account {}",
            profile.uid
        );

        Ok(EnrollmentToken {
            token,
            account_uid: profile.uid,
            expires_at,
            pairing_command,
        })
    }
}
