//! Forge and NeoForge (official installers from their Maven repositories).
//!
//! Verified 2026-09-28 against the live services and by running the installers:
//! - NeoForge: `maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml`;
//!   versions `21.4.157` = Minecraft 1.21.4, year-based `26.3.0.31-beta` = 26.3;
//!   `neoforge-<v>-installer.jar(.sha512)`; `--install-server <dir>` (hash checks on by
//!   default) writes `libraries/net/neoforged/neoforge/<v>/win_args.txt`, launched as
//!   `java @libraries/…/win_args.txt nogui` (~150 s for 21.4.157 incl. downloads).
//! - Forge: `files.minecraftforge.net/…/promotions_slim.json` (`<mc>-recommended`,
//!   `<mc>-latest`), `maven.minecraftforge.net/…/maven-metadata.xml` (`<mc>-<forge>`),
//!   `forge-<mc>-<forge>-installer.jar(.sha512)`; `--installServer`. 1.17+ writes
//!   `libraries/net/minecraftforge/forge/<mc>-<forge>/win_args.txt`; 1.16.5 and 1.12.2
//!   write `forge-<mc>-<forge>.jar` (+ the vanilla jar) in the server root, launched
//!   with `-jar`. The installers ran fine on JDK 25 and validate their downloads.
//!
//! MCPanel downloads the installer itself (SHA-512 from Maven), runs it with the
//! server's Java runtime in the server folder, then removes the installer and its log.

use crate::detect;
use crate::http::HttpClient;
use crate::mojang::MojangClient;
use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::model::{InstalledSoftware, LaunchConfig};
use mcpanel_core::ports::{ExpectedHash, HashAlgorithm};
use mcpanel_core::software::{
    ContentEcosystem, DetectedSoftware, GameVersion, GameVersionKind, InstallPlan, InstallRequest,
    InstallStep, JavaRequirement, LaunchArgs, LaunchResolver, ReleaseChannel, SoftwareBuild,
    SoftwareCaps, SoftwareCatalog, SoftwareDescriptor, SoftwareDetector, SoftwareInstaller,
    SoftwareProvider, TpsSource,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const INSTALLER: &str = "installer.jar";
const INSTALL_TIMEOUT_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Forge,
    NeoForge,
}

impl Flavor {
    fn id(self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
        }
    }
    fn maven(self) -> &'static str {
        match self {
            Self::Forge => "https://maven.minecraftforge.net/net/minecraftforge/forge",
            Self::NeoForge => "https://maven.neoforged.net/releases/net/neoforged/neoforge",
        }
    }
    fn host(self) -> &'static str {
        match self {
            Self::Forge => "maven.minecraftforge.net",
            Self::NeoForge => "maven.neoforged.net",
        }
    }
}

fn check(s: &str) -> CoreResult<()> {
    if s.is_empty()
        || s.len() > 64
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(CoreError::invalid("Invalid version"));
    }
    Ok(())
}

/// Minecraft version of a NeoForge version (`21.4.157` → `1.21.4`, `21.0.5` → `1.21`,
/// `26.3.0.31-beta` → `26.3`, `26.1.2.5` → `26.1.2`).
pub fn neoforge_mc_version(v: &str) -> Option<String> {
    let base = v.split('-').next()?;
    let parts: Vec<&str> = base.split('.').collect();
    let major: u32 = parts.first()?.parse().ok()?;
    if major >= 26 {
        // Year-based: <year>.<drop>.<patch>.<build>
        let (drop, patch) = (parts.get(1)?, parts.get(2)?);
        parts.get(3)?;
        Some(if *patch == "0" {
            format!("{major}.{drop}")
        } else {
            format!("{major}.{drop}.{patch}")
        })
    } else {
        let minor = parts.get(1)?;
        parts.get(2)?;
        Some(if *minor == "0" {
            format!("1.{major}")
        } else {
            format!("1.{major}.{minor}")
        })
    }
}

/// Whether a Forge/NeoForge install for `mc` uses the 1.17+ argument-file layout.
pub fn uses_arg_files(mc: &str) -> bool {
    let mut it = mc.split('.');
    match (
        it.next().and_then(|a| a.parse::<u32>().ok()),
        it.next().and_then(|b| b.parse::<u32>().ok()),
    ) {
        (Some(1), Some(minor)) => minor >= 17,
        (Some(year), _) => year >= 26,
        _ => true,
    }
}

fn versions_from_metadata(xml: &str) -> Vec<String> {
    xml.split("<version>")
        .skip(1)
        .filter_map(|s| s.split("</version>").next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(String::from)
        .collect()
}

#[derive(Deserialize)]
struct Promotions {
    promos: HashMap<String, String>,
}

struct Cache {
    at: Instant,
    /// Minecraft version → loader versions (newest first).
    by_mc: BTreeMap<String, Vec<String>>,
    /// Minecraft version → recommended build (Forge).
    recommended: HashMap<String, String>,
}

pub struct ForgeProvider {
    flavor: Flavor,
    http: HttpClient,
    mojang: Arc<MojangClient>,
    cache: Mutex<Option<Arc<Cache>>>,
}

impl ForgeProvider {
    fn descriptor(flavor: Flavor) -> SoftwareDescriptor {
        SoftwareDescriptor {
            id: flavor.id().into(),
            display_name: flavor.name().into(),
            description: match flavor {
                Flavor::Forge => "The original mod loader; supports Forge mods.",
                Flavor::NeoForge => {
                    "Community fork of Forge for Minecraft 1.20.2+; supports NeoForge mods."
                }
            }
            .into(),
            caps: SoftwareCaps {
                content: vec![
                    match flavor {
                        Flavor::Forge => ContentEcosystem::ForgeMods,
                        Flavor::NeoForge => ContentEcosystem::NeoForgeMods,
                    },
                    ContentEcosystem::Datapacks,
                ],
                is_proxy: false,
                log_dialect: "minecraft".into(),
                stop_command: "stop".into(),
                eula_required: true,
                requires_build_step: false,
                tps_source: Some(TpsSource::VanillaTickQuery),
            },
            download_hosts: vec![flavor.host().into()],
        }
    }

    fn provider(flavor: Flavor, http: HttpClient, mojang: Arc<MojangClient>) -> SoftwareProvider {
        let p = Arc::new(Self {
            flavor,
            http,
            mojang,
            cache: Mutex::new(None),
        });
        SoftwareProvider {
            descriptor: Self::descriptor(flavor),
            catalog: p.clone(),
            installer: p.clone(),
            launcher: Arc::new(ForgeLauncher),
            detector: Some(p),
        }
    }

    pub fn forge(http: HttpClient, mojang: Arc<MojangClient>) -> SoftwareProvider {
        Self::provider(Flavor::Forge, http, mojang)
    }

    pub fn neoforge(http: HttpClient, mojang: Arc<MojangClient>) -> SoftwareProvider {
        Self::provider(Flavor::NeoForge, http, mojang)
    }

    async fn load(&self) -> CoreResult<Arc<Cache>> {
        let mut guard = self.cache.lock().await;
        if let Some(c) = guard.as_ref()
            && c.at.elapsed() < Duration::from_secs(1800)
        {
            return Ok(Arc::clone(c));
        }
        let xml = self
            .http
            .get_bytes(
                &format!("{}/maven-metadata.xml", self.flavor.maven()),
                4 * 1024 * 1024,
            )
            .await?;
        let versions = versions_from_metadata(&String::from_utf8_lossy(&xml));
        let mut by_mc: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for v in versions {
            let mc = match self.flavor {
                Flavor::NeoForge => neoforge_mc_version(&v),
                Flavor::Forge => v.split_once('-').map(|(mc, _)| mc.to_string()),
            };
            if let Some(mc) = mc {
                by_mc.entry(mc).or_default().push(v);
            }
        }
        for list in by_mc.values_mut() {
            list.reverse(); // metadata lists oldest first
        }
        let recommended = match self.flavor {
            Flavor::NeoForge => HashMap::new(),
            Flavor::Forge => {
                let p: Promotions = self
                    .http
                    .get_json("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
                    .await?;
                p.promos
                    .into_iter()
                    .filter_map(|(k, v)| {
                        k.strip_suffix("-recommended")
                            .map(|mc| (mc.to_string(), format!("{mc}-{v}")))
                    })
                    .collect()
            }
        };
        let c = Arc::new(Cache {
            at: Instant::now(),
            by_mc,
            recommended,
        });
        *guard = Some(Arc::clone(&c));
        Ok(c)
    }

    /// The installer's SHA-512 from Maven's checksum file.
    async fn installer_sha512(&self, url: &str) -> CoreResult<String> {
        let text = self.http.get_bytes(&format!("{url}.sha512"), 1024).await?;
        let hex = String::from_utf8_lossy(&text)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        if hex.len() != 128 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!(
                    "{} has no valid SHA-512 for the installer",
                    self.flavor.name()
                ),
            ));
        }
        Ok(hex)
    }
}

fn is_beta(v: &str) -> bool {
    v.contains("-beta") || v.contains("-alpha")
}

#[async_trait]
impl SoftwareCatalog for ForgeProvider {
    async fn game_versions(&self) -> CoreResult<Vec<GameVersion>> {
        let c = self.load().await?;
        // Order by the Mojang manifest (versions are opaque ids).
        let mojang = self.mojang.versions().await?;
        Ok(mojang
            .into_iter()
            .filter(|g| c.by_mc.contains_key(&g.id))
            .map(|g| GameVersion {
                kind: g.kind,
                id: g.id,
                release_time: g.release_time,
            })
            .filter(|g| g.kind == GameVersionKind::Release || g.kind == GameVersionKind::Snapshot)
            .collect())
    }

    async fn builds(&self, game_version: &str) -> CoreResult<Vec<SoftwareBuild>> {
        check(game_version)?;
        let c = self.load().await?;
        let list = c.by_mc.get(game_version).cloned().unwrap_or_default();
        Ok(list
            .into_iter()
            .map(|v| SoftwareBuild {
                channel: if is_beta(&v) {
                    ReleaseChannel::Beta
                } else {
                    ReleaseChannel::Stable
                },
                id: v,
                published_at: None,
            })
            .collect())
    }
}

#[async_trait]
impl SoftwareInstaller for ForgeProvider {
    async fn plan_install(&self, req: &InstallRequest) -> CoreResult<InstallPlan> {
        check(&req.game_version)?;
        let c = self.load().await?;
        let list = c.by_mc.get(&req.game_version).ok_or_else(|| {
            CoreError::new(
                ErrorCode::VersionNotFound,
                format!(
                    "{} is not available for Minecraft {}",
                    self.flavor.name(),
                    req.game_version
                ),
            )
        })?;
        let version = match &req.build {
            Some(b) => {
                check(b)?;
                list.iter().find(|v| *v == b).cloned().ok_or_else(|| {
                    CoreError::new(
                        ErrorCode::VersionNotFound,
                        format!("{} {b} was not found", self.flavor.name()),
                    )
                })?
            }
            None => c
                .recommended
                .get(&req.game_version)
                .filter(|r| list.contains(r))
                .cloned()
                .or_else(|| list.iter().find(|v| !is_beta(v)).cloned())
                .or_else(|| list.first().cloned())
                .ok_or_else(|| {
                    CoreError::new(ErrorCode::VersionNotFound, "No build is available")
                })?,
        };
        let artifact = match self.flavor {
            Flavor::Forge => "forge",
            Flavor::NeoForge => "neoforge",
        };
        let url = format!(
            "{}/{version}/{artifact}-{version}-installer.jar",
            self.flavor.maven()
        );
        let sha512 = self.installer_sha512(&url).await?;
        let java = self.mojang.java_major(&req.game_version).await.unwrap_or(8);
        let jar = if uses_arg_files(&req.game_version) {
            match self.flavor {
                Flavor::Forge => {
                    format!("libraries/net/minecraftforge/forge/{version}/win_args.txt")
                }
                Flavor::NeoForge => {
                    format!("libraries/net/neoforged/neoforge/{version}/win_args.txt")
                }
            }
        } else {
            format!("forge-{version}.jar")
        };
        let install_arg = match self.flavor {
            Flavor::Forge => "--installServer",
            Flavor::NeoForge => "--install-server",
        };
        let short = version
            .strip_prefix(&format!("{}-", req.game_version))
            .unwrap_or(&version)
            .to_string();
        Ok(InstallPlan {
            software_id: self.flavor.id().into(),
            game_version: req.game_version.clone(),
            build: Some(short),
            build_channel: Some(if is_beta(&version) {
                ReleaseChannel::Beta
            } else {
                ReleaseChannel::Stable
            }),
            steps: vec![
                InstallStep::Download {
                    url,
                    dest: INSTALLER.into(),
                    expected_hash: Some(ExpectedHash {
                        algorithm: HashAlgorithm::Sha512,
                        hex: sha512,
                    }),
                    size: None,
                    description: format!(
                        "Downloading the {} {version} installer",
                        self.flavor.name()
                    ),
                },
                InstallStep::RunJava {
                    jar: INSTALLER.into(),
                    args: vec![install_arg.into(), ".".into()],
                    timeout_secs: INSTALL_TIMEOUT_SECS,
                    remove_after: vec![INSTALLER.into(), format!("{INSTALLER}.log")],
                    description: format!(
                        "Running the {} installer (downloads Minecraft and libraries)",
                        self.flavor.name()
                    ),
                },
            ],
            jar,
            java: JavaRequirement {
                min_major: java,
                recommended_major: Some(java),
                recommended_flags: Vec::new(),
            },
            notes: vec![format!(
                "The official {} installer is verified with SHA-512, then run with the selected Java; it downloads Minecraft and its libraries and validates their checksums. This takes a few minutes.",
                self.flavor.name()
            )],
        })
    }
}

/// Forge/NeoForge 1.17+: `java … @libraries/…/win_args.txt nogui`; older Forge: `-jar`.
pub struct ForgeLauncher;

impl LaunchResolver for ForgeLauncher {
    fn launch_args(
        &self,
        _root: &Path,
        installed: &InstalledSoftware,
        _launch: &LaunchConfig,
    ) -> CoreResult<LaunchArgs> {
        mcpanel_core::files::safepath::parse_relative(&installed.jar)
            .map_err(|_| CoreError::invalid("Invalid server jar path"))?;
        let args_file = installed.jar.to_lowercase().ends_with(".txt");
        Ok(LaunchArgs {
            jvm_args: Vec::new(),
            jar: installed.jar.clone(),
            main_class: None,
            class_path: Vec::new(),
            arg_files: if args_file {
                vec![installed.jar.clone()]
            } else {
                Vec::new()
            },
            server_args: vec!["nogui".into()],
        })
    }
}

impl SoftwareDetector for ForgeProvider {
    fn detect(&self, dir: &Path) -> Option<DetectedSoftware> {
        let (lib, id) = match self.flavor {
            Flavor::Forge => (dir.join("libraries/net/minecraftforge/forge"), "forge"),
            Flavor::NeoForge => (dir.join("libraries/net/neoforged/neoforge"), "neoforge"),
        };
        // 1.17+: the newest version folder with an argument file.
        if let Ok(rd) = std::fs::read_dir(&lib) {
            let mut found: Vec<String> = rd
                .flatten()
                .filter(|e| e.path().join("win_args.txt").is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            found.sort();
            if let Some(v) = found.pop() {
                let prefix = if self.flavor == Flavor::Forge {
                    "net/minecraftforge/forge"
                } else {
                    "net/neoforged/neoforge"
                };
                let game_version = match self.flavor {
                    Flavor::Forge => v.split_once('-').map(|(mc, _)| mc.to_string()),
                    Flavor::NeoForge => neoforge_mc_version(&v),
                };
                return Some(DetectedSoftware {
                    software_id: id.into(),
                    game_version,
                    build: Some(v.clone()),
                    jar: format!("libraries/{prefix}/{v}/win_args.txt"),
                    confidence: 95,
                });
            }
        }
        if self.flavor == Flavor::Forge {
            return detect::detect_prefixed(dir, "forge-", "forge");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_versions_map_to_minecraft() {
        assert_eq!(neoforge_mc_version("21.4.157").as_deref(), Some("1.21.4"));
        assert_eq!(neoforge_mc_version("21.0.5-beta").as_deref(), Some("1.21"));
        assert_eq!(neoforge_mc_version("20.2.86").as_deref(), Some("1.20.2"));
        assert_eq!(
            neoforge_mc_version("26.3.0.31-beta").as_deref(),
            Some("26.3")
        );
        assert_eq!(neoforge_mc_version("26.1.2.5").as_deref(), Some("26.1.2"));
        assert_eq!(neoforge_mc_version("garbage"), None);
        assert_eq!(neoforge_mc_version("21.4"), None);
    }

    #[test]
    fn launch_layout_by_version() {
        assert!(!uses_arg_files("1.12.2"));
        assert!(!uses_arg_files("1.16.5"));
        assert!(uses_arg_files("1.17.1"));
        assert!(uses_arg_files("1.20.1"));
        assert!(uses_arg_files("26.3"));
    }

    #[test]
    fn metadata_versions() {
        let xml = "<metadata><versioning><versions><version>1.0</version>\n<version> 2.0 </version></versions></versioning></metadata>";
        assert_eq!(versions_from_metadata(xml), vec!["1.0", "2.0"]);
    }

    #[test]
    fn launcher_uses_the_argument_file() {
        let installed = InstalledSoftware {
            software_id: "neoforge".into(),
            game_version: "1.21.4".into(),
            build: None,
            jar: "libraries/net/neoforged/neoforge/21.4.157/win_args.txt".into(),
            java_min_major: None,
            java_recommended_major: None,
        };
        let launch = LaunchConfig {
            java_runtime_id: None,
            min_memory_mb: 0,
            max_memory_mb: 1024,
            jvm_args: vec![],
            server_args: vec![],
            stop_timeout_secs: 60,
        };
        let a = ForgeLauncher
            .launch_args(Path::new("."), &installed, &launch)
            .unwrap();
        assert_eq!(a.arg_files, vec![installed.jar.clone()]);
        let old = InstalledSoftware {
            jar: "forge-1.16.5-36.2.34.jar".into(),
            ..installed
        };
        assert!(
            ForgeLauncher
                .launch_args(Path::new("."), &old, &launch)
                .unwrap()
                .arg_files
                .is_empty()
        );
    }
}
