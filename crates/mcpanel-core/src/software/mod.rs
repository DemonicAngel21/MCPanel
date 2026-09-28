//! Server software provider abstraction (ADR-0003).
//!
//! A provider is a bundle of small capability traits. Providers return *data* (plans,
//! launch arguments); the core executes them.

pub mod catalog;
pub mod executor;
pub mod jar;

use crate::error::{CoreError, CoreResult};
use crate::model::{InstalledSoftware, LaunchConfig};
use crate::ports::ExpectedHash;
use crate::time::Timestamp;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

pub use catalog::VersionCatalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentEcosystem {
    BukkitPlugins,
    PaperPlugins,
    FabricMods,
    ForgeMods,
    NeoForgeMods,
    QuiltMods,
    Datapacks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TpsSource {
    /// Paper-family `tps` / `mspt` commands.
    PaperCommands,
    /// Vanilla `/tick query` (1.20.3+).
    VanillaTickQuery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftwareCaps {
    pub content: Vec<ContentEcosystem>,
    pub is_proxy: bool,
    pub log_dialect: String,
    pub stop_command: String,
    pub eula_required: bool,
    /// Needs a local build step (e.g. Spigot BuildTools) — not supported in v1.
    pub requires_build_step: bool,
    pub tps_source: Option<TpsSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftwareDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub caps: SoftwareCaps,
    /// HTTPS hosts downloads for this software may come from.
    pub download_hosts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameVersionKind {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameVersion {
    pub id: String,
    pub kind: GameVersionKind,
    pub release_time: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    Stable,
    Beta,
    Alpha,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaRequirement {
    pub min_major: u32,
    pub recommended_major: Option<u32>,
    /// Provider-recommended JVM flags (offered as a preset, never applied silently).
    pub recommended_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftwareBuild {
    pub id: String,
    pub channel: ReleaseChannel,
    pub published_at: Option<Timestamp>,
}

#[derive(Debug, Clone)]
pub struct InstallRequest {
    pub game_version: String,
    /// `None` = latest stable build.
    pub build: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstallStep {
    Download {
        url: String,
        /// Destination relative to the server root.
        dest: String,
        expected_hash: Option<ExpectedHash>,
        size: Option<u64>,
        description: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    pub software_id: String,
    pub game_version: String,
    pub build: Option<String>,
    pub build_channel: Option<ReleaseChannel>,
    pub steps: Vec<InstallStep>,
    /// Server jar relative to the server root.
    pub jar: String,
    pub java: JavaRequirement,
    pub notes: Vec<String>,
}

/// Arguments contributed by the software (the core adds java, memory and user args).
#[derive(Debug, Clone, Default)]
pub struct LaunchArgs {
    pub jvm_args: Vec<String>,
    pub jar: String,
    pub server_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedSoftware {
    pub software_id: String,
    pub game_version: Option<String>,
    pub build: Option<String>,
    pub jar: String,
    /// 0–100; the highest-confidence detection wins.
    pub confidence: u8,
}

#[async_trait]
pub trait SoftwareCatalog: Send + Sync {
    async fn game_versions(&self) -> CoreResult<Vec<GameVersion>>;
    async fn builds(&self, game_version: &str) -> CoreResult<Vec<SoftwareBuild>>;
}

#[async_trait]
pub trait SoftwareInstaller: Send + Sync {
    async fn plan_install(&self, req: &InstallRequest) -> CoreResult<InstallPlan>;
}

pub trait LaunchResolver: Send + Sync {
    fn launch_args(
        &self,
        installed: &InstalledSoftware,
        launch: &LaunchConfig,
    ) -> CoreResult<LaunchArgs>;
}

pub trait SoftwareDetector: Send + Sync {
    fn detect(&self, dir: &Path) -> Option<DetectedSoftware>;
}

#[derive(Clone)]
pub struct SoftwareProvider {
    pub descriptor: SoftwareDescriptor,
    pub catalog: Arc<dyn SoftwareCatalog>,
    pub installer: Arc<dyn SoftwareInstaller>,
    pub launcher: Arc<dyn LaunchResolver>,
    pub detector: Option<Arc<dyn SoftwareDetector>>,
}

/// All registered providers. Adding a provider = implementing the traits + `register`.
#[derive(Default, Clone)]
pub struct ProviderRegistry {
    software: Vec<SoftwareProvider>,
    /// Catalog used as the authoritative Minecraft version list (Mojang).
    reference_catalog: Option<Arc<dyn SoftwareCatalog>>,
    content: Vec<Arc<dyn crate::content::ContentProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_software(&mut self, provider: SoftwareProvider) {
        self.software
            .retain(|p| p.descriptor.id != provider.descriptor.id);
        self.software.push(provider);
    }

    pub fn register_content(&mut self, provider: Arc<dyn crate::content::ContentProvider>) {
        let id = provider.info().id.clone();
        self.content.retain(|p| p.info().id != id);
        self.content.push(provider);
    }

    pub fn content_providers(&self) -> &[Arc<dyn crate::content::ContentProvider>] {
        &self.content
    }

    pub fn get_content(&self, id: &str) -> CoreResult<Arc<dyn crate::content::ContentProvider>> {
        self.content
            .iter()
            .find(|p| p.info().id == id)
            .cloned()
            .ok_or_else(|| CoreError::not_found(format!("Unknown content provider '{id}'")))
    }

    pub fn set_reference_catalog(&mut self, catalog: Arc<dyn SoftwareCatalog>) {
        self.reference_catalog = Some(catalog);
    }

    pub fn reference_catalog(&self) -> Option<Arc<dyn SoftwareCatalog>> {
        self.reference_catalog.clone()
    }

    pub fn software(&self) -> &[SoftwareProvider] {
        &self.software
    }

    pub fn get_software(&self, id: &str) -> CoreResult<&SoftwareProvider> {
        self.software
            .iter()
            .find(|p| p.descriptor.id == id)
            .ok_or_else(|| CoreError::not_found(format!("Unknown server software '{id}'")))
    }

    /// Run all detectors and return the most confident result.
    pub fn detect(&self, dir: &Path) -> Option<DetectedSoftware> {
        self.software
            .iter()
            .filter_map(|p| p.detector.as_ref().and_then(|d| d.detect(dir)))
            .max_by_key(|d| d.confidence)
    }
}
