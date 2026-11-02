//! Multihost management service.
//!
//! The hosts view includes the local machine and Google Compute Engine instances.
//! Google OAuth authorizes direct Compute Engine REST calls. Refresh credentials are held
//! by AccountService in the platform secret store.

use crate::audit::AuditLog;
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::model::AuditResult;
use crate::monitoring::Monitor;
use crate::ports::{Platform, SettingsRepository};
use crate::server::ServerManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub const HOSTS_SETTINGS_KEY: &str = "multihost.hosts";

fn clear_unverified_remote_data(host: &mut HostNode) {
    host.status = HostStatus::Unreachable;
    host.os_info = None;
    host.cpu_count = None;
    host.total_memory_bytes = None;
    host.used_memory_bytes = None;
    host.servers_count = 0;
    host.running_servers_count = 0;
    host.latency_ms = None;
    host.last_seen = None;
}

fn validate_google_vm(vm: &GoogleCloudVm) -> CoreResult<()> {
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= 63
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && value.as_bytes()[0].is_ascii_lowercase()
            && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
    };
    if !valid(&vm.project_id) || !valid(&vm.zone) || !valid(&vm.instance_name) {
        return Err(CoreError::invalid(
            "Project ID, zone, and instance name must use lowercase letters, numbers, and hyphens.",
        ));
    }
    Ok(())
}

fn gce_host_status(details: &Value) -> HostStatus {
    match details.get("status").and_then(Value::as_str) {
        Some("RUNNING") => HostStatus::Online,
        Some("TERMINATED" | "SUSPENDED") => HostStatus::Offline,
        Some("PROVISIONING" | "STAGING") => HostStatus::Starting,
        Some("STOPPING" | "SUSPENDING") => HostStatus::Stopping,
        Some(_) => HostStatus::Unreachable,
        None => HostStatus::Unreachable,
    }
}

fn google_external_ip(details: &Value) -> Option<String> {
    details
        .get("networkInterfaces")?
        .as_array()?
        .iter()
        .flat_map(|interface| {
            interface
                .get("accessConfigs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find_map(|config| {
            config
                .get("natIP")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCloudVm {
    pub project_id: String,
    pub zone: String,
    pub instance_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    Online,
    Offline,
    Starting,
    Stopping,
    Unreachable,
    AuthRequired,
}

impl HostStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Starting => "starting",
            Self::Stopping => "stopping",
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
    #[serde(default)]
    pub google_cloud: Option<GoogleCloudVm>,
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
    pub hosts: Vec<HostNode>,
}

pub struct MultihostService {
    account: Arc<crate::account::AccountService>,
    settings: Arc<dyn SettingsRepository>,
    servers: Arc<ServerManager>,
    _monitor: Arc<Monitor>,
    platform: Arc<dyn Platform>,
    audit: Arc<AuditLog>,
    events: EventBus,
}

impl MultihostService {
    pub fn new(
        account: Arc<crate::account::AccountService>,
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
            google_cloud: None,
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

    pub async fn status(&self) -> CoreResult<MultihostStatus> {
        let local = self.local_host().await;
        let mut hosts = vec![local];
        let mut remote = self.stored_remote_hosts().await?;
        for host in &mut remote {
            if host.google_cloud.is_none() {
                clear_unverified_remote_data(host);
            }
        }
        hosts.extend(remote);

        Ok(MultihostStatus { hosts })
    }

    pub async fn list_hosts(&self) -> CoreResult<Vec<HostNode>> {
        let local = self.local_host().await;
        let mut hosts = vec![local];
        let mut remote = self.stored_remote_hosts().await?;
        for host in &mut remote {
            if host.google_cloud.is_none() {
                clear_unverified_remote_data(host);
            }
        }
        hosts.extend(remote);
        Ok(hosts)
    }

    pub async fn add_google_host(
        &self,
        name: String,
        vm: GoogleCloudVm,
        actor: &str,
    ) -> CoreResult<HostNode> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(CoreError::invalid(
                "Host name must be between 1 and 64 characters.",
            ));
        }
        validate_google_vm(&vm)?;
        let details = self.describe_google_vm(&vm).await?;
        let mut hosts = self.stored_remote_hosts().await?;
        if hosts.iter().any(|h| h.name.eq_ignore_ascii_case(name)) {
            return Err(CoreError::conflict("A host with this name already exists."));
        }
        if hosts.iter().any(|h| {
            h.google_cloud.as_ref().is_some_and(|existing| {
                existing.project_id == vm.project_id
                    && existing.zone == vm.zone
                    && existing.instance_name == vm.instance_name
            })
        }) {
            return Err(CoreError::conflict(
                "This Google Cloud VM is already saved.",
            ));
        }

        let now = crate::time::Timestamp::now().millis();
        let mut host = HostNode {
            id: format!("host_{}", uuid::Uuid::new_v4().simple()),
            name: name.to_string(),
            endpoint: google_external_ip(&details),
            google_cloud: Some(vm.clone()),
            is_local: false,
            status: gce_host_status(&details),
            tags: vec!["google-cloud".into(), "gce".into()],
            os_info: None,
            cpu_count: None,
            total_memory_bytes: None,
            used_memory_bytes: None,
            servers_count: 0,
            running_servers_count: 0,
            latency_ms: None,
            last_seen: None,
            created_at: now,
        };
        if host.status != HostStatus::Offline {
            host.status = HostStatus::Unreachable;
        }
        hosts.push(host.clone());
        self.save_remote_hosts(&hosts).await?;
        self.audit
            .record(
                actor,
                "multihost.add_google_host",
                None,
                Some(name.to_string()),
                AuditResult::Success,
                serde_json::json!({
                    "host_id": host.id,
                    "project_id": vm.project_id,
                    "zone": vm.zone,
                    "instance_name": vm.instance_name,
                }),
            )
            .await;
        self.events.publish(DomainEvent::HostsChanged);
        Ok(host)
    }

    pub async fn refresh_google_host(&self, host_id: &str) -> CoreResult<HostNode> {
        let mut hosts = self.stored_remote_hosts().await?;
        let host = hosts
            .iter_mut()
            .find(|host| host.id == host_id)
            .ok_or_else(|| CoreError::not_found("Host not found."))?;
        let vm = host.google_cloud.as_ref().ok_or_else(|| {
            CoreError::new(
                ErrorCode::Unsupported,
                "This saved host is not a Google Compute Engine VM.",
            )
        })?;
        let details = self.describe_google_vm(vm).await?;
        host.endpoint = google_external_ip(&details);
        host.status = gce_host_status(&details);
        host.os_info = None;
        host.cpu_count = None;
        host.total_memory_bytes = None;
        host.used_memory_bytes = None;
        host.latency_ms = None;
        host.last_seen = Some(crate::time::Timestamp::now().millis());

        let updated = host.clone();
        self.save_remote_hosts(&hosts).await?;
        self.events.publish(DomainEvent::HostsChanged);
        Ok(updated)
    }

    pub async fn set_google_host_power(
        &self,
        host_id: &str,
        start: bool,
        actor: &str,
    ) -> CoreResult<()> {
        let hosts = self.stored_remote_hosts().await?;
        let host = hosts
            .iter()
            .find(|host| host.id == host_id)
            .ok_or_else(|| CoreError::not_found("Host not found."))?;
        let vm = host.google_cloud.as_ref().ok_or_else(|| {
            CoreError::new(
                ErrorCode::Unsupported,
                "This saved host is not a Google Compute Engine VM.",
            )
        })?;
        validate_google_vm(vm)?;
        let action = if start { "start" } else { "stop" };
        let url = format!(
            "https://compute.googleapis.com/compute/v1/projects/{}/zones/{}/instances/{}/{}",
            vm.project_id, vm.zone, vm.instance_name, action
        );
        self.account.google_compute_request("POST", &url).await?;
        self.audit
            .record(
                actor,
                if start {
                    "multihost.google_start"
                } else {
                    "multihost.google_stop"
                },
                None,
                Some(host.name.clone()),
                AuditResult::Success,
                serde_json::json!({
                    "host_id": host.id,
                    "project_id": vm.project_id,
                    "zone": vm.zone,
                    "instance_name": vm.instance_name,
                }),
            )
            .await;
        self.events.publish(DomainEvent::HostsChanged);
        Ok(())
    }

    async fn describe_google_vm(&self, vm: &GoogleCloudVm) -> CoreResult<Value> {
        let url = format!(
            "https://compute.googleapis.com/compute/v1/projects/{}/zones/{}/instances/{}",
            vm.project_id, vm.zone, vm.instance_name
        );
        self.account.google_compute_request("GET", &url).await
    }

    pub async fn add_host(
        &self,
        name: String,
        endpoint: String,
        tags: Vec<String>,
        actor: &str,
    ) -> CoreResult<HostNode> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(CoreError::invalid(
                "Host name must be between 1 and 64 characters.",
            ));
        }
        let endpoint = endpoint.trim().trim_end_matches('/');
        let authority = endpoint
            .strip_prefix("https://")
            .unwrap_or_default()
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        if !endpoint.starts_with("https://")
            || authority.is_empty()
            || authority.starts_with(':')
            || endpoint.contains('@')
            || endpoint.chars().any(char::is_whitespace)
        {
            return Err(CoreError::invalid(
                "Remote host endpoints must be HTTPS URLs without embedded credentials.",
            ));
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
            google_cloud: None,
            is_local: false,
            status: HostStatus::Unreachable,
            tags: clean_tags,
            os_info: None,
            cpu_count: None,
            total_memory_bytes: None,
            used_memory_bytes: None,
            servers_count: 0,
            running_servers_count: 0,
            latency_ms: None,
            last_seen: None,
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
}
