//! Paper provider using PaperMC's Fill v3 API (the v2 API is sunset — verified
//! 2026-09-28, see docs/architecture/verification-log.md).

use crate::detect;
use crate::http::{HttpClient, parse_rfc3339};
use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::{ExpectedHash, HashAlgorithm};
use mcpanel_core::software::jar::SingleJarLauncher;
use mcpanel_core::software::{
    ContentEcosystem, DetectedSoftware, GameVersion, GameVersionKind, InstallPlan, InstallRequest,
    InstallStep, JavaRequirement, ReleaseChannel, SoftwareBuild, SoftwareCaps, SoftwareCatalog,
    SoftwareDescriptor, SoftwareDetector, SoftwareInstaller, SoftwareProvider, TpsSource,
};
use mcpanel_core::time::Timestamp;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const BASE: &str = "https://fill.papermc.io/v3/projects/paper";

#[derive(Debug, Deserialize)]
struct Project {
    versions: HashMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct VersionInfo {
    version: VersionDetail,
}

#[derive(Debug, Deserialize)]
struct VersionDetail {
    java: Option<JavaInfo>,
}

#[derive(Debug, Deserialize)]
struct JavaInfo {
    version: Option<JavaVersion>,
    flags: Option<JavaFlags>,
}

#[derive(Debug, Deserialize)]
struct JavaVersion {
    minimum: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct JavaFlags {
    #[serde(default)]
    recommended: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Build {
    id: u64,
    time: Option<String>,
    channel: String,
    #[serde(default)]
    downloads: HashMap<String, BuildDownload>,
}

#[derive(Debug, Deserialize)]
struct BuildDownload {
    name: String,
    checksums: Checksums,
    size: Option<u64>,
    url: String,
}

#[derive(Debug, Deserialize)]
struct Checksums {
    sha256: Option<String>,
}

pub(crate) fn channel(s: &str) -> ReleaseChannel {
    match s {
        "STABLE" => ReleaseChannel::Stable,
        "BETA" => ReleaseChannel::Beta,
        _ => ReleaseChannel::Alpha,
    }
}

pub(crate) fn guess_kind(id: &str) -> GameVersionKind {
    let l = id.to_ascii_lowercase();
    if l.contains("-pre") || l.contains("-rc") || l.contains("snapshot") || l.contains('w') {
        GameVersionKind::Snapshot
    } else {
        GameVersionKind::Release
    }
}

pub struct PaperProvider {
    http: HttpClient,
}

impl PaperProvider {
    pub fn descriptor() -> SoftwareDescriptor {
        SoftwareDescriptor {
            id: "paper".into(),
            display_name: "Paper".into(),
            description: "High-performance server with Bukkit/Paper plugin support.".into(),
            caps: SoftwareCaps {
                content: vec![
                    ContentEcosystem::BukkitPlugins,
                    ContentEcosystem::PaperPlugins,
                    ContentEcosystem::Datapacks,
                ],
                is_proxy: false,
                log_dialect: "minecraft".into(),
                stop_command: "stop".into(),
                eula_required: true,
                requires_build_step: false,
                tps_source: Some(TpsSource::PaperCommands),
            },
            download_hosts: vec!["fill-data.papermc.io".into()],
        }
    }

    pub fn provider(http: HttpClient) -> SoftwareProvider {
        let p = Arc::new(Self { http });
        SoftwareProvider {
            descriptor: Self::descriptor(),
            catalog: p.clone(),
            installer: p.clone(),
            launcher: Arc::new(SingleJarLauncher::default()),
            detector: Some(p),
        }
    }

    fn check_version(v: &str) -> CoreResult<()> {
        if v.is_empty()
            || v.len() > 64
            || !v
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(CoreError::invalid("Invalid version"));
        }
        Ok(())
    }

    async fn builds_raw(&self, version: &str) -> CoreResult<Vec<Build>> {
        Self::check_version(version)?;
        self.http
            .get_json(&format!("{BASE}/versions/{version}/builds"))
            .await
    }
}

#[async_trait]
impl SoftwareCatalog for PaperProvider {
    async fn game_versions(&self) -> CoreResult<Vec<GameVersion>> {
        let p: Project = self.http.get_json(BASE).await?;
        Ok(p.versions
            .into_values()
            .flatten()
            .map(|id| GameVersion {
                kind: guess_kind(&id),
                id,
                release_time: None,
            })
            .collect())
    }

    async fn builds(&self, game_version: &str) -> CoreResult<Vec<SoftwareBuild>> {
        Ok(self
            .builds_raw(game_version)
            .await?
            .into_iter()
            .map(|b| SoftwareBuild {
                id: b.id.to_string(),
                channel: channel(&b.channel),
                published_at: b.time.as_deref().and_then(parse_rfc3339).map(Timestamp),
            })
            .collect())
    }
}

#[async_trait]
impl SoftwareInstaller for PaperProvider {
    async fn plan_install(&self, req: &InstallRequest) -> CoreResult<InstallPlan> {
        Self::check_version(&req.game_version)?;
        let info: VersionInfo = self
            .http
            .get_json(&format!("{BASE}/versions/{}", req.game_version))
            .await?;
        let mut notes = Vec::new();
        let build: Build = match &req.build {
            Some(id) => {
                if !id.chars().all(|c| c.is_ascii_digit()) {
                    return Err(CoreError::invalid("Invalid build id"));
                }
                self.http
                    .get_json(&format!("{BASE}/versions/{}/builds/{id}", req.game_version))
                    .await?
            }
            None => {
                let builds = self.builds_raw(&req.game_version).await?;
                let mut iter = builds.into_iter();
                let all: Vec<Build> = iter.by_ref().collect();
                let has_stable = all.iter().any(|b| b.channel == "STABLE");
                let chosen = if has_stable {
                    all.into_iter().find(|b| b.channel == "STABLE")
                } else {
                    notes.push("No stable Paper build exists for this version yet; the newest experimental build was selected.".into());
                    all.into_iter().next()
                };
                chosen.ok_or_else(|| {
                    CoreError::new(
                        ErrorCode::VersionNotFound,
                        "No Paper builds are available for this version",
                    )
                })?
            }
        };
        let dl = build.downloads.get("server:default").ok_or_else(|| {
            CoreError::new(
                ErrorCode::ProviderError,
                "Paper build has no server download",
            )
        })?;
        let sha256 = dl.checksums.sha256.clone().ok_or_else(|| {
            CoreError::new(
                ErrorCode::ProviderError,
                "Paper build has no SHA-256 checksum",
            )
        })?;
        mcpanel_core::files::safepath::validate_name(&dl.name).map_err(|_| {
            CoreError::new(
                ErrorCode::ProviderError,
                "Paper returned an unsafe file name",
            )
        })?;
        let java = info.version.java;
        let min = java
            .as_ref()
            .and_then(|j| j.version.as_ref())
            .and_then(|v| v.minimum)
            .unwrap_or(8);
        let flags = java
            .and_then(|j| j.flags)
            .map(|f| f.recommended)
            .unwrap_or_default();
        let ch = channel(&build.channel);
        if ch != ReleaseChannel::Stable && req.build.is_some() {
            notes.push("The selected build is experimental.".into());
        }
        notes.push("Verified with the SHA-256 checksum published by PaperMC.".into());
        Ok(InstallPlan {
            software_id: "paper".into(),
            game_version: req.game_version.clone(),
            build: Some(build.id.to_string()),
            build_channel: Some(ch),
            steps: vec![InstallStep::Download {
                url: dl.url.clone(),
                dest: dl.name.clone(),
                expected_hash: Some(ExpectedHash {
                    algorithm: HashAlgorithm::Sha256,
                    hex: sha256,
                }),
                size: dl.size,
                description: format!("Downloading Paper {} build {}", req.game_version, build.id),
            }],
            jar: dl.name.clone(),
            java: JavaRequirement {
                min_major: min,
                recommended_major: None,
                recommended_flags: flags,
            },
            notes,
        })
    }
}

impl SoftwareDetector for PaperProvider {
    fn detect(&self, dir: &Path) -> Option<DetectedSoftware> {
        detect::detect_prefixed(dir, "paper", "paper")
    }
}
