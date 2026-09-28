//! Application directory layout. Resolved by the host/platform and passed into the core.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    /// `%LOCALAPPDATA%\MCPanel` on Windows.
    pub data_dir: PathBuf,
    /// Default parent directory for new servers (`%USERPROFILE%\MCPanel\Servers`).
    pub default_servers_dir: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf, default_servers_dir: PathBuf) -> Self {
        Self {
            data_dir,
            default_servers_dir,
        }
    }

    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join("mcpanel.db")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }

    /// Staging area for downloads; never inside a server directory.
    pub fn staging_dir(&self) -> PathBuf {
        self.data_dir.join("staging")
    }

    /// MCPanel-owned per-server data (console captures etc.), keyed by server id.
    pub fn server_data_dir(&self, server_id: &crate::ids::ServerId) -> PathBuf {
        self.data_dir.join("servers").join(server_id.to_string())
    }

    pub fn console_dir(&self, server_id: &crate::ids::ServerId) -> PathBuf {
        self.server_data_dir(server_id).join("console")
    }

    /// Whether `path` lies inside MCPanel's own data directory (servers must not).
    pub fn is_inside_data_dir(&self, path: &Path) -> bool {
        crate::files::path_starts_with_ci(path, &self.data_dir)
    }
}
