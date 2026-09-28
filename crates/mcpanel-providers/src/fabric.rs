//! Fabric server software (meta API v2, <https://meta.fabricmc.net>).
//!
//! Verified 2026-09-28 against the live services:
//! - `GET /v2/versions/game` → `[{version, stable}]` newest first;
//!   `GET /v2/versions/loader/{game}` → `[{loader{version, stable}, intermediary, …}]`.
//! - `GET /v2/versions/loader/{game}/{loader}/server/json` → a launcher profile with
//!   `mainClass` (`net.fabricmc.loader.impl.launch.knot.KnotServer`) and `libraries`
//!   (`{name, url, sha1?, sha256?, sha512?, size?}`); the loader (and, for obfuscated
//!   versions, intermediary) carry no hash in the profile but Fabric's Maven publishes
//!   `<artifact>.jar.sha512`. 26.x profiles have no intermediary (unobfuscated game).
//! - The official installer downloads libraries without verifying hashes, and the meta
//!   "server launcher" jar has no published hash. MCPanel therefore installs Fabric
//!   itself: the vanilla server jar from Mojang (SHA-1 from the version manifest), every
//!   library with its SHA-512, and a manifest-only launch jar like the installer's
//!   (`Main-Class` = the profile's main class, `Class-Path` = the libraries only). The
//!   game jar is passed as `-Dfabric.gameJarPath=server.jar` (what Fabric's
//!   `FabricServerLauncher` sets); Fabric Loader unpacks the 1.18+ bundler itself.

use crate::http::HttpClient;
use crate::mojang::MojangClient;
use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::model::{InstalledSoftware, LaunchConfig};
use mcpanel_core::ports::{ExpectedHash, HashAlgorithm};
use mcpanel_core::software::jar::{jar_has_entry_prefix, read_jar_entry};
use mcpanel_core::software::{
    ContentEcosystem, DetectedSoftware, GameVersion, GameVersionKind, InstallPlan, InstallRequest,
    InstallStep, JavaRequirement, ReleaseChannel, SoftwareBuild, SoftwareCaps, SoftwareCatalog,
    SoftwareDescriptor, SoftwareDetector, SoftwareInstaller, SoftwareProvider,
};
use mcpanel_core::software::{LaunchArgs, LaunchResolver};
use serde::Deserialize;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

const META: &str = "https://meta.fabricmc.net/v2";
const MAVEN_HOST: &str = "maven.fabricmc.net";
pub const LAUNCH_JAR: &str = "fabric-server-launch.jar";
const SERVER_JAR: &str = "server.jar";

#[derive(Deserialize)]
struct MetaGame {
    version: String,
    stable: bool,
}

#[derive(Deserialize)]
struct LoaderEntry {
    loader: LoaderInfo,
}

#[derive(Deserialize)]
struct LoaderInfo {
    version: String,
    stable: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    main_class: String,
    libraries: Vec<Library>,
}

#[derive(Deserialize)]
struct Library {
    name: String,
    url: String,
    sha512: Option<String>,
    size: Option<u64>,
}

pub struct FabricProvider {
    http: HttpClient,
    mojang: Arc<MojangClient>,
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

/// `group:artifact:version` → `group/path/artifact/version/artifact-version.jar`.
pub fn maven_path(name: &str) -> CoreResult<String> {
    let parts: Vec<&str> = name.split(':').collect();
    let [group, artifact, version] = parts.as_slice() else {
        return Err(CoreError::new(
            ErrorCode::ProviderError,
            format!("Unexpected library name '{name}'"),
        ));
    };
    for p in [group, artifact, version] {
        if p.is_empty()
            || !p
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
            || p.contains("..")
        {
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("Unexpected library name '{name}'"),
            ));
        }
    }
    Ok(format!(
        "{}/{artifact}/{version}/{artifact}-{version}.jar",
        group.replace('.', "/")
    ))
}

/// A jar containing only a manifest, like the one Fabric's installer writes.
pub fn launch_jar(main_class: &str, class_path: &[String]) -> CoreResult<Vec<u8>> {
    let mut manifest = String::from("Manifest-Version: 1.0\r\n");
    manifest.push_str(&wrap_header("Main-Class", main_class));
    let cp: Vec<String> = class_path.iter().map(|p| p.replace(' ', "%20")).collect();
    manifest.push_str(&wrap_header("Class-Path", &cp.join(" ")));
    manifest.push_str("\r\n");
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(&mut buf);
    let opts = zip::write::SimpleFileOptions::default();
    let zip_err =
        |e: zip::result::ZipError| CoreError::internal(format!("Cannot write the launch jar: {e}"));
    w.start_file("META-INF/MANIFEST.MF", opts)
        .map_err(zip_err)?;
    w.write_all(manifest.as_bytes())
        .map_err(|e| CoreError::internal(e.to_string()))?;
    w.finish().map_err(zip_err)?;
    Ok(buf.into_inner())
}

/// Manifest header lines are at most 72 bytes; continuation lines start with a space.
fn wrap_header(name: &str, value: &str) -> String {
    let line = format!("{name}: {value}");
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut start = 0;
    let mut first = true;
    while start < bytes.len() {
        let width = if first { 72 } else { 71 };
        let mut end = (start + width).min(bytes.len());
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        if !first {
            out.push(' ');
        }
        out.push_str(&line[start..end]);
        out.push_str("\r\n");
        start = end;
        first = false;
    }
    out
}

impl FabricProvider {
    pub fn descriptor() -> SoftwareDescriptor {
        SoftwareDescriptor {
            id: "fabric".into(),
            display_name: "Fabric".into(),
            description:
                "Lightweight mod loader; supports Fabric mods (install Fabric API for most mods)."
                    .into(),
            caps: SoftwareCaps {
                content: vec![ContentEcosystem::FabricMods, ContentEcosystem::Datapacks],
                is_proxy: false,
                log_dialect: "minecraft".into(),
                stop_command: "stop".into(),
                eula_required: true,
                requires_build_step: false,
                tps_source: None,
            },
            download_hosts: vec![
                MAVEN_HOST.into(),
                "piston-data.mojang.com".into(),
                "launcher.mojang.com".into(),
            ],
        }
    }

    pub fn provider(http: HttpClient, mojang: Arc<MojangClient>) -> SoftwareProvider {
        let p = Arc::new(Self { http, mojang });
        SoftwareProvider {
            descriptor: Self::descriptor(),
            catalog: p.clone(),
            installer: p.clone(),
            launcher: Arc::new(FabricLauncher),
            detector: Some(p),
        }
    }

    async fn loaders(&self, game_version: &str) -> CoreResult<Vec<LoaderInfo>> {
        check(game_version)?;
        let entries: Vec<LoaderEntry> = self
            .http
            .get_json(&format!("{META}/versions/loader/{game_version}"))
            .await
            .map_err(|e| {
                if e.code == ErrorCode::VersionNotFound {
                    CoreError::new(
                        ErrorCode::VersionNotFound,
                        format!("Fabric does not support Minecraft {game_version}"),
                    )
                } else {
                    e
                }
            })?;
        Ok(entries.into_iter().map(|e| e.loader).collect())
    }

    /// SHA-512 of a library: from the profile, else Fabric's Maven checksum file.
    async fn library_sha512(&self, lib: &Library, url: &str) -> CoreResult<String> {
        if let Some(h) = &lib.sha512 {
            return Ok(h.to_lowercase());
        }
        let text = self.http.get_bytes(&format!("{url}.sha512"), 1024).await?;
        let hex = String::from_utf8_lossy(&text)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        if hex.len() != 128 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("Fabric's Maven has no valid SHA-512 for {}", lib.name),
            ));
        }
        Ok(hex)
    }
}

#[async_trait]
impl SoftwareCatalog for FabricProvider {
    async fn game_versions(&self) -> CoreResult<Vec<GameVersion>> {
        let v: Vec<MetaGame> = self.http.get_json(&format!("{META}/versions/game")).await?;
        Ok(v.into_iter()
            .map(|g| GameVersion {
                kind: if g.stable {
                    GameVersionKind::Release
                } else {
                    GameVersionKind::Snapshot
                },
                id: g.version,
                release_time: None,
            })
            .collect())
    }

    async fn builds(&self, game_version: &str) -> CoreResult<Vec<SoftwareBuild>> {
        Ok(self
            .loaders(game_version)
            .await?
            .into_iter()
            .map(|l| SoftwareBuild {
                id: l.version,
                channel: if l.stable {
                    ReleaseChannel::Stable
                } else {
                    ReleaseChannel::Beta
                },
                published_at: None,
            })
            .collect())
    }
}

#[async_trait]
impl SoftwareInstaller for FabricProvider {
    async fn plan_install(&self, req: &InstallRequest) -> CoreResult<InstallPlan> {
        check(&req.game_version)?;
        let loaders = self.loaders(&req.game_version).await?;
        let loader = match &req.build {
            Some(b) => {
                check(b)?;
                loaders.iter().find(|l| &l.version == b).ok_or_else(|| {
                    CoreError::new(
                        ErrorCode::VersionNotFound,
                        format!("Fabric Loader {b} was not found"),
                    )
                })?
            }
            None => loaders
                .iter()
                .find(|l| l.stable)
                .or_else(|| loaders.first())
                .ok_or_else(|| {
                    CoreError::new(ErrorCode::VersionNotFound, "No Fabric Loader is available")
                })?,
        };
        let profile: Profile = self
            .http
            .get_json(&format!(
                "{META}/versions/loader/{}/{}/server/json",
                req.game_version, loader.version
            ))
            .await?;
        if !profile
            .main_class
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '$'))
        {
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                "Unexpected Fabric main class",
            ));
        }

        let (vanilla_url, vanilla_sha1, vanilla_size, java) =
            self.mojang.server_download(&req.game_version).await?;
        let mut steps = vec![InstallStep::Download {
            url: vanilla_url,
            dest: SERVER_JAR.into(),
            expected_hash: Some(ExpectedHash {
                algorithm: HashAlgorithm::Sha1,
                hex: vanilla_sha1,
            }),
            size: Some(vanilla_size),
            description: format!("Downloading Minecraft {} server", req.game_version),
        }];
        let mut class_path = Vec::new();
        for lib in &profile.libraries {
            let rel = maven_path(&lib.name)?;
            let base = lib.url.trim_end_matches('/');
            if base != format!("https://{MAVEN_HOST}") {
                return Err(CoreError::new(
                    ErrorCode::ProviderError,
                    format!("Library {} comes from an unexpected repository", lib.name),
                ));
            }
            let url = format!("{base}/{rel}");
            let sha512 = self.library_sha512(lib, &url).await?;
            let dest = format!("libraries/{rel}");
            steps.push(InstallStep::Download {
                url,
                dest: dest.clone(),
                expected_hash: Some(ExpectedHash {
                    algorithm: HashAlgorithm::Sha512,
                    hex: sha512,
                }),
                size: lib.size,
                description: format!("Downloading {}", lib.name),
            });
            class_path.push(dest);
        }
        // Libraries only: the game jar is passed as `fabric.gameJarPath` (see
        // `FabricLauncher`); on the system class path it would confuse Fabric Loader's
        // class-loader separation (verified: "trying to load … from target class loader").
        steps.push(InstallStep::WriteFile {
            dest: LAUNCH_JAR.into(),
            contents: launch_jar(&profile.main_class, &class_path)?,
            description: "Writing the Fabric launch jar".into(),
        });
        Ok(InstallPlan {
            software_id: "fabric".into(),
            game_version: req.game_version.clone(),
            build: Some(loader.version.clone()),
            build_channel: Some(if loader.stable {
                ReleaseChannel::Stable
            } else {
                ReleaseChannel::Beta
            }),
            steps,
            jar: LAUNCH_JAR.into(),
            java: JavaRequirement {
                min_major: java,
                recommended_major: Some(java),
                recommended_flags: Vec::new(),
            },
            notes: vec![format!(
                "Fabric Loader {}: the Minecraft server comes from Mojang (SHA-1 verified) and every Fabric library is verified with SHA-512. Most mods also need Fabric API from the Mods tab.",
                loader.version
            )],
        })
    }
}

/// `java -Dfabric.gameJarPath=server.jar … -cp <libraries> <main class> nogui`.
///
/// Fabric's Knot builds its class path from `java.class.path` only; with `-jar` that is
/// just the launch jar and the loader refuses to start ("trying to load
/// FabricLoaderImpl from target class loader" — verified with loader 0.19.5). The
/// libraries are therefore passed with `-cp`, read from the launch jar's manifest
/// (written at install time), and the game jar via `fabric.gameJarPath`.
pub struct FabricLauncher;

/// (main class, class path) from a manifest (continuation lines joined).
pub fn read_manifest(text: &str) -> Option<(String, Vec<String>)> {
    let joined = text.replace("\r\n ", "").replace("\n ", "");
    let get = |key: &str| {
        joined
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key}: ")))
            .map(|v| v.trim().to_string())
    };
    let main = get("Main-Class")?;
    let cp = get("Class-Path")
        .map(|v| {
            v.split(' ')
                .filter(|p| !p.is_empty())
                .map(|p| p.replace("%20", " "))
                .collect()
        })
        .unwrap_or_default();
    Some((main, cp))
}

impl LaunchResolver for FabricLauncher {
    fn launch_args(
        &self,
        root: &Path,
        installed: &InstalledSoftware,
        _launch: &LaunchConfig,
    ) -> CoreResult<LaunchArgs> {
        mcpanel_core::files::safepath::parse_relative(&installed.jar)
            .map_err(|_| CoreError::invalid("Invalid server jar path"))?;
        let manifest = read_jar_entry(
            &root.join(&installed.jar),
            "META-INF/MANIFEST.MF",
            256 * 1024,
        )
        .map(|b| String::from_utf8_lossy(&b).to_string())
        .ok_or_else(|| {
            CoreError::new(
                ErrorCode::PathNotFound,
                format!(
                    "{} is missing or damaged; create or import the Fabric server again",
                    installed.jar
                ),
            )
        })?;
        let (main, class_path) = read_manifest(&manifest).ok_or_else(|| {
            CoreError::new(
                ErrorCode::ProviderError,
                "The Fabric launch jar has no Main-Class",
            )
        })?;
        if !main
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '$'))
        {
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                "Unexpected Fabric main class",
            ));
        }
        for entry in &class_path {
            mcpanel_core::files::safepath::parse_relative(entry).map_err(|_| {
                CoreError::new(
                    ErrorCode::PathRejected,
                    "Unsafe entry in the Fabric class path",
                )
            })?;
        }
        Ok(LaunchArgs {
            jvm_args: vec![format!("-Dfabric.gameJarPath={SERVER_JAR}")],
            jar: installed.jar.clone(),
            main_class: Some(main),
            class_path,
            server_args: vec!["nogui".into()],
        })
    }
}

impl SoftwareDetector for FabricProvider {
    fn detect(&self, dir: &Path) -> Option<DetectedSoftware> {
        let jar = dir.join(LAUNCH_JAR);
        let md = std::fs::symlink_metadata(&jar).ok()?;
        if !md.is_file() {
            return None;
        }
        let manifest = read_jar_entry(&jar, "META-INF/MANIFEST.MF", 64 * 1024)
            .map(|b| String::from_utf8_lossy(&b).to_string());
        let fabric = manifest
            .as_deref()
            .is_some_and(|m| m.contains("net.fabricmc"))
            || jar_has_entry_prefix(&jar, "net/fabricmc/");
        fabric.then(|| DetectedSoftware {
            software_id: "fabric".into(),
            game_version: None,
            build: None,
            jar: LAUNCH_JAR.into(),
            confidence: 90,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn maven_paths() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.19.5").unwrap(),
            "net/fabricmc/fabric-loader/0.19.5/fabric-loader-0.19.5.jar"
        );
        assert_eq!(
            maven_path("net.fabricmc:sponge-mixin:0.17.4+mixin.0.8.7").unwrap(),
            "net/fabricmc/sponge-mixin/0.17.4+mixin.0.8.7/sponge-mixin-0.17.4+mixin.0.8.7.jar"
        );
        for bad in ["a:b", "a:b:c:d", "a:..:c", "a/b:c:d", "a:b:c d"] {
            assert!(maven_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn launch_jar_manifest_is_wrapped_correctly() {
        let cp: Vec<String> = (0..12)
            .map(|i| format!("libraries/org/ow2/asm/asm-commons/9.10.1/asm-commons-9.10.1-{i}.jar"))
            .chain(std::iter::once("server.jar".to_string()))
            .collect();
        let bytes = launch_jar("net.fabricmc.loader.impl.launch.knot.KnotServer", &cp).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        zip.by_name("META-INF/MANIFEST.MF")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.lines().all(|l| l.len() <= 72), "{text}");
        // Unwrap continuation lines and read the class path back.
        let unwrapped = text.replace("\r\n ", "");
        let cp_line = unwrapped
            .lines()
            .find(|l| l.starts_with("Class-Path: "))
            .unwrap();
        let entries: Vec<&str> = cp_line["Class-Path: ".len()..].split(' ').collect();
        assert_eq!(entries.len(), 13);
        assert!(unwrapped.contains("Main-Class: net.fabricmc.loader.impl.launch.knot.KnotServer"));
        let (main, parsed) = read_manifest(&text).unwrap();
        assert_eq!(main, "net.fabricmc.loader.impl.launch.knot.KnotServer");
        assert_eq!(parsed, cp);
    }
}
