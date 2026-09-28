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
        assert!(plan.java.min_major >= 21, "{id} java requirement");
        let InstallStep::Download { expected_hash, .. } = &plan.steps[0];
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
