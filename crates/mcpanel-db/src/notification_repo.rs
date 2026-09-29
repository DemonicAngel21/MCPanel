//! Notification inbox.

use crate::db_err;
use crate::repos::SqliteRepos;
use async_trait::async_trait;
use mcpanel_core::error::CoreResult;
use mcpanel_core::ids::ServerId;
use mcpanel_core::notify::{Category, Notification, Severity};
use mcpanel_core::ports::NotificationRepository;
use mcpanel_core::time::Timestamp;
use sqlx::Row;
use std::str::FromStr;

#[async_trait]
impl NotificationRepository for SqliteRepos {
    async fn insert(&self, n: &Notification) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO notifications (id, created_at, server_id, category, severity, title, body, read)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&n.id)
        .bind(n.created_at.millis())
        .bind(n.server_id.map(|s| s.to_string()))
        .bind(n.category.as_str())
        .bind(n.severity.as_str())
        .bind(&n.title)
        .bind(&n.body)
        .bind(n.read)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn list(&self, limit: u32) -> CoreResult<Vec<Notification>> {
        let rows =
            sqlx::query("SELECT * FROM notifications ORDER BY created_at DESC, id DESC LIMIT ?")
                .bind(limit as i64)
                .fetch_all(self.pool())
                .await
                .map_err(db_err)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            // Rows of categories this version does not know are skipped.
            let Some(category) =
                Category::parse(&r.try_get::<String, _>("category").map_err(db_err)?)
            else {
                continue;
            };
            out.push(Notification {
                id: r.try_get("id").map_err(db_err)?,
                created_at: Timestamp(r.try_get("created_at").map_err(db_err)?),
                server_id: r
                    .try_get::<Option<String>, _>("server_id")
                    .map_err(db_err)?
                    .and_then(|s| ServerId::from_str(&s).ok()),
                category,
                severity: Severity::parse(&r.try_get::<String, _>("severity").map_err(db_err)?),
                title: r.try_get("title").map_err(db_err)?,
                body: r.try_get("body").map_err(db_err)?,
                read: r.try_get("read").map_err(db_err)?,
            });
        }
        Ok(out)
    }

    async fn unread_count(&self) -> CoreResult<u32> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE read = 0")
            .fetch_one(self.pool())
            .await
            .map_err(db_err)?;
        Ok(n.max(0) as u32)
    }

    async fn mark_read(&self, ids: Option<&[String]>) -> CoreResult<()> {
        match ids {
            None => {
                sqlx::query("UPDATE notifications SET read = 1 WHERE read = 0")
                    .execute(self.pool())
                    .await
                    .map_err(db_err)?;
            }
            Some(ids) => {
                for id in ids {
                    sqlx::query("UPDATE notifications SET read = 1 WHERE id = ?")
                        .bind(id)
                        .execute(self.pool())
                        .await
                        .map_err(db_err)?;
                }
            }
        }
        Ok(())
    }

    async fn clear(&self) -> CoreResult<()> {
        sqlx::query("DELETE FROM notifications")
            .execute(self.pool())
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn prune(&self, keep: u32) -> CoreResult<()> {
        sqlx::query(
            "DELETE FROM notifications WHERE id NOT IN
               (SELECT id FROM notifications ORDER BY created_at DESC, id DESC LIMIT ?)",
        )
        .bind(keep as i64)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }
}
