//! Spiget (<https://spiget.org>, a third-party API for SpigotMC resources, not operated by
//! SpigotMC) as a content provider. Verified 2026-09-29 against `https://api.spiget.org/v2`
//! (swagger: github.com/SpiGetOrg/Documentation):
//!
//! - `GET /search/resources/{query}?field=name&size&page&sort=-downloads&fields=…` and
//!   `GET /resources?size&page&sort=…` → resources with `id`, `name`, `tag`, `external`,
//!   `premium` (absent = false), `file{type (".jar", ".sk", "external"), url,
//!   externalUrl?}`, `testedVersions`, `downloads`, `updateDate` (unix seconds),
//!   `version{id}`; `GET /resources/{id}` (404 if unknown);
//!   `GET /resources/{id}/versions/latest` → `{id, name, releaseDate}`.
//! - `GET /resources/{id}/download` redirects (302) to the file Spiget stored for the
//!   resource's latest version on `cdn.spiget.org`. Version-specific downloads redirect
//!   to spigotmc.org (not automatable) and the proxy endpoint is strictly rate limited,
//!   so only the latest version is installable.
//! - Spiget publishes no file hashes: downloads are unverified (the plan says so) and
//!   still pass the plugin descriptor check. Premium and external resources are listed
//!   but never downloaded.

use crate::http::HttpClient;
use async_trait::async_trait;
use mcpanel_core::content::{
    ContentFile, ContentKind, ContentProvider, ContentProviderInfo, ContentTarget, ContentVersion,
    ProjectSummary, ReleaseChannel, SearchPage, SearchQuery, SearchSort,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::software::ContentEcosystem;
use mcpanel_core::time::Timestamp;
use serde::Deserialize;
use std::collections::HashMap;

const API: &str = "https://api.spiget.org/v2";
const CDN_HOST: &str = "cdn.spiget.org";
const FIELDS: &str =
    "id,name,tag,external,premium,file,testedVersions,downloads,updateDate,version";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Resource {
    id: u64,
    name: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    external: bool,
    #[serde(default)]
    premium: bool,
    file: Option<FileInfo>,
    #[serde(default)]
    tested_versions: Vec<String>,
    #[serde(default)]
    downloads: u64,
    update_date: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileInfo {
    #[serde(rename = "type", default)]
    kind: String,
    external_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    id: u64,
    name: String,
    release_date: Option<i64>,
}

fn check_id(id: &str) -> CoreResult<u64> {
    id.parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| CoreError::invalid("Invalid Spiget resource id"))
}

fn not_found(e: CoreError) -> CoreError {
    if e.code == ErrorCode::VersionNotFound {
        CoreError::new(ErrorCode::NotFound, "The resource was not found on Spiget")
    } else {
        e
    }
}

fn page_url(id: u64) -> String {
    format!("https://www.spigotmc.org/resources/{id}/")
}

fn summary(r: &Resource) -> ProjectSummary {
    ProjectSummary {
        provider: "spiget".into(),
        id: r.id.to_string(),
        slug: r.id.to_string(),
        name: r.name.clone(),
        description: r.tag.clone(),
        author: None,
        downloads: r.downloads,
        icon_url: None,
        page_url: page_url(r.id),
        updated: r.update_date.map(|s| Timestamp(s * 1000)),
        license: None,
    }
}

/// Only plain, free jar resources can be downloaded.
fn installable(r: &Resource) -> bool {
    !r.premium && !r.external && r.file.as_ref().is_some_and(|f| f.kind == ".jar")
}

pub struct Spiget {
    http: HttpClient,
    info: ContentProviderInfo,
}

impl Spiget {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            info: ContentProviderInfo {
                id: "spiget".into(),
                display_name: "SpigotMC (via Spiget)".into(),
                website: "https://spiget.org".into(),
                download_hosts: vec![CDN_HOST.into()],
                kinds: vec![ContentKind::Plugin],
                hash_lookup: false,
            },
        }
    }

    async fn resource(&self, id: u64) -> CoreResult<Resource> {
        self.http
            .get_json(&format!("{API}/resources/{id}?fields={FIELDS}"))
            .await
            .map_err(not_found)
    }

    async fn latest(&self, r: &Resource) -> CoreResult<ContentVersion> {
        let v: Version = self
            .http
            .get_json(&format!("{API}/resources/{}/versions/latest", r.id))
            .await
            .map_err(not_found)?;
        let file = if installable(r) {
            // Resolve the documented download redirect and accept only Spiget's CDN.
            let target = self
                .http
                .redirect_target(&format!("{API}/resources/{}/download", r.id))
                .await?;
            let Some(url) = target.filter(|u| {
                reqwest::Url::parse(u)
                    .is_ok_and(|p| p.scheme() == "https" && p.host_str() == Some(CDN_HOST))
            }) else {
                return Err(CoreError::new(
                    ErrorCode::ProviderError,
                    format!("Spiget has no downloadable file for {}", r.name),
                ));
            };
            Some(ContentFile {
                url,
                file_name: format!("{}.jar", sanitize(&r.name, r.id)),
                size: None,
                hash: None,
            })
        } else {
            None
        };
        let external_url = if file.is_none() {
            if r.premium {
                Some(page_url(r.id))
            } else {
                r.file
                    .as_ref()
                    .and_then(|f| f.external_url.clone())
                    .or_else(|| Some(page_url(r.id)))
            }
        } else {
            None
        };
        Ok(ContentVersion {
            provider: "spiget".into(),
            project_id: r.id.to_string(),
            id: v.id.to_string(),
            name: v.name.clone(),
            version_number: v.name,
            channel: ReleaseChannel::Release,
            game_versions: r.tested_versions.clone(),
            loaders: vec!["spigot".into()],
            published: v.release_date.map(|s| Timestamp(s * 1000)),
            file,
            external_url,
            dependencies: Vec::new(),
        })
    }
}

/// A safe jar file name from the resource name (letters, digits, `-`, `_`, `.`).
fn sanitize(name: &str, id: u64) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches(['-', '.']).to_string();
    let s: String = s.chars().take(48).collect();
    if s.is_empty() {
        format!("spigot-{id}")
    } else {
        s
    }
}

#[async_trait]
impl ContentProvider for Spiget {
    fn info(&self) -> &ContentProviderInfo {
        &self.info
    }

    fn supports(&self, target: &ContentTarget) -> bool {
        target.kind == ContentKind::Plugin
            && target.ecosystems.contains(&ContentEcosystem::BukkitPlugins)
    }

    async fn search(&self, q: &SearchQuery) -> CoreResult<SearchPage> {
        let size = q.limit.clamp(1, 50);
        let page = q.offset / size + 1;
        let sort = match q.sort {
            SearchSort::Updated => "-updateDate",
            SearchSort::Relevance | SearchSort::Downloads => "-downloads",
        };
        let text = q.text.trim();
        let url = if text.is_empty() {
            reqwest::Url::parse_with_params(
                &format!("{API}/resources"),
                &[
                    ("size", size.to_string()),
                    ("page", page.to_string()),
                    ("sort", sort.into()),
                    ("fields", FIELDS.into()),
                ],
            )
        } else {
            let mut u = reqwest::Url::parse(&format!("{API}/search/resources/"))
                .map_err(|e| CoreError::internal(e.to_string()))?;
            u.path_segments_mut()
                .map_err(|_| CoreError::internal("bad url"))?
                .pop_if_empty()
                .push(text);
            u.query_pairs_mut()
                .append_pair("field", "name")
                .append_pair("size", &size.to_string())
                .append_pair("page", &page.to_string())
                .append_pair("sort", sort)
                .append_pair("fields", FIELDS);
            Ok(u)
        }
        .map_err(|e| CoreError::internal(e.to_string()))?;
        let found: Vec<Resource> = match self.http.get_json(url.as_str()).await {
            Ok(v) => v,
            // Spiget answers a search without results with 404.
            Err(e) if e.code == ErrorCode::VersionNotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let hits: Vec<ProjectSummary> = found.iter().map(summary).collect();
        let total = q.offset as u64
            + hits.len() as u64
            + if hits.len() as u32 == size {
                size as u64
            } else {
                0
            };
        Ok(SearchPage { hits, total })
    }

    async fn project(&self, id: &str) -> CoreResult<ProjectSummary> {
        Ok(summary(&self.resource(check_id(id)?).await?))
    }

    async fn versions(
        &self,
        project_id: &str,
        _target: &ContentTarget,
    ) -> CoreResult<Vec<ContentVersion>> {
        let r = self.resource(check_id(project_id)?).await?;
        Ok(vec![self.latest(&r).await?])
    }

    async fn version(&self, project_id: &str, version_id: &str) -> CoreResult<ContentVersion> {
        let r = self.resource(check_id(project_id)?).await?;
        let v = self.latest(&r).await?;
        if v.id != version_id {
            return Err(CoreError::new(
                ErrorCode::Unsupported,
                "Spiget only offers the latest version of a resource",
            ));
        }
        Ok(v)
    }

    async fn identify(&self, _sha512: &[String]) -> CoreResult<HashMap<String, ContentVersion>> {
        Ok(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe() {
        assert_eq!(sanitize("LuckPerms", 1), "LuckPerms");
        assert_eq!(sanitize("[SK] Anti Spam!", 1), "SK--Anti-Spam");
        assert_eq!(sanitize("../..", 7), "spigot-7");
        assert_eq!(sanitize("日本語", 9), "spigot-9");
    }

    #[test]
    fn only_free_jar_resources_are_installable() {
        let r = |premium, external, kind: &str| Resource {
            id: 1,
            name: "x".into(),
            tag: String::new(),
            external,
            premium,
            file: Some(FileInfo {
                kind: kind.into(),
                external_url: None,
            }),
            tested_versions: vec![],
            downloads: 0,
            update_date: None,
        };
        assert!(installable(&r(false, false, ".jar")));
        assert!(!installable(&r(true, false, ".jar")));
        assert!(!installable(&r(false, true, "external")));
        assert!(!installable(&r(false, false, ".sk")));
    }

    #[test]
    fn resources_parse_with_missing_optional_fields() {
        let r: Resource = serde_json::from_str(
            r#"{"external":true,"file":{"type":"external","size":0,"sizeUnit":"","url":"x",
                "externalUrl":"https://github.com/EssentialsX/Essentials/releases"},
                "name":"EssentialsX","id":9089}"#,
        )
        .unwrap();
        assert!(!r.premium && r.external);
        assert_eq!(
            r.file.unwrap().external_url.as_deref(),
            Some("https://github.com/EssentialsX/Essentials/releases")
        );
    }
}
