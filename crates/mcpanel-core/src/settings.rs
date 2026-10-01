//! Application settings (MCPanel's own preferences; stored in the database).

use crate::error::{CoreError, CoreResult};
use crate::events::{DomainEvent, EventBus};
use crate::ports::SettingsRepository;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: ThemePreference,
    /// Whether the one-time "MCPanel is still running in the tray" notice was shown.
    pub tray_notice_shown: bool,
    /// Lines kept in memory per server console.
    pub console_buffer_lines: u32,
    /// Graceful-stop timeout used when quitting MCPanel.
    pub quit_stop_timeout_secs: u32,
    /// Where new backups are written; `None` = the default backups directory. Only set
    /// through [`SettingsService::set_backups_dir`] (from a folder the user picked).
    pub backups_dir: Option<String>,
    /// Query running servers for TPS/MSPT (their replies appear in the server's log).
    pub tick_sampling: bool,
    /// Accent color: a preset name, "system" (the Windows accent) or `#rrggbb`.
    pub accent: String,
    /// The first-time setup was finished or skipped.
    pub onboarding_completed: bool,
}

/// Accent presets offered in Settings (green is MCPanel's own).
pub const ACCENT_PRESETS: &[&str] = &[
    "green", "emerald", "teal", "blue", "violet", "rose", "orange", "amber",
];

/// A preset, "system" or a `#rrggbb` color.
pub fn valid_accent(s: &str) -> bool {
    s == "system"
        || ACCENT_PRESETS.contains(&s)
        || (s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            tray_notice_shown: false,
            console_buffer_lines: 20_000,
            quit_stop_timeout_secs: 90,
            backups_dir: None,
            tick_sampling: true,
            accent: "green".into(),
            onboarding_completed: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettingsPatch {
    pub theme: Option<ThemePreference>,
    pub tray_notice_shown: Option<bool>,
    pub console_buffer_lines: Option<u32>,
    pub quit_stop_timeout_secs: Option<u32>,
    #[serde(default)]
    pub tick_sampling: Option<bool>,
    /// Persisted form only; ignored by [`SettingsService::update`].
    #[serde(default)]
    pub backups_dir: Option<String>,
    #[serde(default)]
    pub accent: Option<String>,
    #[serde(default)]
    pub onboarding_completed: Option<bool>,
}

pub struct SettingsService {
    repo: Arc<dyn SettingsRepository>,
    events: EventBus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::SettingsRepository;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemorySettings(Mutex<HashMap<String, serde_json::Value>>);

    #[async_trait]
    impl SettingsRepository for MemorySettings {
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

    fn service() -> SettingsService {
        SettingsService::new(Arc::new(MemorySettings::default()), EventBus::default())
    }

    #[tokio::test]
    async fn fresh_install_starts_with_setup_incomplete() {
        assert!(!service().get().await.unwrap().onboarding_completed);
    }

    #[tokio::test]
    async fn completed_setup_is_persisted() {
        let settings = service();
        settings
            .update(AppSettingsPatch {
                onboarding_completed: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(settings.get().await.unwrap().onboarding_completed);
    }

    #[tokio::test]
    async fn skipped_setup_uses_the_same_persisted_completion_flag() {
        let settings = service();
        // Skip setup intentionally marks the one-time flow complete so it is not shown again.
        settings
            .update(AppSettingsPatch {
                onboarding_completed: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(settings.get().await.unwrap().onboarding_completed);
    }
}

const KEY: &str = "app";

impl SettingsService {
    pub fn new(repo: Arc<dyn SettingsRepository>, events: EventBus) -> Self {
        Self { repo, events }
    }

    pub async fn get(&self) -> CoreResult<AppSettings> {
        let Some(v) = self.repo.get(KEY).await? else {
            return Ok(AppSettings::default());
        };
        // Unknown/old shapes fall back to defaults field by field.
        let mut s = AppSettings::default();
        if let Ok(patch) = serde_json::from_value::<AppSettingsPatch>(v) {
            apply(&mut s, patch);
        }
        Ok(s)
    }

    pub async fn update(&self, patch: AppSettingsPatch) -> CoreResult<AppSettings> {
        if let Some(n) = patch.console_buffer_lines
            && !(1_000..=200_000).contains(&n)
        {
            return Err(CoreError::invalid(
                "Console buffer must be between 1,000 and 200,000 lines",
            ));
        }
        if let Some(n) = patch.quit_stop_timeout_secs
            && !(10..=600).contains(&n)
        {
            return Err(CoreError::invalid(
                "Quit timeout must be between 10 and 600 seconds",
            ));
        }
        let mut s = self.get().await?;
        apply(
            &mut s,
            AppSettingsPatch {
                backups_dir: None,
                ..patch
            },
        );
        self.save(s).await
    }

    /// Choose the backups directory (`None` restores the default).
    pub async fn set_backups_dir(
        &self,
        dir: Option<std::path::PathBuf>,
    ) -> CoreResult<AppSettings> {
        let mut s = self.get().await?;
        s.backups_dir = dir.map(|d| d.to_string_lossy().to_string());
        self.save(s).await
    }

    async fn save(&self, s: AppSettings) -> CoreResult<AppSettings> {
        let value = serde_json::to_value(&s).map_err(|e| CoreError::internal(e.to_string()))?;
        self.repo.set(KEY, &value).await?;
        self.events
            .publish(DomainEvent::SettingsChanged { key: KEY.into() });
        Ok(s)
    }
}

fn apply(s: &mut AppSettings, p: AppSettingsPatch) {
    if let Some(v) = p.theme {
        s.theme = v;
    }
    if let Some(v) = p.tray_notice_shown {
        s.tray_notice_shown = v;
    }
    if let Some(v) = p.console_buffer_lines {
        s.console_buffer_lines = v;
    }
    if let Some(v) = p.quit_stop_timeout_secs {
        s.quit_stop_timeout_secs = v;
    }
    if let Some(v) = p.tick_sampling {
        s.tick_sampling = v;
    }
    if p.backups_dir.is_some() {
        s.backups_dir = p.backups_dir;
    }
    if let Some(v) = p.accent
        && valid_accent(&v)
    {
        s.accent = v;
    }
    if let Some(v) = p.onboarding_completed {
        s.onboarding_completed = v;
    }
}
