//! Hangar (PaperMC's plugin repository) content provider, API v1
//! (<https://hangar.papermc.io/api-docs>).
//!
//! Verified 2026-09-28 against the live API:
//! - `GET /api/v1/projects?q&platform=PAPER&version&sort=-downloads|-updated&limit&offset`
//!   → `{pagination{count,limit,offset}, result[]}`; projects have a numeric `id`,
//!   `namespace{owner,slug}`, `stats{downloads}`, `lastUpdated`.
//! - `GET /api/v1/projects/{slugOrId}`; 404 for unknown projects.
//! - `GET /api/v1/projects/{slugOrId}/versions?platform=PAPER&platformVersion&channel`
//!   → newest first; `name` identifies a version; `channel.name` (Release, Beta,
//!   Snapshot, …); `downloads.PAPER` = `{fileInfo{name,sizeBytes,sha256Hash},
//!   downloadUrl (hangarcdn.papermc.io) | externalUrl}`; `pluginDependencies.PAPER` =
//!   `[{name, projectId?, required, externalUrl?}]`.
//! - `GET /api/v1/projects/{slugOrId}/versions/{name}`.
//!
//! Versions that are only an `externalUrl` are listed but never downloaded
//! automatically (unknown host, no hash). Hangar has no lookup by file hash.

use crate::http::{HttpClient, parse_rfc3339};
use async_trait::async_trait;
use mcpanel_core::content::{
    ContentDependency, ContentFile, ContentKind, ContentProvider, ContentProviderInfo,
    ContentTarget, ContentVersion, DependencyKind, ProjectSummary, ReleaseChannel, SearchPage,
    SearchQuery, SearchSort,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::{ExpectedHash, HashAlgorithm};
use mcpanel_core::software::ContentEcosystem;
use mcpanel_core::time::Timestamp;
use serde::Deserialize;
use std::collections::HashMap;

const API: &str = "https://hangar.papermc.io/api/v1";
const PLATFORM: &str = "PAPER";

pub struct Hangar {
    http: HttpClient,
    info: ContentProviderInfo,
}

impl Hangar {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            info: ContentProviderInfo {
                id: "hangar".into(),
                display_name: "Hangar".into(),
                website: "https://hangar.papermc.io".into(),
                download_hosts: vec!["hangarcdn.papermc.io".into()],
                kinds: vec![ContentKind::Plugin],
                hash_lookup: false,
            },
        }
    }
}

fn check_id(id: &str) -> CoreResult<&str> {
    if (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        Ok(id)
    } else {
        Err(CoreError::invalid("Invalid Hangar project id"))
    }
}

fn not_found(e: CoreError, what: &str) -> CoreError {
    if e.code == ErrorCode::VersionNotFound {
        CoreError::new(
            ErrorCode::NotFound,
            format!("{what} was not found on Hangar"),
        )
    } else {
        e
    }
}

#[derive(Deserialize)]
struct Page<T> {
    pagination: Pagination,
    result: Vec<T>,
}

#[derive(Deserialize)]
struct Pagination {
    count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: u64,
    name: String,
    namespace: Namespace,
    #[serde(default)]
    description: String,
    stats: Option<Stats>,
    last_updated: Option<String>,
    avatar_url: Option<String>,
    settings: Option<Settings>,
}

#[derive(Deserialize)]
struct Namespace {
    owner: String,
    slug: String,
}

#[derive(Deserialize)]
struct Stats {
    #[serde(default)]
    downloads: u64,
}

#[derive(Deserialize)]
struct Settings {
    license: Option<LicenseInfo>,
}

#[derive(Deserialize)]
struct LicenseInfo {
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    name: String,
    project_id: Option<u64>,
    created_at: Option<String>,
    channel: Channel,
    #[serde(default)]
    downloads: HashMap<String, Download>,
    #[serde(default)]
    plugin_dependencies: HashMap<String, Vec<PluginDep>>,
    #[serde(default)]
    platform_dependencies: HashMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Channel {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Download {
    file_info: Option<FileInfo>,
    external_url: Option<String>,
    download_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileInfo {
    name: String,
    size_bytes: Option<u64>,
    sha256_hash: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginDep {
    name: String,
    project_id: Option<u64>,
    #[serde(default)]
    required: bool,
    external_url: Option<String>,
}

fn summary(p: Project) -> ProjectSummary {
    ProjectSummary {
        provider: "hangar".into(),
        page_url: format!(
            "https://hangar.papermc.io/{}/{}",
            p.namespace.owner, p.namespace.slug
        ),
        id: p.id.to_string(),
        slug: p.namespace.slug,
        name: p.name,
        description: p.description,
        author: Some(p.namespace.owner),
        downloads: p.stats.map_or(0, |s| s.downloads),
        icon_url: p.avatar_url,
        updated: p
            .last_updated
            .as_deref()
            .and_then(parse_rfc3339)
            .map(Timestamp),
        license: p.settings.and_then(|s| s.license).and_then(|l| l.name),
    }
}

fn to_version(v: Version, project_id: &str) -> ContentVersion {
    let dl = v.downloads.get(PLATFORM);
    let file = dl.and_then(|d| {
        let info = d.file_info.as_ref()?;
        Some(ContentFile {
            url: d.download_url.clone()?,
            file_name: info.name.clone(),
            size: info.size_bytes,
            hash: info.sha256_hash.as_ref().map(|h| ExpectedHash {
                algorithm: HashAlgorithm::Sha256,
                hex: h.to_lowercase(),
            }),
        })
    });
    ContentVersion {
        provider: "hangar".into(),
        project_id: v
            .project_id
            .map_or_else(|| project_id.to_string(), |p| p.to_string()),
        id: v.name.clone(),
        name: v.name.clone(),
        version_number: v.name,
        channel: match v.channel.name.to_lowercase().as_str() {
            "release" => ReleaseChannel::Release,
            "beta" => ReleaseChannel::Beta,
            _ => ReleaseChannel::Alpha,
        },
        game_versions: v
            .platform_dependencies
            .get(PLATFORM)
            .cloned()
            .unwrap_or_default(),
        loaders: vec!["paper".into()],
        published: v
            .created_at
            .as_deref()
            .and_then(parse_rfc3339)
            .map(Timestamp),
        external_url: if file.is_none() {
            dl.and_then(|d| d.external_url.clone())
        } else {
            None
        },
        file,
        dependencies: v
            .plugin_dependencies
            .get(PLATFORM)
            .map(|deps| {
                deps.iter()
                    .map(|d| ContentDependency {
                        project_id: d.project_id.map(|p| p.to_string()),
                        version_id: None,
                        name: Some(d.name.clone()),
                        kind: if d.required {
                            DependencyKind::Required
                        } else {
                            DependencyKind::Optional
                        },
                        external_url: d.external_url.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[async_trait]
impl ContentProvider for Hangar {
    fn info(&self) -> &ContentProviderInfo {
        &self.info
    }

    fn supports(&self, target: &ContentTarget) -> bool {
        target.kind == ContentKind::Plugin
            && target.ecosystems.contains(&ContentEcosystem::PaperPlugins)
    }

    async fn search(&self, q: &SearchQuery) -> CoreResult<SearchPage> {
        let mut params: Vec<(&str, String)> = vec![
            ("q", q.text.clone()),
            ("platform", PLATFORM.into()),
            ("version", q.target.game_version.clone()),
            ("limit", q.limit.to_string()),
            ("offset", q.offset.to_string()),
        ];
        match q.sort {
            SearchSort::Relevance if !q.text.is_empty() => {
                params.push(("prioritizeExactMatch", "true".into()))
            }
            SearchSort::Relevance | SearchSort::Downloads => {
                params.push(("sort", "-downloads".into()))
            }
            SearchSort::Updated => params.push(("sort", "-updated".into())),
        }
        let url = reqwest::Url::parse_with_params(&format!("{API}/projects"), &params)
            .map_err(|e| CoreError::internal(e.to_string()))?;
        let page: Page<Project> = self.http.get_json(url.as_str()).await?;
        Ok(SearchPage {
            total: page.pagination.count,
            hits: page.result.into_iter().map(summary).collect(),
        })
    }

    async fn project(&self, id: &str) -> CoreResult<ProjectSummary> {
        let p: Project = self
            .http
            .get_json(&format!("{API}/projects/{}", check_id(id)?))
            .await
            .map_err(|e| not_found(e, "The project"))?;
        Ok(summary(p))
    }

    async fn versions(
        &self,
        project_id: &str,
        target: &ContentTarget,
    ) -> CoreResult<Vec<ContentVersion>> {
        let url = reqwest::Url::parse_with_params(
            &format!("{API}/projects/{}/versions", check_id(project_id)?),
            &[
                ("platform", PLATFORM),
                ("platformVersion", target.game_version.as_str()),
                ("limit", "25"),
            ],
        )
        .map_err(|e| CoreError::internal(e.to_string()))?;
        let page: Page<Version> = self
            .http
            .get_json(url.as_str())
            .await
            .map_err(|e| not_found(e, "The project"))?;
        let mut out: Vec<ContentVersion> = page
            .result
            .into_iter()
            .map(|v| to_version(v, project_id))
            .collect();
        out.sort_by_key(|v| std::cmp::Reverse(v.published));
        Ok(out)
    }

    async fn version(&self, project_id: &str, version_id: &str) -> CoreResult<ContentVersion> {
        let mut url = reqwest::Url::parse(&format!(
            "{API}/projects/{}/versions/",
            check_id(project_id)?
        ))
        .map_err(|e| CoreError::internal(e.to_string()))?;
        url.path_segments_mut()
            .map_err(|_| CoreError::internal("bad url"))?
            .pop_if_empty()
            .push(version_id);
        let v: Version = self
            .http
            .get_json(url.as_str())
            .await
            .map_err(|e| not_found(e, "The version"))?;
        Ok(to_version(v, project_id))
    }

    async fn identify(&self, _sha512: &[String]) -> CoreResult<HashMap<String, ContentVersion>> {
        Ok(HashMap::new())
    }
}
