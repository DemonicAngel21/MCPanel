#![allow(clippy::unwrap_used)]
//! Plugin manager end-to-end tests: the real core pipeline (plan → download → verify →
//! descriptor check → place/queue → record) against the deterministic TestHub provider.

mod common;

use common::content_fixture::{plugin_jar, sha512};
use common::harness::*;
use mcpanel_core::content::InstallRequest;
use mcpanel_core::error::ErrorCode;
use mcpanel_core::ids::{JobId, ServerId};
use mcpanel_core::jobs::JobStatus;
use mcpanel_core::lifecycle::LifecycleState;
use std::time::{Duration, Instant};

fn req(project: &str, version: Option<&str>) -> InstallRequest {
    InstallRequest {
        provider: "testhub".into(),
        project_id: project.into(),
        version_id: version.map(Into::into),
        with_dependencies: true,
    }
}

async fn job(h: &Harness, id: JobId) -> Result<serde_json::Value, (ErrorCode, String)> {
    let start = Instant::now();
    loop {
        let j = h.core.jobs.get(id).await.unwrap().unwrap();
        match j.status {
            JobStatus::Succeeded => return Ok(j.result.unwrap_or_default()),
            JobStatus::Running | JobStatus::Queued => {}
            _ => return Err((j.error_code.unwrap(), j.error_message.unwrap_or_default())),
        }
        assert!(start.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

async fn install(
    h: &Harness,
    id: ServerId,
    r: InstallRequest,
) -> Result<serde_json::Value, (ErrorCode, String)> {
    let j = h
        .core
        .content
        .install(id, r, "test")
        .await
        .map_err(|e| (e.code, e.message))?;
    job(h, j).await
}

fn plugins(h: &Harness, server: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(h.servers.path().join(server).join("plugins"))
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn install_with_dependencies_update_disable_and_remove() {
    let h = harness().await;
    let (id, _) = add_server(&h, "plugins", "{}", true, 60).await;

    // Plan: the requested plugin first, then its required dependency.
    let plan = h
        .core
        .content
        .plan(id, &req("alpha", Some("a1")))
        .await
        .unwrap();
    let names: Vec<_> = plan
        .items
        .iter()
        .map(|i| (i.project.name.as_str(), i.required_by.as_deref()))
        .collect();
    assert_eq!(names, vec![("Alpha", None), ("Beta", Some("Alpha"))]);
    assert!(!plan.deferred && plan.unresolved.is_empty());

    install(&h, id, req("alpha", Some("a1"))).await.unwrap();
    assert_eq!(
        plugins(&h, "plugins"),
        vec!["Alpha-1.0.jar", "Beta-1.0.jar"]
    );
    let list = h.core.content.list(id).await.unwrap();
    let alpha = list
        .entries
        .iter()
        .find(|e| e.file_name == "Alpha-1.0.jar")
        .unwrap();
    assert_eq!(
        alpha.descriptor.as_ref().unwrap().name.as_deref(),
        Some("Alpha")
    );
    assert_eq!(
        alpha
            .record
            .as_ref()
            .unwrap()
            .source
            .as_ref()
            .unwrap()
            .version_id,
        "a1"
    );
    assert_eq!(
        alpha.record.as_ref().unwrap().sha512.as_deref(),
        Some(sha512(&plugin_jar("Alpha", "1.0")).as_str())
    );

    // Installing the same version again is refused; the dependency is not re-planned.
    assert_eq!(
        h.core
            .content
            .plan(id, &req("alpha", Some("a1")))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );

    // Updates: a2 is newer; installing it replaces the old file (old → trash).
    let updates = h.core.content.check_updates(id).await.unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].latest.id, "a2");
    let plan = h
        .core
        .content
        .plan(id, &req("alpha", Some("a2")))
        .await
        .unwrap();
    assert_eq!(plan.items.len(), 1, "Beta is already installed");
    assert_eq!(plan.items[0].replaces.as_deref(), Some("Alpha-1.0.jar"));
    install(&h, id, req("alpha", Some("a2"))).await.unwrap();
    assert_eq!(
        plugins(&h, "plugins"),
        vec!["Alpha-2.0.jar", "Beta-1.0.jar"]
    );
    assert!(h.core.content.check_updates(id).await.unwrap().is_empty());
    let trash = h
        .servers
        .path()
        .join("plugins")
        .join(".mcpanel")
        .join("trash");
    assert!(
        std::fs::read_dir(trash).unwrap().next().is_some(),
        "old version kept in the trash"
    );

    // Disable → moved aside; enable → back; remove → trash and record gone.
    assert!(
        !h.core
            .content
            .set_enabled(id, "Beta-1.0.jar", false, "test")
            .await
            .unwrap()
    );
    assert_eq!(plugins(&h, "plugins"), vec!["Alpha-2.0.jar"]);
    let list = h.core.content.list(id).await.unwrap();
    assert!(
        list.entries
            .iter()
            .any(|e| e.file_name == "Beta-1.0.jar" && !e.enabled)
    );
    h.core
        .content
        .set_enabled(id, "Beta-1.0.jar", true, "test")
        .await
        .unwrap();
    h.core
        .content
        .remove(id, "Alpha-2.0.jar", "test")
        .await
        .unwrap();
    assert_eq!(plugins(&h, "plugins"), vec!["Beta-1.0.jar"]);
    let list = h.core.content.list(id).await.unwrap();
    assert!(
        !list
            .entries
            .iter()
            .any(|e| e.file_name.starts_with("Alpha"))
    );
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_downloads_and_non_plugins_are_rejected_without_changes() {
    let h = harness().await;
    let (id, _) = add_server(&h, "reject", "{}", true, 60).await;
    let (code, _) = install(&h, id, req("notaplugin", None)).await.unwrap_err();
    assert_eq!(code, ErrorCode::ArchiveRejected);
    let (code, _) = install(&h, id, req("badhash", None)).await.unwrap_err();
    assert_eq!(code, ErrorCode::HashMismatch);
    assert_eq!(
        h.core
            .content
            .plan(id, &req("external", None))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unsupported,
        "external-only files are never downloaded"
    );
    assert!(plugins(&h, "reject").is_empty());
    let incoming = h
        .servers
        .path()
        .join("reject")
        .join(".mcpanel")
        .join("incoming");
    assert!(
        std::fs::read_dir(incoming)
            .map(|rd| rd.count() == 0)
            .unwrap_or(true),
        "staging cleaned"
    );
    // Unsafe file names from a provider are refused (validated before downloading).
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn running_server_changes_are_queued_and_applied_when_it_stops() {
    let h = harness().await;
    let (id, _) = add_server(&h, "live", "{}", true, 60).await;
    install(&h, id, req("beta", None)).await.unwrap();
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;

    let r = install(&h, id, req("alpha", Some("a1"))).await.unwrap();
    assert_eq!(r["deferred"], true);
    assert!(
        h.core
            .content
            .remove(id, "Beta-1.0.jar", "test")
            .await
            .unwrap(),
        "queued"
    );
    assert_eq!(
        plugins(&h, "live"),
        vec!["Beta-1.0.jar"],
        "nothing changes while running"
    );
    let list = h.core.content.list(id).await.unwrap();
    assert!(list.running);
    assert_eq!(list.pending_installs.len(), 1);
    assert_eq!(h.core.content.pending(id).await.unwrap().len(), 2);

    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    let start = Instant::now();
    while !h.core.content.pending(id).await.unwrap().is_empty() {
        assert!(start.elapsed() < T, "pending changes applied after stop");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(plugins(&h, "live"), vec!["Alpha-1.0.jar"]);

    // A change queued before a restart is applied before the new process starts.
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core
        .content
        .set_enabled(id, "Alpha-1.0.jar", false, "test")
        .await
        .unwrap();
    h.core.servers.restart(id, "test").await.unwrap();
    let start = Instant::now();
    while !h.core.content.pending(id).await.unwrap().is_empty() {
        assert!(start.elapsed() < T, "applied by the launch hook");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    wait_state(&h, id, LifecycleState::Running, T).await;
    assert!(plugins(&h, "live").is_empty());
    let console: Vec<String> = h
        .core
        .servers
        .console(id)
        .snapshot(None, 200)
        .into_iter()
        .map(|l| l.text)
        .collect();
    assert!(
        console
            .iter()
            .any(|l| l.contains("Applied pending change: disable Alpha-1.0.jar")),
        "{console:#?}"
    );
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn manually_added_plugins_are_identified_by_hash() {
    let h = harness().await;
    let (id, _) = add_server(&h, "manual", "{}", true, 60).await;
    let dir = h.servers.path().join("manual").join("plugins");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("alpha-old.jar"), plugin_jar("Alpha", "1.0")).unwrap();
    std::fs::write(dir.join("Custom.jar"), plugin_jar("Custom", "0.1")).unwrap();
    let list = h.core.content.list(id).await.unwrap();
    assert!(
        list.entries.iter().all(|e| e.record.is_none()),
        "unmanaged at first"
    );

    let updates = h.core.content.check_updates(id).await.unwrap();
    assert_eq!(updates.len(), 1, "identified by hash, and a2 is newer");
    assert_eq!(updates[0].file_name, "alpha-old.jar");
    let list = h.core.content.list(id).await.unwrap();
    let alpha = list
        .entries
        .iter()
        .find(|e| e.file_name == "alpha-old.jar")
        .unwrap();
    assert_eq!(
        alpha
            .record
            .as_ref()
            .unwrap()
            .source
            .as_ref()
            .unwrap()
            .project_id,
        "alpha"
    );
    assert!(
        list.entries
            .iter()
            .find(|e| e.file_name == "Custom.jar")
            .unwrap()
            .record
            .is_none()
    );

    // Updating replaces the manually added file.
    install(&h, id, req("alpha", Some("a2"))).await.unwrap();
    let mut files = plugins(&h, "manual");
    files.retain(|f| f.ends_with(".jar"));
    assert_eq!(files, vec!["Alpha-2.0.jar", "Beta-1.0.jar", "Custom.jar"]);
    h.finish().await;
}
