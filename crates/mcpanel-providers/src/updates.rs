//! GitHub Releases provider implementation for the update checker and downloader.

use crate::http::HttpClient;
use async_trait::async_trait;
use futures::StreamExt;
use mcpanel_core::error::{CoreError, CoreResult};
use mcpanel_core::updates::{
    ReleaseAsset, ReleaseInfo, UpdateCheckResult, UpdateClient, is_newer_version,
};
use serde::Deserialize;
use std::path::Path;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Deserialize)]
struct GitHubAssetJson {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseJson {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAssetJson>,
}

pub struct GitHubUpdateClient {
    http: HttpClient,
}

impl GitHubUpdateClient {
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }

    fn select_installer_asset(assets: &[GitHubAssetJson]) -> Option<ReleaseAsset> {
        // Priority 1: .exe installer (e.g. MCPanel-Setup.exe or containing setup / x64)
        if let Some(a) = assets.iter().find(|a| {
            let lower = a.name.to_lowercase();
            lower.ends_with(".exe") && (lower.contains("setup") || lower.contains("x64"))
        }) {
            return Some(ReleaseAsset {
                name: a.name.clone(),
                download_url: a.browser_download_url.clone(),
                size: a.size,
            });
        }

        // Priority 2: Any .exe
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".exe"))
        {
            return Some(ReleaseAsset {
                name: a.name.clone(),
                download_url: a.browser_download_url.clone(),
                size: a.size,
            });
        }

        // Priority 3: .msi installer
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".msi"))
        {
            return Some(ReleaseAsset {
                name: a.name.clone(),
                download_url: a.browser_download_url.clone(),
                size: a.size,
            });
        }

        // Priority 4: .zip archive
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".zip"))
        {
            return Some(ReleaseAsset {
                name: a.name.clone(),
                download_url: a.browser_download_url.clone(),
                size: a.size,
            });
        }

        // Fallback: first asset
        assets.first().map(|a| ReleaseAsset {
            name: a.name.clone(),
            download_url: a.browser_download_url.clone(),
            size: a.size,
        })
    }
}

#[async_trait]
impl UpdateClient for GitHubUpdateClient {
    async fn check_for_updates(
        &self,
        current_version: &str,
        allow_prereleases: bool,
    ) -> CoreResult<UpdateCheckResult> {
        let custom_url = std::env::var("MCPANEL_UPDATE_URL").ok();
        let release: Option<GitHubReleaseJson> = if let Some(url) = custom_url {
            tracing::info!(target: "mcpanel::updates", url = %url, "checking custom update endpoint");
            self.http.get_json(&url).await.ok()
        } else if allow_prereleases {
            let url = "https://api.github.com/repos/mcpanel/mcpanel/releases";
            let list: Result<Vec<GitHubReleaseJson>, _> = self.http.get_json(url).await;
            list.ok().and_then(|mut v| {
                if v.is_empty() {
                    None
                } else {
                    Some(v.remove(0))
                }
            })
        } else {
            let url = "https://api.github.com/repos/mcpanel/mcpanel/releases/latest";
            self.http.get_json(url).await.ok()
        };

        let Some(rel) = release else {
            return Ok(UpdateCheckResult {
                current_version: current_version.to_string(),
                latest_version: current_version.to_string(),
                has_update: false,
                release: None,
            });
        };

        let clean_version = rel
            .tag_name
            .trim()
            .trim_start_matches(['v', 'V'])
            .to_string();
        let has_update = is_newer_version(current_version, &clean_version);
        let asset = Self::select_installer_asset(&rel.assets);

        let release_info = ReleaseInfo {
            version: clean_version.clone(),
            tag_name: rel.tag_name,
            name: rel.name,
            body: rel.body,
            published_at: rel.published_at,
            prerelease: rel.prerelease,
            asset,
        };

        Ok(UpdateCheckResult {
            current_version: current_version.to_string(),
            latest_version: clean_version,
            has_update,
            release: Some(release_info),
        })
    }

    async fn download_asset(
        &self,
        asset: &ReleaseAsset,
        target_file: &Path,
        progress_cb: Box<dyn Fn(u64, Option<u64>) + Send + Sync>,
    ) -> CoreResult<()> {
        if let Some(parent) = target_file.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                CoreError::internal(format!("Failed to create download folder: {e}"))
            })?;
        }

        let resp = self
            .http
            .inner()
            .get(&asset.download_url)
            .send()
            .await
            .map_err(|e| CoreError::internal(format!("Download failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(CoreError::internal(format!(
                "Download server returned HTTP {}",
                resp.status()
            )));
        }

        let total_bytes = resp.content_length().or(Some(asset.size));
        let mut stream = resp.bytes_stream();
        let mut file = tokio::fs::File::create(target_file)
            .await
            .map_err(|e| CoreError::internal(format!("Failed to create installer file: {e}")))?;

        let mut downloaded_bytes: u64 = 0;
        progress_cb(downloaded_bytes, total_bytes);

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| {
                CoreError::internal(format!("Error streaming installer download: {e}"))
            })?;
            file.write_all(&chunk)
                .await
                .map_err(|e| CoreError::internal(format!("Error writing installer file: {e}")))?;
            downloaded_bytes += chunk.len() as u64;
            progress_cb(downloaded_bytes, total_bytes);
        }

        file.flush()
            .await
            .map_err(|e| CoreError::internal(format!("Failed to flush installer file: {e}")))?;

        Ok(())
    }
}
