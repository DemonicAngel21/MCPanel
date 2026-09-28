//! Restart policies and crash history.

use crate::db_err;
use crate::repos::SqliteRepos;
use async_trait::async_trait;
use mcpanel_core::crash::{CrashAction, CrashEvent, RestartPolicy};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ids::ServerId;
use mcpanel_core::ports::CrashRepository;
use mcpanel_core::time::Timestamp;
use sqlx::Row;
use std::str::FromStr;

fn corrupt() -> CoreError {
    CoreError::new(ErrorCode::Database, "Corrupt server id in database")
}

#[async_trait]
impl CrashRepository for SqliteRepos {
    async fn policy(&self, server_id: ServerId) -> CoreResult<Option<RestartPolicy>> {
        let row = sqlx::query("SELECT * FROM server_restart_policies WHERE server_id = ?")
            .bind(server_id.to_string())
            .fetch_optional(self.pool())
            .await
            .map_err(db_err)?;
        row.map(|r| {
            let n = |c: &str| -> CoreResult<u32> {
                Ok(r.try_get::<i64, _>(c).map_err(db_err)?.max(0) as u32)
            };
            Ok(RestartPolicy {
                server_id,
                enabled: r.try_get("enabled").map_err(db_err)?,
                max_attempts: n("max_attempts")?,
                window_secs: n("window_secs")?,
                delay_secs: n("delay_secs")?,
                stable_secs: n("stable_secs")?,
                crash_backup: r.try_get("crash_backup").map_err(db_err)?,
            })
        })
        .transpose()
    }

    async fn save_policy(&self, p: &RestartPolicy) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO server_restart_policies (server_id, enabled, max_attempts, window_secs, delay_secs, stable_secs, crash_backup)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (server_id) DO UPDATE SET enabled = excluded.enabled, max_attempts = excluded.max_attempts,
                window_secs = excluded.window_secs, delay_secs = excluded.delay_secs,
                stable_secs = excluded.stable_secs, crash_backup = excluded.crash_backup",
        )
        .bind(p.server_id.to_string())
        .bind(p.enabled)
        .bind(p.max_attempts as i64)
        .bind(p.window_secs as i64)
        .bind(p.delay_secs as i64)
        .bind(p.stable_secs as i64)
        .bind(p.crash_backup)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn insert(&self, e: &CrashEvent) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO crash_events (id, server_id, occurred_at, exit_code, kind, message, attempt, action,
                restart_at, crash_report, console_tail_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&e.id)
        .bind(e.server_id.to_string())
        .bind(e.occurred_at.millis())
        .bind(e.exit_code)
        .bind(&e.kind)
        .bind(&e.message)
        .bind(e.attempt as i64)
        .bind(e.action.as_str())
        .bind(e.restart_at.map(|t| t.millis()))
        .bind(&e.crash_report)
        .bind(serde_json::to_string(&e.console_tail).map_err(|x| CoreError::internal(x.to_string()))?)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        // Keep the newest 100 crashes per server.
        sqlx::query(
            "DELETE FROM crash_events WHERE server_id = ?1 AND id NOT IN
               (SELECT id FROM crash_events WHERE server_id = ?1 ORDER BY occurred_at DESC LIMIT 100)",
        )
        .bind(e.server_id.to_string())
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn recent(&self, server_id: ServerId, limit: u32) -> CoreResult<Vec<CrashEvent>> {
        let rows = sqlx::query("SELECT * FROM crash_events WHERE server_id = ? ORDER BY occurred_at DESC, id DESC LIMIT ?")
            .bind(server_id.to_string())
            .bind(limit as i64)
            .fetch_all(self.pool())
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|r| {
                let g = |e| db_err(e);
                let tail: String = r.try_get("console_tail_json").map_err(g)?;
                Ok(CrashEvent {
                    id: r.try_get("id").map_err(g)?,
                    server_id: ServerId::from_str(&r.try_get::<String, _>("server_id").map_err(g)?)
                        .map_err(|_| corrupt())?,
                    occurred_at: Timestamp(r.try_get("occurred_at").map_err(g)?),
                    exit_code: r
                        .try_get::<Option<i64>, _>("exit_code")
                        .map_err(g)?
                        .map(|c| c as i32),
                    kind: r.try_get("kind").map_err(g)?,
                    message: r.try_get("message").map_err(g)?,
                    attempt: r.try_get::<i64, _>("attempt").map_err(g)? as u32,
                    action: CrashAction::parse(&r.try_get::<String, _>("action").map_err(g)?),
                    restart_at: r
                        .try_get::<Option<i64>, _>("restart_at")
                        .map_err(g)?
                        .map(Timestamp),
                    crash_report: r.try_get("crash_report").map_err(g)?,
                    console_tail: serde_json::from_str(&tail).unwrap_or_default(),
                })
            })
            .collect()
    }
}
