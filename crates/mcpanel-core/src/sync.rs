//! User preferences cloud synchronization with Firebase/Firestore.
//!
//! Synchronizes non-sensitive user settings (theme, accent color, console buffer, timeouts)
//! across authenticated devices. Local paths (backups_dir), secrets, passwords,
//! server files and tokens are strictly excluded.
//! See `docs/development.md`.

use crate::error::CoreResult;
use crate::ports::SettingsRepository;
use crate::settings::{AppSettings, AppSettingsPatch, ThemePreference, valid_accent};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const SYNC_SCHEMA_VERSION: u32 = 1;
pub const SYNC_TIMESTAMP_KEY: &str = "settings.synced_updated_at";

/// Approved non-sensitive user preferences for cloud synchronization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncedSettings {
    pub schema_version: u32,
    /// Unix timestamp in milliseconds.
    pub updated_at: i64,
    pub theme: ThemePreference,
    pub accent: String,
    pub console_buffer_lines: u32,
    pub quit_stop_timeout_secs: u32,
    pub tick_sampling: bool,
}

impl SyncedSettings {
    /// Extract safely from local [`AppSettings`]. Local paths (`backups_dir`),
    /// setup flags (`onboarding_completed`), notices, and secrets are strictly excluded.
    pub fn from_app_settings(s: &AppSettings, updated_at: i64) -> Self {
        Self {
            schema_version: SYNC_SCHEMA_VERSION,
            updated_at,
            theme: s.theme,
            accent: s.accent.clone(),
            console_buffer_lines: s.console_buffer_lines,
            quit_stop_timeout_secs: s.quit_stop_timeout_secs,
            tick_sampling: s.tick_sampling,
        }
    }

    /// Convert into an [`AppSettingsPatch`] for applying to local storage.
    /// Strictly leaves local-only settings untouched.
    pub fn to_patch(&self) -> AppSettingsPatch {
        AppSettingsPatch {
            theme: Some(self.theme),
            accent: if valid_accent(&self.accent) {
                Some(self.accent.clone())
            } else {
                None
            },
            console_buffer_lines: Some(self.console_buffer_lines),
            quit_stop_timeout_secs: Some(self.quit_stop_timeout_secs),
            tick_sampling: Some(self.tick_sampling),
            tray_notice_shown: None,
            backups_dir: None,
            onboarding_completed: None,
        }
    }
}

/// Remote cloud backend interface for settings synchronization (e.g. Firebase Firestore).
#[async_trait::async_trait]
pub trait SettingsSyncBackend: Send + Sync {
    async fn pull_settings(
        &self,
        id_token: &SecretString,
        uid: &str,
    ) -> CoreResult<Option<SyncedSettings>>;

    async fn push_settings(
        &self,
        id_token: &SecretString,
        uid: &str,
        settings: &SyncedSettings,
    ) -> CoreResult<()>;
}

/// Orchestrates settings cloud synchronization with conflict resolution and offline resilience.
pub struct SettingsSyncService {
    repo: Arc<dyn SettingsRepository>,
}

pub enum SyncOutcome {
    /// Cloud had newer settings; applied patch locally.
    AppliedCloud(AppSettingsPatch),
    /// Local settings were newer or cloud was empty; uploaded local settings.
    UploadedLocal,
    /// Settings were already in sync.
    InSync,
    /// Offline or unconfigured; no-op.
    Skipped,
}

impl SettingsSyncService {
    pub fn new(repo: Arc<dyn SettingsRepository>) -> Self {
        Self { repo }
    }

    pub async fn local_updated_at(&self) -> CoreResult<i64> {
        Ok(self
            .repo
            .get(SYNC_TIMESTAMP_KEY)
            .await?
            .and_then(|v| v.as_i64())
            .unwrap_or(0))
    }

    pub async fn set_local_updated_at(&self, ts: i64) -> CoreResult<()> {
        self.repo
            .set(SYNC_TIMESTAMP_KEY, &serde_json::Value::from(ts))
            .await
    }

    /// Synchronize settings upon signing into an authenticated account.
    /// Implements last-write-wins conflict resolution and initial cloud baseline migration.
    pub async fn sync_on_login(
        &self,
        backend: &dyn SettingsSyncBackend,
        id_token: &SecretString,
        uid: &str,
        local: &AppSettings,
    ) -> CoreResult<SyncOutcome> {
        let cloud_res = backend.pull_settings(id_token, uid).await;
        let cloud_opt = match cloud_res {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(target: "mcpanel::sync", "Failed to pull cloud settings (offline/unsupported): {}", e.message);
                return Ok(SyncOutcome::Skipped);
            }
        };

        let local_ts = self.local_updated_at().await?;

        match cloud_opt {
            Some(cloud) => {
                if cloud.schema_version != SYNC_SCHEMA_VERSION {
                    tracing::info!(target: "mcpanel::sync", "Skipping cloud settings with unsupported schema version {}", cloud.schema_version);
                    return Ok(SyncOutcome::Skipped);
                }

                if cloud.updated_at > local_ts {
                    // Cloud has newer settings: adopt them locally.
                    self.set_local_updated_at(cloud.updated_at).await?;
                    Ok(SyncOutcome::AppliedCloud(cloud.to_patch()))
                } else if local_ts > cloud.updated_at {
                    // Local settings were modified more recently: push to cloud.
                    let payload = SyncedSettings::from_app_settings(local, local_ts);
                    if let Err(e) = backend.push_settings(id_token, uid, &payload).await {
                        tracing::warn!(target: "mcpanel::sync", "Failed to push local settings: {}", e.message);
                    }
                    Ok(SyncOutcome::UploadedLocal)
                } else {
                    Ok(SyncOutcome::InSync)
                }
            }
            None => {
                // First login / empty cloud: upload current local settings as cloud baseline.
                let now = jiff::Timestamp::now().as_millisecond();
                let payload = SyncedSettings::from_app_settings(local, now);
                self.set_local_updated_at(now).await?;
                if let Err(e) = backend.push_settings(id_token, uid, &payload).await {
                    tracing::warn!(target: "mcpanel::sync", "Failed to upload initial cloud baseline: {}", e.message);
                }
                Ok(SyncOutcome::UploadedLocal)
            }
        }
    }

    /// Best-effort push when local settings are updated.
    pub async fn push_local_change(
        &self,
        backend: &dyn SettingsSyncBackend,
        id_token: &SecretString,
        uid: &str,
        local: &AppSettings,
    ) -> CoreResult<()> {
        let now = jiff::Timestamp::now().as_millisecond();
        self.set_local_updated_at(now).await?;
        let payload = SyncedSettings::from_app_settings(local, now);
        if let Err(e) = backend.push_settings(id_token, uid, &payload).await {
            tracing::warn!(target: "mcpanel::sync", "Settings change push failed (offline): {}", e.message);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryRepo(Mutex<HashMap<String, serde_json::Value>>);

    #[async_trait::async_trait]
    impl SettingsRepository for MemoryRepo {
        async fn get(&self, key: &str) -> CoreResult<Option<serde_json::Value>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        async fn set(&self, key: &str, value: &serde_json::Value) -> CoreResult<()> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), value.clone());
            Ok(())
        }
        async fn all(&self) -> CoreResult<Vec<(String, serde_json::Value)>> {
            Ok(self.0.lock().unwrap().clone().into_iter().collect())
        }
    }

    struct MockBackend {
        cloud: Mutex<Option<SyncedSettings>>,
    }

    #[async_trait::async_trait]
    impl SettingsSyncBackend for MockBackend {
        async fn pull_settings(
            &self,
            _id_token: &SecretString,
            _uid: &str,
        ) -> CoreResult<Option<SyncedSettings>> {
            Ok(self.cloud.lock().unwrap().clone())
        }
        async fn push_settings(
            &self,
            _id_token: &SecretString,
            _uid: &str,
            settings: &SyncedSettings,
        ) -> CoreResult<()> {
            *self.cloud.lock().unwrap() = Some(settings.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn sync_safety_excludes_local_only_fields() {
        let local = AppSettings {
            backups_dir: Some("C:\\Sensitive\\Backups".into()),
            onboarding_completed: true,
            tray_notice_shown: true,
            accent: "rose".into(),
            theme: ThemePreference::Dark,
            ..Default::default()
        };

        let synced = SyncedSettings::from_app_settings(&local, 1000);
        assert_eq!(synced.accent, "rose");
        assert_eq!(synced.theme, ThemePreference::Dark);

        let patch = synced.to_patch();
        assert_eq!(patch.accent.as_deref(), Some("rose"));
        assert_eq!(patch.theme, Some(ThemePreference::Dark));
        // Local-only paths and flags MUST NOT be in the patch:
        assert!(patch.backups_dir.is_none());
        assert!(patch.onboarding_completed.is_none());
        assert!(patch.tray_notice_shown.is_none());
    }

    #[tokio::test]
    async fn first_login_migrates_local_settings_to_cloud_baseline() {
        let repo = Arc::new(MemoryRepo::default());
        let service = SettingsSyncService::new(repo);
        let backend = MockBackend {
            cloud: Mutex::new(None),
        };

        let local = AppSettings {
            accent: "blue".into(),
            theme: ThemePreference::Light,
            ..Default::default()
        };

        let outcome = service
            .sync_on_login(
                &backend,
                &SecretString::from("dummy".to_string()),
                "uid-1",
                &local,
            )
            .await
            .unwrap();

        assert!(matches!(outcome, SyncOutcome::UploadedLocal));
        let cloud = backend.cloud.lock().unwrap().clone().unwrap();
        assert_eq!(cloud.accent, "blue");
        assert_eq!(cloud.theme, ThemePreference::Light);
    }

    #[tokio::test]
    async fn newer_cloud_settings_override_local_settings() {
        let repo = Arc::new(MemoryRepo::default());
        let service = SettingsSyncService::new(repo);
        service.set_local_updated_at(100).await.unwrap();

        let cloud_settings = SyncedSettings {
            schema_version: 1,
            updated_at: 200,
            theme: ThemePreference::Dark,
            accent: "emerald".into(),
            console_buffer_lines: 5000,
            quit_stop_timeout_secs: 60,
            tick_sampling: false,
        };
        let backend = MockBackend {
            cloud: Mutex::new(Some(cloud_settings)),
        };

        let local = AppSettings::default();
        let outcome = service
            .sync_on_login(
                &backend,
                &SecretString::from("dummy".to_string()),
                "uid-1",
                &local,
            )
            .await
            .unwrap();

        if let SyncOutcome::AppliedCloud(patch) = outcome {
            assert_eq!(patch.accent.as_deref(), Some("emerald"));
            assert_eq!(patch.theme, Some(ThemePreference::Dark));
            assert_eq!(patch.console_buffer_lines, Some(5000));
        } else {
            panic!("Expected AppliedCloud outcome");
        }
    }
}
