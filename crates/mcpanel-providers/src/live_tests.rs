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
