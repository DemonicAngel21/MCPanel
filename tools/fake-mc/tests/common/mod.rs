//! Helpers shared by the integration tests (each test crate uses a subset).
#![allow(dead_code)]

pub mod content_fixture;
pub mod harness;
pub mod mc_client;
pub mod real;

use std::time::{Duration, Instant};

/// Close the database, then delete the test directories and fail if that is impossible.
///
/// `TempDir`'s `Drop` ignores errors, and on Windows a directory holding an open file
/// cannot be deleted, so a test that forgets to release files would silently leave data
/// in `%TEMP%`. Background writers (console capture) close their files shortly after the
/// server stops, so deletion is retried for a few seconds.
pub async fn cleanup(db: &mcpanel_db::Database, dirs: Vec<tempfile::TempDir>) {
    db.close().await;
    for dir in dirs {
        let path = dir.keep();
        let started = Instant::now();
        loop {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(e) if started.elapsed() < Duration::from_secs(10) => {
                    let _ = e;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => panic!("test left files behind in {}: {e}", path.display()),
            }
        }
    }
}
