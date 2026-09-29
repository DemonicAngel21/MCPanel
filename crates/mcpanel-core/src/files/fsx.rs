//! Low-level filesystem helpers that never follow reparse points (symlinks/junctions).
//! Uses only `std`, with a small `cfg(windows)` branch for the reparse-point attribute.

use crate::error::{CoreError, CoreResult};
use std::fs;
use std::io::Write;
use std::path::Path;

/// True for symlinks, junctions and any other reparse point.
pub fn is_reparse_point(md: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    md.file_type().is_symlink()
}

/// Strip the Windows verbatim prefix produced by `canonicalize` (`\\?\C:\x` → `C:\x`,
/// `\\?\UNC\srv\share` → `\\srv\share`) for storage and display.
pub fn simplify(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return std::path::PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
            return std::path::PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

fn is_directory_link(md: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
        md.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
    }
    #[cfg(not(windows))]
    {
        let _ = md;
        false
    }
}

/// Case-insensitive prefix check on path components (Windows semantics).
pub fn path_starts_with_ci(path: &Path, prefix: &Path) -> bool {
    let norm = |p: &Path| -> Vec<String> {
        let s = p.to_string_lossy().replace('/', "\\");
        let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
        s.split('\\')
            .filter(|c| !c.is_empty())
            .map(|c| c.to_lowercase())
            .collect()
    };
    let p = norm(path);
    let pre = norm(prefix);
    pre.len() <= p.len() && pre.iter().zip(&p).all(|(a, b)| a == b)
}

/// Remove a file, a link, or a directory tree. Reparse points are removed as links;
/// their targets are never touched.
pub fn remove_tree_no_follow(path: &Path) -> CoreResult<()> {
    let md = fs::symlink_metadata(path).map_err(|e| CoreError::io("Cannot inspect path", &e))?;
    if is_reparse_point(&md) {
        // A directory junction/symlink is removed with remove_dir on Windows; the link
        // metadata reports "symlink", so consult the directory attribute instead.
        let r = if is_directory_link(&md) {
            fs::remove_dir(path).or_else(|_| fs::remove_file(path))
        } else {
            fs::remove_file(path).or_else(|_| fs::remove_dir(path))
        };
        return r.map_err(|e| CoreError::io("Cannot remove link", &e));
    }
    if md.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| CoreError::io("Cannot read directory", &e))? {
            let entry = entry.map_err(|e| CoreError::io("Cannot read directory", &e))?;
            remove_tree_no_follow(&entry.path())?;
        }
        fs::remove_dir(path).map_err(|e| CoreError::io("Cannot remove directory", &e))
    } else {
        clear_readonly(path, &md);
        fs::remove_file(path).map_err(|e| CoreError::io("Cannot remove file", &e))
    }
}

fn clear_readonly(path: &Path, md: &fs::Metadata) {
    let mut perms = md.permissions();
    if perms.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = fs::set_permissions(path, perms);
    }
}

/// Statistics from a recursive copy.
#[derive(Debug, Default, Clone, Copy)]
pub struct CopyStats {
    pub files: u64,
    pub bytes: u64,
    pub skipped_links: u64,
    pub skipped_filtered: u64,
}

/// Recursively copy `src` to `dst` (which must not exist). Reparse points are skipped.
/// `filter` returns false for entries (relative to `src`) that must not be copied.
pub fn copy_tree_no_follow(
    src: &Path,
    dst: &Path,
    filter: &dyn Fn(&Path) -> bool,
) -> CoreResult<CopyStats> {
    let mut stats = CopyStats::default();
    copy_inner(src, dst, Path::new(""), filter, &mut stats)?;
    Ok(stats)
}

fn copy_inner(
    src: &Path,
    dst: &Path,
    rel: &Path,
    filter: &dyn Fn(&Path) -> bool,
    stats: &mut CopyStats,
) -> CoreResult<()> {
    let md = fs::symlink_metadata(src).map_err(|e| CoreError::io("Cannot inspect source", &e))?;
    if is_reparse_point(&md) {
        stats.skipped_links += 1;
        return Ok(());
    }
    if !rel.as_os_str().is_empty() && !filter(rel) {
        stats.skipped_filtered += 1;
        return Ok(());
    }
    if md.is_dir() {
        fs::create_dir(dst).map_err(|e| CoreError::io("Cannot create directory", &e))?;
        for entry in fs::read_dir(src).map_err(|e| CoreError::io("Cannot read directory", &e))? {
            let entry = entry.map_err(|e| CoreError::io("Cannot read directory", &e))?;
            let name = entry.file_name();
            copy_inner(
                &entry.path(),
                &dst.join(&name),
                &rel.join(&name),
                filter,
                stats,
            )?;
        }
    } else {
        if dst.exists() {
            return Err(CoreError::new(
                crate::error::ErrorCode::PathExists,
                format!("'{}' already exists", dst.display()),
            ));
        }
        let n = fs::copy(src, dst).map_err(|e| CoreError::io("Cannot copy file", &e))?;
        stats.files += 1;
        stats.bytes += n;
    }
    Ok(())
}

/// Write `bytes` to `path` atomically: temp file in the same directory, flush + fsync,
/// then rename over the destination.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    let dir = path
        .parent()
        .ok_or_else(|| CoreError::invalid("Path has no parent directory"))?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        ".{file_name}.mcpanel-tmp-{}",
        uuid::Uuid::now_v7().simple()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        f.sync_all()?;
        drop(f);
        if let Ok(md) = fs::metadata(path)
            && md.permissions().readonly()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "file is read-only",
            ));
        }
        fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::io("Cannot write file", &e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_ci() {
        assert!(path_starts_with_ci(
            Path::new(r"C:\Users\A\AppData\Local\MCPanel\x"),
            Path::new(r"c:\users\a\appdata\local\mcpanel")
        ));
        assert!(path_starts_with_ci(
            Path::new(r"\\?\C:\Users\A\MCPanel"),
            Path::new(r"C:\Users\A")
        ));
        assert!(!path_starts_with_ci(
            Path::new(r"C:\Users\AB"),
            Path::new(r"C:\Users\A")
        ));
    }

    #[test]
    fn atomic_write_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn remove_tree_does_not_follow_junction() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("precious.txt"), b"keep").unwrap();
        let tree = dir.path().join("tree");
        fs::create_dir(&tree).unwrap();
        let link = tree.join("link");
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        remove_tree_no_follow(&tree).unwrap();
        assert!(!tree.exists());
        assert!(outside.path().join("precious.txt").exists());
    }
}

/// Make sure every folder from `root` down to `dir` is a real folder (not a link or
/// junction that could lead outside `root`), creating missing ones.
pub fn ensure_real_dir_chain(root: &Path, dir: &Path) -> CoreResult<()> {
    let rel = dir
        .strip_prefix(root)
        .map_err(|_| CoreError::internal("folder outside the server root"))?;
    let mut cur = root.to_path_buf();
    for c in rel.components() {
        cur.push(c);
        match fs::symlink_metadata(&cur) {
            Ok(md) if is_reparse_point(&md) || !md.is_dir() => {
                return Err(CoreError::new(
                    crate::error::ErrorCode::PathRejected,
                    format!("'{}' is a link or not a folder", cur.display()),
                ));
            }
            Ok(_) => {}
            Err(_) => {
                fs::create_dir(&cur).map_err(|e| CoreError::io("Cannot create folder", &e))?
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    #[test]
    fn creates_missing_folders_and_rejects_files_in_the_way() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join(".mcpanel/trash");
        ensure_real_dir_chain(d.path(), &dir).unwrap();
        assert!(dir.is_dir());
        std::fs::write(d.path().join("blocker"), b"x").unwrap();
        assert!(ensure_real_dir_chain(d.path(), &d.path().join("blocker/sub")).is_err());
        assert!(ensure_real_dir_chain(d.path(), Path::new("C:/elsewhere")).is_err());
    }
}
