//! Restore: preview the difference, extract into staging with hash verification, then
//! swap the server directory contents with rollback on failure.
//!
//! Staging lives in `<server>/.mcpanel/restore/<id>` so every move is a same-volume
//! rename. The `.mcpanel` directory itself (trash, MCPanel state) is never replaced.

use super::archive::{
    ScanEntry, backup_limits, check_entries_match, hash_file, read_manifest_from, scan,
};
use super::{BackupManifest, MANIFEST_NAME};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::files::archive::{open_zip, plan_entries, write_entries};
use crate::files::fsx;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RestorePreview {
    /// Files in the backup that do not exist now.
    pub added: u64,
    /// Files that exist now and are not in the backup (they will be removed).
    pub removed: u64,
    /// Files whose size differs (for `.jar` files: whose content differs).
    pub changed: u64,
    pub unchanged: u64,
    /// Plugin/mod/server jars that differ, are added or removed.
    pub changed_jars: Vec<String>,
    /// Up to 20 of the removed paths.
    pub removed_sample: Vec<String>,
    pub total_bytes: u64,
    pub contains_sensitive: bool,
}

fn is_jar(path: &str) -> bool {
    path.to_lowercase().ends_with(".jar")
}

/// Compare a backup's manifest with the current contents of `root`.
pub fn preview(root: &Path, manifest: &BackupManifest) -> CoreResult<RestorePreview> {
    let current: HashMap<String, ScanEntry> = scan(root)?
        .entries
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| (e.key().to_lowercase(), e))
        .collect();
    let mut p = RestorePreview {
        total_bytes: manifest.total_bytes,
        contains_sensitive: manifest.contains_sensitive,
        ..Default::default()
    };
    for f in &manifest.files {
        match current.get(&f.path.to_lowercase()) {
            None => {
                p.added += 1;
                if is_jar(&f.path) {
                    p.changed_jars.push(f.path.clone());
                }
            }
            Some(cur) if cur.size != f.size => {
                p.changed += 1;
                if is_jar(&f.path) {
                    p.changed_jars.push(f.path.clone());
                }
            }
            Some(cur) if is_jar(&f.path) => {
                let same = hash_file(&cur.path).is_ok_and(|(_, h)| h == f.sha256);
                if same {
                    p.unchanged += 1;
                } else {
                    p.changed += 1;
                    p.changed_jars.push(f.path.clone());
                }
            }
            Some(_) => p.unchanged += 1,
        }
    }
    let in_backup: std::collections::HashSet<String> = manifest
        .files
        .iter()
        .map(|f| f.path.to_lowercase())
        .collect();
    let mut removed: Vec<&ScanEntry> = current
        .iter()
        .filter(|(k, _)| !in_backup.contains(*k))
        .map(|(_, e)| e)
        .collect();
    removed.sort_by_key(|e| e.key());
    p.removed = removed.len() as u64;
    for e in &removed {
        if is_jar(&e.key()) {
            p.changed_jars.push(e.key());
        }
    }
    p.removed_sample = removed.iter().take(20).map(|e| e.key()).collect();
    p.changed_jars.sort();
    p.changed_jars.dedup();
    Ok(p)
}

/// The staging directory used by a restore.
pub fn work_dir(root: &Path, id: &str) -> PathBuf {
    root.join(".mcpanel").join("restore").join(id)
}

/// Extract and verify the backup into staging, then replace the contents of `root`.
/// Returns the number of files restored.
pub fn restore_into(
    root: &Path,
    archive: &Path,
    work: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> CoreResult<u64> {
    let mut zip = open_zip(archive)?;
    let manifest = read_manifest_from(&mut zip)?;
    let planned = plan_entries(&mut zip, backup_limits(&manifest))?;
    check_entries_match(&planned, &manifest)?;
    let planned: Vec<_> = planned
        .into_iter()
        .filter(|p| !p.key().eq_ignore_ascii_case(MANIFEST_NAME))
        .collect();

    // `.mcpanel/restore` must be real folders: the old server tree is moved there.
    if let Some(parent) = work.parent() {
        fsx::ensure_real_dir_chain(root, parent)?;
    }
    if work.exists() {
        fsx::remove_tree_no_follow(work)?;
    }
    let new_dir = work.join("new");
    let old_dir = work.join("old");
    let result = (|| -> CoreResult<u64> {
        let (report, written) =
            write_entries(&mut zip, &planned, &new_dir, true, cancelled, progress)?;
        let expected: HashMap<String, &str> = manifest
            .files
            .iter()
            .map(|f| (f.path.to_lowercase(), f.sha256.as_str()))
            .collect();
        for w in &written {
            if expected.get(&w.key.to_lowercase()).copied() != w.sha256.as_deref() {
                return Err(CoreError::new(
                    ErrorCode::ArchiveRejected,
                    format!(
                        "The backup is damaged: '{}' does not match its recorded hash",
                        w.key
                    ),
                ));
            }
        }
        if cancelled() {
            return Err(CoreError::cancelled());
        }
        fs::create_dir_all(&old_dir).map_err(|e| CoreError::io("Cannot prepare restore", &e))?;
        swap(root, &new_dir, &old_dir)?;
        Ok(report.files)
    })();
    let _ = fsx::remove_tree_no_follow(work);
    // Remove `.mcpanel/restore` when it is empty.
    if let Some(parent) = work.parent() {
        let _ = fs::remove_dir(parent);
    }
    result
}

fn top_level(dir: &Path, skip_reserved: bool) -> CoreResult<Vec<std::ffi::OsString>> {
    let mut names = Vec::new();
    for e in fs::read_dir(dir).map_err(|e| CoreError::io("Cannot read directory", &e))? {
        let e = e.map_err(|e| CoreError::io("Cannot read directory", &e))?;
        let name = e.file_name();
        if skip_reserved && name.to_string_lossy().eq_ignore_ascii_case(".mcpanel") {
            continue;
        }
        names.push(name);
    }
    Ok(names)
}

fn busy(e: &std::io::Error, what: &std::ffi::OsStr) -> CoreError {
    CoreError::io(
        format!(
            "Cannot move '{}' — close any program using the server folder and try again",
            what.to_string_lossy()
        ),
        e,
    )
}

/// Move the current contents of `root` to `old`, then the contents of `new` into `root`.
/// On failure every move is undone.
fn swap(root: &Path, new_dir: &Path, old_dir: &Path) -> CoreResult<()> {
    let mut moved_out = Vec::new();
    for name in top_level(root, true)? {
        if let Err(e) = fs::rename(root.join(&name), old_dir.join(&name)) {
            rollback(root, old_dir, &moved_out, new_dir, &[]);
            return Err(busy(&e, &name));
        }
        moved_out.push(name);
    }
    let mut moved_in = Vec::new();
    for name in top_level(new_dir, false)? {
        if let Err(e) = fs::rename(new_dir.join(&name), root.join(&name)) {
            rollback(root, old_dir, &moved_out, new_dir, &moved_in);
            return Err(busy(&e, &name));
        }
        moved_in.push(name);
    }
    Ok(())
}

fn rollback(
    root: &Path,
    old_dir: &Path,
    moved_out: &[std::ffi::OsString],
    new_dir: &Path,
    moved_in: &[std::ffi::OsString],
) {
    for name in moved_in.iter().rev() {
        if let Err(e) = fs::rename(root.join(name), new_dir.join(name)) {
            tracing::error!(target: "mcpanel::backup", "restore rollback failed for {:?}: {e}", name);
        }
    }
    for name in moved_out.iter().rev() {
        if let Err(e) = fs::rename(old_dir.join(name), root.join(name)) {
            tracing::error!(target: "mcpanel::backup", "restore rollback failed for {:?}: {e}", name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::archive::{tests::manifest_template, write_archive};

    fn backup_of(root: &Path, dest: &Path) -> BackupManifest {
        write_archive(
            &scan(root).unwrap(),
            manifest_template(),
            dest,
            &|| false,
            &mut |_| {},
        )
        .unwrap()
    }

    #[test]
    fn restore_replaces_contents_and_keeps_reserved_dir() {
        let server = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let root = server.path();
        fs::create_dir_all(root.join("world")).unwrap();
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("world/level.dat"), b"v1").unwrap();
        fs::write(root.join("plugins/a.jar"), b"jar-v1").unwrap();
        fs::create_dir_all(root.join(".mcpanel/trash")).unwrap();
        fs::write(root.join(".mcpanel/trash/t"), b"t").unwrap();
        let zip = out.path().join("b.zip");
        let m = backup_of(root, &zip);

        // Change things after the backup.
        fs::write(root.join("world/level.dat"), b"v2-longer").unwrap();
        fs::write(root.join("plugins/a.jar"), b"jar-v2").unwrap();
        fs::write(root.join("plugins/b.jar"), b"new").unwrap();
        fs::write(root.join("extra.txt"), b"x").unwrap();

        let p = preview(root, &m).unwrap();
        assert_eq!((p.added, p.removed, p.changed, p.unchanged), (0, 2, 2, 0));
        assert_eq!(p.changed_jars, vec!["plugins/a.jar", "plugins/b.jar"]);

        let n = restore_into(root, &zip, &work_dir(root, "r1"), &|| false, &mut |_| {}).unwrap();
        assert_eq!(n, 2);
        assert_eq!(fs::read(root.join("world/level.dat")).unwrap(), b"v1");
        assert_eq!(fs::read(root.join("plugins/a.jar")).unwrap(), b"jar-v1");
        assert!(!root.join("plugins/b.jar").exists());
        assert!(!root.join("extra.txt").exists());
        assert_eq!(fs::read(root.join(".mcpanel/trash/t")).unwrap(), b"t");
        assert!(!root.join(".mcpanel/restore").exists(), "staging removed");
    }

    #[test]
    fn locked_file_rolls_back_the_swap() {
        use std::os::windows::fs::OpenOptionsExt;
        let server = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let root = server.path();
        fs::create_dir_all(root.join("world")).unwrap();
        fs::write(root.join("a.txt"), b"backup").unwrap();
        fs::write(root.join("world/level.dat"), b"v1").unwrap();
        let zip = out.path().join("b.zip");
        backup_of(root, &zip);
        fs::write(root.join("a.txt"), b"current").unwrap();
        fs::write(root.join("z.txt"), b"z").unwrap();

        // Hold z.txt open without FILE_SHARE_DELETE so it cannot be moved.
        let _held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 /* FILE_SHARE_READ */)
            .open(root.join("z.txt"))
            .unwrap();
        let err =
            restore_into(root, &zip, &work_dir(root, "r2"), &|| false, &mut |_| {}).unwrap_err();
        assert!(err.message.contains("z.txt"), "{}", err.message);
        // Everything is back as it was.
        assert_eq!(fs::read(root.join("a.txt")).unwrap(), b"current");
        assert_eq!(fs::read(root.join("world/level.dat")).unwrap(), b"v1");
        assert!(root.join("z.txt").exists());
    }
}
