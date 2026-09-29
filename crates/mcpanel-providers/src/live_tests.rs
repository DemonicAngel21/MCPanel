//! Live contract tests against the real provider APIs.
//! Run with `cargo test -p mcpanel-providers --features live-tests`.

use crate::builtin_registry;
use mcpanel_core::software::{InstallRequest, InstallStep};

#[tokio::test]
async fn all_providers_list_versions_and_plan_installs() {
    let http = crate::http_client().unwrap();
    let registry = builtin_registry(&http);
    for (id, version) in [
        ("vanilla", "1.21.4"),
        ("paper", "1.21.11"),
        ("purpur", "1.21.11"),
        ("fabric", "1.21.11"),
        ("quilt", "1.21.4"),
        ("neoforge", "1.21.4"),
        ("forge", "1.20.1"),
    ] {
        let p = registry.get_software(id).unwrap();
        let versions = p.catalog.game_versions().await.unwrap();
        assert!(
            versions.iter().any(|v| v.id == version),
            "{id} lists {version}"
        );
        let plan = p
            .installer
            .plan_install(&InstallRequest {
                game_version: version.into(),
                build: None,
            })
            .await
            .unwrap();
        mcpanel_core::software::executor::PlanExecutor::validate(&plan, &p.descriptor).unwrap();
        assert!(plan.java.min_major >= 17, "{id} java requirement");
        let InstallStep::Download { expected_hash, .. } = &plan.steps[0] else {
            panic!("{id}: first step is not a download");
        };
        assert!(expected_hash.is_some(), "{id} provides a checksum");
    }
}

#[tokio::test]
async fn test_matrix_versions_have_server_downloads() {
    let http = crate::http_client().unwrap();
    let registry = builtin_registry(&http);
    let vanilla = registry.get_software("vanilla").unwrap();
    for (v, java) in [("1.12.2", 8), ("1.16.5", 8), ("1.20.1", 17), ("1.21.4", 21)] {
        let plan = vanilla
            .installer
            .plan_install(&InstallRequest {
                game_version: v.into(),
                build: None,
            })
            .await
            .unwrap();
        assert_eq!(plan.java.min_major, java, "{v}");
    }
}

fn paper_target(version: &str) -> mcpanel_core::content::ContentTarget {
    use mcpanel_core::software::ContentEcosystem;
    mcpanel_core::content::ContentTarget {
        kind: mcpanel_core::content::ContentKind::Plugin,
        software_id: "paper".into(),
        game_version: version.into(),
        ecosystems: vec![
            ContentEcosystem::BukkitPlugins,
            ContentEcosystem::PaperPlugins,
        ],
    }
}

#[tokio::test]
async fn modrinth_search_versions_and_identify() {
    use mcpanel_core::content::{ContentProvider, SearchQuery, SearchSort};
    use mcpanel_core::ports::HashAlgorithm;
    let http = crate::http_client().unwrap();
    let m = crate::modrinth::Modrinth::new(http);
    let target = paper_target("1.21.4");
    let page = m
        .search(&SearchQuery {
            text: "luckperms".into(),
            target: target.clone(),
            sort: SearchSort::Relevance,
            offset: 0,
            limit: 5,
        })
        .await
        .unwrap();
    let lp = page
        .hits
        .iter()
        .find(|h| h.slug == "luckperms")
        .expect("LuckPerms found");
    assert!(lp.page_url.starts_with("https://modrinth.com/plugin/"));
    let versions = m.versions(&lp.id, &target).await.unwrap();
    let v = &versions[0];
    assert!(v.game_versions.contains(&"1.21.4".to_string()));
    let file = v.file.as_ref().unwrap();
    assert!(file.url.starts_with("https://cdn.modrinth.com/"));
    let hash = file.hash.as_ref().unwrap();
    assert_eq!(hash.algorithm, HashAlgorithm::Sha512);
    assert_eq!(m.version(&lp.id, &v.id).await.unwrap().id, v.id);
    let found = m.identify(&[hash.hex.clone()]).await.unwrap();
    assert_eq!(found[&hash.hex].project_id, lp.id);
    assert_eq!(m.project("luckperms").await.unwrap().name, "LuckPerms");
    assert_eq!(
        m.project("zz-no-such-project-zz").await.unwrap_err().code,
        mcpanel_core::error::ErrorCode::NotFound
    );
}

#[tokio::test]
async fn hangar_search_versions_and_version_by_name() {
    use mcpanel_core::content::{ContentProvider, SearchQuery, SearchSort};
    use mcpanel_core::ports::HashAlgorithm;
    let http = crate::http_client().unwrap();
    let h = crate::hangar::Hangar::new(http);
    let target = paper_target("1.21.4");
    let page = h
        .search(&SearchQuery {
            text: "ViaVersion".into(),
            target: target.clone(),
            sort: SearchSort::Relevance,
            offset: 0,
            limit: 5,
        })
        .await
        .unwrap();
    let via = page
        .hits
        .iter()
        .find(|p| p.slug == "ViaVersion")
        .expect("ViaVersion found");
    let versions = h.versions(&via.id, &target).await.unwrap();
    let v = versions.iter().find(|v| v.file.is_some()).unwrap();
    let file = v.file.as_ref().unwrap();
    assert!(file.url.starts_with("https://hangarcdn.papermc.io/"));
    assert_eq!(file.hash.as_ref().unwrap().algorithm, HashAlgorithm::Sha256);
    // Version names can contain '+' (e.g. "5.12.1-SNAPSHOT+1072").
    assert_eq!(h.version(&via.id, &v.id).await.unwrap().id, v.id);
    assert_eq!(h.project(&via.id).await.unwrap().slug, "ViaVersion");
}

#[tokio::test]
async fn geysermc_latest_builds_download_and_match_their_hash() {
    use mcpanel_core::content::ContentProvider;
    use mcpanel_core::ports::HashAlgorithm;
    use sha2::Digest;
    let http = crate::http_client().unwrap();
    let g = crate::geysermc::GeyserMc::new(http.clone());
    let target = paper_target("1.21.4");
    assert!(g.supports(&target));
    assert!(!g.supports(&paper_target("1.20.4")));
    for project in ["geyser", "floodgate"] {
        let versions = g.versions(project, &target).await.unwrap();
        let v = versions.first().unwrap();
        assert_eq!(g.version(project, &v.id).await.unwrap().id, v.id);
        let file = v.file.as_ref().unwrap();
        assert!(file.file_name.to_lowercase().contains("spigot"));
        let hash = file.hash.as_ref().unwrap();
        assert_eq!(hash.algorithm, HashAlgorithm::Sha256);
        if project == "floodgate" {
            let bytes = http.get_bytes(&file.url, 64 << 20).await.unwrap();
            assert_eq!(hex::encode(sha2::Sha256::digest(&bytes)), hash.hex);
        }
    }
}

#[tokio::test]
async fn spiget_search_latest_version_and_cdn_download() {
    use mcpanel_core::content::{ContentProvider, SearchQuery, SearchSort};
    let http = crate::http_client().unwrap();
    let sp = crate::spiget::Spiget::new(http.clone());
    let target = paper_target("1.21.4");
    assert!(sp.supports(&target));
    let page = sp
        .search(&SearchQuery {
            text: "LuckPerms".into(),
            target: target.clone(),
            sort: SearchSort::Downloads,
            offset: 0,
            limit: 5,
        })
        .await
        .unwrap();
    let lp = page
        .hits
        .iter()
        .find(|p| p.name == "LuckPerms")
        .expect("found");
    assert_eq!(lp.page_url, "https://www.spigotmc.org/resources/28140/");
    let versions = sp.versions(&lp.id, &target).await.unwrap();
    assert_eq!(versions.len(), 1);
    let file = versions[0].file.as_ref().expect("installable");
    assert!(
        file.url.starts_with("https://cdn.spiget.org/"),
        "{}",
        file.url
    );
    assert!(file.hash.is_none(), "Spiget publishes no hashes");
    assert_eq!(
        sp.version(&lp.id, &versions[0].id).await.unwrap().id,
        versions[0].id
    );
    // External resources are listed but not downloadable.
    let ex = sp.versions("9089", &target).await.unwrap();
    assert!(ex[0].file.is_none());
    assert!(
        ex[0]
            .external_url
            .as_deref()
            .unwrap_or("")
            .starts_with("https://")
    );
    // Unknown resources are "not found"; empty searches are empty.
    assert_eq!(
        sp.project("999999999").await.unwrap_err().code,
        mcpanel_core::error::ErrorCode::NotFound
    );
    let none = sp
        .search(&SearchQuery {
            text: "zzqqxx-no-such-plugin-zzqqxx".into(),
            target,
            sort: SearchSort::Relevance,
            offset: 0,
            limit: 5,
        })
        .await
        .unwrap();
    assert!(none.hits.is_empty());
}

/// Without credentials: each provider's real token endpoint answers a code exchange with an
/// invalid client ID with an OAuth error that MCPanel reports clearly (endpoints and
/// error mapping are right; no account or app registration involved).
#[tokio::test]
async fn cloud_token_endpoints_reject_an_unknown_client_clearly() {
    use mcpanel_core::cloud::CloudStorageProvider;
    let http = crate::http_client().unwrap();
    let bogus = Some("mcpanel-live-test-invalid-client".to_string());
    let providers: Vec<Box<dyn CloudStorageProvider>> = vec![
        Box::new(crate::cloud::google_drive::GoogleDrive::new(
            http.clone(),
            bogus.clone(),
        )),
        Box::new(crate::cloud::dropbox::Dropbox::new(http.clone(), bogus)),
    ];
    for p in providers {
        let redirect = p.info().redirect.uri(43917);
        let e = p
            .exchange_code("invalid-code", "a".repeat(43).as_str(), &redirect)
            .await
            .err()
            .expect("rejected");
        eprintln!("{}: {}", p.info().display_name, e.message);
        assert_eq!(e.code, mcpanel_core::error::ErrorCode::ProviderError);
        assert!(
            e.message.starts_with(p.info().display_name),
            "{}",
            e.message
        );
        assert!(
            !e.message.contains("HTTP 404"),
            "endpoint exists: {}",
            e.message
        );
    }
}
