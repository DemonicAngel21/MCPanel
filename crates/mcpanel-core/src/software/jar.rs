//! Shared helpers for "single executable jar" server software (Vanilla, Paper, Purpur).

use super::{LaunchArgs, LaunchResolver};
use crate::error::{CoreError, CoreResult};
use crate::model::{InstalledSoftware, LaunchConfig};
use std::io::Read;
use std::path::Path;

/// Launches `java … -jar <jar> nogui`.
#[derive(Debug, Clone)]
pub struct SingleJarLauncher {
    pub server_args: Vec<String>,
}

impl Default for SingleJarLauncher {
    fn default() -> Self {
        Self {
            server_args: vec!["nogui".to_string()],
        }
    }
}

impl LaunchResolver for SingleJarLauncher {
    fn launch_args(
        &self,
        _root: &Path,
        installed: &InstalledSoftware,
        _launch: &LaunchConfig,
    ) -> CoreResult<LaunchArgs> {
        crate::files::safepath::parse_relative(&installed.jar)
            .map_err(|_| CoreError::invalid("Invalid server jar path"))?;
        Ok(LaunchArgs {
            jvm_args: Vec::new(),
            jar: installed.jar.clone(),
            main_class: None,
            class_path: Vec::new(),
            server_args: self.server_args.clone(),
        })
    }
}

/// Read one entry of a jar (bounded size). Used by detectors; treats the jar as
/// untrusted input.
pub fn read_jar_entry(jar: &Path, entry: &str, max_bytes: u64) -> Option<Vec<u8>> {
    let file = std::fs::File::open(jar).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let f = zip.by_name(entry).ok()?;
    if f.size() > max_bytes {
        return None;
    }
    let mut buf = Vec::with_capacity(f.size() as usize);
    f.take(max_bytes).read_to_end(&mut buf).ok()?;
    Some(buf)
}

pub fn jar_has_entry_prefix(jar: &Path, prefix: &str) -> bool {
    let Ok(file) = std::fs::File::open(jar) else {
        return false;
    };
    let Ok(zip) = zip::ZipArchive::new(file) else {
        return false;
    };
    zip.file_names().any(|n| n.starts_with(prefix))
}

/// Jar files directly in `dir` (not recursive), largest first.
pub fn root_jars(dir: &Path) -> Vec<(String, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut jars: Vec<(String, u64)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let md = std::fs::symlink_metadata(e.path()).ok()?;
            (md.is_file() && name.to_ascii_lowercase().ends_with(".jar"))
                .then_some((name, md.len()))
        })
        .collect();
    jars.sort_by_key(|j| std::cmp::Reverse(j.1));
    jars
}
