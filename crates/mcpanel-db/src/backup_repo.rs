//! Backup records and policies.

use crate::db_err;
use crate::repos::SqliteRepos;
use async_trait::async_trait;
use mcpanel_core::backup::{BackupKind, BackupPolicy, BackupRecord, BackupStatus, Retention};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ids::{BackupId, ServerId};
use mcpanel_core::ports::BackupRepository;
use mcpanel_core::time::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use std::path::PathBuf;
use std::str::FromStr;

fn corrupt(what: &str) -> CoreError {
    CoreError::new(ErrorCode::Database, format!("Corrupt {what} in database"))
}

fn row_to_backup(r: &SqliteRow) -> CoreResult<BackupRecord> {
    let g = |e| db_err(e);
    let id: String = r.try_get("id").map_err(g)?;
    let server_id: Option<String> = r.try_get("server_id").map_err(g)?;
    let kind: String = r.try_get("kind").map_err(g)?;
    let skipped: String = r.try_get("skipped_json").map_err(g)?;
    Ok(BackupRecord {
        id: BackupId::from_str(&id).map_err(|_| corrupt("backup id"))?,
        server_id: server_id
            .map(|s| ServerId::from_str(&s).map_err(|_| corrupt("server id")))
            .transpose()?,
        server_name: r.try_get("server_name").map_err(g)?,
        kind: BackupKind::parse(&kind).ok_or_else(|| corrupt("backup kind"))?,
        status: BackupStatus::parse(&r.try_get::<String, _>("status").map_err(g)?),
        path: PathBuf::from(r.try_get::<String, _>("path").map_err(g)?),
        created_at: Timestamp(r.try_get("created_at").map_err(g)?),
        finished_at: r
            .try_get::<Option<i64>, _>("finished_at")
            .map_err(g)?
            .map(Timestamp),
        size_bytes: r.try_get::<i64, _>("size_bytes").map_err(g)? as u64,
        content_bytes: r.try_get::<i64, _>("content_bytes").map_err(g)? as u64,
        file_count: r.try_get::<i64, _>("file_count").map_err(g)? as u64,
        sha256: r.try_get("sha256").map_err(g)?,
        live: r.try_get("live").map_err(g)?,
        contains_sensitive: r.try_get("contains_sensitive").map_err(g)?,
        encrypted: r.try_get("encrypted").map_err(g)?,
        software_id: r.try_get("software_id").map_err(g)?,
        game_version: r.try_get("game_version").map_err(g)?,
        note: r.try_get("note").map_err(g)?,
        protected: r.try_get("protected").map_err(g)?,
        skipped: serde_json::from_str(&skipped).unwrap_or_default(),
        error_message: r.try_get("error_message").map_err(g)?,
    })
}

fn row_to_policy(r: &SqliteRow) -> CoreResult<BackupPolicy> {
    let g = |e| db_err(e);
    let id: String = r.try_get("server_id").map_err(g)?;
    let n =
        |col: &str| -> CoreResult<u32> { Ok(r.try_get::<i64, _>(col).map_err(g)?.max(0) as u32) };
    Ok(BackupPolicy {
        server_id: ServerId::from_str(&id).map_err(|_| corrupt("server id"))?,
        enabled: r.try_get("enabled").map_err(g)?,
        interval_minutes: n("interval_minutes")?,
        skip_if_idle: r.try_get("skip_if_idle").map_err(g)?,
        retention: Retention {
            keep_last: n("keep_last")?,
            keep_daily: n("keep_daily")?,
            keep_weekly: n("keep_weekly")?,
            keep_monthly: n("keep_monthly")?,
        },
        max_total_gb: n("max_total_gb")?,
        last_run_at: r
            .try_get::<Option<i64>, _>("last_run_at")
            .map_err(g)?
            .map(Timestamp),
    })
}

fn skipped_json(b: &BackupRecord) -> CoreResult<String> {
    serde_json::to_string(&b.skipped).map_err(|e| CoreError::internal(e.to_string()))
}

#[async_trait]
impl BackupRepository for SqliteRepos {
    async fn insert(&self, b: &BackupRecord) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO backups (id, server_id, server_name, kind, status, path, created_at, finished_at,
                size_bytes, content_bytes, file_count, sha256, live, contains_sensitive, software_id,
                game_version, note, protected, skipped_json, error_message, encrypted)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(b.id.to_string())
        .bind(b.server_id.map(|s| s.to_string()))
        .bind(&b.server_name)
        .bind(b.kind.as_str())
        .bind(b.status.as_str())
        .bind(b.path.to_string_lossy().to_string())
        .bind(b.created_at.millis())
        .bind(b.finished_at.map(|t| t.millis()))
        .bind(b.size_bytes as i64)
        .bind(b.content_bytes as i64)
        .bind(b.file_count as i64)
        .bind(&b.sha256)
        .bind(b.live)
        .bind(b.contains_sensitive)
        .bind(&b.software_id)
        .bind(&b.game_version)
        .bind(&b.note)
        .bind(b.protected)
        .bind(skipped_json(b)?)
        .bind(&b.error_message)
        .bind(b.encrypted)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn update(&self, b: &BackupRecord) -> CoreResult<()> {
        sqlx::query(
            "UPDATE backups SET status = ?, finished_at = ?, size_bytes = ?, content_bytes = ?,
                file_count = ?, sha256 = ?, contains_sensitive = ?, note = ?, protected = ?,
                skipped_json = ?, error_message = ?, encrypted = ?, path = ?
             WHERE id = ?",
        )
        .bind(b.status.as_str())
        .bind(b.finished_at.map(|t| t.millis()))
        .bind(b.size_bytes as i64)
        .bind(b.content_bytes as i64)
        .bind(b.file_count as i64)
        .bind(&b.sha256)
        .bind(b.contains_sensitive)
        .bind(&b.note)
        .bind(b.protected)
        .bind(skipped_json(b)?)
        .bind(&b.error_message)
        .bind(b.encrypted)
        .bind(b.path.to_string_lossy().to_string())
        .bind(b.id.to_string())
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn get(&self, id: BackupId) -> CoreResult<Option<BackupRecord>> {
        sqlx::query("SELECT * FROM backups WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.pool())
            .await
            .map_err(db_err)?
            .as_ref()
            .map(row_to_backup)
            .transpose()
    }

    async fn list(&self, server_id: Option<ServerId>) -> CoreResult<Vec<BackupRecord>> {
        sqlx::query(
            "SELECT * FROM backups WHERE (?1 IS NULL OR server_id = ?1)
             ORDER BY created_at DESC, id DESC",
        )
        .bind(server_id.map(|s| s.to_string()))
        .fetch_all(self.pool())
        .await
        .map_err(db_err)?
        .iter()
        .map(row_to_backup)
        .collect()
    }

    async fn delete(&self, id: BackupId) -> CoreResult<()> {
        sqlx::query("DELETE FROM backups WHERE id = ?")
            .bind(id.to_string())
            .execute(self.pool())
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn policy(&self, server_id: ServerId) -> CoreResult<Option<BackupPolicy>> {
        sqlx::query("SELECT * FROM backup_policies WHERE server_id = ?")
            .bind(server_id.to_string())
            .fetch_optional(self.pool())
            .await
            .map_err(db_err)?
            .as_ref()
            .map(row_to_policy)
            .transpose()
    }

    async fn policies(&self) -> CoreResult<Vec<BackupPolicy>> {
        sqlx::query("SELECT * FROM backup_policies")
            .fetch_all(self.pool())
            .await
            .map_err(db_err)?
            .iter()
            .map(row_to_policy)
            .collect()
    }

    async fn save_policy(&self, p: &BackupPolicy) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO backup_policies (server_id, enabled, interval_minutes, skip_if_idle,
                keep_last, keep_daily, keep_weekly, keep_monthly, last_run_at, max_total_gb)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (server_id) DO UPDATE SET enabled = excluded.enabled,
                interval_minutes = excluded.interval_minutes, skip_if_idle = excluded.skip_if_idle,
                keep_last = excluded.keep_last, keep_daily = excluded.keep_daily,
                keep_weekly = excluded.keep_weekly, keep_monthly = excluded.keep_monthly,
                last_run_at = excluded.last_run_at, max_total_gb = excluded.max_total_gb",
        )
        .bind(p.server_id.to_string())
        .bind(p.enabled)
        .bind(p.interval_minutes as i64)
        .bind(p.skip_if_idle)
        .bind(p.retention.keep_last as i64)
        .bind(p.retention.keep_daily as i64)
        .bind(p.retention.keep_weekly as i64)
        .bind(p.retention.keep_monthly as i64)
        .bind(p.last_run_at.map(|t| t.millis()))
        .bind(p.max_total_gb as i64)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }
}
