//! Live contract tests against the real provider APIs.
//! Run with `cargo test -p mcpanel-providers --features live-tests`.

use crate::builtin_registry;
use mcpanel_core::software::{InstallRequest, InstallStep};

/// Uses only MCPanel's self-managed credential. The test creates a uniquely named tunnel,
/// verifies it appears in the live account list, and removes it again.
#[cfg(windows)]
#[tokio::test]
async fn playit_live_tunnel_create_and_delete() {
    use mcpanel_core::playit_agent::PlayitAgent;
    use mcpanel_core::playit_api::{NewPlayitTunnel, PlayitApi, PlayitTunnelKind};
    use mcpanel_core::ports::{SecretStore, SettingsRepository};
    use mcpanel_platform::{NativePlatform, NativeSecretStore};
    use secrecy::SecretString;
    use std::path::Path;
    use std::sync::Arc;

    struct LiveSettings;
    #[async_trait::async_trait]
    impl SettingsRepository for LiveSettings {
        async fn get(&self, _: &str) -> mcpanel_core::CoreResult<Option<serde_json::Value>> {
            Ok(None)
        }
        async fn set(&self, _: &str, _: &serde_json::Value) -> mcpanel_core::CoreResult<()> {
            Ok(())
        }
        async fn all(&self) -> mcpanel_core::CoreResult<Vec<(String, serde_json::Value)>> {
            Ok(Vec::new())
        }
    }

    let _ = tracing_subscriber::fmt()
        .with_env_filter("mcpanel::playit_api=debug")
        .with_test_writer()
        .try_init();

    let store = NativeSecretStore::new("MCPanel");
    let key: SecretString = store
        .get("playit-agent-secret")
        .expect("read MCPanel's Playit credential")
        .expect("link MCPanel's own Playit agent before running this live test");
    let api = crate::playit::PlayitWebApi::new(&crate::http_client().unwrap());
    let agent = api.agent(&key).await.expect("read live Playit agent");
    assert!(
        agent.self_managed,
        "credential must belong to MCPanel's agent"
    );
    eprintln!(
        "Playit live agent accepted: self-managed, account status {}",
        agent.account_status
    );

    // This opt-in live test owns the approved MCPanel credential. Start MCPanel's own
    // daemon on the same pipe as the desktop app so Playit has a connected agent while
    // validating tunnel creation; stop it afterward only if this test started it.
    let paths = mcpanel_platform::default_paths().expect("resolve MCPanel data directory");
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in paths.data_dir.to_string_lossy().to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let socket = format!(r"\\.\pipe\mcpanel-playit-{hash:016x}");
    let runtime = PlayitAgent::new(
        Arc::new(NativePlatform::new()),
        Arc::new(store),
        Arc::new(LiveSettings),
        Path::new(&paths.data_dir),
        socket,
    );
    runtime.set_api(Arc::new(crate::playit::PlayitWebApi::new(
        &crate::http_client().unwrap(),
    )));
    let started_here = !runtime
        .status()
        .await
        .expect("read agent runtime status")
        .running;
    if started_here {
        runtime
            .start()
            .await
            .expect("start MCPanel's approved Playit agent");
    }

    let name = format!("MCPanel-debug-{}", uuid::Uuid::new_v4().simple());
    let created = api
        .create(
            &key,
            &agent.agent_id,
            &NewPlayitTunnel {
                name,
                kind: PlayitTunnelKind::MinecraftJava,
                local_ip: "127.0.0.1".into(),
                local_port: 25565,
                enabled: true,
            },
        )
        .await;
    let id = match created {
        Ok(id) => id,
        Err(e) => {
            if started_here {
                let _ = runtime.stop().await;
            }
            let code = e
                .details
                .as_ref()
                .and_then(|d| d.get("playit"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("provider-error");
            panic!("live Playit tunnel creation failed with sanitized code {code}");
        }
    };

    let list = api.tunnels(&key).await;
    let deleted = api.delete(&key, &id).await;
    if started_here {
        runtime
            .stop()
            .await
            .expect("stop temporary Playit agent after live verification");
    }
    assert!(deleted.is_ok(), "remove the temporary live tunnel");
    let list = list.expect("list live Playit tunnels");
    assert!(
        list.iter().any(|t| t.id == id),
        "created tunnel appears in list"
    );
    eprintln!("Playit live tunnel create, list, and cleanup succeeded");
}

/// Replaces only MCPanel's own key after a human approves a real Playit claim.
/// Run explicitly with `--ignored --exact live_tests::playit_live_relink_create_delete`.
#[cfg(windows)]
#[tokio::test]
#[ignore = "requires the owner to approve a live Playit claim in a browser"]
async fn playit_live_relink_create_delete() {
    use mcpanel_core::playit_api::{NewPlayitTunnel, PlayitApi, PlayitTunnelKind};
    use mcpanel_core::ports::SecretStore;
    use mcpanel_platform::NativeSecretStore;
    use secrecy::SecretString;
    use std::time::Duration;

    let _ = tracing_subscriber::fmt()
        .with_env_filter("mcpanel::playit_api=debug")
        .with_test_writer()
        .try_init();
    let store = NativeSecretStore::new("MCPanel");
    let old_key = store
        .get("playit-agent-secret")
        .expect("read MCPanel's Playit credential")
        .expect("MCPanel's agent must already have a credential");
    let http = crate::http_client().unwrap();
    let api = crate::playit::PlayitWebApi::new(&http);
    let old_agent = api
        .agent(&old_key)
        .await
        .expect("read MCPanel's current self-managed agent");
    assert!(old_agent.self_managed);

    let playit_exe = mcpanel_core::tunnels::PlayitTunnel::locate().expect("installed playit CLI");
    let version_output = tokio::process::Command::new(playit_exe)
        .arg("version")
        .output()
        .await
        .expect("read installed Playit version");
    assert!(version_output.status.success());
    let version = format!(
        "playit {}",
        String::from_utf8_lossy(&version_output.stdout).trim()
    );

    let claim_code = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
    let state = api
        .claim_setup(&claim_code, &version)
        .await
        .expect("register real Playit claim");
    assert!(matches!(
        state.as_str(),
        "WaitingForUserVisit" | "WaitingForUser"
    ));
    eprintln!("Approve MCPanel's replacement agent: https://playit.gg/claim/{claim_code}");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10 * 60);
    loop {
        assert!(tokio::time::Instant::now() < deadline, "approval timed out");
        match api
            .claim_setup(&claim_code, &version)
            .await
            .expect("poll Playit claim")
            .as_str()
        {
            "UserAccepted" => break,
            "UserRejected" => panic!("Playit claim was rejected"),
            _ => tokio::time::sleep(Duration::from_secs(1)).await,
        }
    }
    let key: SecretString = api
        .claim_exchange(&claim_code)
        .await
        .expect("exchange approved Playit claim");
    store
        .set("playit-agent-secret", &key)
        .expect("replace MCPanel's rejected credential");
    let agent = api.agent(&key).await.expect("validate replacement key");
    assert!(agent.self_managed);

    struct LiveSettings;
    #[async_trait::async_trait]
    impl mcpanel_core::ports::SettingsRepository for LiveSettings {
        async fn get(&self, _: &str) -> mcpanel_core::CoreResult<Option<serde_json::Value>> {
            Ok(None)
        }
        async fn set(&self, _: &str, _: &serde_json::Value) -> mcpanel_core::CoreResult<()> {
            Ok(())
        }
        async fn all(&self) -> mcpanel_core::CoreResult<Vec<(String, serde_json::Value)>> {
            Ok(Vec::new())
        }
    }
    let paths = mcpanel_platform::default_paths().expect("resolve MCPanel data directory");
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in paths.data_dir.to_string_lossy().to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let runtime = mcpanel_core::playit_agent::PlayitAgent::new(
        std::sync::Arc::new(mcpanel_platform::NativePlatform::new()),
        std::sync::Arc::new(store),
        std::sync::Arc::new(LiveSettings),
        &paths.data_dir,
        format!(r"\\.\pipe\mcpanel-playit-{hash:016x}"),
    );
    runtime.set_api(std::sync::Arc::new(crate::playit::PlayitWebApi::new(&http)));
    let started_here = !runtime.status().await.expect("read agent status").running;
    if started_here {
        runtime.start().await.expect("start newly approved agent");
    }

    let name = format!("MCPanel-debug-{}", uuid::Uuid::new_v4().simple());
    let created = api
        .create(
            &key,
            &agent.agent_id,
            &NewPlayitTunnel {
                name,
                kind: PlayitTunnelKind::MinecraftJava,
                local_ip: "127.0.0.1".into(),
                local_port: 25565,
                enabled: true,
            },
        )
        .await;
    let id = match created {
        Ok(id) => id,
        Err(e) => {
            if started_here {
                let _ = runtime.stop().await;
            }
            let code = e
                .details
                .as_ref()
                .and_then(|d| d.get("playit"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("provider-error");
            panic!("live Playit tunnel creation failed with sanitized code {code}");
        }
    };
    let listed = api.tunnels(&key).await;
    let deleted = api.delete(&key, &id).await;
    if started_here {
        runtime
            .stop()
            .await
            .expect("stop temporary Playit agent after live verification");
    }
    assert!(deleted.is_ok(), "remove the temporary live tunnel");
    assert!(
        listed.unwrap().iter().any(|t| t.id == id),
        "created tunnel appears in live list"
    );
    eprintln!("Playit relink and live tunnel create/list/delete succeeded");
}

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
