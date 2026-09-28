//! Built-in server templates (`data/templates.json`): presets for the create wizard and
//! the policies/plugins applied after a server is created from one.
//!
//! A template is resolved against the chosen software and Minecraft version: property
//! entries outside their `since`/`until` range are skipped, every value is validated
//! against the property schema, and plugins are only offered for plugin-capable
//! software. Templates never carry JVM flags.

use crate::audit::AuditLog;
use crate::backup::{BackupPolicy, BackupService};
use crate::config::schema;
use crate::content::{ContentKind, ContentService, InstallRequest};
use crate::crash::{CrashService, RestartPolicy};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::ids::{JobId, ServerId};
use crate::model::AuditResult;
use crate::server::ServerManager;
use crate::software::VersionCatalog;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRange {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateProperty {
    pub key: String,
    pub value: String,
    pub since: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSuggestion {
    pub interval_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplatePlugin {
    pub provider: String,
    pub project: String,
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    /// Preferred software, in order.
    pub software: Vec<String>,
    pub memory_mb: Option<MemoryRange>,
    pub properties: Vec<TemplateProperty>,
    pub backups: Option<BackupSuggestion>,
    pub auto_restart: bool,
    pub plugins: Vec<TemplatePlugin>,
}

#[derive(Deserialize)]
struct TemplateFile {
    templates: Vec<Template>,
}

static TEMPLATES: LazyLock<Vec<Template>> = LazyLock::new(|| {
    let raw = include_str!("../../../data/templates.json");
    match serde_json::from_str::<TemplateFile>(raw) {
        Ok(f) => f.templates,
        Err(e) => {
            tracing::error!("invalid templates.json: {e}");
            Vec::new()
        }
    }
});

pub fn builtin() -> &'static [Template] {
    &TEMPLATES
}

/// A template made concrete for one software + Minecraft version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTemplate {
    pub template_id: String,
    pub properties: Vec<(String, String)>,
    pub plugins: Vec<TemplatePlugin>,
    pub memory_mb: Option<MemoryRange>,
    /// Entries that were skipped and why.
    pub notes: Vec<String>,
}

async fn in_range(p: &TemplateProperty, version: &str, catalog: &VersionCatalog) -> Option<bool> {
    let since = match &p.since {
        Some(s) => catalog.is_at_least(version, s).await?,
        None => true,
    };
    let until = match &p.until {
        Some(u) => !catalog.is_at_least(version, u).await?,
        None => true,
    };
    Some(since && until)
}

/// Resolve a template for `software_id` + `game_version` (pure given the catalog).
pub async fn resolve(
    t: &Template,
    game_version: &str,
    plugins_supported: bool,
    catalog: &VersionCatalog,
) -> ResolvedTemplate {
    let mut properties: Vec<(String, String)> = Vec::new();
    let mut notes = Vec::new();
    for p in &t.properties {
        match in_range(p, game_version, catalog).await {
            Some(true) => {}
            Some(false) => continue,
            None => {
                notes.push(format!(
                    "{}: skipped (Minecraft {game_version} is not in the version list)",
                    p.key
                ));
                continue;
            }
        }
        if properties.iter().any(|(k, _)| k == &p.key) {
            continue; // first applicable entry wins
        }
        let s = schema::schema_for(&p.key, game_version, catalog)
            .await
            .map(|(s, _)| s);
        match schema::validate_value(s.as_ref(), &p.key, &p.value) {
            Ok(()) => properties.push((p.key.clone(), p.value.clone())),
            Err(e) => notes.push(format!("{}: skipped ({})", p.key, e.message)),
        }
    }
    let plugins = if plugins_supported {
        t.plugins.clone()
    } else {
        Vec::new()
    };
    if !plugins_supported && !t.plugins.is_empty() {
        notes.push("Suggested plugins are skipped: this software does not support plugins".into());
    }
    ResolvedTemplate {
        template_id: t.id.clone(),
        properties,
        plugins,
        memory_mb: t.memory_mb.clone(),
        notes,
    }
}

pub struct TemplateService {
    servers: Arc<ServerManager>,
    backups: Arc<BackupService>,
    crashes: Arc<CrashService>,
    content: Arc<ContentService>,
    audit: Arc<AuditLog>,
    versions: Arc<VersionCatalog>,
}

impl TemplateService {
    pub fn new(
        servers: Arc<ServerManager>,
        backups: Arc<BackupService>,
        crashes: Arc<CrashService>,
        content: Arc<ContentService>,
        audit: Arc<AuditLog>,
        versions: Arc<VersionCatalog>,
    ) -> Arc<Self> {
        Arc::new(Self {
            servers,
            backups,
            crashes,
            content,
            audit,
            versions,
        })
    }

    pub fn list(&self) -> &'static [Template] {
        builtin()
    }

    fn get(&self, id: &str) -> CoreResult<&'static Template> {
        builtin()
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| CoreError::not_found(format!("Unknown template '{id}'")))
    }

    fn plugins_supported(&self, software_id: &str) -> bool {
        self.servers
            .registry()
            .get_software(software_id)
            .is_ok_and(|p| {
                ContentKind::for_ecosystems(&p.descriptor.caps.content) == Some(ContentKind::Plugin)
            })
    }

    pub async fn resolve(
        &self,
        id: &str,
        software_id: &str,
        game_version: &str,
    ) -> CoreResult<ResolvedTemplate> {
        let t = self.get(id)?;
        if !t.software.iter().any(|s| s == software_id) {
            return Err(CoreError::new(
                ErrorCode::Unsupported,
                format!(
                    "The {} template is not meant for this server software",
                    t.name
                ),
            ));
        }
        Ok(resolve(
            t,
            game_version,
            self.plugins_supported(software_id),
            &self.versions,
        )
        .await)
    }

    /// Apply a template's policies and install the chosen suggested plugins on a newly
    /// created server. Returns the plugin install jobs.
    pub async fn apply(
        &self,
        server_id: ServerId,
        id: &str,
        plugins: &[String],
        actor: &str,
    ) -> CoreResult<Vec<JobId>> {
        let t = self.get(id)?;
        let server = self.servers.get(server_id).await?;
        if let Some(b) = &t.backups {
            self.backups
                .set_policy(
                    BackupPolicy {
                        enabled: true,
                        interval_minutes: b.interval_minutes,
                        ..BackupPolicy::default_for(server_id)
                    },
                    actor,
                )
                .await?;
        }
        self.crashes
            .set_policy(
                RestartPolicy {
                    enabled: t.auto_restart,
                    ..RestartPolicy::default_for(server_id)
                },
                actor,
            )
            .await?;
        let mut jobs = Vec::new();
        if self.plugins_supported(&server.software.software_id) {
            for p in t.plugins.iter().filter(|p| plugins.contains(&p.project)) {
                let job = self
                    .content
                    .install(
                        server_id,
                        InstallRequest {
                            provider: p.provider.clone(),
                            project_id: p.project.clone(),
                            version_id: None,
                            with_dependencies: true,
                        },
                        actor,
                    )
                    .await?;
                jobs.push(job);
            }
        }
        self.audit
            .record(
                actor,
                "server.template_applied",
                Some(server_id),
                Some(t.name.clone()),
                AuditResult::Success,
                serde_json::json!({ "template": t.id, "plugins": jobs.len() }),
            )
            .await;
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::software::{GameVersion, GameVersionKind, SoftwareBuild, SoftwareCatalog};
    use crate::time::Timestamp;
    use async_trait::async_trait;

    struct Versions;

    #[async_trait]
    impl SoftwareCatalog for Versions {
        async fn game_versions(&self) -> CoreResult<Vec<GameVersion>> {
            // Newest first, like the Mojang manifest.
            let ids = [
                "26.3", "1.21.4", "1.20.1", "1.19", "1.18", "1.16.5", "1.14", "1.12.2",
            ];
            Ok(ids
                .iter()
                .enumerate()
                .map(|(i, id)| GameVersion {
                    id: id.to_string(),
                    kind: GameVersionKind::Release,
                    release_time: Some(Timestamp(1_000_000 - i as i64)),
                })
                .collect())
        }
        async fn builds(&self, _: &str) -> CoreResult<Vec<SoftwareBuild>> {
            Ok(vec![])
        }
    }

    fn catalog() -> VersionCatalog {
        VersionCatalog::new(Some(Arc::new(Versions)))
    }

    #[tokio::test]
    async fn every_builtin_template_resolves_cleanly_for_the_test_matrix() {
        let c = catalog();
        assert!(builtin().len() >= 5);
        for t in builtin() {
            for v in ["1.12.2", "1.16.5", "1.20.1", "1.21.4", "26.3"] {
                let r = resolve(t, v, true, &c).await;
                assert!(r.notes.is_empty(), "{} on {v}: {:?}", t.id, r.notes);
                let keys: Vec<&str> = r.properties.iter().map(|(k, _)| k.as_str()).collect();
                let mut dedup = keys.clone();
                dedup.dedup();
                assert_eq!(keys.len(), dedup.len(), "{} on {v}: duplicate keys", t.id);
            }
        }
    }

    #[tokio::test]
    async fn version_ranges_pick_the_right_values() {
        let c = catalog();
        let creative = builtin().iter().find(|t| t.id == "creative").unwrap();
        let get = |r: &ResolvedTemplate, k: &str| {
            r.properties
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        let new = resolve(creative, "1.21.4", true, &c).await;
        assert_eq!(get(&new, "level-type").as_deref(), Some("minecraft:flat"));
        assert_eq!(get(&new, "gamemode").as_deref(), Some("creative"));
        let old = resolve(creative, "1.12.2", true, &c).await;
        assert_eq!(get(&old, "level-type").as_deref(), Some("flat"));
        assert_eq!(get(&old, "gamemode").as_deref(), Some("1"));
        let tuned = builtin().iter().find(|t| t.id == "performance").unwrap();
        assert!(
            get(
                &resolve(tuned, "1.16.5", true, &c).await,
                "simulation-distance"
            )
            .is_none()
        );
        assert_eq!(
            get(
                &resolve(tuned, "1.20.1", true, &c).await,
                "simulation-distance"
            )
            .as_deref(),
            Some("6")
        );
    }

    #[tokio::test]
    async fn plugins_are_only_offered_for_plugin_software() {
        let c = catalog();
        let tuned = builtin().iter().find(|t| t.id == "performance").unwrap();
        assert_eq!(resolve(tuned, "1.21.4", true, &c).await.plugins.len(), 1);
        let r = resolve(tuned, "1.21.4", false, &c).await;
        assert!(r.plugins.is_empty() && !r.notes.is_empty());
    }
}
