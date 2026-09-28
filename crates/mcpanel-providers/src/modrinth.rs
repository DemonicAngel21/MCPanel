//! Modrinth content provider (API v2, <https://docs.modrinth.com/api/>).
//!
//! Verified 2026-09-28 against the live API:
//! - `GET /v2/search?query&facets&index&offset&limit` → `{hits, offset, limit, total_hits}`;
//!   facets are AND-ed outer arrays of OR-ed `"key:value"` strings (`project_type`,
//!   `categories` = loaders, `versions`). Multi-platform projects report
//!   `project_type: "mod"` but match `project_type:plugin` via `all_project_types`.
//! - `GET /v2/project/{id|slug}`, `GET /v2/projects?ids=[…]`.
//! - `GET /v2/project/{id}/version?loaders=[…]&game_versions=[…]` → newest first; files
//!   carry `hashes.sha512` / `sha1`, `url` on `cdn.modrinth.com`, `size`, `primary`;
//!   dependencies `{version_id?, project_id?, file_name?, dependency_type}`.
//! - `GET /v2/version/{id}`; `POST /v2/version_files {hashes, algorithm}` → hash → version.
//! - Rate limit 300 requests/minute (`X-Ratelimit-*` headers); a descriptive User-Agent
//!   is sent.

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

const API: &str = "https://api.modrinth.com/v2";

pub struct Modrinth {
    http: HttpClient,
    info: ContentProviderInfo,
}

impl Modrinth {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            info: ContentProviderInfo {
                id: "modrinth".into(),
                display_name: "Modrinth".into(),
                website: "https://modrinth.com".into(),
                download_hosts: vec!["cdn.modrinth.com".into()],
                kinds: vec![ContentKind::Plugin, ContentKind::Mod],
                hash_lookup: true,
            },
        }
    }
}

/// Modrinth loader ids for a server.
pub fn loaders(target: &ContentTarget) -> Vec<&'static str> {
    let mut out = Vec::new();
    for e in &target.ecosystems {
        match (target.kind, e) {
            (ContentKind::Plugin, ContentEcosystem::BukkitPlugins) => {
                out.extend(["bukkit", "spigot"])
            }
            (ContentKind::Plugin, ContentEcosystem::PaperPlugins) => out.push("paper"),
            (ContentKind::Mod, ContentEcosystem::FabricMods) => out.push("fabric"),
            (ContentKind::Mod, ContentEcosystem::QuiltMods) => out.push("quilt"),
            (ContentKind::Mod, ContentEcosystem::ForgeMods) => out.push("forge"),
            (ContentKind::Mod, ContentEcosystem::NeoForgeMods) => out.push("neoforge"),
            _ => {}
        }
    }
    if target.kind == ContentKind::Plugin && target.software_id == "purpur" {
        out.push("purpur");
    }
    out.sort();
    out.dedup();
    out
}

fn check_id(id: &str) -> CoreResult<&str> {
    if (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        Ok(id)
    } else {
        Err(CoreError::invalid("Invalid Modrinth id"))
    }
}

fn not_found(e: CoreError, what: &str) -> CoreError {
    if e.code == ErrorCode::VersionNotFound {
        CoreError::new(
            ErrorCode::NotFound,
            format!("{what} was not found on Modrinth"),
        )
    } else {
        e
    }
}

fn json_list(items: &[&str]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<Hit>,
    total_hits: u64,
}

#[derive(Deserialize)]
struct Hit {
    project_id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    author: Option<String>,
    #[serde(default)]
    downloads: u64,
    icon_url: Option<String>,
    date_modified: Option<String>,
    license: Option<String>,
    #[serde(default)]
    project_type: String,
}

#[derive(Deserialize)]
struct Project {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    downloads: u64,
    icon_url: Option<String>,
    updated: Option<String>,
    license: Option<License>,
    #[serde(default)]
    project_type: String,
}

#[derive(Deserialize)]
struct License {
    id: String,
}

#[derive(Deserialize)]
struct Version {
    id: String,
    project_id: String,
    name: String,
    version_number: String,
    version_type: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    date_published: Option<String>,
    #[serde(default)]
    files: Vec<VFile>,
    #[serde(default)]
    dependencies: Vec<Dep>,
}

#[derive(Deserialize)]
struct VFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    size: Option<u64>,
    #[serde(default)]
    hashes: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Dep {
    version_id: Option<String>,
    project_id: Option<String>,
    file_name: Option<String>,
    dependency_type: String,
}

fn page_url(project_type: &str, slug: &str) -> String {
    let kind = match project_type {
        "plugin" | "mod" | "datapack" | "modpack" | "resourcepack" | "shader" => project_type,
        _ => "project",
    };
    format!("https://modrinth.com/{kind}/{slug}")
}

fn ts(s: &Option<String>) -> Option<Timestamp> {
    s.as_deref().and_then(parse_rfc3339).map(Timestamp)
}

impl From<Version> for ContentVersion {
    fn from(v: Version) -> Self {
        let file = v
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| v.files.first())
            .map(|f| ContentFile {
                url: f.url.clone(),
                file_name: f.filename.clone(),
                size: f.size,
                hash: f
                    .hashes
                    .get("sha512")
                    .map(|h| ExpectedHash {
                        algorithm: HashAlgorithm::Sha512,
                        hex: h.to_lowercase(),
                    })
                    .or_else(|| {
                        f.hashes.get("sha1").map(|h| ExpectedHash {
                            algorithm: HashAlgorithm::Sha1,
                            hex: h.to_lowercase(),
                        })
                    }),
            });
        ContentVersion {
            provider: "modrinth".into(),
            project_id: v.project_id,
            id: v.id,
            name: v.name,
            version_number: v.version_number,
            channel: match v.version_type.as_str() {
                "release" => ReleaseChannel::Release,
                "beta" => ReleaseChannel::Beta,
                _ => ReleaseChannel::Alpha,
            },
            game_versions: v.game_versions,
            loaders: v.loaders,
            published: ts(&v.date_published),
            file,
            external_url: None,
            dependencies: v
                .dependencies
                .into_iter()
                .map(|d| ContentDependency {
                    project_id: d.project_id,
                    version_id: d.version_id,
                    name: d.file_name,
                    kind: match d.dependency_type.as_str() {
                        "required" => DependencyKind::Required,
                        "incompatible" => DependencyKind::Incompatible,
                        "embedded" => DependencyKind::Embedded,
                        _ => DependencyKind::Optional,
                    },
                    external_url: None,
                })
                .collect(),
        }
    }
}

#[async_trait]
impl ContentProvider for Modrinth {
    fn info(&self) -> &ContentProviderInfo {
        &self.info
    }

    fn supports(&self, target: &ContentTarget) -> bool {
        !loaders(target).is_empty()
    }

    async fn search(&self, q: &SearchQuery) -> CoreResult<SearchPage> {
        let project_type = match q.target.kind {
            ContentKind::Plugin => "project_type:plugin",
            ContentKind::Mod => "project_type:mod",
        };
        let cats: Vec<String> = loaders(&q.target)
            .iter()
            .map(|l| format!("categories:{l}"))
            .collect();
        let mut facets = vec![
            serde_json::json!([project_type]),
            serde_json::json!(cats),
            serde_json::json!([format!("versions:{}", q.target.game_version)]),
        ];
        if q.target.kind == ContentKind::Mod {
            // Hide client-only mods (e.g. Sodium) from server searches.
            facets.push(serde_json::json!([
                "server_side:required",
                "server_side:optional"
            ]));
        }
        let facets = serde_json::Value::Array(facets).to_string();
        let index = match q.sort {
            SearchSort::Relevance => "relevance",
            SearchSort::Downloads => "downloads",
            SearchSort::Updated => "updated",
        };
        let url = reqwest::Url::parse_with_params(
            &format!("{API}/search"),
            &[
                ("query", q.text.as_str()),
                ("facets", facets.as_str()),
                ("index", index),
                ("offset", &q.offset.to_string()),
                ("limit", &q.limit.to_string()),
            ],
        )
        .map_err(|e| CoreError::internal(e.to_string()))?;
        let r: SearchResponse = self.http.get_json(url.as_str()).await?;
        Ok(SearchPage {
            total: r.total_hits,
            hits: r
                .hits
                .into_iter()
                .map(|h| ProjectSummary {
                    provider: "modrinth".into(),
                    page_url: page_url(
                        if q.target.kind == ContentKind::Plugin {
                            "plugin"
                        } else {
                            &h.project_type
                        },
                        &h.slug,
                    ),
                    id: h.project_id,
                    slug: h.slug,
                    name: h.title,
                    description: h.description,
                    author: h.author,
                    downloads: h.downloads,
                    icon_url: h.icon_url,
                    updated: ts(&h.date_modified),
                    license: h.license,
                })
                .collect(),
        })
    }

    async fn project(&self, id: &str) -> CoreResult<ProjectSummary> {
        let p: Project = self
            .http
            .get_json(&format!("{API}/project/{}", check_id(id)?))
            .await
            .map_err(|e| not_found(e, "The project"))?;
        Ok(ProjectSummary {
            provider: "modrinth".into(),
            page_url: page_url(&p.project_type, &p.slug),
            id: p.id,
            slug: p.slug,
            name: p.title,
            description: p.description,
            author: None,
            downloads: p.downloads,
            icon_url: p.icon_url,
            updated: ts(&p.updated),
            license: p.license.map(|l| l.id),
        })
    }

    async fn versions(
        &self,
        project_id: &str,
        target: &ContentTarget,
    ) -> CoreResult<Vec<ContentVersion>> {
        let loaders = loaders(target);
        let url = reqwest::Url::parse_with_params(
            &format!("{API}/project/{}/version", check_id(project_id)?),
            &[
                ("loaders", json_list(&loaders)),
                ("game_versions", json_list(&[target.game_version.as_str()])),
            ],
        )
        .map_err(|e| CoreError::internal(e.to_string()))?;
        let v: Vec<Version> = self
            .http
            .get_json(url.as_str())
            .await
            .map_err(|e| not_found(e, "The project"))?;
        let mut out: Vec<ContentVersion> = v.into_iter().map(Into::into).collect();
        out.sort_by_key(|v| std::cmp::Reverse(v.published));
        Ok(out)
    }

    async fn version(&self, _project_id: &str, version_id: &str) -> CoreResult<ContentVersion> {
        let v: Version = self
            .http
            .get_json(&format!("{API}/version/{}", check_id(version_id)?))
            .await
            .map_err(|e| not_found(e, "The version"))?;
        Ok(v.into())
    }

    async fn identify(&self, sha512: &[String]) -> CoreResult<HashMap<String, ContentVersion>> {
        if sha512.is_empty() {
            return Ok(HashMap::new());
        }
        let hashes: Vec<&str> = sha512
            .iter()
            .map(String::as_str)
            .filter(|h| h.len() == 128)
            .collect();
        let r: HashMap<String, Version> = self
            .http
            .post_json(
                &format!("{API}/version_files"),
                &serde_json::json!({ "hashes": hashes, "algorithm": "sha512" }),
            )
            .await?;
        Ok(r.into_iter()
            .map(|(h, v)| (h.to_lowercase(), v.into()))
            .collect())
    }
}
