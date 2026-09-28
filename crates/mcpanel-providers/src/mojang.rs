//! Vanilla server provider backed by Mojang's version manifest (verified 2026-09-28,
//! see docs/architecture/verification-log.md).

use crate::detect;
use crate::http::{HttpClient, parse_rfc3339};
use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::{ExpectedHash, HashAlgorithm};
use mcpanel_core::software::jar::SingleJarLauncher;
use mcpanel_core::software::{
    DetectedSoftware, GameVersion, GameVersionKind, InstallPlan, InstallRequest, InstallStep,
    JavaRequirement, SoftwareBuild, SoftwareCaps, SoftwareCatalog, SoftwareDescriptor,
    SoftwareDetector, SoftwareInstaller, SoftwareProvider, TpsSource,
};
use mcpanel_core::time::Timestamp;
use serde::Deserialize;
use sha1::Digest;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    versions: Vec<ManifestVersion>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestVersion {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    release_time: String,
    sha1: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionJson {
    java_version: Option<JavaVersion>,
    downloads: Option<Downloads>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JavaVersion {
    major_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct Downloads {
    server: Option<Artifact>,
}

#[derive(Debug, Clone, Deserialize)]
struct Artifact {
    sha1: String,
    size: u64,
    url: String,
}

fn kind_of(s: &str) -> GameVersionKind {
    match s {
        "release" => GameVersionKind::Release,
        "old_beta" => GameVersionKind::OldBeta,
        "old_alpha" => GameVersionKind::OldAlpha,
        _ => GameVersionKind::Snapshot,
    }
}

/// Client for Mojang metadata, shared by providers that need the Java requirement of a
/// game version (e.g. Purpur).
pub struct MojangClient {
    http: HttpClient,
    cache: Mutex<Option<(Instant, Arc<Manifest>)>>,
}

impl MojangClient {
    pub fn new(http: HttpClient) -> Arc<Self> {
        Arc::new(Self {
            http,
            cache: Mutex::new(None),
        })
    }

    async fn manifest(&self) -> CoreResult<Arc<Manifest>> {
        let mut guard = self.cache.lock().await;
        if let Some((at, m)) = guard.as_ref()
            && at.elapsed() < Duration::from_secs(1800)
        {
            return Ok(Arc::clone(m));
        }
        match self.http.get_json::<Manifest>(MANIFEST_URL).await {
            Ok(m) => {
                let m = Arc::new(m);
                *guard = Some((Instant::now(), Arc::clone(&m)));
                Ok(m)
            }
            Err(e) => match guard.as_ref() {
                Some((_, m)) => Ok(Arc::clone(m)),
                None => Err(e),
            },
        }
    }

    async fn version_json(&self, id: &str) -> CoreResult<VersionJson> {
        let manifest = self.manifest().await?;
        let entry = manifest
            .versions
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::VersionNotFound,
                    format!("Minecraft {id} was not found"),
                )
            })?;
        // The manifest pins the SHA-1 of each version document: verify it.
        let bytes = self.http.get_bytes(&entry.url, 4 * 1024 * 1024).await?;
        let actual = hex::encode(sha1::Sha1::digest(&bytes));
        if !actual.eq_ignore_ascii_case(&entry.sha1) {
            return Err(CoreError::new(
                ErrorCode::HashMismatch,
                "Mojang version metadata failed verification",
            ));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("Unexpected Mojang metadata: {e}"),
            )
        })
    }

    /// The vanilla server download of a version: (url, sha1, size, Java major).
    pub async fn server_download(&self, id: &str) -> CoreResult<(String, String, u64, u32)> {
        let v = self.version_json(id).await?;
        let java = v
            .java_version
            .as_ref()
            .map(|j| j.major_version)
            .unwrap_or(8);
        let server = v.downloads.and_then(|d| d.server).ok_or_else(|| {
            CoreError::new(
                ErrorCode::Unsupported,
                format!("Mojang does not provide a server download for {id}"),
            )
        })?;
        Ok((server.url, server.sha1, server.size, java))
    }

    /// Java major version Mojang declares for a game version (defaults to 8 for old
    /// versions without the field).
    pub async fn java_major(&self, id: &str) -> CoreResult<u32> {
        Ok(self
            .version_json(id)
            .await?
            .java_version
            .map(|j| j.major_version)
            .unwrap_or(8))
    }

    pub async fn versions(&self) -> CoreResult<Vec<GameVersion>> {
        let m = self.manifest().await?;
        Ok(m.versions
            .iter()
            .map(|v| GameVersion {
                id: v.id.clone(),
                kind: kind_of(&v.kind),
                release_time: parse_rfc3339(&v.release_time).map(Timestamp),
            })
            .collect())
    }
}

pub struct VanillaProvider {
    mojang: Arc<MojangClient>,
}

impl VanillaProvider {
    pub fn descriptor() -> SoftwareDescriptor {
        SoftwareDescriptor {
            id: "vanilla".into(),
            display_name: "Vanilla".into(),
            description: "The official Minecraft server from Mojang. No plugins or mods.".into(),
            caps: SoftwareCaps {
                content: vec![mcpanel_core::software::ContentEcosystem::Datapacks],
                is_proxy: false,
                log_dialect: "minecraft".into(),
                stop_command: "stop".into(),
                eula_required: true,
                requires_build_step: false,
                tps_source: Some(TpsSource::VanillaTickQuery),
            },
            download_hosts: vec![
                "piston-data.mojang.com".into(),
                "launcher.mojang.com".into(),
            ],
        }
    }

    pub fn provider(mojang: Arc<MojangClient>) -> SoftwareProvider {
        let p = Arc::new(Self { mojang });
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
impl SoftwareCatalog for VanillaProvider {
    async fn game_versions(&self) -> CoreResult<Vec<GameVersion>> {
        self.mojang.versions().await
    }

    async fn builds(&self, _game_version: &str) -> CoreResult<Vec<SoftwareBuild>> {
        Ok(Vec::new())
    }
}

/// The Mojang manifest is also the reference catalog for version ordering.
pub struct MojangCatalog(pub Arc<MojangClient>);

#[async_trait]
impl SoftwareCatalog for MojangCatalog {
    async fn game_versions(&self) -> CoreResult<Vec<GameVersion>> {
        self.0.versions().await
    }
    async fn builds(&self, _game_version: &str) -> CoreResult<Vec<SoftwareBuild>> {
        Ok(Vec::new())
    }
}

fn jar_name(version: &str) -> String {
    let name = format!("minecraft_server.{version}.jar");
    if mcpanel_core::files::safepath::validate_name(&name).is_ok() {
        name
    } else {
        "server.jar".into()
    }
}

#[async_trait]
impl SoftwareInstaller for VanillaProvider {
    async fn plan_install(&self, req: &InstallRequest) -> CoreResult<InstallPlan> {
        let v = self.mojang.version_json(&req.game_version).await?;
        let server = v.downloads.and_then(|d| d.server).ok_or_else(|| {
            CoreError::new(
                ErrorCode::Unsupported,
                format!(
                    "Mojang does not provide a server download for {}",
                    req.game_version
                ),
            )
        })?;
        let java = v.java_version.map(|j| j.major_version).unwrap_or(8);
        let jar = jar_name(&req.game_version);
        Ok(InstallPlan {
            software_id: "vanilla".into(),
            game_version: req.game_version.clone(),
            build: None,
            build_channel: None,
            steps: vec![InstallStep::Download {
                url: server.url,
                dest: jar.clone(),
                expected_hash: Some(ExpectedHash {
                    algorithm: HashAlgorithm::Sha1,
                    hex: server.sha1,
                }),
                size: Some(server.size),
                description: format!("Downloading Minecraft {} server", req.game_version),
            }],
            jar,
            java: JavaRequirement {
                min_major: java,
                recommended_major: Some(java),
                recommended_flags: Vec::new(),
            },
            notes: vec!["Downloaded from Mojang and verified against the SHA-1 published in Mojang's signed-over-HTTPS version manifest.".into()],
        })
    }
}

impl SoftwareDetector for VanillaProvider {
    fn detect(&self, dir: &Path) -> Option<DetectedSoftware> {
        detect::detect_vanilla(dir)
    }
}
