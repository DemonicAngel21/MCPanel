//! Executes provider-produced [`InstallPlan`]s. Providers never touch the filesystem.

use super::{InstallPlan, InstallStep, SoftwareDescriptor};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::files::SafeRoot;
use crate::jobs::JobContext;
use crate::ports::{DownloadRequest, Downloader};
use std::path::Path;
use std::sync::Arc;

pub struct PlanExecutor {
    downloader: Arc<dyn Downloader>,
}

pub(crate) fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    // Reject userinfo and explicit ports to keep the allowlist meaningful.
    if host.contains('@') || host.contains(':') {
        return None;
    }
    Some(host)
}

impl PlanExecutor {
    pub fn new(downloader: Arc<dyn Downloader>) -> Self {
        Self { downloader }
    }

    /// Validate a plan against the provider's declared download hosts.
    pub fn validate(plan: &InstallPlan, descriptor: &SoftwareDescriptor) -> CoreResult<()> {
        for step in &plan.steps {
            match step {
                InstallStep::Download { url, dest, .. } => {
                    let host = host_of(url).ok_or_else(|| {
                        CoreError::new(ErrorCode::ProviderError, "Download URL must be plain HTTPS")
                    })?;
                    if !descriptor
                        .download_hosts
                        .iter()
                        .any(|h| h.eq_ignore_ascii_case(host))
                    {
                        return Err(CoreError::new(
                            ErrorCode::ProviderError,
                            format!(
                                "Download host '{host}' is not trusted for {}",
                                descriptor.display_name
                            ),
                        ));
                    }
                    crate::files::safepath::parse_relative(dest)?;
                }
                InstallStep::WriteFile { dest, contents, .. } => {
                    crate::files::safepath::parse_relative(dest)?;
                    if contents.len() > 1024 * 1024 {
                        return Err(CoreError::new(
                            ErrorCode::ProviderError,
                            "Generated file is too large",
                        ));
                    }
                }
            }
        }
        crate::files::safepath::parse_relative(&plan.jar)?;
        Ok(())
    }

    /// Run the plan. Files are downloaded into `staging` and moved into the server root
    /// only after verification.
    pub async fn execute(
        &self,
        plan: &InstallPlan,
        root: &SafeRoot,
        staging: &Path,
        ctx: &JobContext,
        progress_range: (f32, f32),
    ) -> CoreResult<()> {
        tokio::fs::create_dir_all(staging)
            .await
            .map_err(|e| CoreError::io("Cannot create staging directory", &e))?;
        let total_steps = plan.steps.len().max(1) as f32;
        for (i, step) in plan.steps.iter().enumerate() {
            ctx.check_cancelled()?;
            match step {
                InstallStep::WriteFile {
                    dest,
                    contents,
                    description,
                } => {
                    ctx.progress(None, description.clone());
                    write_generated(root, dest, contents).await?;
                }
                InstallStep::Download {
                    url,
                    dest,
                    expected_hash,
                    size,
                    description,
                } => {
                    let target = root.resolve(dest)?;
                    target.ensure_no_reparse_points()?;
                    let tmp = staging.join(format!("download-{i}.part"));
                    let (lo, hi) = progress_range;
                    let span = (hi - lo) / total_steps;
                    let base = lo + span * i as f32;
                    let ctx2 = ctx.clone();
                    let desc = description.clone();
                    let progress = move |done: u64, total: Option<u64>| {
                        let frac = total.filter(|t| *t > 0).map(|t| done as f32 / t as f32);
                        ctx2.progress(
                            frac.map(|f| base + span * f),
                            format!("{desc} ({:.1} MB)", done as f64 / 1_048_576.0),
                        );
                    };
                    let req = DownloadRequest {
                        url: url.clone(),
                        expected_hash: expected_hash.clone(),
                        expected_size: *size,
                    };
                    self.downloader
                        .download(&req, &tmp, &progress, ctx.cancellation())
                        .await?;
                    if let Some(parent) = target.absolute().parent() {
                        tokio::fs::create_dir_all(parent)
                            .await
                            .map_err(|e| CoreError::io("Cannot create directory", &e))?;
                    }
                    let dest_abs = target.absolute();
                    if tokio::fs::symlink_metadata(&dest_abs).await.is_ok() {
                        tokio::fs::remove_file(&dest_abs)
                            .await
                            .map_err(|e| CoreError::io("Cannot replace existing file", &e))?;
                    }
                    if tokio::fs::rename(&tmp, &dest_abs).await.is_err() {
                        // Different volume: copy then remove.
                        tokio::fs::copy(&tmp, &dest_abs)
                            .await
                            .map_err(|e| CoreError::io("Cannot install file", &e))?;
                        let _ = tokio::fs::remove_file(&tmp).await;
                    }
                }
            }
        }
        let _ = tokio::fs::remove_dir_all(staging).await;
        Ok(())
    }
}

/// Write a provider-generated file inside the server root (no links followed).
async fn write_generated(root: &SafeRoot, dest: &str, contents: &[u8]) -> CoreResult<()> {
    let target = root.resolve(dest)?;
    target.ensure_no_reparse_points()?;
    if let Some(parent) = target.absolute().parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| CoreError::io("Cannot create directory", &e))?;
    }
    let path = target.absolute();
    let bytes = contents.to_vec();
    tokio::task::spawn_blocking(move || crate::files::fsx::atomic_write(&path, &bytes))
        .await
        .map_err(|e| CoreError::internal(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_parsing() {
        assert_eq!(
            host_of("https://fill-data.papermc.io/v1/x.jar"),
            Some("fill-data.papermc.io")
        );
        assert_eq!(host_of("http://example.com/x"), None);
        assert_eq!(host_of("https://user@evil.com/x"), None);
        assert_eq!(host_of("https://evil.com:8443/x"), None);
    }
}
