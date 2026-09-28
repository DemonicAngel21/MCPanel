#![allow(clippy::unwrap_used)]
//! Applying a built-in template to a created server (policies and audit).

mod common;

use common::harness::*;
use mcpanel_core::error::ErrorCode;

#[tokio::test(flavor = "multi_thread")]
async fn applying_a_template_sets_backup_and_restart_policies() {
    let h = harness().await;
    let (id, _) = add_server(&h, "tpl", "{}", true, 60).await;

    let jobs = h
        .core
        .templates
        .apply(id, "hardcore", &[], "test")
        .await
        .unwrap();
    assert!(jobs.is_empty());
    let backups = h.core.backups.policy(id).await.unwrap();
    assert!(backups.enabled);
    assert_eq!(backups.interval_minutes, 180);
    assert!(
        backups.last_run_at.is_some(),
        "the schedule starts counting now"
    );
    assert!(h.core.crashes.policy(id).await.unwrap().enabled);

    let audit = h
        .core
        .audit
        .query(&mcpanel_core::model::AuditQuery {
            server_id: Some(id),
            before: None,
            limit: 20,
        })
        .await
        .unwrap();
    assert!(
        audit
            .iter()
            .any(|a| a.action == "server.template_applied"
                && a.target.as_deref() == Some("Hardcore"))
    );

    // Resolution is software-aware: this template is not for "fake" software.
    let err = h
        .core
        .templates
        .resolve("survival", "fake", "1.21.4")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
    assert_eq!(
        h.core
            .templates
            .apply(id, "nope", &[], "test")
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    h.finish().await;
}
