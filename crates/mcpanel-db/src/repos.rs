//! Repository implementations. Domain types are mapped explicitly (no ORM leakage).

use crate::db_err;
use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ids::{AuditId, JavaRuntimeId, JobId, ServerId};
use mcpanel_core::jobs::{JobRecord, JobStatus};
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::model::{
    AuditEntry, AuditQuery, AuditResult, InstalledSoftware, JavaRuntime, JavaSource, LaunchConfig,
    RuntimeStateRecord, Server,
};
use mcpanel_core::ports::{
    AuditRepository, JavaRuntimeRepository, JobRepository, ServerRepository, SettingsRepository,
};
use mcpanel_core::time::Timestamp;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::str::FromStr;

pub struct SqliteRepos {
    pool: SqlitePool,
}

impl SqliteRepos {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn parse_id<T: FromStr<Err = CoreError>>(s: &str) -> CoreResult<T> {
    T::from_str(s)
        .map_err(|_| CoreError::new(ErrorCode::Database, format!("Corrupt id in database: {s}")))
}

fn opt_id<T: FromStr<Err = CoreError>>(s: Option<String>) -> CoreResult<Option<T>> {
    s.map(|v| parse_id(&v)).transpose()
}

fn ts(v: Option<i64>) -> Option<Timestamp> {
    v.map(Timestamp)
}

fn json_vec(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

fn to_json<T: serde::Serialize>(v: &T) -> CoreResult<String> {
    serde_json::to_string(v).map_err(|e| CoreError::internal(e.to_string()))
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.message().contains("UNIQUE"))
}

macro_rules! server_select {
    () => {
        "SELECT s.id, s.name, s.directory, s.software_id, s.game_version, s.build, s.jar,
        s.java_min_major, s.java_recommended_major, s.created_at, s.updated_at,
        l.java_runtime_id, l.min_memory_mb, l.max_memory_mb, l.jvm_args_json, l.server_args_json, l.stop_timeout_secs
     FROM servers s LEFT JOIN server_launch_configs l ON l.server_id = s.id"
    };
}
const SERVER_SELECT: &str = server_select!();
const SERVER_SELECT_BY_ID: &str = concat!(server_select!(), " WHERE s.id = ?");

fn row_to_server(r: &SqliteRow) -> CoreResult<Server> {
    let g = |e| db_err(e);
    Ok(Server {
        id: parse_id(&r.try_get::<String, _>("id").map_err(g)?)?,
        name: r.try_get("name").map_err(g)?,
        directory: PathBuf::from(r.try_get::<String, _>("directory").map_err(g)?),
        software: InstalledSoftware {
            software_id: r.try_get("software_id").map_err(g)?,
            game_version: r.try_get("game_version").map_err(g)?,
            build: r.try_get("build").map_err(g)?,
            jar: r.try_get("jar").map_err(g)?,
            java_min_major: r
                .try_get::<Option<i64>, _>("java_min_major")
                .map_err(g)?
                .map(|v| v as u32),
            java_recommended_major: r
                .try_get::<Option<i64>, _>("java_recommended_major")
                .map_err(g)?
                .map(|v| v as u32),
        },
        launch: LaunchConfig {
            java_runtime_id: opt_id(r.try_get("java_runtime_id").map_err(g)?)?,
            min_memory_mb: r
                .try_get::<Option<i64>, _>("min_memory_mb")
                .map_err(g)?
                .unwrap_or(1024) as u32,
            max_memory_mb: r
                .try_get::<Option<i64>, _>("max_memory_mb")
                .map_err(g)?
                .unwrap_or(2048) as u32,
            jvm_args: json_vec(
                &r.try_get::<Option<String>, _>("jvm_args_json")
                    .map_err(g)?
                    .unwrap_or_default(),
            ),
            server_args: json_vec(
                &r.try_get::<Option<String>, _>("server_args_json")
                    .map_err(g)?
                    .unwrap_or_default(),
            ),
            stop_timeout_secs: r
                .try_get::<Option<i64>, _>("stop_timeout_secs")
                .map_err(g)?
                .unwrap_or(LaunchConfig::DEFAULT_STOP_TIMEOUT_SECS as i64)
                as u32,
        },
        created_at: Timestamp(r.try_get("created_at").map_err(g)?),
        updated_at: Timestamp(r.try_get("updated_at").map_err(g)?),
    })
}

async fn upsert_launch(tx: &mut sqlx::SqliteConnection, s: &Server) -> CoreResult<()> {
    sqlx::query(
        "INSERT INTO server_launch_configs (server_id, java_runtime_id, min_memory_mb, max_memory_mb, jvm_args_json, server_args_json, stop_timeout_secs)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (server_id) DO UPDATE SET java_runtime_id = excluded.java_runtime_id,
            min_memory_mb = excluded.min_memory_mb, max_memory_mb = excluded.max_memory_mb,
            jvm_args_json = excluded.jvm_args_json, server_args_json = excluded.server_args_json,
            stop_timeout_secs = excluded.stop_timeout_secs",
    )
    .bind(s.id.to_string())
    .bind(s.launch.java_runtime_id.map(|j| j.to_string()))
    .bind(s.launch.min_memory_mb as i64)
    .bind(s.launch.max_memory_mb as i64)
    .bind(to_json(&s.launch.jvm_args)?)
    .bind(to_json(&s.launch.server_args)?)
    .bind(s.launch.stop_timeout_secs as i64)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[async_trait]
impl ServerRepository for SqliteRepos {
    async fn list(&self) -> CoreResult<Vec<Server>> {
        let rows = sqlx::query(SERVER_SELECT)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(row_to_server).collect()
    }

    async fn get(&self, id: ServerId) -> CoreResult<Option<Server>> {
        let row = sqlx::query(SERVER_SELECT_BY_ID)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(row_to_server).transpose()
    }

    async fn insert(&self, s: &Server) -> CoreResult<()> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query(
            "INSERT INTO servers (id, name, directory, software_id, game_version, build, jar, java_min_major, java_recommended_major, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(s.id.to_string())
        .bind(&s.name)
        .bind(s.directory.to_string_lossy().to_string())
        .bind(&s.software.software_id)
        .bind(&s.software.game_version)
        .bind(&s.software.build)
        .bind(&s.software.jar)
        .bind(s.software.java_min_major.map(|v| v as i64))
        .bind(s.software.java_recommended_major.map(|v| v as i64))
        .bind(s.created_at.millis())
        .bind(s.updated_at.millis())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                CoreError::new(ErrorCode::Conflict, "A server with this folder is already registered")
            } else {
                db_err(e)
            }
        })?;
        upsert_launch(&mut tx, s).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(&self, s: &Server) -> CoreResult<()> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let res = sqlx::query(
            "UPDATE servers SET name = ?, directory = ?, software_id = ?, game_version = ?, build = ?, jar = ?,
                java_min_major = ?, java_recommended_major = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&s.name)
        .bind(s.directory.to_string_lossy().to_string())
        .bind(&s.software.software_id)
        .bind(&s.software.game_version)
        .bind(&s.software.build)
        .bind(&s.software.jar)
        .bind(s.software.java_min_major.map(|v| v as i64))
        .bind(s.software.java_recommended_major.map(|v| v as i64))
        .bind(s.updated_at.millis())
        .bind(s.id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if res.rows_affected() == 0 {
            return Err(CoreError::new(
                ErrorCode::ServerNotFound,
                "Server not found",
            ));
        }
        upsert_launch(&mut tx, s).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn delete(&self, id: ServerId) -> CoreResult<()> {
        sqlx::query("DELETE FROM servers WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn runtime_states(&self) -> CoreResult<Vec<RuntimeStateRecord>> {
        let rows = sqlx::query("SELECT * FROM server_runtime_state")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|r| {
                let g = |e| db_err(e);
                Ok(RuntimeStateRecord {
                    server_id: Some(parse_id(&r.try_get::<String, _>("server_id").map_err(g)?)?),
                    last_state: r
                        .try_get::<Option<String>, _>("last_state")
                        .map_err(g)?
                        .and_then(|s| LifecycleState::parse(&s)),
                    pid: r
                        .try_get::<Option<i64>, _>("pid")
                        .map_err(g)?
                        .map(|v| v as u32),
                    process_start_time: r
                        .try_get::<Option<i64>, _>("process_start_time")
                        .map_err(g)?
                        .map(|v| v as u64),
                    last_started_at: ts(r.try_get("last_started_at").map_err(g)?),
                    last_ready_at: ts(r.try_get("last_ready_at").map_err(g)?),
                    last_stopped_at: ts(r.try_get("last_stopped_at").map_err(g)?),
                    last_exit_code: r
                        .try_get::<Option<i64>, _>("last_exit_code")
                        .map_err(g)?
                        .map(|v| v as i32),
                })
            })
            .collect()
    }

    async fn save_runtime_state(&self, id: ServerId, s: &RuntimeStateRecord) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO server_runtime_state (server_id, last_state, pid, process_start_time, last_started_at, last_ready_at, last_stopped_at, last_exit_code)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (server_id) DO UPDATE SET last_state = excluded.last_state, pid = excluded.pid,
                process_start_time = excluded.process_start_time, last_started_at = excluded.last_started_at,
                last_ready_at = excluded.last_ready_at,
                last_stopped_at = COALESCE(excluded.last_stopped_at, server_runtime_state.last_stopped_at),
                last_exit_code = excluded.last_exit_code",
        )
        .bind(id.to_string())
        .bind(s.last_state.map(|v| v.as_str()))
        .bind(s.pid.map(|v| v as i64))
        .bind(s.process_start_time.map(|v| v as i64))
        .bind(s.last_started_at.map(|t| t.millis()))
        .bind(s.last_ready_at.map(|t| t.millis()))
        .bind(s.last_stopped_at.map(|t| t.millis()))
        .bind(s.last_exit_code.map(|v| v as i64))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

fn row_to_java(r: &SqliteRow) -> CoreResult<JavaRuntime> {
    let g = |e| db_err(e);
    Ok(JavaRuntime {
        id: parse_id(&r.try_get::<String, _>("id").map_err(g)?)?,
        path: PathBuf::from(r.try_get::<String, _>("path").map_err(g)?),
        major: r.try_get::<i64, _>("major").map_err(g)? as u32,
        version: r.try_get("version").map_err(g)?,
        vendor: r.try_get("vendor").map_err(g)?,
        arch: r.try_get("arch").map_err(g)?,
        is_64bit: r.try_get::<i64, _>("is_64bit").map_err(g)? != 0,
        source: JavaSource::parse(&r.try_get::<String, _>("source").map_err(g)?),
        valid: r.try_get::<i64, _>("valid").map_err(g)? != 0,
        validation_error: r.try_get("validation_error").map_err(g)?,
        validated_at: Timestamp(r.try_get("validated_at").map_err(g)?),
    })
}

#[async_trait]
impl JavaRuntimeRepository for SqliteRepos {
    async fn list(&self) -> CoreResult<Vec<JavaRuntime>> {
        let rows = sqlx::query("SELECT * FROM java_runtimes")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(row_to_java).collect()
    }

    async fn get(&self, id: JavaRuntimeId) -> CoreResult<Option<JavaRuntime>> {
        let row = sqlx::query("SELECT * FROM java_runtimes WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(row_to_java).transpose()
    }

    async fn upsert(&self, rt: &JavaRuntime) -> CoreResult<JavaRuntimeId> {
        let path = rt.path.to_string_lossy().to_string();
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM java_runtimes WHERE path = ? COLLATE NOCASE")
                .bind(&path)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        let id = match &existing {
            Some(s) => parse_id(s)?,
            None => rt.id,
        };
        sqlx::query(
            "INSERT INTO java_runtimes (id, path, major, version, vendor, arch, is_64bit, source, valid, validation_error, validated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET path = excluded.path, major = excluded.major, version = excluded.version,
                vendor = excluded.vendor, arch = excluded.arch, is_64bit = excluded.is_64bit, source = excluded.source,
                valid = excluded.valid, validation_error = excluded.validation_error, validated_at = excluded.validated_at",
        )
        .bind(id.to_string())
        .bind(&path)
        .bind(rt.major as i64)
        .bind(&rt.version)
        .bind(&rt.vendor)
        .bind(&rt.arch)
        .bind(rt.is_64bit as i64)
        .bind(rt.source.as_str())
        .bind(rt.valid as i64)
        .bind(&rt.validation_error)
        .bind(rt.validated_at.millis())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(id)
    }

    async fn delete(&self, id: JavaRuntimeId) -> CoreResult<()> {
        sqlx::query("DELETE FROM java_runtimes WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[async_trait]
impl AuditRepository for SqliteRepos {
    async fn insert(&self, e: &AuditEntry) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO audit_events (id, occurred_at, actor, action, server_id, target, result, metadata_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(e.id.to_string())
        .bind(e.occurred_at.millis())
        .bind(&e.actor)
        .bind(&e.action)
        .bind(e.server_id.map(|s| s.to_string()))
        .bind(&e.target)
        .bind(e.result.as_str())
        .bind(e.metadata.to_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn query(&self, q: &AuditQuery) -> CoreResult<Vec<AuditEntry>> {
        let rows = sqlx::query(
            "SELECT * FROM audit_events
             WHERE (?1 IS NULL OR server_id = ?1) AND (?2 IS NULL OR occurred_at < ?2)
             ORDER BY occurred_at DESC, id DESC LIMIT ?3",
        )
        .bind(q.server_id.map(|s| s.to_string()))
        .bind(q.before.map(|t| t.millis()))
        .bind(q.limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(|r| {
                let g = |e| db_err(e);
                Ok(AuditEntry {
                    id: parse_id::<AuditId>(&r.try_get::<String, _>("id").map_err(g)?)?,
                    occurred_at: Timestamp(r.try_get("occurred_at").map_err(g)?),
                    actor: r.try_get("actor").map_err(g)?,
                    action: r.try_get("action").map_err(g)?,
                    server_id: opt_id(r.try_get("server_id").map_err(g)?)?,
                    target: r.try_get("target").map_err(g)?,
                    result: if r.try_get::<String, _>("result").map_err(g)? == "success" {
                        AuditResult::Success
                    } else {
                        AuditResult::Failure
                    },
                    metadata: serde_json::from_str(
                        &r.try_get::<String, _>("metadata_json").map_err(g)?,
                    )
                    .unwrap_or(serde_json::Value::Null),
                })
            })
            .collect()
    }
}

fn row_to_job(r: &SqliteRow) -> CoreResult<JobRecord> {
    let g = |e| db_err(e);
    let code: Option<String> = r.try_get("error_code").map_err(g)?;
    Ok(JobRecord {
        id: parse_id::<JobId>(&r.try_get::<String, _>("id").map_err(g)?)?,
        kind: r.try_get("kind").map_err(g)?,
        server_id: opt_id(r.try_get("server_id").map_err(g)?)?,
        status: JobStatus::parse(&r.try_get::<String, _>("status").map_err(g)?),
        progress: r
            .try_get::<Option<f64>, _>("progress")
            .map_err(g)?
            .map(|p| p as f32),
        message: r.try_get("message").map_err(g)?,
        created_at: Timestamp(r.try_get("created_at").map_err(g)?),
        started_at: ts(r.try_get("started_at").map_err(g)?),
        finished_at: ts(r.try_get("finished_at").map_err(g)?),
        error_code: code.and_then(|c| serde_json::from_value(serde_json::Value::String(c)).ok()),
        error_message: r.try_get("error_message").map_err(g)?,
        result: r
            .try_get::<Option<String>, _>("result_json")
            .map_err(g)?
            .and_then(|s| serde_json::from_str(&s).ok()),
    })
}

#[async_trait]
impl JobRepository for SqliteRepos {
    async fn insert(&self, j: &JobRecord) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO jobs (id, kind, server_id, status, progress, message, created_at, started_at, finished_at, error_code, error_message, result_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(j.id.to_string())
        .bind(&j.kind)
        .bind(j.server_id.map(|s| s.to_string()))
        .bind(j.status.as_str())
        .bind(j.progress.map(|p| p as f64))
        .bind(&j.message)
        .bind(j.created_at.millis())
        .bind(j.started_at.map(|t| t.millis()))
        .bind(j.finished_at.map(|t| t.millis()))
        .bind(j.error_code.and_then(|c| serde_json::to_value(c).ok()).and_then(|v| v.as_str().map(String::from)))
        .bind(&j.error_message)
        .bind(j.result.as_ref().map(|v| v.to_string()))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn finish(
        &self,
        id: JobId,
        status: JobStatus,
        at: Timestamp,
        error_code: Option<ErrorCode>,
        error_message: Option<String>,
        result: Option<serde_json::Value>,
    ) -> CoreResult<()> {
        sqlx::query(
            "UPDATE jobs SET status = ?, finished_at = ?, error_code = ?, error_message = ?, result_json = ?,
                progress = CASE WHEN ? = 'succeeded' THEN 1.0 ELSE progress END
             WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(at.millis())
        .bind(error_code.and_then(|c| serde_json::to_value(c).ok()).and_then(|v| v.as_str().map(String::from)))
        .bind(error_message)
        .bind(result.map(|v| v.to_string()))
        .bind(status.as_str())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn get(&self, id: JobId) -> CoreResult<Option<JobRecord>> {
        let row = sqlx::query("SELECT * FROM jobs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(row_to_job).transpose()
    }

    async fn recent(&self, limit: u32) -> CoreResult<Vec<JobRecord>> {
        let rows = sqlx::query("SELECT * FROM jobs ORDER BY created_at DESC LIMIT ?")
            .bind(limit.clamp(1, 500) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(row_to_job).collect()
    }

    async fn mark_interrupted(&self, at: Timestamp) -> CoreResult<u64> {
        let res = sqlx::query(
            "UPDATE jobs SET status = 'failed', finished_at = ?, error_code = 'CANCELLED',
                error_message = 'Interrupted because MCPanel was closed'
             WHERE status IN ('queued', 'running')",
        )
        .bind(at.millis())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(res.rows_affected())
    }
}

#[async_trait]
impl SettingsRepository for SqliteRepos {
    async fn get(&self, key: &str) -> CoreResult<Option<serde_json::Value>> {
        let v: Option<String> =
            sqlx::query_scalar("SELECT value_json FROM app_settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        Ok(v.and_then(|s| serde_json::from_str(&s).ok()))
    }

    async fn set(&self, key: &str, value: &serde_json::Value) -> CoreResult<()> {
        sqlx::query(
            "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?, ?, ?)
             ON CONFLICT (key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value.to_string())
        .bind(Timestamp::now().millis())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn all(&self) -> CoreResult<Vec<(String, serde_json::Value)>> {
        let rows = sqlx::query("SELECT key, value_json FROM app_settings")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|r| {
                let k: String = r.try_get("key").map_err(db_err)?;
                let v: String = r.try_get("value_json").map_err(db_err)?;
                Ok((
                    k,
                    serde_json::from_str(&v).unwrap_or(serde_json::Value::Null),
                ))
            })
            .collect()
    }
}
