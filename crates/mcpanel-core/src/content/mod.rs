//! Content (plugins; mods from v0.3): providers, installed content and the install
//! pipeline (spec §7).
//!
//! resolve → plan (dependencies, loader/Minecraft compatibility) → user confirms →
//! download to staging (HTTPS, trusted hosts) → verify hash → validate the jar's
//! descriptor → atomic move (the replaced file goes to the trash) → record; rollback on
//! failure. On a running server changes are queued (the JVM locks loaded jars on
//! Windows) and applied when it stops or before it next starts.

pub mod descriptor;
pub mod service;

pub use service::{
    ContentEntry, ContentService, ContentServiceDeps, InstallPlan, InstallRequest, PendingChange,
    PendingKind, PlannedInstall, UpdateInfo,
};

use crate::error::CoreResult;
use crate::ids::ServerId;
use crate::ports::ExpectedHash;
use crate::software::ContentEcosystem;
use crate::time::Timestamp;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Plugin,
    Mod,
}

impl ContentKind {
    /// The server folder this kind of content lives in.
    pub fn folder(self) -> &'static str {
        match self {
            Self::Plugin => "plugins",
            Self::Mod => "mods",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Mod => "mod",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "plugin" => Some(Self::Plugin),
            "mod" => Some(Self::Mod),
            _ => None,
        }
    }

    /// Which kind a server's content ecosystems support (plugins win for hybrids).
    pub fn for_ecosystems(ecosystems: &[ContentEcosystem]) -> Option<Self> {
        use ContentEcosystem as E;
        if ecosystems
            .iter()
            .any(|e| matches!(e, E::BukkitPlugins | E::PaperPlugins))
        {
            Some(Self::Plugin)
        } else if ecosystems.iter().any(|e| {
            matches!(
                e,
                E::FabricMods | E::ForgeMods | E::NeoForgeMods | E::QuiltMods
            )
        }) {
            Some(Self::Mod)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    Release,
    Beta,
    Alpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

/// What content must be compatible with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentTarget {
    pub kind: ContentKind,
    pub software_id: String,
    pub game_version: String,
    pub ecosystems: Vec<ContentEcosystem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    #[default]
    Relevance,
    Downloads,
    Updated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub target: ContentTarget,
    pub sort: SearchSort,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub provider: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub author: Option<String>,
    pub downloads: u64,
    pub icon_url: Option<String>,
    pub page_url: String,
    pub updated: Option<Timestamp>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchPage {
    pub hits: Vec<ProjectSummary>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFile {
    pub url: String,
    pub file_name: String,
    pub size: Option<u64>,
    /// `None` = the provider publishes no hash; the download is "unverified".
    pub hash: Option<ExpectedHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub kind: DependencyKind,
    /// Only a web page is known (cannot be installed automatically).
    pub external_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentVersion {
    pub provider: String,
    pub project_id: String,
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub channel: ReleaseChannel,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub published: Option<Timestamp>,
    /// `None` when the file is only available from an external site.
    pub file: Option<ContentFile>,
    pub external_url: Option<String>,
    pub dependencies: Vec<ContentDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentProviderInfo {
    pub id: String,
    pub display_name: String,
    pub website: String,
    /// HTTPS hosts files may be downloaded from.
    pub download_hosts: Vec<String>,
    pub kinds: Vec<ContentKind>,
    /// Whether installed files can be identified by their SHA-512.
    pub hash_lookup: bool,
}

/// A source of plugins/mods (Modrinth, Hangar, …). Implementations map the target's
/// software and ecosystems to their own filters.
#[async_trait]
pub trait ContentProvider: Send + Sync {
    fn info(&self) -> &ContentProviderInfo;
    /// Whether this provider has content for the target at all.
    fn supports(&self, target: &ContentTarget) -> bool;
    async fn search(&self, query: &SearchQuery) -> CoreResult<SearchPage>;
    async fn project(&self, id: &str) -> CoreResult<ProjectSummary>;
    /// Versions compatible with the target, newest first.
    async fn versions(
        &self,
        project_id: &str,
        target: &ContentTarget,
    ) -> CoreResult<Vec<ContentVersion>>;
    async fn version(&self, project_id: &str, version_id: &str) -> CoreResult<ContentVersion>;
    /// Identify files by SHA-512 (hex). Unknown hashes are absent from the result.
    async fn identify(&self, sha512: &[String]) -> CoreResult<HashMap<String, ContentVersion>>;
}

/// Where a managed file came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentSource {
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
}

/// MCPanel's record of a content file it installed or identified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledContent {
    pub server_id: ServerId,
    pub kind: ContentKind,
    pub file_name: String,
    pub name: String,
    pub version_number: Option<String>,
    pub source: Option<ContentSource>,
    pub sha512: Option<String>,
    pub installed_at: Timestamp,
    pub updated_at: Timestamp,
}
