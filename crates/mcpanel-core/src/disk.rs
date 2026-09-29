//! Disk usage of a server folder, grouped the way users think about it (worlds,
//! plugins/mods, logs, everything else). Links/junctions are not followed.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskUsage {
    pub total_bytes: u64,
    pub worlds_bytes: u64,
    pub content_bytes: u64,
    pub logs_bytes: u64,
    pub other_bytes: u64,
    /// Files counted (the walk stops early for extremely large folders).
    pub files: u64,
    pub truncated: bool,
}

const MAX_FILES: u64 = 2_000_000;

fn walk(dir: &Path, files: &mut u64, truncated: &mut bool) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for e in entries.flatten() {
        if *files >= MAX_FILES {
            *truncated = true;
            break;
        }
        let Ok(md) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        if crate::files::fsx::is_reparse_point(&md) {
            continue;
        }
        if md.is_dir() {
            total += walk(&e.path(), files, truncated);
        } else if md.is_file() {
            *files += 1;
            total += md.len();
        }
    }
    total
}

/// Measure `root`; `level_name` is the world folder from server.properties.
pub fn usage(root: &Path, level_name: &str) -> DiskUsage {
    let mut u = DiskUsage::default();
    let (mut files, mut truncated) = (0u64, false);
    let worlds: Vec<String> = [
        level_name.to_string(),
        format!("{level_name}_nether"),
        format!("{level_name}_the_end"),
    ]
    .into();
    let Ok(entries) = std::fs::read_dir(root) else {
        return u;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(md) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        if crate::files::fsx::is_reparse_point(&md) {
            continue;
        }
        let size = if md.is_dir() {
            walk(&e.path(), &mut files, &mut truncated)
        } else {
            files += 1;
            md.len()
        };
        let lower = name.to_ascii_lowercase();
        if worlds.iter().any(|w| w.eq_ignore_ascii_case(&name)) {
            u.worlds_bytes += size;
        } else if matches!(lower.as_str(), "plugins" | "mods" | "config") {
            u.content_bytes += size;
        } else if matches!(lower.as_str(), "logs" | "crash-reports" | "debug") {
            u.logs_bytes += size;
        } else {
            u.other_bytes += size;
        }
        u.total_bytes += size;
    }
    u.files = files;
    u.truncated = truncated;
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_worlds_content_and_logs() {
        let d = tempfile::tempdir().unwrap();
        let w = |p: &str, n: usize| {
            let path = d.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![0u8; n]).unwrap();
        };
        w("survival/level.dat", 100);
        w("survival_nether/DIM-1/r.mca", 50);
        w("plugins/a.jar", 30);
        w("logs/latest.log", 20);
        w("server.jar", 7);
        let u = usage(d.path(), "survival");
        assert_eq!(u.worlds_bytes, 150);
        assert_eq!(u.content_bytes, 30);
        assert_eq!(u.logs_bytes, 20);
        assert_eq!(u.other_bytes, 7);
        assert_eq!(u.total_bytes, 207);
        assert_eq!(u.files, 5);
    }
}
