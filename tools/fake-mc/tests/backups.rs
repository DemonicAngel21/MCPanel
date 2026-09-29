#![allow(clippy::unwrap_used)]
//! Backup end-to-end tests: real core, SQLite and file system, with `fake-mc` as the
//! running server for live backups.

mod common;

use common::harness::*;
use mcpanel_core::Core;
use mcpanel_core::backup::{
    BackupKind, BackupPolicy, BackupStatus, CreateBackupRequest, Retention,
};
use mcpanel_core::console::ConsoleStream;
use mcpanel_core::error::ErrorCode;
use mcpanel_core::ids::{BackupId, JobId, ServerId};
use mcpanel_core::jobs::JobStatus;
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::time::Timestamp;
use std::io::Read;
use std::time::{Duration, Instant};

async fn wait_job(core: &Core, id: JobId) -> Result<serde_json::Value, String> {
    let start = Instant::now();
    loop {
        let job = core.jobs.get(id).await.unwrap().unwrap();
        match job.status {
            JobStatus::Succeeded => return Ok(job.result.unwrap_or_default()),
            JobStatus::Running | JobStatus::Queued => {}
            _ => return Err(job.error_message.unwrap_or_default()),
        }
        assert!(start.elapsed() < Duration::from_secs(60), "job timed out");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn backup(h: &Harness, id: ServerId, note: Option<&str>) -> BackupId {
    let job = h
        .core
        .backups
        .create(
            CreateBackupRequest {
                server_id: id,
                note: note.map(Into::into),
            },
            "test",
        )
        .await
        .unwrap();
    let r = wait_job(&h.core, job).await.unwrap();
    r["backupId"].as_str().unwrap().parse().unwrap()
}

fn zip_text(path: &std::path::Path, name: &str) -> Option<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut e = z.by_name(name).ok()?;
    let mut s = String::new();
    e.read_to_string(&mut s).unwrap();
    Some(s)
}

fn write_world(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("world/region")).unwrap();
    std::fs::create_dir_all(dir.join("plugins/floodgate")).unwrap();
    std::fs::write(dir.join("world/level.dat"), "original").unwrap();
    std::fs::write(dir.join("world/region/r.0.0.mca"), vec![1u8; 200_000]).unwrap();
    std::fs::write(dir.join("plugins/floodgate/key.pem"), "secret-key").unwrap();
    std::fs::write(dir.join("plugins/a.jar"), "jar-1").unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stopped_server_backup_verify_and_restore() {
    let h = harness().await;
    let (id, _) = add_server(&h, "survival", "{}", true, 60).await;
    let dir = h.servers.path().join("survival");
    write_world(&dir);

    let bid = backup(&h, id, Some("first")).await;
    let b = h.core.backups.get(bid).await.unwrap();
    assert_eq!(b.status, BackupStatus::Ready);
    assert_eq!(b.kind, BackupKind::Manual);
    assert!(!b.live);
    assert!(
        b.contains_sensitive,
        "the Floodgate key is included and flagged"
    );
    assert_eq!(b.note.as_deref(), Some("first"));
    assert!(b.path.is_file() && b.path.starts_with(h.data.path().join("backups")));
    assert!(b.path.extension().is_some_and(|e| e == "zip"));
    assert_eq!(
        zip_text(&b.path, "world/level.dat").as_deref(),
        Some("original")
    );
    assert!(zip_text(&b.path, "mcpanel-manifest.json").is_some());

    let job = h.core.backups.verify(bid, "test").await.unwrap();
    let report = wait_job(&h.core, job).await.unwrap();
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["filesChecked"], 8, "{report}");

    // Change the server after the backup, including its recorded software.
    std::fs::write(dir.join("world/level.dat"), "changed!").unwrap();
    std::fs::write(dir.join("plugins/b.jar"), "new").unwrap();
    let mut s = h.core.servers.get(id).await.unwrap();
    s.software.game_version = "1.21.5".into();
    h.db.repositories().servers.update(&s).await.unwrap();

    let preview = h.core.backups.restore_preview(bid).await.unwrap();
    assert_eq!(preview.removed, 1);
    assert_eq!(preview.changed_jars, vec!["plugins/b.jar"]);

    let job = h.core.backups.restore(bid, "test").await.unwrap();
    let r = wait_job(&h.core, job).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("world/level.dat")).unwrap(),
        "original"
    );
    assert!(!dir.join("plugins/b.jar").exists());
    assert_eq!(
        h.core.servers.get(id).await.unwrap().software.game_version,
        "1.21.4",
        "the software recorded in the backup is restored"
    );

    // The state before the restore was saved and is protected from retention.
    let pre: BackupId = r["preRestoreBackupId"].as_str().unwrap().parse().unwrap();
    let pre = h.core.backups.get(pre).await.unwrap();
    assert_eq!(pre.kind, BackupKind::PreRestore);
    assert!(pre.protected);
    assert_eq!(
        zip_text(&pre.path, "world/level.dat").as_deref(),
        Some("changed!")
    );

    // Deleting removes the file.
    h.core.backups.delete(bid, "test").await.unwrap();
    assert!(!b.path.exists());
    assert_eq!(h.core.backups.list(Some(id)).await.unwrap().len(), 1);
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_backup_pauses_saving_and_blocks_conflicting_operations() {
    let h = harness().await;
    let (id, _) = add_server(
        &h,
        "live",
        r#"{"world": true, "save_delay_ms": 1500}"#,
        true,
        60,
    )
    .await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;

    let job = h
        .core
        .backups
        .create(
            CreateBackupRequest {
                server_id: id,
                note: None,
            },
            "test",
        )
        .await
        .unwrap();
    // While the backup runs: no second backup, no restart, no restore.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let busy = h
        .core
        .backups
        .create(
            CreateBackupRequest {
                server_id: id,
                note: None,
            },
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(busy.code, ErrorCode::ServerBusy);
    assert_eq!(
        h.core.servers.restart(id, "test").await.unwrap_err().code,
        ErrorCode::ServerBusy
    );
    let r = wait_job(&h.core, job).await.unwrap();
    let bid: BackupId = r["backupId"].as_str().unwrap().parse().unwrap();
    let b = h.core.backups.get(bid).await.unwrap();
    assert!(b.live);
    // The flushed world is in the backup; the locked session.lock is excluded.
    assert_eq!(
        zip_text(&b.path, "world/level.dat").as_deref(),
        Some("saves=1")
    );
    assert!(zip_text(&b.path, "world/session.lock").is_none());
    assert!(b.skipped.is_empty(), "{:?}", b.skipped);

    // Saving was paused and resumed, in that order.
    let commands: Vec<String> = h
        .core
        .servers
        .console(id)
        .snapshot(None, 500)
        .into_iter()
        .filter(|l| l.stream == ConsoleStream::Command)
        .map(|l| l.text)
        .collect();
    assert_eq!(commands, vec!["save-off", "save-all flush", "save-on"]);
    assert_eq!(
        h.core.servers.view(id).await.unwrap().runtime.state,
        LifecycleState::Running
    );

    // Restoring needs a stopped server.
    assert_eq!(
        h.core.backups.restore(bid, "test").await.unwrap_err().code,
        ErrorCode::ServerBusy
    );
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn scheduler_runs_due_backups_and_applies_retention() {
    let h = harness().await;
    let (id, _) = add_server(&h, "sched", "{}", true, 60).await;
    write_world(&h.servers.path().join("sched"));
    let manual = backup(&h, id, None).await;

    let policy = h
        .core
        .backups
        .set_policy(
            BackupPolicy {
                enabled: true,
                interval_minutes: 15,
                skip_if_idle: false,
                retention: Retention {
                    keep_last: 2,
                    keep_daily: 0,
                    keep_weekly: 0,
                    keep_monthly: 0,
                },
                ..BackupPolicy::default_for(id)
            },
            "test",
        )
        .await
        .unwrap();
    let start = policy.last_run_at.unwrap().millis();
    // Not due yet.
    assert!(
        h.core
            .backups
            .tick(Timestamp(start + 60_000))
            .await
            .unwrap()
            .is_empty()
    );
    for n in 1..=3 {
        let jobs = h
            .core
            .backups
            .tick(Timestamp(start + n * 16 * 60_000))
            .await
            .unwrap();
        assert_eq!(jobs.len(), 1, "tick {n}");
        let r = wait_job(&h.core, jobs[0]).await.unwrap();
        assert_eq!(r["removed"], if n == 3 { 1 } else { 0 });
    }
    let all = h.core.backups.list(Some(id)).await.unwrap();
    let scheduled = all
        .iter()
        .filter(|b| b.backup.kind == BackupKind::Scheduled)
        .count();
    assert_eq!(scheduled, 2, "retention keeps the newest two");
    assert!(
        all.iter().any(|b| b.backup.id == manual),
        "manual backups are never pruned"
    );
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_servers_are_not_backed_up_again() {
    let h = harness().await;
    let (id, _) = add_server(&h, "idle", "{}", true, 60).await;
    let policy = h
        .core
        .backups
        .set_policy(
            BackupPolicy {
                enabled: true,
                interval_minutes: 15,
                ..BackupPolicy::default_for(id)
            },
            "test",
        )
        .await
        .unwrap();
    let start = policy.last_run_at.unwrap().millis();
    // No backup exists yet → one is made.
    let jobs = h
        .core
        .backups
        .tick(Timestamp(start + 16 * 60_000))
        .await
        .unwrap();
    assert_eq!(jobs.len(), 1);
    wait_job(&h.core, jobs[0]).await.unwrap();
    // The server has not run since → skipped, and the schedule moves on.
    let jobs = h
        .core
        .backups
        .tick(Timestamp(start + 32 * 60_000))
        .await
        .unwrap();
    assert!(jobs.is_empty());
    assert_eq!(h.core.backups.list(Some(id)).await.unwrap().len(), 1);
    let p = h.core.backups.policy(id).await.unwrap();
    assert_eq!(p.last_run_at, Some(Timestamp(start + 32 * 60_000)));

    // After the server ran, the next slot backs up again.
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    let jobs = h
        .core
        .backups
        .tick(Timestamp(start + 48 * 60_000))
        .await
        .unwrap();
    assert_eq!(jobs.len(), 1);
    wait_job(&h.core, jobs[0]).await.unwrap();
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn backups_folder_must_not_overlap_a_server() {
    let h = harness().await;
    let (id, _) = add_server(&h, "overlap", "{}", true, 60).await;
    let inside = h.servers.path().join("overlap").join("backups");
    let err = h
        .core
        .backups
        .set_backups_dir(Some(inside), "test")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::DirectoryNotAllowed);
    let err = h
        .core
        .backups
        .set_backups_dir(Some(h.servers.path().to_path_buf()), "test")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::DirectoryNotAllowed);
    let elsewhere = h.data.path().join("elsewhere");
    h.core
        .backups
        .set_backups_dir(Some(elsewhere.clone()), "test")
        .await
        .unwrap();
    let bid = backup(&h, id, None).await;
    assert!(
        h.core
            .backups
            .get(bid)
            .await
            .unwrap()
            .path
            .starts_with(&elsewhere)
    );
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn encrypted_backups_need_the_key_and_the_recovery_kit_restores_it() {
    use mcpanel_core::ports::SecretStore;
    use secrecy::SecretString;
    let h = harness().await;
    let (id, _) = add_server(&h, "vault", "{}", true, 60).await;
    let dir = h.servers.path().join("vault");
    write_world(&dir);
    let enc = &h.core.encryption;
    let kit = h.data.path().join("recovery-kit.txt");

    let short = enc
        .setup(SecretString::from("short".to_string()), &kit)
        .await
        .unwrap_err();
    assert_eq!(short.code, ErrorCode::InvalidInput);
    let st = enc
        .setup(
            SecretString::from("a long enough passphrase".to_string()),
            &kit,
        )
        .await
        .unwrap();
    assert!(st.configured && st.key_available && st.encrypt_backups);
    let recipient = st.recipient.clone().unwrap();
    let kit_text = std::fs::read_to_string(&kit).unwrap();
    assert!(kit_text.contains(&recipient) && kit_text.contains("BEGIN AGE ENCRYPTED FILE"));
    assert!(
        !kit_text.contains("AGE-SECRET-KEY"),
        "the kit holds no plaintext key"
    );

    let bid = backup(&h, id, None).await;
    let b = h.core.backups.get(bid).await.unwrap();
    assert!(b.encrypted);
    assert!(
        b.path.to_string_lossy().ends_with(".zip.age"),
        "{}",
        b.path.display()
    );
    assert!(zip::ZipArchive::new(std::fs::File::open(&b.path).unwrap()).is_err());
    let r = wait_job(&h.core, h.core.backups.verify(bid, "test").await.unwrap())
        .await
        .unwrap();
    assert_eq!(r["ok"], true, "{r}");

    // Restore through decryption; no plaintext copy is left behind.
    std::fs::write(dir.join("world/level.dat"), "changed").unwrap();
    wait_job(&h.core, h.core.backups.restore(bid, "test").await.unwrap())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("world/level.dat")).unwrap(),
        "original"
    );
    let leftovers: Vec<_> = std::fs::read_dir(b.path.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".mcpanel-plain-")
        })
        .collect();
    assert!(leftovers.is_empty());

    // Without the key (e.g. a new computer) the backup cannot be opened…
    h.secrets.delete(mcpanel_core::crypto::MASTER_KEY).unwrap();
    let st = enc.status().await.unwrap();
    assert!(st.configured && !st.key_available && !st.encrypt_backups);
    let e = h.core.backups.restore(bid, "test").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    // …and new backups are not encrypted to a key that is not here.
    let plain = backup(&h, id, None).await;
    assert!(!h.core.backups.get(plain).await.unwrap().encrypted);

    // The Recovery Kit with its passphrase brings the key back.
    let wrong = enc
        .import(&kit, SecretString::from("not the passphrase".to_string()))
        .await
        .unwrap_err();
    assert_eq!(wrong.code, ErrorCode::InvalidInput);
    let st = enc
        .import(
            &kit,
            SecretString::from("a long enough passphrase".to_string()),
        )
        .await
        .unwrap();
    assert!(st.key_available && st.encrypt_backups);
    assert_eq!(st.recipient.as_deref(), Some(recipient.as_str()));
    let r = wait_job(&h.core, h.core.backups.verify(bid, "test").await.unwrap())
        .await
        .unwrap();
    assert_eq!(r["ok"], true);

    // Encryption can be switched off for new backups.
    enc.set_encrypt_backups(false).await.unwrap();
    let off = backup(&h, id, None).await;
    assert!(!h.core.backups.get(off).await.unwrap().encrypted);
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn size_cap_removes_the_oldest_scheduled_backups() {
    use mcpanel_core::backup::BackupRecord;
    let h = harness().await;
    let (id, _) = add_server(&h, "capped", "{}", true, 60).await;
    let dir = h.data.path().join("fake-backups");
    std::fs::create_dir_all(&dir).unwrap();
    // Three scheduled backups recorded at 600 MiB each, one manual one.
    let mut ids = Vec::new();
    for (i, kind) in [
        BackupKind::Scheduled,
        BackupKind::Scheduled,
        BackupKind::Manual,
        BackupKind::Scheduled,
    ]
    .into_iter()
    .enumerate()
    {
        let path = dir.join(format!("b{i}.zip"));
        std::fs::write(&path, b"x").unwrap();
        let b = BackupRecord {
            id: BackupId::new(),
            server_id: Some(id),
            server_name: "capped".into(),
            kind,
            status: BackupStatus::Ready,
            path,
            created_at: Timestamp(1_000_000 + i as i64 * 60_000),
            finished_at: Some(Timestamp(1_000_000 + i as i64 * 60_000)),
            size_bytes: 600 << 20,
            content_bytes: 0,
            file_count: 0,
            sha256: None,
            live: false,
            contains_sensitive: false,
            encrypted: false,
            software_id: "fake".into(),
            game_version: "1.21.4".into(),
            note: None,
            protected: false,
            skipped: vec![],
            error_message: None,
        };
        h.db.repositories().backups.insert(&b).await.unwrap();
        ids.push(b.id);
    }
    let keep_all = Retention {
        keep_last: 10,
        keep_daily: 0,
        keep_weekly: 0,
        keep_monthly: 0,
    };
    // Without a cap GFS keeps everything.
    assert_eq!(
        h.core.backups.apply_retention(id, keep_all).await.unwrap(),
        0
    );
    h.core
        .backups
        .set_policy(
            BackupPolicy {
                retention: keep_all,
                max_total_gb: 1,
                ..BackupPolicy::default_for(id)
            },
            "test",
        )
        .await
        .unwrap();
    // 1 GiB fits one 600 MiB scheduled backup: the two older scheduled ones go; the
    // manual backup is never removed by retention.
    assert_eq!(
        h.core.backups.apply_retention(id, keep_all).await.unwrap(),
        2
    );
    let left: Vec<BackupId> = h
        .core
        .backups
        .list(Some(id))
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.backup.id)
        .collect();
    assert!(left.contains(&ids[3]) && left.contains(&ids[2]));
    assert!(!left.contains(&ids[0]) && !left.contains(&ids[1]));
    h.finish().await;
}
