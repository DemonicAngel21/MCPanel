//! Installed content records and queued content changes.

use crate::db_err;
use crate::repos::SqliteRepos;
use async_trait::async_trait;
use mcpanel_core::content::{ContentKind, ContentSource, InstalledContent, PendingChange};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ids::ServerId;
use mcpanel_core::ports::ContentRepository;
use mcpanel_core::time::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use std::str::FromStr;

fn corrupt(what: &str) -> CoreError {
    CoreError::new(ErrorCode::Database, format!("Corrupt {what} in database"))
}

fn row_to_content(r: &SqliteRow) -> CoreResult<InstalledContent> {
    let g = |e| db_err(e);
    let provider: Option<String> = r.try_get("provider").map_err(g)?;
    let project_id: Option<String> = r.try_get("project_id").map_err(g)?;
    let version_id: Option<String> = r.try_get("version_id").map_err(g)?;
    Ok(InstalledContent {
        server_id: ServerId::from_str(&r.try_get::<String, _>("server_id").map_err(g)?)
            .map_err(|_| corrupt("server id"))?,
        kind: ContentKind::parse(&r.try_get::<String, _>("kind").map_err(g)?)
            .ok_or_else(|| corrupt("content kind"))?,
        file_name: r.try_get("file_name").map_err(g)?,
        name: r.try_get("name").map_err(g)?,
        version_number: r.try_get("version_number").map_err(g)?,
        source: match (provider, project_id, version_id) {
            (Some(provider), Some(project_id), Some(version_id)) => Some(ContentSource {
                provider,
                project_id,
                version_id,
            }),
            _ => None,
        },
        sha512: r.try_get("sha512").map_err(g)?,
        installed_at: Timestamp(r.try_get("installed_at").map_err(g)?),
        updated_at: Timestamp(r.try_get("updated_at").map_err(g)?),
    })
}

#[async_trait]
impl ContentRepository for SqliteRepos {
    async fn installed(&self, server_id: ServerId) -> CoreResult<Vec<InstalledContent>> {
        sqlx::query(
            "SELECT * FROM installed_content WHERE server_id = ? ORDER BY name COLLATE NOCASE",
        )
        .bind(server_id.to_string())
        .fetch_all(self.pool())
        .await
        .map_err(db_err)?
        .iter()
        .map(row_to_content)
        .collect()
    }

    async fn upsert(&self, c: &InstalledContent) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO installed_content (server_id, kind, file_name, name, version_number, provider,
                project_id, version_id, sha512, installed_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (server_id, kind, file_name) DO UPDATE SET
                name = excluded.name, version_number = excluded.version_number,
                provider = excluded.provider, project_id = excluded.project_id,
                version_id = excluded.version_id, sha512 = excluded.sha512,
                updated_at = excluded.updated_at",
        )
        .bind(c.server_id.to_string())
        .bind(c.kind.as_str())
        .bind(&c.file_name)
        .bind(&c.name)
        .bind(&c.version_number)
        .bind(c.source.as_ref().map(|s| s.provider.clone()))
        .bind(c.source.as_ref().map(|s| s.project_id.clone()))
        .bind(c.source.as_ref().map(|s| s.version_id.clone()))
        .bind(&c.sha512)
        .bind(c.installed_at.millis())
        .bind(c.updated_at.millis())
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn remove(
        &self,
        server_id: ServerId,
        kind: ContentKind,
        file_name: &str,
    ) -> CoreResult<()> {
        sqlx::query(
            "DELETE FROM installed_content WHERE server_id = ? AND kind = ? AND file_name = ?",
        )
        .bind(server_id.to_string())
        .bind(kind.as_str())
        .bind(file_name)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn pending(&self, server_id: ServerId) -> CoreResult<Vec<PendingChange>> {
        sqlx::query(
            "SELECT change_json FROM pending_changes WHERE server_id = ? ORDER BY created_at, id",
        )
        .bind(server_id.to_string())
        .fetch_all(self.pool())
        .await
        .map_err(db_err)?
        .iter()
        .map(|r| {
            let json: String = r.try_get("change_json").map_err(db_err)?;
            serde_json::from_str(&json).map_err(|_| corrupt("pending change"))
        })
        .collect()
    }

    async fn add_pending(&self, p: &PendingChange) -> CoreResult<()> {
        let json = serde_json::to_string(p).map_err(|e| CoreError::internal(e.to_string()))?;
        sqlx::query("INSERT INTO pending_changes (id, server_id, kind, change_json, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(&p.id)
            .bind(p.server_id.to_string())
            .bind(p.kind.as_str())
            .bind(json)
            .bind(p.created_at.millis())
            .execute(self.pool())
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn remove_pending(&self, id: &str) -> CoreResult<()> {
        sqlx::query("DELETE FROM pending_changes WHERE id = ?")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn servers_with_pending(&self) -> CoreResult<Vec<ServerId>> {
        sqlx::query("SELECT DISTINCT server_id FROM pending_changes")
            .fetch_all(self.pool())
            .await
            .map_err(db_err)?
            .iter()
            .map(|r| {
                let s: String = r.try_get("server_id").map_err(db_err)?;
                ServerId::from_str(&s).map_err(|_| corrupt("server id"))
            })
            .collect()
    }
}
