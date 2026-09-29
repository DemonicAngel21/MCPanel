//! GeyserMC's official download API (<https://geysermc.org/wiki/api/downloads/>) as a
//! content provider for Geyser and Floodgate on Paper/Spigot servers.
//!
//! Verified 2026-09-29 against the live API (`https://download.geysermc.org/v2`):
//! - `GET /projects/{project}` → `{project_id, project_name, versions[]}` (oldest first).
//! - `GET /projects/{project}/versions/{version}/builds` → `{builds[]}` (oldest first);
//!   a build has `build`, `time`, `channel` ("default"), `changes[]` and
//!   `downloads{platform: {name, sha256}}` (Geyser: spigot, fabric, neoforge, velocity,
//!   bungeecord, standalone, viaproxy; Floodgate: spigot, bungee, velocity).
//! - `GET /projects/{project}/versions/{version}/builds/{build}` → one build; 404 if
//!   unknown.
//! - `GET …/builds/{build}/downloads/{platform}` serves the jar.
//!
//! Geyser supports whatever Java version its latest build targets (2.11.3: 26.1.1–26.2)
//! and runs on Spigot/Paper 1.20.5+; older servers need ViaVersion. Builds are therefore
//! offered independent of the server's Minecraft version. The mod builds (Fabric/NeoForge)
//! only support the latest Minecraft version, so modded servers get Geyser from Modrinth,
//! where each build is tagged with its Minecraft version.

use crate::http::{HttpClient, parse_rfc3339};
use async_trait::async_trait;
use mcpanel_core::content::{
    ContentFile, ContentKind, ContentProvider, ContentProviderInfo, ContentTarget, ContentVersion,
    ProjectSummary, ReleaseChannel, SearchPage, SearchQuery,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::{ExpectedHash, HashAlgorithm};
use mcpanel_core::software::ContentEcosystem;
use mcpanel_core::time::Timestamp;
use serde::Deserialize;
use std::collections::HashMap;

const API: &str = "https://download.geysermc.org/v2";
const PLATFORM: &str = "spigot";
/// Builds offered per project (newest first).
const MAX_BUILDS: usize = 10;

struct Project {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    page: &'static str,
}

const PROJECTS: &[Project] = &[
    Project {
        id: "geyser",
        name: "Geyser",
        description: "Lets Minecraft: Bedrock Edition players join your Java server.",
        page: "https://geysermc.org/download/?project=geyser",
    },
    Project {
        id: "floodgate",
        name: "Floodgate",
        description: "Lets Bedrock players join through Geyser without a Java account.",
        page: "https://geysermc.org/download/?project=floodgate",
    },
];

fn project(id: &str) -> CoreResult<&'static Project> {
    PROJECTS.iter().find(|p| p.id == id).ok_or_else(|| {
        CoreError::new(
            ErrorCode::NotFound,
            format!("'{id}' is not a GeyserMC project"),
        )
    })
}

fn summary(p: &Project) -> ProjectSummary {
    ProjectSummary {
        provider: "geysermc".into(),
        id: p.id.into(),
        slug: p.id.into(),
        name: p.name.into(),
        description: p.description.into(),
        author: Some("GeyserMC".into()),
        downloads: 0,
        icon_url: None,
        page_url: p.page.into(),
        updated: None,
        license: Some("MIT".into()),
    }
}

/// Minecraft 1.20.5 or newer (Geyser-Spigot's minimum). Unparseable versions pass.
pub fn spigot_supported(game_version: &str) -> bool {
    let nums: Vec<u32> = game_version
        .split(['.', '-'])
        .map_while(|p| p.parse().ok())
        .collect();
    match nums.as_slice() {
        [1, minor, rest @ ..] => (*minor, rest.first().copied().unwrap_or(0)) >= (20, 5),
        [major, ..] => *major > 1,
        [] => true,
    }
}

#[derive(Deserialize)]
struct ProjectInfo {
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct Builds {
    builds: Vec<Build>,
}

#[derive(Deserialize)]
struct Build {
    build: u32,
    time: Option<String>,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    downloads: HashMap<String, Download>,
}

#[derive(Deserialize)]
struct Download {
    name: String,
    sha256: String,
}

fn check_version(v: &str) -> CoreResult<&str> {
    if (1..=32).contains(&v.len())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        Ok(v)
    } else {
        Err(CoreError::invalid("Invalid GeyserMC version"))
    }
}

/// Version ids are `<version>-b<build>`, like the builds' own version strings.
fn split_id(id: &str) -> CoreResult<(&str, u32)> {
    id.rsplit_once("-b")
        .and_then(|(v, b)| Some((check_version(v).ok()?, b.parse().ok()?)))
        .ok_or_else(|| CoreError::invalid("Invalid GeyserMC build id"))
}

fn to_version(project: &str, version: &str, b: Build) -> ContentVersion {
    let file = b.downloads.get(PLATFORM).map(|d| ContentFile {
        url: format!(
            "{API}/projects/{project}/versions/{version}/builds/{}/downloads/{PLATFORM}",
            b.build
        ),
        file_name: d.name.clone(),
        size: None,
        hash: Some(ExpectedHash {
            algorithm: HashAlgorithm::Sha256,
            hex: d.sha256.to_lowercase(),
        }),
    });
    let id = format!("{version}-b{}", b.build);
    ContentVersion {
        provider: "geysermc".into(),
        project_id: project.into(),
        name: id.clone(),
        version_number: id.clone(),
        id,
        channel: if b.channel.is_empty() || b.channel == "default" {
            ReleaseChannel::Release
        } else {
            ReleaseChannel::Beta
        },
        game_versions: Vec::new(),
        loaders: vec![PLATFORM.into()],
        published: b.time.as_deref().and_then(parse_rfc3339).map(Timestamp),
        file,
        external_url: None,
        dependencies: Vec::new(),
    }
}

pub struct GeyserMc {
    http: HttpClient,
    info: ContentProviderInfo,
}

impl GeyserMc {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            info: ContentProviderInfo {
                id: "geysermc".into(),
                display_name: "GeyserMC".into(),
                website: "https://geysermc.org".into(),
                download_hosts: vec!["download.geysermc.org".into()],
                kinds: vec![ContentKind::Plugin],
                hash_lookup: false,
            },
        }
    }

    fn not_found(e: CoreError, what: &str) -> CoreError {
        if e.code == ErrorCode::VersionNotFound {
            CoreError::new(ErrorCode::NotFound, format!("{what} was not found"))
        } else {
            e
        }
    }
}

#[async_trait]
impl ContentProvider for GeyserMc {
    fn info(&self) -> &ContentProviderInfo {
        &self.info
    }

    fn supports(&self, target: &ContentTarget) -> bool {
        target.kind == ContentKind::Plugin
            && target.ecosystems.iter().any(|e| {
                matches!(
                    e,
                    ContentEcosystem::BukkitPlugins | ContentEcosystem::PaperPlugins
                )
            })
            && spigot_supported(&target.game_version)
    }

    async fn search(&self, q: &SearchQuery) -> CoreResult<SearchPage> {
        let text = q.text.trim().to_lowercase();
        let hits: Vec<ProjectSummary> = PROJECTS
            .iter()
            .filter(|p| {
                text.is_empty()
                    || p.id.contains(&text)
                    || text.contains(p.id)
                    || p.description.to_lowercase().contains(&text)
                    || text.contains("bedrock")
            })
            .map(summary)
            .collect();
        let total = hits.len() as u64;
        Ok(SearchPage {
            hits: hits
                .into_iter()
                .skip(q.offset as usize)
                .take(q.limit as usize)
                .collect(),
            total,
        })
    }

    async fn project(&self, id: &str) -> CoreResult<ProjectSummary> {
        Ok(summary(project(id)?))
    }

    async fn versions(
        &self,
        project_id: &str,
        _target: &ContentTarget,
    ) -> CoreResult<Vec<ContentVersion>> {
        let p = project(project_id)?;
        let info: ProjectInfo = self
            .http
            .get_json(&format!("{API}/projects/{}", p.id))
            .await
            .map_err(|e| Self::not_found(e, p.name))?;
        let Some(version) = info.versions.last() else {
            return Ok(Vec::new());
        };
        let version = check_version(version)?;
        let builds: Builds = self
            .http
            .get_json(&format!(
                "{API}/projects/{}/versions/{version}/builds",
                p.id
            ))
            .await?;
        Ok(builds
            .builds
            .into_iter()
            .rev()
            .take(MAX_BUILDS)
            .map(|b| to_version(p.id, version, b))
            .filter(|v| v.file.is_some())
            .collect())
    }

    async fn version(&self, project_id: &str, version_id: &str) -> CoreResult<ContentVersion> {
        let p = project(project_id)?;
        let (version, build) = split_id(version_id)?;
        let b: Build = self
            .http
            .get_json(&format!(
                "{API}/projects/{}/versions/{version}/builds/{build}",
                p.id
            ))
            .await
            .map_err(|e| Self::not_found(e, "The build"))?;
        Ok(to_version(p.id, version, b))
    }

    async fn identify(&self, _sha512: &[String]) -> CoreResult<HashMap<String, ContentVersion>> {
        Ok(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_spigot_version() {
        for ok in [
            "1.20.5", "1.20.6", "1.21", "1.21.11", "26.1", "26.3", "27.1",
        ] {
            assert!(spigot_supported(ok), "{ok}");
        }
        for old in ["1.20.4", "1.20", "1.19.4", "1.8.8"] {
            assert!(!spigot_supported(old), "{old}");
        }
    }

    #[test]
    fn build_ids_round_trip() {
        assert_eq!(split_id("2.11.3-b1247").unwrap(), ("2.11.3", 1247));
        assert!(split_id("2.11.3").is_err());
        assert!(split_id("../x-b1").is_err());
    }

    #[test]
    fn builds_map_to_spigot_downloads_with_sha256() {
        let b: Build = serde_json::from_str(
            r#"{"build":141,"time":"2026-09-17T14:53:03.841Z","channel":"default",
               "downloads":{"spigot":{"name":"floodgate-spigot.jar","sha256":"ABC"},
                            "velocity":{"name":"floodgate-velocity.jar","sha256":"def"}}}"#,
        )
        .unwrap();
        let v = to_version("floodgate", "2.2.5", b);
        assert_eq!(v.id, "2.2.5-b141");
        assert_eq!(v.channel, ReleaseChannel::Release);
        let f = v.file.unwrap();
        assert_eq!(f.file_name, "floodgate-spigot.jar");
        assert_eq!(
            f.url,
            "https://download.geysermc.org/v2/projects/floodgate/versions/2.2.5/builds/141/downloads/spigot"
        );
        assert_eq!(f.hash.unwrap().hex, "abc");
    }
}
