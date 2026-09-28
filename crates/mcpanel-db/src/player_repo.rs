//! Player session history.

use crate::db_err;
use crate::repos::SqliteRepos;
use async_trait::async_trait;
use mcpanel_core::error::CoreResult;
use mcpanel_core::ids::ServerId;
use mcpanel_core::ports::{PlayerRepository, PlayerStats};
use mcpanel_core::time::Timestamp;
use sqlx::Row;

#[async_trait]
impl PlayerRepository for SqliteRepos {
    async fn session_started(
        &self,
        server_id: ServerId,
        name: &str,
        at: Timestamp,
    ) -> CoreResult<()> {
        let mut tx = self.pool().begin().await.map_err(db_err)?;
        // A join without a leave (e.g. a missed event) ends the previous session.
        sqlx::query(
            "UPDATE player_sessions SET left_at = ?, interrupted = 1
             WHERE server_id = ? AND player_name = ? COLLATE NOCASE AND left_at IS NULL",
        )
        .bind(at.millis())
        .bind(server_id.to_string())
        .bind(name)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "INSERT INTO player_sessions (server_id, player_name, joined_at) VALUES (?, ?, ?)",
        )
        .bind(server_id.to_string())
        .bind(name)
        .bind(at.millis())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn session_ended(
        &self,
        server_id: ServerId,
        name: &str,
        at: Timestamp,
    ) -> CoreResult<()> {
        sqlx::query(
            "UPDATE player_sessions SET left_at = MAX(?, joined_at)
             WHERE server_id = ? AND player_name = ? COLLATE NOCASE AND left_at IS NULL",
        )
        .bind(at.millis())
        .bind(server_id.to_string())
        .bind(name)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn end_open_sessions(&self, server_id: ServerId, at: Timestamp) -> CoreResult<u64> {
        let r = sqlx::query(
            "UPDATE player_sessions SET left_at = MAX(?, joined_at) WHERE server_id = ? AND left_at IS NULL",
        )
        .bind(at.millis())
        .bind(server_id.to_string())
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(r.rows_affected())
    }

    async fn interrupt_open_sessions(&self) -> CoreResult<u64> {
        let r = sqlx::query(
            "UPDATE player_sessions SET left_at = joined_at, interrupted = 1 WHERE left_at IS NULL",
        )
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(r.rows_affected())
    }

    async fn stats(&self, server_id: ServerId) -> CoreResult<Vec<PlayerStats>> {
        let rows = sqlx::query(
            "SELECT (SELECT s2.player_name FROM player_sessions s2
                     WHERE s2.server_id = s.server_id AND s2.player_name = s.player_name COLLATE NOCASE
                     ORDER BY s2.joined_at DESC LIMIT 1) AS name,
                    MIN(joined_at) AS first_seen,
                    MAX(COALESCE(left_at, joined_at)) AS last_seen,
                    SUM(CASE WHEN interrupted = 0 AND left_at IS NOT NULL THEN left_at - joined_at ELSE 0 END) AS play_ms,
                    COUNT(*) AS sessions
             FROM player_sessions s WHERE server_id = ?
             GROUP BY player_name COLLATE NOCASE
             ORDER BY last_seen DESC",
        )
        .bind(server_id.to_string())
        .fetch_all(self.pool())
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(|r| {
                Ok(PlayerStats {
                    name: r.try_get("name").map_err(db_err)?,
                    first_seen: Timestamp(r.try_get("first_seen").map_err(db_err)?),
                    last_seen: Timestamp(r.try_get("last_seen").map_err(db_err)?),
                    total_play_ms: r.try_get("play_ms").map_err(db_err)?,
                    sessions: r.try_get::<i64, _>("sessions").map_err(db_err)? as u32,
                })
            })
            .collect()
    }
}
