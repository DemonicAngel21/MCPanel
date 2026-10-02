//! Heuristic and descriptor-based identification of imported plugins and mods.
//!
//! Inspects jar files, parses descriptors (`plugin.yml`, `fabric.mod.json`, `mods.toml`),
//! analyzes file names, versions, and loaders, and resolves them to known plugins
//! and providers (Modrinth, Hangar, SpigotMC) with confidence ratings.

use super::InstalledContent;
use super::descriptor::Descriptor;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionConfidence {
    /// 100%: matched by exact package / verified record / verified SHA-512.
    Exact,
    /// 80-95%: matched internal descriptor main class, canonical ID, or metadata.
    High,
    /// 60-79%: matched descriptor plugin name or clean recognizable filename.
    Medium,
    /// 30-59%: inferred from filename pattern alone without descriptor verification.
    Low,
    /// <30%: ambiguous, obfuscated, custom or unidentifiable jar.
    Unknown,
}

impl DetectionConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedPlugin {
    pub file_name: String,
    pub name: String,
    pub version: Option<String>,
    pub provider: Option<String>,
    pub project_id: Option<String>,
    pub platform: Option<String>,
    pub confidence: DetectionConfidence,
    pub description: Option<String>,
}

struct CanonicalPlugin {
    canonical_name: &'static str,
    provider: &'static str,
    project_id: &'static str,
    platform: &'static str,
    main_classes: &'static [&'static str],
    ids: &'static [&'static str],
    description: &'static str,
}

const CANONICAL_PLUGINS: &[CanonicalPlugin] = &[
    CanonicalPlugin {
        canonical_name: "EssentialsX",
        provider: "modrinth",
        project_id: "essentialsx",
        platform: "paper",
        main_classes: &[
            "com.earth2me.essentials.Essentials",
            "net.ess3.Essentials",
            "com.earth2me.essentials.spawn.EssentialsSpawn",
            "com.earth2me.essentials.chat.EssentialsChat",
        ],
        ids: &["essentialsx", "essentials", "essentials-x"],
        description: "Essential commands, economy, player kits, homes, and moderation suite.",
    },
    CanonicalPlugin {
        canonical_name: "LuckPerms",
        provider: "modrinth",
        project_id: "luckperms",
        platform: "bukkit",
        main_classes: &[
            "me.lucko.luckperms.bukkit.loader.BukkitLoaderPlugin",
            "me.lucko.luckperms.fabric.loader.FabricLoaderPlugin",
            "me.lucko.luckperms.sponge.loader.SpongeLoaderPlugin",
        ],
        ids: &["luckperms", "luck-perms"],
        description: "Fast, advanced permissions management plugin with web-based editor.",
    },
    CanonicalPlugin {
        canonical_name: "Geyser",
        provider: "modrinth",
        project_id: "geyser",
        platform: "bukkit",
        main_classes: &[
            "org.geysermc.geyser.platform.spigot.GeyserSpigotPlugin",
            "org.geysermc.geyser.platform.fabric.GeyserFabricMod",
            "org.geysermc.geyser.platform.sponge.GeyserSpongePlugin",
        ],
        ids: &["geyser", "geyser-spigot", "geysermc", "geyser-fabric"],
        description: "Enables Bedrock Edition clients to connect to Java Edition servers.",
    },
    CanonicalPlugin {
        canonical_name: "Floodgate",
        provider: "modrinth",
        project_id: "floodgate",
        platform: "bukkit",
        main_classes: &[
            "org.geysermc.floodgate.spigot.FloodgateSpigot",
            "org.geysermc.floodgate.fabric.FloodgateFabric",
        ],
        ids: &["floodgate", "floodgate-spigot", "floodgate-fabric"],
        description: "Allows Bedrock players to join without owning Java Edition accounts.",
    },
    CanonicalPlugin {
        canonical_name: "CoreProtect",
        provider: "modrinth",
        project_id: "coreprotect",
        platform: "bukkit",
        main_classes: &["net.coreprotect.CoreProtect"],
        ids: &["coreprotect", "core-protect"],
        description: "Fast, efficient block logging, rollback, and anti-grief inspection tool.",
    },
    CanonicalPlugin {
        canonical_name: "WorldGuard",
        provider: "modrinth",
        project_id: "worldguard",
        platform: "bukkit",
        main_classes: &["com.sk89q.worldguard.bukkit.WorldGuardPlugin"],
        ids: &["worldguard", "world-guard"],
        description: "Region protection, flags, safe zones, and server building security.",
    },
    CanonicalPlugin {
        canonical_name: "WorldEdit",
        provider: "modrinth",
        project_id: "worldedit",
        platform: "bukkit",
        main_classes: &[
            "com.sk89q.worldedit.bukkit.WorldEditPlugin",
            "com.sk89q.worldedit.fabric.WorldEditFabric",
        ],
        ids: &["worldedit", "world-edit"],
        description: "In-game Minecraft world manipulation, terraforming, and building utility.",
    },
    CanonicalPlugin {
        canonical_name: "Vault",
        provider: "modrinth",
        project_id: "vault",
        platform: "bukkit",
        main_classes: &["net.milkbowl.vault.Vault"],
        ids: &["vault"],
        description: "Standard API layer connecting permissions, chat, and economy systems.",
    },
    CanonicalPlugin {
        canonical_name: "ViaVersion",
        provider: "modrinth",
        project_id: "viaversion",
        platform: "bukkit",
        main_classes: &[
            "com.viaversion.viaversion.ViaVersionPlugin",
            "com.viaversion.viaversion.sponge.ViaVersionSponge",
        ],
        ids: &["viaversion", "via-version"],
        description: "Allows clients on newer Minecraft versions to join older servers.",
    },
    CanonicalPlugin {
        canonical_name: "ViaBackwards",
        provider: "modrinth",
        project_id: "viabackwards",
        platform: "bukkit",
        main_classes: &["com.viaversion.viabackwards.ViaBackwardsPlugin"],
        ids: &["viabackwards", "via-backwards"],
        description: "Allows clients on older Minecraft versions to join newer servers.",
    },
    CanonicalPlugin {
        canonical_name: "AuthMeReloaded",
        provider: "modrinth",
        project_id: "authmereloaded",
        platform: "bukkit",
        main_classes: &["fr.xephi.authme.AuthMe"],
        ids: &["authme", "authmereloaded", "authme-reloaded"],
        description: "Authentication and password security system for offline-mode / cracked servers.",
    },
    CanonicalPlugin {
        canonical_name: "FastLogin",
        provider: "modrinth",
        project_id: "fastlogin",
        platform: "bukkit",
        main_classes: &["com.github.games647.fastlogin.bukkit.FastLoginBukkit"],
        ids: &["fastlogin", "fast-login"],
        description: "Auto-authenticates premium accounts while protecting cracked / offline players.",
    },
    CanonicalPlugin {
        canonical_name: "SkinsRestorer",
        provider: "modrinth",
        project_id: "skinsrestorer",
        platform: "bukkit",
        main_classes: &["skinsrestorer.bukkit.SkinsRestorer"],
        ids: &["skinsrestorer", "skins-restorer"],
        description: "Restores custom player skins on offline-mode and cracked servers.",
    },
    CanonicalPlugin {
        canonical_name: "Chunky",
        provider: "modrinth",
        project_id: "chunky",
        platform: "bukkit",
        main_classes: &[
            "org.popcraft.chunky.ChunkyBukkit",
            "org.popcraft.chunky.ChunkyFabric",
        ],
        ids: &["chunky", "chunky-bukkit", "chunky-fabric"],
        description: "World chunk pre-generator that completely eliminates exploration lag spikes.",
    },
    CanonicalPlugin {
        canonical_name: "Spark",
        provider: "modrinth",
        project_id: "spark",
        platform: "bukkit",
        main_classes: &[
            "me.lucko.spark.bukkit.SparkBukkitPlugin",
            "me.lucko.spark.fabric.SparkFabricPlugin",
            "me.lucko.spark.forge.SparkForgeMod",
        ],
        ids: &["spark", "spark-mod"],
        description: "Lightweight profiler tracking server TPS, CPU usage, and memory leaks.",
    },
    CanonicalPlugin {
        canonical_name: "ProtocolLib",
        provider: "modrinth",
        project_id: "protocollib",
        platform: "bukkit",
        main_classes: &["com.comphenix.protocol.ProtocolLib"],
        ids: &["protocollib", "protocol-lib"],
        description: "Low-level packet reading and manipulation library for Bukkit plugins.",
    },
    CanonicalPlugin {
        canonical_name: "Multiverse-Core",
        provider: "modrinth",
        project_id: "multiverse-core",
        platform: "bukkit",
        main_classes: &["com.onarandombox.MultiverseCore.MultiverseCore"],
        ids: &["multiverse-core", "multiverse", "multiverse_core"],
        description: "Multi-world management for Bukkit and Paper servers.",
    },
    CanonicalPlugin {
        canonical_name: "Plan",
        provider: "modrinth",
        project_id: "plan",
        platform: "bukkit",
        main_classes: &["com.djrapitops.plan.Plan"],
        ids: &["plan", "player-analytics"],
        description: "Comprehensive player analytics, server activity graphs, and web dashboard.",
    },
    CanonicalPlugin {
        canonical_name: "DriveBackupV2",
        provider: "modrinth",
        project_id: "drivebackupv2",
        platform: "bukkit",
        main_classes: &["com.drivebackup.drivebackupv2.DriveBackupV2"],
        ids: &["drivebackupv2", "drive-backup-v2"],
        description: "Automated remote cloud backups to Google Drive, OneDrive, and external storage.",
    },
    CanonicalPlugin {
        canonical_name: "OpenInv",
        provider: "modrinth",
        project_id: "openinv",
        platform: "bukkit",
        main_classes: &["com.lishid.openinv.OpenInv"],
        ids: &["openinv", "open-inv"],
        description: "Inspect and manage inventories and enderchests of online and offline players.",
    },
    CanonicalPlugin {
        canonical_name: "Lithium",
        provider: "modrinth",
        project_id: "lithium",
        platform: "fabric",
        main_classes: &["me.jellysquid.mods.lithium.common.LithiumMod"],
        ids: &["lithium"],
        description: "General-purpose physics, mob AI, and entity ticking optimization mod.",
    },
    CanonicalPlugin {
        canonical_name: "FerriteCore",
        provider: "modrinth",
        project_id: "ferrite-core",
        platform: "fabric",
        main_classes: &["malte0811.ferritecore.FerriteCore"],
        ids: &["ferrite-core", "ferritecore"],
        description: "Memory usage optimization mod reducing server and client RAM footprint.",
    },
    CanonicalPlugin {
        canonical_name: "Sodium",
        provider: "modrinth",
        project_id: "sodium",
        platform: "fabric",
        main_classes: &["me.jellysquid.mods.sodium.client.SodiumClientMod"],
        ids: &["sodium"],
        description: "High-performance client rendering engine mod.",
    },
    CanonicalPlugin {
        canonical_name: "Fabric API",
        provider: "modrinth",
        project_id: "fabric-api",
        platform: "fabric",
        main_classes: &[],
        ids: &["fabric-api", "fabric"],
        description: "Core hook and event library for Fabric mods.",
    },
];

/// Parses common plugin/mod filename patterns to extract `(clean_name, version, platform)`.
///
/// Handles patterns such as:
/// - `EssentialsX-2.20.1.jar` -> ("EssentialsX", Some("2.20.1"), None)
/// - `LuckPerms-Bukkit-5.4.102.jar` -> ("LuckPerms", Some("5.4.102"), Some("bukkit"))
/// - `Geyser-Spigot.jar` -> ("Geyser", None, Some("spigot"))
/// - `worldguard-bukkit-7.0.9-dist.jar` -> ("worldguard", Some("7.0.9"), Some("bukkit"))
/// - `lithium-fabric-mc1.21.1-0.12.0.jar` -> ("lithium", Some("0.12.0"), Some("fabric"))
pub fn parse_plugin_filename(file_name: &str) -> (String, Option<String>, Option<String>) {
    let name_without_ext = file_name
        .strip_suffix(".jar")
        .or_else(|| file_name.strip_suffix(".jar.disabled"))
        .unwrap_or(file_name);

    let mut parts: Vec<&str> = name_without_ext.split(['-', '_']).collect();
    let mut platform = None;

    // Detect platform tags
    let known_platforms = [
        "bukkit", "spigot", "paper", "fabric", "forge", "neoforge", "quilt",
    ];
    parts.retain(|part| {
        let lower = part.to_ascii_lowercase();
        if known_platforms.contains(&lower.as_str()) {
            if platform.is_none() {
                platform = Some(lower);
            }
            false
        } else {
            !matches!(lower.as_str(), "dist" | "all" | "snapshot" | "release")
        }
    });

    if parts.is_empty() {
        return (name_without_ext.to_string(), None, platform);
    }

    // Attempt to isolate version (digits + dots)
    let mut version = None;
    if parts.len() > 1 {
        let is_version = parts.last().is_some_and(|last| {
            last.chars().any(|c| c.is_ascii_digit())
                && (last.contains('.') || last.chars().all(|c| c.is_ascii_digit()))
        });
        if is_version {
            version = parts.pop().map(|s| s.to_string());
        }
    }

    let clean_name = parts.join("-");
    (
        if clean_name.is_empty() {
            name_without_ext.to_string()
        } else {
            clean_name
        },
        version,
        platform,
    )
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Identifies an imported plugin/mod from file name, descriptor, and optional existing record.
pub fn detect_plugin(
    file_name: &str,
    descriptor: Option<&Descriptor>,
    existing_record: Option<&InstalledContent>,
) -> DetectedPlugin {
    // 1. If an existing record already has an identified source, that is authoritative.
    if let Some(src) = existing_record.and_then(|r| r.source.as_ref()) {
        return DetectedPlugin {
            file_name: file_name.to_string(),
            name: existing_record.map(|r| r.name.clone()).unwrap_or_default(),
            version: existing_record.and_then(|r| r.version_number.clone()),
            provider: Some(src.provider.clone()),
            project_id: Some(src.project_id.clone()),
            platform: None,
            confidence: DetectionConfidence::Exact,
            description: None,
        };
    }

    let (parsed_name, parsed_version, parsed_platform) = parse_plugin_filename(file_name);

    // 2. Descriptor inspection (main class or mod ID matches known plugins)
    if let Some(d) = descriptor {
        // Match main class with known plugins
        if let Some(main) = &d.main {
            for canon in CANONICAL_PLUGINS {
                if canon.main_classes.contains(&main.as_str()) {
                    return DetectedPlugin {
                        file_name: file_name.to_string(),
                        name: canon.canonical_name.to_string(),
                        version: d.version.clone().or(parsed_version),
                        provider: Some(canon.provider.to_string()),
                        project_id: Some(canon.project_id.to_string()),
                        platform: Some(canon.platform.to_string()),
                        confidence: DetectionConfidence::High,
                        description: Some(canon.description.to_string()),
                    };
                }
            }
        }

        // Match descriptor ID / Name with known canonical plugins
        let id_norm = d.id.as_deref().map(normalize);
        let name_norm = d.name.as_deref().map(normalize);

        for canon in CANONICAL_PLUGINS {
            let canon_id = normalize(canon.project_id);
            let canon_name = normalize(canon.canonical_name);

            let id_matched = id_norm.as_ref().is_some_and(|id| {
                *id == canon_id || canon.ids.iter().any(|&alias| normalize(alias) == *id)
            });
            let name_matched = name_norm.as_ref().is_some_and(|n| {
                *n == canon_id
                    || *n == canon_name
                    || canon.ids.iter().any(|&alias| normalize(alias) == *n)
            });

            if id_matched || name_matched {
                return DetectedPlugin {
                    file_name: file_name.to_string(),
                    name: canon.canonical_name.to_string(),
                    version: d.version.clone().or(parsed_version),
                    provider: Some(canon.provider.to_string()),
                    project_id: Some(canon.project_id.to_string()),
                    platform: Some(canon.platform.to_string()),
                    confidence: DetectionConfidence::High,
                    description: Some(canon.description.to_string()),
                };
            }
        }

        // Descriptor has a valid name, but it is not in the canonical list
        if let Some(name) = &d.name
            && !name.trim().is_empty()
        {
            return DetectedPlugin {
                file_name: file_name.to_string(),
                name: name.clone(),
                version: d.version.clone().or(parsed_version),
                provider: None,
                project_id: None,
                platform: parsed_platform,
                confidence: DetectionConfidence::Medium,
                description: d.description.clone(),
            };
        }
    }

    // 3. No descriptor or descriptor uninformative: try filename heuristic matching
    let parsed_norm = normalize(&parsed_name);
    for canon in CANONICAL_PLUGINS {
        let canon_id = normalize(canon.project_id);
        let canon_name = normalize(canon.canonical_name);

        if parsed_norm == canon_id
            || parsed_norm == canon_name
            || canon
                .ids
                .iter()
                .any(|&alias| normalize(alias) == parsed_norm)
        {
            return DetectedPlugin {
                file_name: file_name.to_string(),
                name: canon.canonical_name.to_string(),
                version: parsed_version,
                provider: Some(canon.provider.to_string()),
                project_id: Some(canon.project_id.to_string()),
                platform: parsed_platform.or_else(|| Some(canon.platform.to_string())),
                confidence: DetectionConfidence::Medium,
                description: Some(canon.description.to_string()),
            };
        }
    }

    // 4. Check for ambiguous / low-confidence / unknown filenames
    let lower_file = file_name.to_ascii_lowercase();
    let is_ambiguous = lower_file.starts_with("plugin")
        || lower_file.starts_with("mod")
        || lower_file.starts_with("custom")
        || lower_file.starts_with("server")
        || lower_file.starts_with("temp")
        || lower_file.starts_with("patch")
        || parsed_name.len() < 2;

    if is_ambiguous {
        DetectedPlugin {
            file_name: file_name.to_string(),
            name: parsed_name,
            version: parsed_version,
            provider: None,
            project_id: None,
            platform: parsed_platform,
            confidence: DetectionConfidence::Unknown,
            description: None,
        }
    } else {
        DetectedPlugin {
            file_name: file_name.to_string(),
            name: parsed_name,
            version: parsed_version,
            provider: None,
            project_id: None,
            platform: parsed_platform,
            confidence: DetectionConfidence::Low,
            description: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plugin_filenames() {
        let (name, ver, plat) = parse_plugin_filename("EssentialsX-2.20.1.jar");
        assert_eq!(name, "EssentialsX");
        assert_eq!(ver.as_deref(), Some("2.20.1"));
        assert_eq!(plat, None);

        let (name, ver, plat) = parse_plugin_filename("LuckPerms-Bukkit-5.4.102.jar");
        assert_eq!(name, "LuckPerms");
        assert_eq!(ver.as_deref(), Some("5.4.102"));
        assert_eq!(plat.as_deref(), Some("bukkit"));

        let (name, ver, plat) = parse_plugin_filename("Geyser-Spigot.jar");
        assert_eq!(name, "Geyser");
        assert_eq!(ver, None);
        assert_eq!(plat.as_deref(), Some("spigot"));
    }

    #[test]
    fn detects_canonical_plugin_from_main_class() {
        let desc = Descriptor {
            format: "plugin.yml".into(),
            name: Some("CustomPluginName".into()),
            version: Some("1.2.3".into()),
            main: Some("com.earth2me.essentials.Essentials".into()),
            id: None,
            description: None,
            website: None,
            client_only: false,
        };

        let detected = detect_plugin("random-utility.jar", Some(&desc), None);
        assert_eq!(detected.name, "EssentialsX");
        assert_eq!(detected.confidence, DetectionConfidence::High);
        assert_eq!(detected.provider.as_deref(), Some("modrinth"));
        assert_eq!(detected.project_id.as_deref(), Some("essentialsx"));
        assert_eq!(detected.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn marks_ambiguous_jar_as_unknown() {
        let detected = detect_plugin("plugin.jar", None, None);
        assert_eq!(detected.confidence, DetectionConfidence::Unknown);

        let detected_custom = detect_plugin("custom-build.jar", None, None);
        assert_eq!(detected_custom.confidence, DetectionConfidence::Unknown);
    }
}
