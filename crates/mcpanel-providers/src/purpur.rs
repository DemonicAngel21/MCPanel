//! Purpur provider (api.purpurmc.org v2, verified 2026-09-28). Purpur publishes only MD5
//! checksums, which MCPanel verifies for integrity but does not present as strong
//! verification.

use crate::detect;
use crate::http::HttpClient;
use crate::mojang::MojangClient;
use crate::paper::guess_kind;
use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::{ExpectedHash, HashAlgorithm};
use mcpanel_core::software::jar::SingleJarLauncher;
use mcpanel_core::software::{
    ContentEcosystem, DetectedSoftware, GameVersion, InstallPlan, InstallRequest, InstallStep,
    JavaRequirement, ReleaseChannel, SoftwareBuild, SoftwareCaps, SoftwareCatalog,
    SoftwareDescriptor, SoftwareDetector, SoftwareInstaller, SoftwareProvider, TpsSource,
};
use mcpanel_core::time::Timestamp;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

const BASE: &str = "https://api.purpurmc.org/v2/purpur";

#[derive(Debug, Deserialize)]
struct Project {
    versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Version {
    builds: Builds,
}

#[derive(Debug, Deserialize)]
struct Builds {
    latest: String,
    all: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Build {
    build: String,
    result: String,
    timestamp: Option<i64>,
    md5: Option<String>,
}

pub struct PurpurProvider {
    http: HttpClient,
    mojang: Arc<MojangClient>,
}

fn check(s: &str) -> CoreResult<()> {
    if s.is_empty()
        || s.len() > 64
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(CoreError::invalid("Invalid version or build"));
    }
    Ok(())
}

impl PurpurProvider {
    pub fn descriptor() -> SoftwareDescriptor {
        SoftwareDescriptor {
            id: "purpur".into(),
            display_name: "Purpur".into(),
            description:
                "Paper fork with extra gameplay configuration; supports Bukkit/Paper plugins."
                    .into(),
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
            download_hosts: vec!["api.purpurmc.org".into()],
        }
    }

    pub fn provider(http: HttpClient, mojang: Arc<MojangClient>) -> SoftwareProvider {
        let p = Arc::new(Self { http, mojang });
        SoftwareProvider {
            descriptor: Self::descriptor(),
            catalog: p.clone(),
            installer: p.clone(),
            launcher: Arc::new(SingleJarLauncher::default()),
            detector: Some(p),
        }
    }
}

#[async_trait]
impl SoftwareCatalog for PurpurProvider {
    async fn game_versions(&self) -> CoreResult<Vec<GameVersion>> {
        let p: Project = self.http.get_json(BASE).await?;
        Ok(p.versions
            .into_iter()
            .map(|id| GameVersion {
                kind: guess_kind(&id),
                id,
                release_time: None,
            })
            .collect())
    }

    async fn builds(&self, game_version: &str) -> CoreResult<Vec<SoftwareBuild>> {
        check(game_version)?;
        let v: Version = self
            .http
            .get_json(&format!("{BASE}/{game_version}"))
            .await?;
        Ok(v.builds
            .all
            .into_iter()
            .rev()
            .map(|id| SoftwareBuild {
                id,
                // Purpur has no release channels.
                channel: ReleaseChannel::Stable,
                published_at: None,
            })
            .collect())
    }
}

#[async_trait]
impl SoftwareInstaller for PurpurProvider {
    async fn plan_install(&self, req: &InstallRequest) -> CoreResult<InstallPlan> {
        check(&req.game_version)?;
        let build_id = match &req.build {
            Some(b) => {
                check(b)?;
                b.clone()
            }
            None => {
                let v: Version = self
                    .http
                    .get_json(&format!("{BASE}/{}", req.game_version))
                    .await?;
                v.builds.latest
            }
        };
        let build: Build = self
            .http
            .get_json(&format!("{BASE}/{}/{build_id}", req.game_version))
            .await?;
        if build.result != "SUCCESS" {
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("Purpur build {build_id} did not succeed"),
            ));
        }
        let md5 = build.md5.ok_or_else(|| {
            CoreError::new(ErrorCode::ProviderError, "Purpur build has no checksum")
        })?;
        let java = self.mojang.java_major(&req.game_version).await.unwrap_or(8);
        let jar = format!("purpur-{}-{}.jar", req.game_version, build.build);
        mcpanel_core::files::safepath::validate_name(&jar)?;
        let _ = build.timestamp.map(Timestamp);
        Ok(InstallPlan {
            software_id: "purpur".into(),
            game_version: req.game_version.clone(),
            build: Some(build.build.clone()),
            build_channel: Some(ReleaseChannel::Stable),
            steps: vec![InstallStep::Download {
                url: format!("{BASE}/{}/{}/download", req.game_version, build.build),
                dest: jar.clone(),
                expected_hash: Some(ExpectedHash {
                    algorithm: HashAlgorithm::Md5,
                    hex: md5,
                }),
                size: None,
                description: format!("Downloading Purpur {} build {}", req.game_version, build.build),
            }],
            jar,
            java: JavaRequirement {
                min_major: java,
                recommended_major: None,
                recommended_flags: Vec::new(),
            },
            notes: vec!["Purpur publishes only MD5 checksums: the download is checked for corruption over HTTPS, but MD5 is not a strong authenticity guarantee.".into()],
        })
    }
}

impl SoftwareDetector for PurpurProvider {
    fn detect(&self, dir: &Path) -> Option<DetectedSoftware> {
        detect::detect_prefixed(dir, "purpur", "purpur")
    }
}
