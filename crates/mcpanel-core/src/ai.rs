//! AI Assistant domain types, tools schema, execution, and confirmation management.

use crate::audit::AuditLog;
use crate::backup::BackupService;
use crate::content::{ContentService, InstallRequest, SearchSort};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::files::service::WriteText;
use crate::files::text::TextEncoding;
use crate::ids::{BackupId, ServerId};
use crate::model::AuditResult;
use crate::ports::{SecretStore, SettingsRepository};
use crate::server::{PropertyChange, ServerManager};
use crate::server_files::ServerFiles;
use async_trait::async_trait;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

pub const AI_SECRET_KEY: &str = "ai-api-key";
pub const AI_SETTINGS_KEY: &str = "ai_config";
pub const DEFAULT_PROVIDER: &str = "gemini";
pub const DEFAULT_MODEL: &str = "gemini-3.8-flash";

pub const KNOWN_PROVIDERS: &[&str] = &[
    "gemini",
    "openai",
    "anthropic",
    "deepseek",
    "groq",
    "openrouter",
    "mistral",
    "ollama",
    "lmstudio",
    "custom",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub configured: bool,
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    pub use_shared_key: bool,
    pub sync_api_keys: bool,
    pub has_key_for_provider: bool,
    pub configured_providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfigPatch {
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub use_shared_key: Option<bool>,
    pub sync_api_keys: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolParameterProperty {
    pub r#type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<AiToolParameterProperty>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolParameters {
    pub r#type: String,
    pub properties: HashMap<String, AiToolParameterProperty>,
    pub required: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: AiToolParameters,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiFunctionCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiFunctionResponse {
    pub id: String,
    pub name: String,
    pub response: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiLlmMessage {
    pub role: String, // "system" | "user" | "model" | "function"
    pub content: Option<String>,
    pub function_calls: Option<Vec<AiFunctionCall>>,
    pub function_responses: Option<Vec<AiFunctionResponse>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiLlmRequest {
    pub system_instruction: Option<String>,
    pub messages: Vec<AiLlmMessage>,
    pub tools: Vec<AiToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiLlmResponse {
    pub content: Option<String>,
    pub function_calls: Vec<AiFunctionCall>,
}

#[async_trait]
pub trait AiClient: Send + Sync {
    async fn test_key(
        &self,
        key: &SecretString,
        provider: &str,
        model: &str,
        base_url: Option<&str>,
    ) -> CoreResult<()>;

    async fn complete_turn(
        &self,
        key: &SecretString,
        provider: &str,
        model: &str,
        base_url: Option<&str>,
        request: &AiLlmRequest,
    ) -> CoreResult<AiLlmResponse>;
}

pub struct NoopAiClient;

#[async_trait]
impl AiClient for NoopAiClient {
    async fn test_key(
        &self,
        _key: &SecretString,
        _provider: &str,
        _model: &str,
        _base_url: Option<&str>,
    ) -> CoreResult<()> {
        Err(CoreError::new(
            ErrorCode::ProviderUnavailable,
            "AI client is not configured",
        ))
    }

    async fn complete_turn(
        &self,
        _key: &SecretString,
        _provider: &str,
        _model: &str,
        _base_url: Option<&str>,
        _request: &AiLlmRequest,
    ) -> CoreResult<AiLlmResponse> {
        Err(CoreError::new(
            ErrorCode::ProviderUnavailable,
            "AI client is not configured",
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiPendingConfirmation {
    pub confirmation_id: String,
    pub tool: String,
    pub title: String,
    pub description: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolExecution {
    pub tool: String,
    pub description: String,
    pub is_major: bool,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    pub pending_confirmation: Option<AiPendingConfirmation>,
    pub tool_executions: Option<Vec<AiToolExecution>>,
}

struct StoredConfirmation {
    tool: String,
    args: Value,
    #[allow(dead_code)]
    title: String,
    description: String,
    server_id: Option<ServerId>,
}

pub struct AiService {
    client: Arc<dyn AiClient>,
    secrets: Arc<dyn SecretStore>,
    settings: Arc<dyn SettingsRepository>,
    audit: Arc<AuditLog>,
    servers: Arc<ServerManager>,
    files: Arc<ServerFiles>,
    backups: Arc<BackupService>,
    content: Arc<ContentService>,
    account: Option<Arc<crate::account::AccountService>>,
    pending_confirmations: Mutex<HashMap<String, StoredConfirmation>>,
}

impl AiService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Arc<dyn AiClient>,
        secrets: Arc<dyn SecretStore>,
        settings: Arc<dyn SettingsRepository>,
        audit: Arc<AuditLog>,
        servers: Arc<ServerManager>,
        files: Arc<ServerFiles>,
        backups: Arc<BackupService>,
        content: Arc<ContentService>,
    ) -> Self {
        Self {
            client,
            secrets,
            settings,
            audit,
            servers,
            files,
            backups,
            content,
            account: None,
            pending_confirmations: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_account(mut self, account: Arc<crate::account::AccountService>) -> Self {
        self.account = Some(account);
        self
    }

    /// Retrieve the current logged-in account ID/UID, or "default" if guest/offline/sync disabled.
    async fn current_account_id(&self) -> String {
        let sync_enabled: bool = self
            .settings
            .get(AI_SETTINGS_KEY)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.get("syncApiKeys").and_then(|s| s.as_bool()))
            .unwrap_or(true);

        if !sync_enabled {
            return "default".to_string();
        }

        if let Some(acc) = &self.account
            && let Ok(st) = acc.status().await
            && let Some(prof) = st.profile
            && !prof.uid.is_empty()
        {
            return prof.uid;
        }
        "default".to_string()
    }

    /// Calculate secret store key name for a provider under an account.
    pub fn secret_key_for_provider(account_id: &str, provider: &str, use_shared: bool) -> String {
        let p = provider.to_ascii_lowercase();
        if use_shared {
            format!("ai_api_key:{account_id}:shared")
        } else {
            format!("ai_api_key:{account_id}:{p}")
        }
    }

    /// Resolve an API key checking account-scoped provider keys, shared keys, and legacy fallbacks.
    pub async fn resolve_api_key(
        &self,
        provider: &str,
        use_shared: bool,
    ) -> CoreResult<Option<SecretString>> {
        if Self::is_local_provider(provider) {
            return Ok(Some(SecretString::new("local".into())));
        }
        let account_id = self.current_account_id().await;

        // 1. Account-specific key (provider-specific or shared)
        let key_name = Self::secret_key_for_provider(&account_id, provider, use_shared);
        if let Some(k) = self.secrets.get(&key_name)? {
            return Ok(Some(k));
        }

        // 2. If provider-specific didn't match, check account shared key
        if !use_shared {
            let shared_key = Self::secret_key_for_provider(&account_id, provider, true);
            if let Some(k) = self.secrets.get(&shared_key)? {
                return Ok(Some(k));
            }
        }

        // 3. Fallback: Global provider key without account scope
        let p = provider.to_ascii_lowercase();
        let global_p_key = format!("ai_api_key:{p}");
        if let Some(k) = self.secrets.get(&global_p_key)? {
            return Ok(Some(k));
        }

        // 4. Fallback: Legacy flat AI_SECRET_KEY
        if let Some(k) = self.secrets.get(AI_SECRET_KEY)? {
            return Ok(Some(k));
        }

        Ok(None)
    }

    pub async fn has_key_for_provider(&self, provider: &str, use_shared: bool) -> bool {
        if Self::is_local_provider(provider) {
            return true;
        }
        self.resolve_api_key(provider, use_shared)
            .await
            .ok()
            .flatten()
            .is_some()
    }

    pub async fn configured_providers(&self, use_shared: bool) -> Vec<String> {
        let mut list = Vec::new();
        for &p in KNOWN_PROVIDERS {
            if self.has_key_for_provider(p, use_shared).await {
                list.push(p.to_string());
            }
        }
        list
    }

    /// Whether an action is major and requires explicit user confirmation.
    pub fn is_major_action(tool: &str) -> bool {
        matches!(
            tool,
            "start_server"
                | "stop_server"
                | "restart_server"
                | "delete_server_file"
                | "delete_backup"
                | "restore_backup"
                | "send_console_command"
        )
    }

    /// Whether a provider runs locally on the user's machine without requiring a cloud API key.
    pub fn is_local_provider(provider: &str) -> bool {
        matches!(
            provider.to_ascii_lowercase().as_str(),
            "ollama" | "lmstudio" | "local" | "localai" | "vllm"
        )
    }

    pub async fn get_config(&self) -> CoreResult<AiConfig> {
        let stored = self.settings.get(AI_SETTINGS_KEY).await?;
        let (provider, mut model, base_url, use_shared_key, sync_api_keys) = match stored {
            Some(v) => (
                v.get("provider")
                    .and_then(|p| p.as_str())
                    .unwrap_or(DEFAULT_PROVIDER)
                    .to_string(),
                v.get("model")
                    .and_then(|m| m.as_str())
                    .unwrap_or(DEFAULT_MODEL)
                    .to_string(),
                v.get("baseUrl")
                    .and_then(|b| b.as_str())
                    .map(|b| b.to_string()),
                v.get("useSharedKey")
                    .and_then(|u| u.as_bool())
                    .unwrap_or(false),
                v.get("syncApiKeys")
                    .and_then(|s| s.as_bool())
                    .unwrap_or(true),
            ),
            None => (
                DEFAULT_PROVIDER.to_string(),
                DEFAULT_MODEL.to_string(),
                None,
                false,
                true,
            ),
        };
        if provider == "gemini"
            && (model == "gemini-2.5-flash" || model == "models/gemini-2.5-flash")
        {
            model = DEFAULT_MODEL.to_string();
        }

        let has_key = self.has_key_for_provider(&provider, use_shared_key).await;
        let is_local = Self::is_local_provider(&provider);
        let configured = (is_local || has_key) && provider != "none";
        let configured_providers = self.configured_providers(use_shared_key).await;

        Ok(AiConfig {
            configured,
            provider,
            model,
            base_url,
            use_shared_key,
            sync_api_keys,
            has_key_for_provider: has_key,
            configured_providers,
        })
    }

    pub async fn save_config(&self, patch: AiConfigPatch) -> CoreResult<AiConfig> {
        let current = self.get_config().await?;
        let use_shared_key = patch.use_shared_key.unwrap_or(current.use_shared_key);
        let sync_api_keys = patch.sync_api_keys.unwrap_or(current.sync_api_keys);
        let provider = patch.provider.unwrap_or(current.provider);
        let account_id = if sync_api_keys {
            if let Some(acc) = &self.account
                && let Ok(st) = acc.status().await
                && let Some(prof) = st.profile
                && !prof.uid.is_empty()
            {
                prof.uid
            } else {
                "default".to_string()
            }
        } else {
            "default".to_string()
        };

        // Check if explicitly disconnecting
        let is_disconnect = patch.api_key.as_deref() == Some("") && patch.model.is_none();
        if is_disconnect {
            let p_key = Self::secret_key_for_provider(&account_id, &provider, false);
            let s_key = Self::secret_key_for_provider(&account_id, &provider, true);
            let _ = self.secrets.delete(&p_key);
            let _ = self.secrets.delete(&s_key);
            let _ = self.secrets.delete(AI_SECRET_KEY);

            let value = json!({
                "provider": "none",
                "model": DEFAULT_MODEL,
                "baseUrl": null,
                "useSharedKey": false,
                "syncApiKeys": sync_api_keys,
            });
            self.settings.set(AI_SETTINGS_KEY, &value).await?;
            return self.get_config().await;
        }

        if let Some(key) = patch.api_key {
            let trimmed = key.trim();
            let key_name = Self::secret_key_for_provider(&account_id, &provider, use_shared_key);
            if trimmed.is_empty() {
                let _ = self.secrets.delete(&key_name);
                let _ = self.secrets.delete(AI_SECRET_KEY);
            } else {
                let sec = SecretString::new(trimmed.to_string().into_boxed_str());
                self.secrets.set(&key_name, &sec)?;
            }
        }

        let mut model = patch.model.unwrap_or(current.model);
        if provider == "gemini"
            && (model == "gemini-2.5-flash" || model == "models/gemini-2.5-flash")
        {
            model = DEFAULT_MODEL.to_string();
        }
        let base_url = patch.base_url.or(current.base_url);

        let value = json!({
            "provider": provider,
            "model": model,
            "baseUrl": base_url,
            "useSharedKey": use_shared_key,
            "syncApiKeys": sync_api_keys,
        });
        self.settings.set(AI_SETTINGS_KEY, &value).await?;

        self.get_config().await
    }

    pub async fn test_connection(&self) -> CoreResult<()> {
        let config = self.get_config().await?;
        let key = match self
            .resolve_api_key(&config.provider, config.use_shared_key)
            .await?
        {
            Some(k) => k,
            None if Self::is_local_provider(&config.provider) => SecretString::new("local".into()),
            None => {
                return Err(CoreError::new(
                    ErrorCode::InvalidInput,
                    "No AI API key set for this provider. Please enter an API key or select a local provider.",
                ));
            }
        };
        self.client
            .test_key(
                &key,
                &config.provider,
                &config.model,
                config.base_url.as_deref(),
            )
            .await
    }

    /// Build the full set of MCPanel tools available to the AI.
    pub fn available_tools() -> Vec<AiToolDefinition> {
        vec![
            AiToolDefinition {
                name: "list_servers".into(),
                description: "List all Minecraft servers managed by MCPanel, with IDs, names, status, and ports.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::new(),
                    required: vec![],
                },
            },
            AiToolDefinition {
                name: "get_server_info".into(),
                description: "Get detailed information about a specific Minecraft server.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "server_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The unique ID of the server.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "read_server_properties".into(),
                description: "Read all settings from server.properties for a server.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "server_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The server ID.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "update_server_properties".into(),
                description: "Update settings in server.properties (e.g. max-players, motd, difficulty, gamemode, view-distance). This minor configuration action is executed efficiently without asking for confirmation.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "properties".into(),
                            AiToolParameterProperty {
                                r#type: "object".into(),
                                description: "Key-value map of properties to change (values must be strings, e.g. {\"max-players\": \"50\", \"difficulty\": \"hard\"}).".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into(), "properties".into()],
                },
            },
            AiToolDefinition {
                name: "list_files".into(),
                description: "List files and folders in a server directory (e.g. '', 'plugins', 'config').".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "path".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Relative path inside server directory (e.g. 'plugins', or '' for root).".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "read_file".into(),
                description: "Read the text content of a server file (e.g. plugin configs, bukkit.yml, server.properties, crash reports).".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "path".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Relative file path (e.g. 'plugins/Essentials/config.yml').".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into(), "path".into()],
                },
            },
            AiToolDefinition {
                name: "write_file".into(),
                description: "Write or update text in a server file (e.g. editing plugin configs, yaml settings, scripts). This is a minor action executed efficiently without asking for confirmation every time.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "path".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Relative file path (e.g. 'plugins/Essentials/config.yml').".into(),
                                items: None,
                            },
                        ),
                        (
                            "content".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The full text content to write.".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into(), "path".into(), "content".into()],
                },
            },
            AiToolDefinition {
                name: "list_plugins".into(),
                description: "List all installed plugins and mods for a server.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "server_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The server ID.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "search_plugins".into(),
                description: "Search for plugins or mods on Modrinth and SpigotMC.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "query".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Search terms (e.g. 'Essentials', 'WorldEdit', 'LuckPerms').".into(),
                                items: None,
                            },
                        ),
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Optional server ID to check compatibility against.".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["query".into()],
                },
            },
            AiToolDefinition {
                name: "install_plugin".into(),
                description: "Download and install a plugin or mod from Modrinth or SpigotMC.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "provider".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Provider name ('modrinth' or 'spiget').".into(),
                                items: None,
                            },
                        ),
                        (
                            "project_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Project ID or slug (e.g. 'luckperms' on modrinth).".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into(), "provider".into(), "project_id".into()],
                },
            },
            AiToolDefinition {
                name: "get_console_logs".into(),
                description: "Read the latest console output or logs from a server to check server activity or diagnose crashes.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "lines".into(),
                            AiToolParameterProperty {
                                r#type: "integer".into(),
                                description: "Number of lines to read (default 50, max 200).".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "list_backups".into(),
                description: "List all existing backups for a server.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "server_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The server ID.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "create_backup".into(),
                description: "Create a new backup archive of a server.".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "note".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Optional note for the backup (e.g. 'Before installing Essentials').".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into()],
                },
            },
            // ───────────────── MAJOR TOOLS (REQUIRE CONFIRMATION) ─────────────────
            AiToolDefinition {
                name: "start_server".into(),
                description: "Turn on / start a Minecraft server. [MAJOR ACTION - requires user confirmation].".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "server_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The server ID to start.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "stop_server".into(),
                description: "Turn off / stop a running Minecraft server gracefully. [MAJOR ACTION - requires user confirmation].".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "server_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The server ID to stop.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "restart_server".into(),
                description: "Restart a running Minecraft server. [MAJOR ACTION - requires user confirmation].".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "server_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The server ID to restart.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["server_id".into()],
                },
            },
            AiToolDefinition {
                name: "delete_server_file".into(),
                description: "Permanently delete a file or folder from a server. [MAJOR ACTION - requires user confirmation].".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "path".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "Relative file/folder path to delete.".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into(), "path".into()],
                },
            },
            AiToolDefinition {
                name: "delete_backup".into(),
                description: "Permanently delete a backup archive. [MAJOR ACTION - requires user confirmation].".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "backup_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The backup ID to delete.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["backup_id".into()],
                },
            },
            AiToolDefinition {
                name: "restore_backup".into(),
                description: "Restore a server from a backup archive (overwrites existing files). [MAJOR ACTION - requires user confirmation].".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([(
                        "backup_id".into(),
                        AiToolParameterProperty {
                            r#type: "string".into(),
                            description: "The backup ID to restore.".into(),
                            items: None,
                        },
                    )]),
                    required: vec!["backup_id".into()],
                },
            },
            AiToolDefinition {
                name: "send_console_command".into(),
                description: "Send a command to the running server console (e.g. 'op playerName', 'whitelist add playerName', 'reload'). [MAJOR ACTION - requires user confirmation].".into(),
                parameters: AiToolParameters {
                    r#type: "object".into(),
                    properties: HashMap::from([
                        (
                            "server_id".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The server ID.".into(),
                                items: None,
                            },
                        ),
                        (
                            "command".into(),
                            AiToolParameterProperty {
                                r#type: "string".into(),
                                description: "The console command to run (without leading slash).".into(),
                                items: None,
                            },
                        ),
                    ]),
                    required: vec!["server_id".into(), "command".into()],
                },
            },
        ]
    }

    /// System instructions given to the AI.
    fn system_prompt(&self, focused_server_id: Option<&str>) -> String {
        let mut prompt = String::from(
            "You are MCPanel AI Assistant, an expert, high-speed AI embedded inside the MCPanel desktop application. \
             You have full access to manage local Minecraft Java and Bedrock servers for the user.\n\n\
             Capabilities:\n\
             - You can list, inspect, and configure servers.\n\
             - You can read and edit server files, plugin configurations (YAML, JSON, properties), server.properties, bukkit.yml, etc.\n\
             - You can search, install, and check plugins/mods.\n\
             - You can read console logs and diagnose crashes.\n\
             - You can create, list, and restore backups.\n\n\
             Guidelines for Speed & Efficiency:\n\
             - Be concise and direct in your responses.\n\
             - Execute tasks with the minimum number of tool calls necessary. Do not call redundant listing tools if you already have the server ID.\n\
             - Minor operations (editing plugin configs, editing server.properties, reading files, searching plugins, listing backups) \
               are executed efficiently without bothering the user for confirmation.\n\
             - Major operations (starting, stopping, or restarting servers; deleting files; deleting backups; restoring backups; sending console commands) \
               will automatically be paused by MCPanel to ask the user for confirmation.\n\
             - When editing plugin configs or server.properties, explain what changes you made cleanly in brief Markdown format.",
        );

        if let Some(sid) = focused_server_id {
            prompt.push_str(&format!(
                "\n\nThe user currently has server '{}' selected as context. Use this server ID directly without calling list_servers.",
                sid
            ));
        }

        prompt
    }

    /// Execute a minor tool synchronously in MCPanel core.
    async fn execute_minor_tool(&self, name: &str, args: &Value) -> CoreResult<Value> {
        match name {
            "list_servers" => {
                let views = self.servers.list().await?;
                let mut list = Vec::new();
                for v in views {
                    list.push(json!({
                        "id": v.server.id.to_string(),
                        "name": v.server.name,
                        "game_version": v.server.software.game_version,
                        "loader": v.server.software.software_id,
                        "port": v.port,
                        "state": format!("{:?}", v.runtime.state),
                    }));
                }
                Ok(json!({ "servers": list }))
            }
            "get_server_info" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let v = self.servers.view(sid).await?;
                Ok(json!({
                    "id": v.server.id.to_string(),
                    "name": v.server.name,
                    "game_version": v.server.software.game_version,
                    "loader": v.server.software.software_id,
                    "port": v.port,
                    "state": format!("{:?}", v.runtime.state),
                    "created_at": v.server.created_at.millis(),
                }))
            }
            "read_server_properties" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let props = self.servers.properties(sid).await?;
                let map: HashMap<String, String> = props
                    .properties
                    .into_iter()
                    .filter_map(|p| p.value.map(|v| (p.key, v)))
                    .collect();
                Ok(json!({ "properties": map }))
            }
            "update_server_properties" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let props_val = args
                    .get("properties")
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| {
                        CoreError::new(ErrorCode::InvalidInput, "Missing properties object")
                    })?;

                let mut changes = Vec::new();
                let mut updated_keys = Vec::new();
                for (k, v) in props_val {
                    let val_str = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => v.to_string(),
                    };
                    changes.push(PropertyChange {
                        key: k.clone(),
                        value: Some(val_str),
                    });
                    updated_keys.push(k.clone());
                }

                self.servers.update_properties(sid, changes, "ai").await?;

                Ok(json!({
                    "success": true,
                    "updated_properties": updated_keys,
                }))
            }
            "list_files" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let rel = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let entries = self.files.list(sid, rel.to_string()).await?;
                let list: Vec<Value> = entries
                    .into_iter()
                    .map(|e| {
                        json!({
                            "name": e.name,
                            "is_dir": matches!(e.kind, crate::files::service::EntryKind::Directory),
                            "size": e.size,
                        })
                    })
                    .collect();
                Ok(json!({ "files": list }))
            }
            "read_file" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CoreError::new(ErrorCode::InvalidInput, "Missing path parameter")
                })?;

                let doc = self.files.read_text(sid, path.to_string()).await?;
                Ok(json!({
                    "path": path,
                    "content": doc.content,
                    "encoding": format!("{:?}", doc.encoding),
                }))
            }
            "write_file" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CoreError::new(ErrorCode::InvalidInput, "Missing path parameter")
                })?;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CoreError::new(ErrorCode::InvalidInput, "Missing content parameter")
                    })?;

                self.files
                    .write_text(
                        sid,
                        path.to_string(),
                        WriteText {
                            content: content.to_string(),
                            encoding: TextEncoding {
                                name: "UTF-8".into(),
                                bom: false,
                            },
                            expected_sha256: None,
                        },
                        "ai",
                    )
                    .await?;

                Ok(json!({
                    "success": true,
                    "path": path,
                    "bytes": content.len(),
                }))
            }
            "list_plugins" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let list = self.content.list(sid).await?;
                let items: Vec<Value> = list
                    .entries
                    .into_iter()
                    .map(|e| {
                        let name = e
                            .record
                            .as_ref()
                            .map(|r| r.name.clone())
                            .or_else(|| e.descriptor.as_ref().and_then(|d| d.name.clone()))
                            .unwrap_or_else(|| e.file_name.clone());
                        let version = e
                            .record
                            .as_ref()
                            .and_then(|r| r.version_number.clone())
                            .or_else(|| e.descriptor.as_ref().and_then(|d| d.version.clone()));
                        let provider = e
                            .record
                            .as_ref()
                            .and_then(|r| r.source.as_ref().map(|s| s.provider.clone()));
                        json!({
                            "name": name,
                            "filename": e.file_name,
                            "provider": provider,
                            "enabled": e.enabled,
                            "version": version,
                        })
                    })
                    .collect();
                Ok(json!({ "plugins": items }))
            }
            "search_plugins" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let sid_opt = match args.get("server_id").and_then(|v| v.as_str()) {
                    Some(s) => ServerId::from_str(s).ok(),
                    None => {
                        let servers = self.servers.list().await?;
                        servers.first().map(|v| v.server.id)
                    }
                };

                let sid = sid_opt.ok_or_else(|| {
                    CoreError::new(
                        ErrorCode::NotFound,
                        "No server available for search context",
                    )
                })?;

                let page = self
                    .content
                    .search(sid, "modrinth", query, SearchSort::Relevance, 0, 8)
                    .await?;

                let results: Vec<Value> = page
                    .hits
                    .into_iter()
                    .take(8)
                    .map(|h| {
                        json!({
                            "id": h.id,
                            "title": h.name,
                            "description": h.description,
                            "provider": h.provider,
                            "downloads": h.downloads,
                        })
                    })
                    .collect();
                Ok(json!({ "results": results }))
            }
            "install_plugin" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let provider = args
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("modrinth");
                let project_id = args
                    .get("project_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let req = InstallRequest {
                    provider: provider.to_string(),
                    project_id: project_id.to_string(),
                    version_id: None,
                    with_dependencies: true,
                };
                let job_id = self.content.install(sid, req, "ai").await?;

                Ok(json!({
                    "success": true,
                    "job_id": job_id.to_string(),
                    "message": format!("Installed {} plugin {}", provider, project_id),
                }))
            }
            "get_console_logs" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let max_lines = args.get("lines").and_then(|v| v.as_i64()).unwrap_or(40) as usize;
                let max_lines = max_lines.clamp(1, 100);

                let log_text = match self
                    .files
                    .read_text(sid, "logs/latest.log".to_string())
                    .await
                {
                    Ok(d) => d.content,
                    Err(_) => "logs/latest.log not available or empty".to_string(),
                };

                // For speed and memory, only examine the last 64KB if log is large
                let log_slice = if log_text.len() > 65536 {
                    let offset = log_text.len() - 65536;
                    match log_text[offset..].find('\n') {
                        Some(pos) => &log_text[offset + pos + 1..],
                        None => &log_text[offset..],
                    }
                } else {
                    &log_text[..]
                };

                let lines: Vec<&str> = log_slice.lines().collect();
                let start = if lines.len() > max_lines {
                    lines.len() - max_lines
                } else {
                    0
                };
                let tail = lines[start..].join("\n");
                Ok(json!({ "log_tail": tail }))
            }
            "list_backups" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let backups = self.backups.list(Some(sid)).await?;
                let list: Vec<Value> = backups
                    .into_iter()
                    .map(|b| {
                        json!({
                            "id": b.backup.id.to_string(),
                            "created_at": b.backup.created_at.millis(),
                            "size_bytes": b.backup.size_bytes,
                            "note": b.backup.note,
                        })
                    })
                    .collect();
                Ok(json!({ "backups": list }))
            }
            "create_backup" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let note = args
                    .get("note")
                    .and_then(|v| v.as_str())
                    .map(|n| n.to_string());

                let req = crate::backup::CreateBackupRequest {
                    server_id: sid,
                    note: note.clone(),
                };
                let job_id = self.backups.create(req, "ai").await?;

                Ok(json!({
                    "success": true,
                    "job_id": job_id.to_string(),
                }))
            }
            other => Err(CoreError::new(
                ErrorCode::InvalidInput,
                format!("Unknown tool: {other}"),
            )),
        }
    }

    /// Execute a major action that has been explicitly approved by the user.
    async fn execute_major_tool(
        &self,
        name: &str,
        args: &Value,
        server_id: Option<ServerId>,
    ) -> CoreResult<Value> {
        match name {
            "start_server" => {
                let sid = server_id
                    .ok_or_else(|| CoreError::new(ErrorCode::InvalidInput, "Missing server_id"))?;
                let server = self.servers.get(sid).await?;
                self.servers.start(sid, "ai").await?;

                Ok(
                    json!({ "success": true, "message": format!("Server '{}' started", server.name) }),
                )
            }
            "stop_server" => {
                let sid = server_id
                    .ok_or_else(|| CoreError::new(ErrorCode::InvalidInput, "Missing server_id"))?;
                let server = self.servers.get(sid).await?;
                self.servers.stop(sid, false, "ai").await?;

                Ok(
                    json!({ "success": true, "message": format!("Server '{}' stopped", server.name) }),
                )
            }
            "restart_server" => {
                let sid = server_id
                    .ok_or_else(|| CoreError::new(ErrorCode::InvalidInput, "Missing server_id"))?;
                let server = self.servers.get(sid).await?;
                self.servers.restart(sid, "ai").await?;

                Ok(
                    json!({ "success": true, "message": format!("Server '{}' restarted", server.name) }),
                )
            }
            "delete_server_file" => {
                let sid = server_id
                    .ok_or_else(|| CoreError::new(ErrorCode::InvalidInput, "Missing server_id"))?;
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CoreError::new(ErrorCode::InvalidInput, "Missing path parameter")
                })?;

                self.files
                    .delete(sid, vec![path.to_string()], true, "ai")
                    .await?;

                Ok(json!({ "success": true, "message": format!("Deleted file '{}'", path) }))
            }
            "delete_backup" => {
                let bid_str = args.get("backup_id").and_then(|v| v.as_str()).unwrap_or("");
                let bid = BackupId::from_str(bid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid backup_id"))?;

                self.backups.delete(bid, "ai").await?;

                Ok(json!({ "success": true, "message": format!("Deleted backup '{}'", bid) }))
            }
            "restore_backup" => {
                let bid_str = args.get("backup_id").and_then(|v| v.as_str()).unwrap_or("");
                let bid = BackupId::from_str(bid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid backup_id"))?;

                let job_id = self.backups.restore(bid, "ai").await?;

                Ok(
                    json!({ "success": true, "job_id": job_id.to_string(), "message": format!("Restoring server from backup '{}'", bid) }),
                )
            }
            "send_console_command" => {
                let sid = server_id
                    .ok_or_else(|| CoreError::new(ErrorCode::InvalidInput, "Missing server_id"))?;
                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CoreError::new(ErrorCode::InvalidInput, "Missing command"))?;

                self.servers.send_command(sid, command).await?;

                self.audit
                    .record(
                        "ai",
                        "ai.command",
                        Some(sid),
                        Some(command.to_string()),
                        AuditResult::Success,
                        json!({ "command": command, "actor": "ai", "ai": true }),
                    )
                    .await;

                Ok(json!({ "success": true, "message": format!("Executed command '{}'", command) }))
            }
            other => Err(CoreError::new(
                ErrorCode::InvalidInput,
                format!("Unsupported major tool: {other}"),
            )),
        }
    }

    /// Convert UI chat messages to LLM messages with speed optimizations.
    fn prepare_llm_messages(messages: &[AiChatMessage]) -> Vec<AiLlmMessage> {
        let mut llm_msgs = Vec::new();
        // Limit context to the most recent 12 messages for faster token processing
        let start = if messages.len() > 12 {
            messages.len() - 12
        } else {
            0
        };
        let slice = &messages[start..];

        for (idx, m) in slice.iter().enumerate() {
            let role = match m.role.as_str() {
                "user" => "user".to_string(),
                "assistant" => "model".to_string(),
                "system" => "system".to_string(),
                other => other.to_string(),
            };
            // Truncate older messages' content if very large to save tokens and inference latency
            let is_latest = idx + 1 == slice.len();
            let content = if !is_latest && m.content.len() > 1500 {
                let mut truncated = m.content[..1500].to_string();
                truncated.push_str("\n...[older output truncated for speed]");
                truncated
            } else {
                m.content.clone()
            };
            llm_msgs.push(AiLlmMessage {
                role,
                content: Some(content),
                function_calls: None,
                function_responses: None,
            });
        }
        llm_msgs
    }

    /// Process a chat turn: interacts with LLM, runs minor tools autonomously,
    /// and pauses for user confirmation if a major action is encountered.
    pub async fn chat(
        &self,
        messages: Vec<AiChatMessage>,
        focused_server_id: Option<String>,
    ) -> CoreResult<AiChatMessage> {
        let config = self.get_config().await?;
        let key = match self
            .resolve_api_key(&config.provider, config.use_shared_key)
            .await?
        {
            Some(k) => k,
            None if Self::is_local_provider(&config.provider) => SecretString::new("local".into()),
            None => {
                return Err(CoreError::new(
                    ErrorCode::InvalidInput,
                    "No AI API key configured. Please enter an API key in Settings or Getting Started.",
                ));
            }
        };

        let system_prompt = self.system_prompt(focused_server_id.as_deref());
        let tools = Self::available_tools();
        let mut llm_messages = Self::prepare_llm_messages(&messages);

        let mut executed_tools = Vec::new();
        let mut iterations = 0;

        loop {
            iterations += 1;
            if iterations > 6 {
                return Ok(AiChatMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: "assistant".into(),
                    content: "I have performed multiple operations and paused to prevent loop exhaustion.".into(),
                    timestamp: jiff::Timestamp::now().as_millisecond(),
                    pending_confirmation: None,
                    tool_executions: Some(executed_tools),
                });
            }

            let request = AiLlmRequest {
                system_instruction: Some(system_prompt.clone()),
                messages: llm_messages.clone(),
                tools: tools.clone(),
            };

            let response = self
                .client
                .complete_turn(
                    &key,
                    &config.provider,
                    &config.model,
                    config.base_url.as_deref(),
                    &request,
                )
                .await?;

            if response.function_calls.is_empty() {
                // Done! Got pure text response from LLM
                return Ok(AiChatMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: "assistant".into(),
                    content: response.content.unwrap_or_default(),
                    timestamp: jiff::Timestamp::now().as_millisecond(),
                    pending_confirmation: None,
                    tool_executions: if executed_tools.is_empty() {
                        None
                    } else {
                        Some(executed_tools)
                    },
                });
            }

            // Check if ANY function call is a MAJOR action
            let mut major_call = None;
            for call in &response.function_calls {
                if Self::is_major_action(&call.name) {
                    major_call = Some(call.clone());
                    break;
                }
            }

            if let Some(call) = major_call {
                // We MUST pause and request confirmation from the user!
                let conf_id = format!("conf_{}", uuid::Uuid::new_v4().simple());
                let (title, description, sid) =
                    self.describe_major_call(&call.name, &call.args).await?;

                {
                    let mut lock = self
                        .pending_confirmations
                        .lock()
                        .map_err(|_| CoreError::new(ErrorCode::Internal, "Lock poisoned"))?;
                    lock.insert(
                        conf_id.clone(),
                        StoredConfirmation {
                            tool: call.name.clone(),
                            args: call.args.clone(),
                            title: title.clone(),
                            description: description.clone(),
                            server_id: sid,
                        },
                    );
                }

                let pending = AiPendingConfirmation {
                    confirmation_id: conf_id,
                    tool: call.name.clone(),
                    title,
                    description,
                    params: call.args,
                };

                return Ok(AiChatMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: "assistant".into(),
                    content: response.content.unwrap_or_else(|| {
                        "This action requires your confirmation before proceeding.".into()
                    }),
                    timestamp: jiff::Timestamp::now().as_millisecond(),
                    pending_confirmation: Some(pending),
                    tool_executions: if executed_tools.is_empty() {
                        None
                    } else {
                        Some(executed_tools)
                    },
                });
            }

            // All function calls in this turn are minor actions. Execute them efficiently!
            let mut tool_responses = Vec::new();
            for call in &response.function_calls {
                let call_name = call.name.clone();
                match self.execute_minor_tool(&call_name, &call.args).await {
                    Ok(res_val) => {
                        executed_tools.push(AiToolExecution {
                            tool: call_name.clone(),
                            description: format!("Executed tool {}", call_name),
                            is_major: false,
                            result: Some(res_val.to_string()),
                            error: None,
                        });
                        tool_responses.push(AiFunctionResponse {
                            id: call.id.clone(),
                            name: call_name,
                            response: res_val,
                        });
                    }
                    Err(e) => {
                        let err_msg = e.message.clone();
                        executed_tools.push(AiToolExecution {
                            tool: call_name.clone(),
                            description: format!("Failed tool {}", call_name),
                            is_major: false,
                            result: None,
                            error: Some(err_msg.clone()),
                        });
                        tool_responses.push(AiFunctionResponse {
                            id: call.id.clone(),
                            name: call_name,
                            response: json!({ "error": err_msg }),
                        });
                    }
                }
            }

            // Append model turn with function calls & function response turn to llm_messages
            llm_messages.push(AiLlmMessage {
                role: "model".into(),
                content: response.content,
                function_calls: Some(response.function_calls),
                function_responses: None,
            });

            llm_messages.push(AiLlmMessage {
                role: "function".into(),
                content: None,
                function_calls: None,
                function_responses: Some(tool_responses),
            });
        }
    }

    /// Resume execution after user approves or declines a major action.
    pub async fn confirm_action(
        &self,
        confirmation_id: &str,
        approved: bool,
        messages: Vec<AiChatMessage>,
        focused_server_id: Option<String>,
    ) -> CoreResult<AiChatMessage> {
        let stored = {
            let mut lock = self
                .pending_confirmations
                .lock()
                .map_err(|_| CoreError::new(ErrorCode::Internal, "Lock poisoned"))?;
            lock.remove(confirmation_id)
        }
        .ok_or_else(|| CoreError::new(ErrorCode::NotFound, "Confirmation expired or not found"))?;

        let config = self.get_config().await?;
        let key = match self
            .resolve_api_key(&config.provider, config.use_shared_key)
            .await?
        {
            Some(k) => k,
            None if Self::is_local_provider(&config.provider) => SecretString::new("local".into()),
            None => {
                return Err(CoreError::new(
                    ErrorCode::InvalidInput,
                    "No AI API key set for this provider",
                ));
            }
        };

        let mut executed_tools = Vec::new();
        let tool_resp_val: Value;

        if approved {
            match self
                .execute_major_tool(&stored.tool, &stored.args, stored.server_id)
                .await
            {
                Ok(val) => {
                    executed_tools.push(AiToolExecution {
                        tool: stored.tool.clone(),
                        description: stored.description.clone(),
                        is_major: true,
                        result: Some(val.to_string()),
                        error: None,
                    });
                    tool_resp_val = val;
                }
                Err(e) => {
                    let err_msg = e.message.clone();
                    executed_tools.push(AiToolExecution {
                        tool: stored.tool.clone(),
                        description: stored.description.clone(),
                        is_major: true,
                        result: None,
                        error: Some(err_msg.clone()),
                    });
                    tool_resp_val = json!({ "error": err_msg });
                }
            }
        } else {
            executed_tools.push(AiToolExecution {
                tool: stored.tool.clone(),
                description: format!("Declined by user: {}", stored.description),
                is_major: true,
                result: None,
                error: Some("Declined by user".into()),
            });
            tool_resp_val =
                json!({ "status": "declined", "message": "User declined this action." });
        }

        // Now continue LLM conversation with the action result
        let mut llm_messages = Self::prepare_llm_messages(&messages);

        // Protocol requirement: ensure the assistant message right before the function response
        // contains the corresponding function call with confirmation_id!
        let tool_call = AiFunctionCall {
            id: confirmation_id.to_string(),
            name: stored.tool.clone(),
            args: stored.args.clone(),
        };

        if let Some(last) = llm_messages.last_mut() {
            if last.role == "model" || last.role == "assistant" {
                last.function_calls = Some(vec![tool_call]);
            } else {
                llm_messages.push(AiLlmMessage {
                    role: "model".into(),
                    content: Some("Proceeding with requested action upon confirmation.".into()),
                    function_calls: Some(vec![tool_call]),
                    function_responses: None,
                });
            }
        } else {
            llm_messages.push(AiLlmMessage {
                role: "model".into(),
                content: Some("Proceeding with requested action upon confirmation.".into()),
                function_calls: Some(vec![tool_call]),
                function_responses: None,
            });
        }

        llm_messages.push(AiLlmMessage {
            role: "function".into(),
            content: None,
            function_calls: None,
            function_responses: Some(vec![AiFunctionResponse {
                id: confirmation_id.to_string(),
                name: stored.tool,
                response: tool_resp_val,
            }]),
        });

        let system_prompt = self.system_prompt(focused_server_id.as_deref());
        let tools = Self::available_tools();

        let request = AiLlmRequest {
            system_instruction: Some(system_prompt),
            messages: llm_messages,
            tools,
        };

        let response = self
            .client
            .complete_turn(
                &key,
                &config.provider,
                &config.model,
                config.base_url.as_deref(),
                &request,
            )
            .await?;

        Ok(AiChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            role: "assistant".into(),
            content: response.content.unwrap_or_else(|| {
                if approved {
                    "Action completed successfully.".into()
                } else {
                    "Action was cancelled as requested.".into()
                }
            }),
            timestamp: jiff::Timestamp::now().as_millisecond(),
            pending_confirmation: None,
            tool_executions: Some(executed_tools),
        })
    }

    async fn describe_major_call(
        &self,
        name: &str,
        args: &Value,
    ) -> CoreResult<(String, String, Option<ServerId>)> {
        match name {
            "start_server" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let s = self.servers.get(sid).await?;
                Ok((
                    "Start Server".into(),
                    format!("Start Minecraft server '{}' ({})", s.name, s.id),
                    Some(sid),
                ))
            }
            "stop_server" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let s = self.servers.get(sid).await?;
                Ok((
                    "Stop Server".into(),
                    format!("Gracefully stop Minecraft server '{}' ({})", s.name, s.id),
                    Some(sid),
                ))
            }
            "restart_server" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let s = self.servers.get(sid).await?;
                Ok((
                    "Restart Server".into(),
                    format!("Restart Minecraft server '{}' ({})", s.name, s.id),
                    Some(sid),
                ))
            }
            "delete_server_file" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("file");
                let s = self.servers.get(sid).await?;
                Ok((
                    "Delete File".into(),
                    format!("Permanently delete '{}' on server '{}'", path, s.name),
                    Some(sid),
                ))
            }
            "delete_backup" => {
                let bid = args
                    .get("backup_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("backup");
                Ok((
                    "Delete Backup".into(),
                    format!("Permanently delete backup '{}'", bid),
                    None,
                ))
            }
            "restore_backup" => {
                let bid = args
                    .get("backup_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("backup");
                Ok((
                    "Restore Backup".into(),
                    format!(
                        "Restore server from backup '{}' (replaces server files)",
                        bid
                    ),
                    None,
                ))
            }
            "send_console_command" => {
                let sid_str = args.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid = ServerId::from_str(sid_str)
                    .map_err(|_| CoreError::new(ErrorCode::NotFound, "Invalid server_id"))?;
                let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let s = self.servers.get(sid).await?;
                Ok((
                    "Execute Console Command".into(),
                    format!("Execute command '{}' on server '{}'", cmd, s.name),
                    Some(sid),
                ))
            }
            other => Ok((other.into(), format!("Execute {}", other), None)),
        }
    }
}
