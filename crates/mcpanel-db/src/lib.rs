//! # mcpanel-db
//!
//! SQLite persistence for MCPanel: connection setup (WAL, foreign keys), embedded
//! forward-only migrations with a pre-migration backup, and repository implementations
//! of the `mcpanel-core` ports.

mod repos;

use mcpanel_core::Repositories;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// How many pre-migration backups to keep next to the database.
const KEEP_BACKUPS: usize = 3;

pub(crate) fn db_err(e: sqlx::Error) -> CoreError {
    CoreError::new(ErrorCode::Database, format!("Database error: {e}"))
}

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub applied: Vec<i64>,
    pub backup: Option<PathBuf>,
}

impl Database {
    /// Open (or create) the database at `path` and apply pending migrations.
    pub async fn open(path: &Path) -> CoreResult<(Self, MigrationReport)> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| CoreError::io("Cannot create data directory", &e))?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .map_err(db_err)?;
        let db = Self { pool };
        match db.migrate(Some(path)).await {
            Ok(report) => Ok((db, report)),
            Err(e) => {
                // Release the file before reporting, so it can be restored or replaced.
                db.close().await;
                Err(e)
            }
        }
    }

    /// In-memory database for tests.
    pub async fn open_in_memory() -> CoreResult<Self> {
        let opts = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);
        // A single connection: every in-memory connection is a separate database.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .map_err(db_err)?;
        let db = Self { pool };
        db.migrate(None).await?;
        Ok(db)
    }

    async fn applied_versions(&self) -> CoreResult<Vec<i64>> {
        let exists: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        if exists.is_none() {
            return Ok(Vec::new());
        }
        let rows =
            sqlx::query("SELECT version FROM _sqlx_migrations WHERE success = 1 ORDER BY version")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        rows.iter()
            .map(|r| r.try_get::<i64, _>("version").map_err(db_err))
            .collect()
    }

    async fn migrate(&self, path: Option<&Path>) -> CoreResult<MigrationReport> {
        let known: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
        let applied = self.applied_versions().await?;
        if let Some(unknown) = applied.iter().find(|v| !known.contains(v)) {
            return Err(CoreError::new(
                ErrorCode::SchemaTooNew,
                format!(
                    "The MCPanel database was created by a newer version of MCPanel (schema {unknown}). Update MCPanel or restore a pre-migration backup."
                ),
            ));
        }
        let pending: Vec<i64> = known
            .iter()
            .copied()
            .filter(|v| !applied.contains(v))
            .collect();
        if pending.is_empty() {
            return Ok(MigrationReport {
                applied: Vec::new(),
                backup: None,
            });
        }
        let mut backup = None;
        if !applied.is_empty()
            && let Some(p) = path
        {
            backup = Some(self.backup_before_migration(p).await?);
        }
        MIGRATOR.run(&self.pool).await.map_err(|e| {
            CoreError::new(
                ErrorCode::Database,
                format!("Database migration failed: {e}"),
            )
        })?;
        tracing::info!(target: "mcpanel::db", ?pending, "applied database migrations");
        Ok(MigrationReport {
            applied: pending,
            backup,
        })
    }

    async fn backup_before_migration(&self, db_path: &Path) -> CoreResult<PathBuf> {
        let dir = db_path.parent().unwrap_or(Path::new("."));
        let stem = db_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "mcpanel.db".into());
        let ts = mcpanel_core::time::Timestamp::now().millis();
        let target = dir.join(format!("{stem}.pre-migration-{ts}.bak"));
        sqlx::query("VACUUM INTO ?")
            .bind(target.to_string_lossy().to_string())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        // Prune old backups.
        let prefix = format!("{stem}.pre-migration-");
        if let Ok(mut rd) = tokio::fs::read_dir(dir).await {
            let mut backups = Vec::new();
            while let Ok(Some(e)) = rd.next_entry().await {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with(&prefix) && n.ends_with(".bak") {
                    backups.push(e.path());
                }
            }
            backups.sort();
            let excess = backups.len().saturating_sub(KEEP_BACKUPS);
            for b in backups.into_iter().take(excess) {
                let _ = tokio::fs::remove_file(b).await;
            }
        }
        tracing::info!(target: "mcpanel::db", "pre-migration backup written");
        Ok(target)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn repositories(&self) -> Repositories {
        let r = Arc::new(repos::SqliteRepos::new(self.pool.clone()));
        Repositories {
            servers: r.clone(),
            java: r.clone(),
            audit: r.clone(),
            jobs: r.clone(),
            settings: r,
        }
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests;
