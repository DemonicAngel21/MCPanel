use super::*;
use mcpanel_core::ids::{JavaRuntimeId, ServerId};
use mcpanel_core::jobs::{JobRecord, JobStatus};
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::model::*;
use mcpanel_core::time::Timestamp;

fn server(dir: &str, java: Option<JavaRuntimeId>) -> Server {
    Server {
        id: ServerId::new(),
        name: "Test".into(),
        directory: PathBuf::from(dir),
        software: InstalledSoftware {
            software_id: "paper".into(),
            game_version: "1.21.11".into(),
            build: Some("132".into()),
            jar: "paper-1.21.11-132.jar".into(),
            java_min_major: Some(21),
            java_recommended_major: None,
        },
        launch: LaunchConfig {
            java_runtime_id: java,
            min_memory_mb: 1024,
            max_memory_mb: 4096,
            jvm_args: vec!["-XX:+UseG1GC".into()],
            server_args: vec![],
            stop_timeout_secs: 60,
        },
        created_at: Timestamp(1),
        updated_at: Timestamp(1),
    }
}

fn java(path: &str) -> JavaRuntime {
    JavaRuntime {
        id: JavaRuntimeId::new(),
        path: PathBuf::from(path),
        major: 21,
        version: "21.0.4".into(),
        vendor: Some("Eclipse Adoptium".into()),
        arch: Some("amd64".into()),
        is_64bit: true,
        source: JavaSource::Detected,
        valid: true,
        validation_error: None,
        validated_at: Timestamp(5),
    }
}

#[tokio::test]
async fn server_crud_roundtrip() {
    let db = Database::open_in_memory().await.unwrap();
    let repos = db.repositories();
    let jid = repos
        .java
        .upsert(&java(r"C:\Java\bin\java.exe"))
        .await
        .unwrap();
    let s = server(r"C:\Servers\a", Some(jid));
    repos.servers.insert(&s).await.unwrap();
    let got = repos.servers.get(s.id).await.unwrap().unwrap();
    assert_eq!(got.name, "Test");
    assert_eq!(got.launch, s.launch);
    assert_eq!(got.software, s.software);

    let mut updated = got.clone();
    updated.name = "Renamed".into();
    updated.launch.max_memory_mb = 6144;
    repos.servers.update(&updated).await.unwrap();
    assert_eq!(
        repos
            .servers
            .get(s.id)
            .await
            .unwrap()
            .unwrap()
            .launch
            .max_memory_mb,
        6144
    );

    // Directory uniqueness is case-insensitive.
    let dup = server(r"c:\servers\A", None);
    let err = repos.servers.insert(&dup).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);

    repos.servers.delete(s.id).await.unwrap();
    assert!(repos.servers.get(s.id).await.unwrap().is_none());
}

#[tokio::test]
async fn java_upsert_is_keyed_by_path() {
    let db = Database::open_in_memory().await.unwrap();
    let repos = db.repositories();
    let a = repos
        .java
        .upsert(&java(r"C:\Java\bin\java.exe"))
        .await
        .unwrap();
    let mut again = java(r"c:\java\bin\JAVA.exe");
    again.major = 25;
    let b = repos.java.upsert(&again).await.unwrap();
    assert_eq!(a, b);
    assert_eq!(repos.java.list().await.unwrap().len(), 1);
    assert_eq!(repos.java.get(a).await.unwrap().unwrap().major, 25);
}

#[tokio::test]
async fn deleting_java_nulls_server_reference() {
    let db = Database::open_in_memory().await.unwrap();
    let repos = db.repositories();
    let jid = repos.java.upsert(&java(r"C:\J\java.exe")).await.unwrap();
    let s = server(r"C:\S", Some(jid));
    repos.servers.insert(&s).await.unwrap();
    repos.java.delete(jid).await.unwrap();
    assert_eq!(
        repos
            .servers
            .get(s.id)
            .await
            .unwrap()
            .unwrap()
            .launch
            .java_runtime_id,
        None
    );
}

#[tokio::test]
async fn runtime_state_roundtrip() {
    let db = Database::open_in_memory().await.unwrap();
    let repos = db.repositories();
    let s = server(r"C:\S", None);
    repos.servers.insert(&s).await.unwrap();
    let rec = RuntimeStateRecord {
        server_id: Some(s.id),
        last_state: Some(LifecycleState::Running),
        pid: Some(4242),
        process_start_time: Some(1_700_000_000),
        last_started_at: Some(Timestamp(10)),
        last_ready_at: Some(Timestamp(20)),
        last_stopped_at: None,
        last_exit_code: None,
    };
    repos.servers.save_runtime_state(s.id, &rec).await.unwrap();
    let got = repos.servers.runtime_states().await.unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].pid, Some(4242));
    assert_eq!(got[0].last_state, Some(LifecycleState::Running));
}

#[tokio::test]
async fn jobs_and_interruption() {
    let db = Database::open_in_memory().await.unwrap();
    let repos = db.repositories();
    let job = JobRecord {
        id: mcpanel_core::ids::JobId::new(),
        kind: "server.create".into(),
        server_id: None,
        status: JobStatus::Running,
        progress: Some(0.5),
        message: None,
        created_at: Timestamp(1),
        started_at: Some(Timestamp(1)),
        finished_at: None,
        error_code: None,
        error_message: None,
        result: None,
    };
    repos.jobs.insert(&job).await.unwrap();
    assert_eq!(repos.jobs.mark_interrupted(Timestamp(2)).await.unwrap(), 1);
    let got = repos.jobs.get(job.id).await.unwrap().unwrap();
    assert_eq!(got.status, JobStatus::Failed);
    assert_eq!(got.error_code, Some(ErrorCode::Cancelled));
}

#[tokio::test]
async fn audit_query_filters_and_orders() {
    let db = Database::open_in_memory().await.unwrap();
    let repos = db.repositories();
    let sid = ServerId::new();
    for i in 0..5 {
        repos
            .audit
            .insert(&AuditEntry {
                id: mcpanel_core::ids::AuditId::new(),
                occurred_at: Timestamp(i),
                actor: "user".into(),
                action: "server.start".into(),
                server_id: if i % 2 == 0 { Some(sid) } else { None },
                target: None,
                result: AuditResult::Success,
                metadata: serde_json::json!({ "i": i }),
            })
            .await
            .unwrap();
    }
    let all = repos
        .audit
        .query(&AuditQuery {
            server_id: None,
            before: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].occurred_at, Timestamp(4));
    let scoped = repos
        .audit
        .query(&AuditQuery {
            server_id: Some(sid),
            before: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(scoped.len(), 3);
}

#[tokio::test]
async fn file_database_migrates_and_refuses_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcpanel.db");
    let (db, report) = Database::open(&path).await.unwrap();
    assert!(!report.applied.is_empty());
    assert!(report.backup.is_none(), "fresh database needs no backup");
    db.repositories()
        .settings
        .set("k", &serde_json::json!(1))
        .await
        .unwrap();
    // Simulate a database written by a future MCPanel version.
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (9999, 'future', CURRENT_TIMESTAMP, 1, x'00', 0)")
        .execute(db.pool())
        .await
        .unwrap();
    db.close().await;
    let err = Database::open(&path).await.err().unwrap();
    assert_eq!(err.code, ErrorCode::SchemaTooNew);
    // A refused open releases the file.
    dir.close().unwrap();
}

#[tokio::test]
async fn reopening_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcpanel.db");
    let (db, _) = Database::open(&path).await.unwrap();
    db.close().await;
    let (db, report) = Database::open(&path).await.unwrap();
    assert!(report.applied.is_empty());
    db.close().await;
    dir.close().unwrap();
}
