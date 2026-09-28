//! Server-scoped file operations: resolves the server root, runs the blocking file
//! service on a worker thread, and audits mutating operations.

use crate::error::{CoreError, CoreResult};
use crate::files::SafeRoot;
use crate::files::archive::{ExtractReport, ZipReport};
use crate::files::fsx::CopyStats;
use crate::files::service::{self, FileEntry, TextDocument, WriteText};
use crate::ids::ServerId;
use crate::model::AuditResult;
use crate::server::ServerManager;
use std::path::PathBuf;
use std::sync::Arc;

pub struct ServerFiles {
    servers: Arc<ServerManager>,
}

impl ServerFiles {
    pub fn new(servers: Arc<ServerManager>) -> Self {
        Self { servers }
    }

    async fn root(&self, id: ServerId) -> CoreResult<SafeRoot> {
        let server = self.servers.get(id).await?;
        SafeRoot::open(&server.directory)
    }

    async fn blocking<T: Send + 'static>(
        &self,
        id: ServerId,
        f: impl FnOnce(&SafeRoot) -> CoreResult<T> + Send + 'static,
    ) -> CoreResult<T> {
        let root = self.root(id).await?;
        tokio::task::spawn_blocking(move || f(&root))
            .await
            .map_err(|e| CoreError::internal(format!("file task failed: {e}")))?
    }

    async fn audit<T>(
        &self,
        id: ServerId,
        actor: &str,
        action: &str,
        result: &CoreResult<T>,
        meta: serde_json::Value,
    ) {
        self.servers
            .audit
            .record(
                actor,
                action,
                Some(id),
                None,
                if result.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
                meta,
            )
            .await;
    }

    pub async fn list(&self, id: ServerId, path: String) -> CoreResult<Vec<FileEntry>> {
        self.blocking(id, move |r| service::list_dir(r, &path))
            .await
    }

    pub async fn stat(&self, id: ServerId, path: String) -> CoreResult<FileEntry> {
        self.blocking(id, move |r| service::stat(r, &path)).await
    }

    pub async fn read_text(&self, id: ServerId, path: String) -> CoreResult<TextDocument> {
        self.blocking(id, move |r| service::read_text(r, &path))
            .await
    }

    pub async fn write_text(
        &self,
        id: ServerId,
        path: String,
        req: WriteText,
        actor: &str,
    ) -> CoreResult<TextDocument> {
        let p = path.clone();
        let res = self
            .blocking(id, move |r| service::write_text(r, &p, &req))
            .await;
        self.audit(
            id,
            actor,
            "file.write",
            &res,
            serde_json::json!({ "path": path }),
        )
        .await;
        res
    }

    pub async fn create_dir(
        &self,
        id: ServerId,
        path: String,
        actor: &str,
    ) -> CoreResult<FileEntry> {
        let p = path.clone();
        let res = self.blocking(id, move |r| service::create_dir(r, &p)).await;
        self.audit(
            id,
            actor,
            "file.create_dir",
            &res,
            serde_json::json!({ "path": path }),
        )
        .await;
        res
    }

    pub async fn create_file(
        &self,
        id: ServerId,
        path: String,
        actor: &str,
    ) -> CoreResult<FileEntry> {
        let p = path.clone();
        let res = self
            .blocking(id, move |r| service::create_file(r, &p))
            .await;
        self.audit(
            id,
            actor,
            "file.create",
            &res,
            serde_json::json!({ "path": path }),
        )
        .await;
        res
    }

    pub async fn rename(
        &self,
        id: ServerId,
        path: String,
        new_name: String,
        actor: &str,
    ) -> CoreResult<FileEntry> {
        let (p, n) = (path.clone(), new_name.clone());
        let res = self.blocking(id, move |r| service::rename(r, &p, &n)).await;
        self.audit(
            id,
            actor,
            "file.rename",
            &res,
            serde_json::json!({ "path": path, "newName": new_name }),
        )
        .await;
        res
    }

    pub async fn move_to(
        &self,
        id: ServerId,
        paths: Vec<String>,
        dest: String,
        actor: &str,
    ) -> CoreResult<()> {
        let (p, d) = (paths.clone(), dest.clone());
        let res = self
            .blocking(id, move |r| service::move_to(r, &p, &d))
            .await;
        self.audit(
            id,
            actor,
            "file.move",
            &res,
            serde_json::json!({ "paths": paths, "destination": dest }),
        )
        .await;
        res
    }

    pub async fn copy_to(
        &self,
        id: ServerId,
        paths: Vec<String>,
        dest: String,
        actor: &str,
    ) -> CoreResult<CopyStats> {
        let (p, d) = (paths.clone(), dest.clone());
        let res = self
            .blocking(id, move |r| service::copy_to(r, &p, &d))
            .await;
        self.audit(
            id,
            actor,
            "file.copy",
            &res,
            serde_json::json!({ "paths": paths, "destination": dest }),
        )
        .await;
        res
    }

    pub async fn delete(
        &self,
        id: ServerId,
        paths: Vec<String>,
        permanent: bool,
        actor: &str,
    ) -> CoreResult<()> {
        let p = paths.clone();
        let res = self
            .blocking(id, move |r| service::delete(r, &p, permanent))
            .await;
        self.audit(
            id,
            actor,
            "file.delete",
            &res,
            serde_json::json!({ "paths": paths, "permanent": permanent }),
        )
        .await;
        res
    }

    pub async fn zip(
        &self,
        id: ServerId,
        paths: Vec<String>,
        archive_name: String,
        actor: &str,
    ) -> CoreResult<(FileEntry, ZipReport)> {
        let (p, n) = (paths.clone(), archive_name.clone());
        let res = self.blocking(id, move |r| service::zip(r, &p, &n)).await;
        self.audit(
            id,
            actor,
            "file.zip",
            &res,
            serde_json::json!({ "paths": paths, "archive": archive_name }),
        )
        .await;
        res
    }

    pub async fn unzip(
        &self,
        id: ServerId,
        archive: String,
        dest: String,
        overwrite: bool,
        actor: &str,
    ) -> CoreResult<ExtractReport> {
        let (a, d) = (archive.clone(), dest.clone());
        let res = self
            .blocking(id, move |r| service::unzip(r, &a, &d, overwrite))
            .await;
        self.audit(
            id,
            actor,
            "file.unzip",
            &res,
            serde_json::json!({ "archive": archive, "destination": dest, "overwrite": overwrite }),
        )
        .await;
        res
    }

    pub async fn import(
        &self,
        id: ServerId,
        source: PathBuf,
        dest: String,
        actor: &str,
    ) -> CoreResult<FileEntry> {
        let d = dest.clone();
        let name = source.file_name().map(|n| n.to_string_lossy().to_string());
        let res = self
            .blocking(id, move |r| service::import_from(r, &source, &d))
            .await;
        self.audit(
            id,
            actor,
            "file.import",
            &res,
            serde_json::json!({ "name": name, "destination": dest }),
        )
        .await;
        res
    }

    pub async fn export(
        &self,
        id: ServerId,
        path: String,
        dest: PathBuf,
        actor: &str,
    ) -> CoreResult<u64> {
        let p = path.clone();
        let res = self
            .blocking(id, move |r| service::export_to(r, &p, &dest))
            .await;
        self.audit(
            id,
            actor,
            "file.export",
            &res,
            serde_json::json!({ "path": path }),
        )
        .await;
        res
    }

    pub async fn search(
        &self,
        id: ServerId,
        path: String,
        query: String,
        limit: usize,
    ) -> CoreResult<Vec<FileEntry>> {
        self.blocking(id, move |r| {
            service::search(r, &path, &query, limit.clamp(1, 500))
        })
        .await
    }
}
