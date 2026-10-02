//! Contextual plugin and mod recommendation engine.
//!
//! Evaluates server software, Minecraft version, configuration (online-mode / cracked,
//! Bedrock / Geyser), installed items, and ecosystem to surface highly relevant,
//! compatible plugins across categories (Security, Performance, Administration, etc.).

use crate::software::ContentEcosystem;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationCategory {
    Performance,
    Administration,
    Permissions,
    Protection,
    Crossplay,
    Backup,
    Moderation,
    Gameplay,
    Security,
    Monitoring,
}

impl RecommendationCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Administration => "administration",
            Self::Permissions => "permissions",
            Self::Protection => "protection",
            Self::Crossplay => "crossplay",
            Self::Backup => "backup",
            Self::Moderation => "moderation",
            Self::Gameplay => "gameplay",
            Self::Security => "security",
            Self::Monitoring => "monitoring",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRecommendation {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: RecommendationCategory,
    pub provider: String,
    pub project_id: String,
    pub icon_url: Option<String>,
    pub reason: String,
    pub supported_software: Vec<String>,
    pub supported_ecosystems: Vec<ContentEcosystem>,
    pub min_game_version: Option<String>,
    pub max_game_version: Option<String>,
    pub requires_offline_mode: bool,
    pub requires_bedrock: bool,
}

pub struct RecommendationContext {
    pub software_id: String,
    pub game_version: String,
    pub ecosystems: Vec<ContentEcosystem>,
    pub online_mode: bool,
    pub has_bedrock: bool,
    pub installed: Vec<String>,
}

fn bukkit_ecosystems() -> Vec<ContentEcosystem> {
    vec![
        ContentEcosystem::BukkitPlugins,
        ContentEcosystem::PaperPlugins,
    ]
}

fn fabric_ecosystems() -> Vec<ContentEcosystem> {
    vec![ContentEcosystem::FabricMods, ContentEcosystem::QuiltMods]
}

fn all_ecosystems() -> Vec<ContentEcosystem> {
    vec![
        ContentEcosystem::BukkitPlugins,
        ContentEcosystem::PaperPlugins,
        ContentEcosystem::FabricMods,
        ContentEcosystem::QuiltMods,
        ContentEcosystem::ForgeMods,
        ContentEcosystem::NeoForgeMods,
    ]
}

pub fn catalog() -> Vec<PluginRecommendation> {
    vec![
        // ─── Offline-Mode / Cracked Server Security & Gameplay ───
        PluginRecommendation {
            id: "authmereloaded".into(),
            name: "AuthMeReloaded".into(),
            description: "Industry-standard password authentication and session management plugin.".into(),
            category: RecommendationCategory::Security,
            provider: "modrinth".into(),
            project_id: "authmereloaded".into(),
            icon_url: None,
            reason: "Essential for cracked / offline-mode servers: secures player accounts with passwords to prevent unauthorized access or operator impersonation.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into()],
            supported_ecosystems: bukkit_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: true,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "fastlogin".into(),
            name: "FastLogin".into(),
            description: "Auto-authenticates premium accounts while protecting cracked / offline players.".into(),
            category: RecommendationCategory::Security,
            provider: "modrinth".into(),
            project_id: "fastlogin".into(),
            icon_url: None,
            reason: "Quality of Life for cracked servers: allows paid Minecraft players to login without entering a password while keeping cracked players secured.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into()],
            supported_ecosystems: bukkit_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: true,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "skinsrestorer".into(),
            name: "SkinsRestorer".into(),
            description: "Restores custom skins for offline-mode and cracked players.".into(),
            category: RecommendationCategory::Gameplay,
            provider: "modrinth".into(),
            project_id: "skinsrestorer".into(),
            icon_url: None,
            reason: "Offline mode players normally appear as default Steve or Alex skins. SkinsRestorer allows everyone to show custom skins.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: true,
            requires_bedrock: false,
        },

        // ─── Crossplay / Bedrock ───
        PluginRecommendation {
            id: "geyser".into(),
            name: "Geyser".into(),
            description: "Enables Minecraft Bedrock Edition clients (mobile, console, Windows 10/11) to join Java Edition servers.".into(),
            category: RecommendationCategory::Crossplay,
            provider: "modrinth".into(),
            project_id: "geyser".into(),
            icon_url: None,
            reason: "Bridge Bedrock and Java players together so your friends can play from consoles and mobile devices.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into(), "neoforge".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "floodgate".into(),
            name: "Floodgate".into(),
            description: "Allows Bedrock players to connect through Geyser without owning a Java Edition account.".into(),
            category: RecommendationCategory::Crossplay,
            provider: "modrinth".into(),
            project_id: "floodgate".into(),
            icon_url: None,
            reason: "Pairs with Geyser so Bedrock Edition players can join without needing to buy a separate Java Edition license.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: true,
        },
        PluginRecommendation {
            id: "viaversion".into(),
            name: "ViaVersion".into(),
            description: "Allows clients on newer Minecraft versions to join older servers.".into(),
            category: RecommendationCategory::Crossplay,
            provider: "modrinth".into(),
            project_id: "viaversion".into(),
            icon_url: None,
            reason: "Prevents connection rejections when players update their Minecraft clients before your server updates.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "viabackwards".into(),
            name: "ViaBackwards".into(),
            description: "Allows clients on older Minecraft versions to join newer servers.".into(),
            category: RecommendationCategory::Crossplay,
            provider: "modrinth".into(),
            project_id: "viabackwards".into(),
            icon_url: None,
            reason: "Allows players who haven't updated their clients to still join your updated server.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },

        // ─── Administration & Permissions ───
        PluginRecommendation {
            id: "luckperms".into(),
            name: "LuckPerms".into(),
            description: "Fast, flexible, web-configurable permissions plugin with rich command inheritance.".into(),
            category: RecommendationCategory::Permissions,
            provider: "modrinth".into(),
            project_id: "luckperms".into(),
            icon_url: None,
            reason: "Essential for assigning roles (VIP, Admin, Builder) and configuring granular command permissions.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "essentialsx".into(),
            name: "EssentialsX".into(),
            description: "Comprehensive suite of over 100 essential commands, economy, player kits, homes, and warps.".into(),
            category: RecommendationCategory::Administration,
            provider: "modrinth".into(),
            project_id: "essentialsx".into(),
            icon_url: None,
            reason: "The foundation of server commands: /home, /spawn, /warp, /tpa, player kits, and economy.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into()],
            supported_ecosystems: bukkit_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "vault".into(),
            name: "Vault".into(),
            description: "Common permissions and economy API used by virtually all server plugins.".into(),
            category: RecommendationCategory::Administration,
            provider: "modrinth".into(),
            project_id: "vault".into(),
            icon_url: None,
            reason: "Required dependency for plugins to communicate with your permissions and economy providers.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into()],
            supported_ecosystems: bukkit_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },

        // ─── Protection & Anti-Grief ───
        PluginRecommendation {
            id: "coreprotect".into(),
            name: "CoreProtect".into(),
            description: "High-performance block logging, rollback, and anti-grief inspection tool.".into(),
            category: RecommendationCategory::Protection,
            provider: "modrinth".into(),
            project_id: "coreprotect".into(),
            icon_url: None,
            reason: "Check who broke any block or looted any chest, and roll back griefing incidents in seconds with a single command.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into()],
            supported_ecosystems: bukkit_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "worldguard".into(),
            name: "WorldGuard".into(),
            description: "Create protected zones, prevent PvP in spawn, and disable fire spread or creeper damage.".into(),
            category: RecommendationCategory::Protection,
            provider: "modrinth".into(),
            project_id: "worldguard".into(),
            icon_url: None,
            reason: "Protect server spawns, shops, and communities from explosions, damage, and unauthorized building.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into()],
            supported_ecosystems: bukkit_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "worldedit".into(),
            name: "WorldEdit".into(),
            description: "Powerful in-game world generation and structural manipulation tool.".into(),
            category: RecommendationCategory::Protection,
            provider: "modrinth".into(),
            project_id: "worldedit".into(),
            icon_url: None,
            reason: "Essential for building server structures, clearing areas, and managing custom maps.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into(), "neoforge".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },

        // ─── Performance ───
        PluginRecommendation {
            id: "chunky".into(),
            name: "Chunky".into(),
            description: "Pre-generates world chunks in the background to completely eliminate chunk generation lag spikes.".into(),
            category: RecommendationCategory::Performance,
            provider: "modrinth".into(),
            project_id: "chunky".into(),
            icon_url: None,
            reason: "Chunk generation while players fly or explore is the #1 cause of server stutter. Chunky pre-renders the world cleanly.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into(), "neoforge".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "lithium".into(),
            name: "Lithium".into(),
            description: "General-purpose optimization mod improving server physics, entity ticking, and chunk loading.".into(),
            category: RecommendationCategory::Performance,
            provider: "modrinth".into(),
            project_id: "lithium".into(),
            icon_url: None,
            reason: "Must-have optimization mod for Fabric servers: optimizes physics and mob AI without altering game mechanics.".into(),
            supported_software: vec!["fabric".into(), "quilt".into()],
            supported_ecosystems: fabric_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
        PluginRecommendation {
            id: "ferrite-core".into(),
            name: "FerriteCore".into(),
            description: "Memory optimization mod significantly reducing server and client RAM consumption.".into(),
            category: RecommendationCategory::Performance,
            provider: "modrinth".into(),
            project_id: "ferrite-core".into(),
            icon_url: None,
            reason: "Reduces memory allocations and RAM usage, helping servers run smoothly on lower memory limits.".into(),
            supported_software: vec!["fabric".into(), "forge".into(), "neoforge".into(), "quilt".into()],
            supported_ecosystems: vec![
                ContentEcosystem::FabricMods,
                ContentEcosystem::QuiltMods,
                ContentEcosystem::ForgeMods,
                ContentEcosystem::NeoForgeMods,
            ],
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },

        // ─── Monitoring & Diagnostics ───
        PluginRecommendation {
            id: "spark".into(),
            name: "Spark".into(),
            description: "Lightweight performance profiler tracking TPS drops, CPU spikes, and memory leaks.".into(),
            category: RecommendationCategory::Monitoring,
            provider: "modrinth".into(),
            project_id: "spark".into(),
            icon_url: None,
            reason: "Diagnose TPS drops, entity lag, and slow plugins with detailed flame graphs and instant profiler links.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into(), "fabric".into(), "neoforge".into(), "forge".into()],
            supported_ecosystems: all_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },

        // ─── Backup ───
        PluginRecommendation {
            id: "drivebackupv2".into(),
            name: "DriveBackupV2".into(),
            description: "Automated off-site cloud backups to Google Drive, OneDrive, Dropbox, and external destinations.".into(),
            category: RecommendationCategory::Backup,
            provider: "modrinth".into(),
            project_id: "drivebackupv2".into(),
            icon_url: None,
            reason: "Safeguard your server worlds against hardware failure with automated scheduled cloud backups.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into()],
            supported_ecosystems: bukkit_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },

        // ─── Moderation ───
        PluginRecommendation {
            id: "openinv".into(),
            name: "OpenInv".into(),
            description: "Inspect and manage inventories and enderchests of online and offline players.".into(),
            category: RecommendationCategory::Moderation,
            provider: "modrinth".into(),
            project_id: "openinv".into(),
            icon_url: None,
            reason: "Allows server operators to search and confiscate illegal or duplicated items even when the player is offline.".into(),
            supported_software: vec!["paper".into(), "purpur".into(), "spigot".into()],
            supported_ecosystems: bukkit_ecosystems(),
            min_game_version: None,
            max_game_version: None,
            requires_offline_mode: false,
            requires_bedrock: false,
        },
    ]
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Evaluates context and filters catalog to return contextual recommendations.
pub fn filter_recommendations(context: &RecommendationContext) -> Vec<PluginRecommendation> {
    let mut results = Vec::new();
    let norm_installed: Vec<String> = context.installed.iter().map(|s| norm(s)).collect();

    for rec in catalog() {
        // 1. Offline mode filter
        if rec.requires_offline_mode && context.online_mode {
            continue;
        }

        // 2. Bedrock filter
        if rec.requires_bedrock && !context.has_bedrock {
            continue;
        }

        // 3. Platform / Ecosystem filter
        let software_matches = rec
            .supported_software
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&context.software_id));

        let ecosystem_matches = rec
            .supported_ecosystems
            .iter()
            .any(|e| context.ecosystems.contains(e));

        if !software_matches && !ecosystem_matches {
            continue;
        }

        // 4. Installed check
        let rec_id_norm = norm(&rec.id);
        let rec_proj_norm = norm(&rec.project_id);
        let rec_name_norm = norm(&rec.name);

        let is_installed = norm_installed.iter().any(|inst| {
            *inst == rec_id_norm
                || *inst == rec_proj_norm
                || *inst == rec_name_norm
                || inst.contains(&rec_id_norm)
                || rec_id_norm.contains(inst)
        });

        if is_installed {
            continue;
        }

        results.push(rec);
    }

    // Sort to prioritize security on offline-mode servers, then crossplay on bedrock servers, then performance & admin
    results.sort_by(|a, b| {
        let score = |r: &PluginRecommendation| -> i32 {
            if !context.online_mode && r.requires_offline_mode {
                100
            } else if context.has_bedrock && (r.id == "geyser" || r.id == "floodgate") {
                90
            } else {
                match r.category {
                    RecommendationCategory::Security => 80,
                    RecommendationCategory::Performance => 70,
                    RecommendationCategory::Administration => 60,
                    RecommendationCategory::Permissions => 55,
                    RecommendationCategory::Protection => 50,
                    RecommendationCategory::Crossplay => 40,
                    RecommendationCategory::Monitoring => 35,
                    RecommendationCategory::Backup => 30,
                    RecommendationCategory::Moderation => 25,
                    RecommendationCategory::Gameplay => 20,
                }
            }
        };
        score(b).cmp(&score(a))
    });

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommends_security_plugins_for_offline_mode_servers() {
        let ctx = RecommendationContext {
            software_id: "paper".into(),
            game_version: "1.21.1".into(),
            ecosystems: vec![
                ContentEcosystem::PaperPlugins,
                ContentEcosystem::BukkitPlugins,
            ],
            online_mode: false,
            has_bedrock: false,
            installed: vec![],
        };

        let recs = filter_recommendations(&ctx);
        assert!(recs.iter().any(|r| r.id == "authmereloaded"));
        assert!(recs.iter().any(|r| r.id == "fastlogin"));
        assert!(recs.iter().any(|r| r.id == "skinsrestorer"));
        // Top recommendation should be AuthMeReloaded
        assert_eq!(recs[0].id, "authmereloaded");
    }

    #[test]
    fn filters_out_authme_when_online_mode_is_true() {
        let ctx = RecommendationContext {
            software_id: "paper".into(),
            game_version: "1.21.1".into(),
            ecosystems: vec![
                ContentEcosystem::PaperPlugins,
                ContentEcosystem::BukkitPlugins,
            ],
            online_mode: true,
            has_bedrock: false,
            installed: vec![],
        };

        let recs = filter_recommendations(&ctx);
        assert!(!recs.iter().any(|r| r.id == "authmereloaded"));
        assert!(!recs.iter().any(|r| r.id == "fastlogin"));
        assert!(!recs.iter().any(|r| r.id == "skinsrestorer"));
    }

    #[test]
    fn recommends_fabric_performance_mods_for_fabric_server() {
        let ctx = RecommendationContext {
            software_id: "fabric".into(),
            game_version: "1.21.1".into(),
            ecosystems: vec![ContentEcosystem::FabricMods],
            online_mode: true,
            has_bedrock: false,
            installed: vec![],
        };

        let recs = filter_recommendations(&ctx);
        assert!(recs.iter().any(|r| r.id == "lithium"));
        assert!(recs.iter().any(|r| r.id == "ferrite-core"));
        // Bukkit-only plugins must not be recommended
        assert!(!recs.iter().any(|r| r.id == "essentialsx"));
        assert!(!recs.iter().any(|r| r.id == "coreprotect"));
    }

    #[test]
    fn does_not_recommend_already_installed_plugins() {
        let ctx = RecommendationContext {
            software_id: "paper".into(),
            game_version: "1.21.1".into(),
            ecosystems: vec![
                ContentEcosystem::PaperPlugins,
                ContentEcosystem::BukkitPlugins,
            ],
            online_mode: false,
            has_bedrock: false,
            installed: vec!["AuthMeReloaded".into(), "luckperms-5.4.102.jar".into()],
        };

        let recs = filter_recommendations(&ctx);
        assert!(!recs.iter().any(|r| r.id == "authmereloaded"));
        assert!(!recs.iter().any(|r| r.id == "luckperms"));
        assert!(recs.iter().any(|r| r.id == "fastlogin"));
    }
}
