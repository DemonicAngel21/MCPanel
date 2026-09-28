//! ZIP creation and *safe* extraction.
//!
//! Extraction defends against: zip-slip (every entry name must pass `SafePath` rules),
//! symlink entries, zip bombs (entry count, total size, per-entry compression ratio,
//! and actual-vs-declared size), duplicate / case-colliding entries, and overwrite
//! surprises (conflicts are reported unless overwrite is requested). Entries are first
//! extracted into a staging directory and then moved into place.

use super::fsx::{self, is_reparse_point};
use super::safepath::{SafePath, parse_relative};
use super::sensitivity::{Sensitivity, classify};
use crate::error::{CoreError, CoreResult, ErrorCode};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct ExtractLimits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
    /// Maximum uncompressed/compressed ratio for entries larger than 1 MiB.
    pub max_ratio: u64,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_entries: 200_000,
            max_total_bytes: 20 * 1024 * 1024 * 1024,
            max_ratio: 1_000,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ExtractReport {
    pub files: u64,
    pub directories: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ZipReport {
    pub files: u64,
    pub bytes: u64,
    pub skipped_links: u64,
    pub skipped_sensitive: u64,
}

const S_IFMT: u32 = 0o170000;
const S_IFLNK: u32 = 0o120000;

fn rejected(msg: impl Into<String>) -> CoreError {
    CoreError::new(ErrorCode::ArchiveRejected, msg.into())
}

pub(crate) struct PlannedEntry {
    pub index: usize,
    pub components: Vec<String>,
    pub is_dir: bool,
    pub size: u64,
}

impl PlannedEntry {
    pub fn key(&self) -> String {
        self.components.join("/")
    }
}

pub(crate) fn open_zip(archive: &Path) -> CoreResult<zip::ZipArchive<fs::File>> {
    let file = fs::File::open(archive).map_err(|e| CoreError::io("Cannot open archive", &e))?;
    zip::ZipArchive::new(file).map_err(|e| rejected(format!("Not a valid ZIP archive: {e}")))
}

/// Phase 1: validate every entry before anything is written.
pub(crate) fn plan_entries(
    zip: &mut zip::ZipArchive<fs::File>,
    limits: ExtractLimits,
) -> CoreResult<Vec<PlannedEntry>> {
    if zip.len() > limits.max_entries {
        return Err(rejected(format!(
            "Archive has {} entries (limit {})",
            zip.len(),
            limits.max_entries
        )));
    }
    let mut planned = Vec::with_capacity(zip.len());
    let mut seen = HashSet::new();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let entry = zip
            .by_index_raw(i)
            .map_err(|e| rejected(format!("Corrupt archive entry: {e}")))?;
        let raw_name = entry.name().to_string();
        if raw_name.contains('\\') {
            return Err(rejected(format!(
                "Entry '{raw_name}' uses backslashes (not allowed)"
            )));
        }
        let is_dir = raw_name.ends_with('/');
        let components = parse_relative(raw_name.trim_end_matches('/'))
            .map_err(|e| rejected(format!("Unsafe entry name '{raw_name}': {}", e.message)))?;
        if components.is_empty() {
            continue;
        }
        if components[0].eq_ignore_ascii_case(".mcpanel") {
            return Err(rejected(
                "Archive entries must not target the .mcpanel directory",
            ));
        }
        if let Some(mode) = entry.unix_mode()
            && mode & S_IFMT == S_IFLNK
        {
            return Err(rejected(format!("Entry '{raw_name}' is a symbolic link")));
        }
        let key = components.join("/").to_lowercase();
        if !seen.insert(key) {
            return Err(rejected(format!(
                "Entry '{raw_name}' is duplicated (names are case-insensitive on Windows)"
            )));
        }
        let size = entry.size();
        let compressed = entry.compressed_size().max(1);
        if !is_dir && size > 1024 * 1024 && size / compressed > limits.max_ratio {
            return Err(rejected(format!(
                "Entry '{raw_name}' has a suspicious compression ratio"
            )));
        }
        total = total.saturating_add(size);
        if total > limits.max_total_bytes {
            return Err(rejected("Archive expands beyond the allowed size"));
        }
        planned.push(PlannedEntry {
            index: i,
            components,
            is_dir,
            size,
        });
    }

    // A file entry must not also be a parent directory of another entry.
    let file_keys: HashSet<String> = planned
        .iter()
        .filter(|p| !p.is_dir)
        .map(|p| p.components.join("/").to_lowercase())
        .collect();
    for p in &planned {
        for depth in 1..p.components.len() {
            let parent = p.components[..depth].join("/").to_lowercase();
            if file_keys.contains(&parent) {
                return Err(rejected(
                    "Archive contains a file and a directory with the same name",
                ));
            }
        }
    }
    Ok(planned)
}

/// A file written by [`write_entries`]: its archive path and, when hashing, its SHA-256.
pub(crate) struct WrittenFile {
    pub key: String,
    pub sha256: Option<String>,
}

/// Phase 2: write planned entries below `staging` (created if missing). Never trusts the
/// declared size. `cancelled` is polled between entries; `progress` receives bytes.
pub(crate) fn write_entries(
    zip: &mut zip::ZipArchive<fs::File>,
    planned: &[PlannedEntry],
    staging: &Path,
    hash: bool,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> CoreResult<(ExtractReport, Vec<WrittenFile>)> {
    use sha2::Digest;
    fs::create_dir_all(staging).map_err(|e| CoreError::io("Cannot create staging", &e))?;
    let mut report = ExtractReport::default();
    let mut written_files = Vec::new();
    for p in planned {
        if cancelled() {
            return Err(CoreError::cancelled());
        }
        let mut target = staging.to_path_buf();
        p.components.iter().for_each(|c| target.push(c));
        if p.is_dir {
            fs::create_dir_all(&target)
                .map_err(|e| CoreError::io("Cannot create directory", &e))?;
            report.directories += 1;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| CoreError::io("Cannot create directory", &e))?;
        }
        let mut entry = zip
            .by_index(p.index)
            .map_err(|e| rejected(format!("Cannot read entry: {e}")))?;
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| CoreError::io("Cannot create file", &e))?;
        // Read at most declared + 1 bytes.
        let mut limited = (&mut entry).take(p.size + 1);
        let mut hasher = hash.then(sha2::Sha256::new);
        let mut buf = vec![0u8; 64 * 1024];
        let mut written: u64 = 0;
        loop {
            let n = limited
                .read(&mut buf)
                .map_err(|e| rejected(format!("Cannot extract entry: {e}")))?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > p.size {
                return Err(rejected(
                    "Entry is larger than declared (possible zip bomb)",
                ));
            }
            if let Some(h) = hasher.as_mut() {
                h.update(&buf[..n]);
            }
            out.write_all(&buf[..n])
                .map_err(|e| CoreError::io("Cannot write file", &e))?;
            progress(n as u64);
        }
        out.flush()
            .map_err(|e| CoreError::io("Cannot write file", &e))?;
        report.files += 1;
        report.bytes += written;
        written_files.push(WrittenFile {
            key: p.key(),
            sha256: hasher.map(|h| hex::encode(h.finalize())),
        });
    }
    Ok((report, written_files))
}

/// Extract `archive` into directory `dest`. `staging` must be an empty, MCPanel-owned
/// directory on the same volume as `dest` (it is removed afterwards).
pub fn extract_zip(
    archive: &Path,
    dest: &SafePath,
    staging: &Path,
    limits: ExtractLimits,
    overwrite: bool,
) -> CoreResult<ExtractReport> {
    dest.ensure_no_reparse_points()?;
    let mut zip = open_zip(archive)?;
    let planned = plan_entries(&mut zip, limits)?;

    // Conflicts with existing files.
    if !overwrite {
        let dest_abs = dest.absolute();
        let conflicts: Vec<String> = planned
            .iter()
            .filter(|p| !p.is_dir)
            .filter(|p| {
                let mut t = dest_abs.clone();
                p.components.iter().for_each(|c| t.push(c));
                fs::symlink_metadata(&t).is_ok()
            })
            .take(20)
            .map(|p| p.components.join("/"))
            .collect();
        if !conflicts.is_empty() {
            return Err(CoreError::new(
                ErrorCode::PathExists,
                "Some files in the archive already exist in the destination",
            )
            .with_details(serde_json::json!({ "conflicts": conflicts })));
        }
    }

    let result = (|| -> CoreResult<ExtractReport> {
        let (report, _) =
            write_entries(&mut zip, &planned, staging, false, &|| false, &mut |_| {})?;
        // Phase 3: move into place.
        move_merge(staging, &dest.absolute(), overwrite)?;
        Ok(report)
    })();
    let _ = fsx::remove_tree_no_follow(staging);
    result
}

/// Move the contents of `src` into `dst`, merging directories. Never descends into an
/// existing reparse point in the destination.
fn move_merge(src: &Path, dst: &Path, overwrite: bool) -> CoreResult<()> {
    if let Ok(md) = fs::symlink_metadata(dst) {
        if is_reparse_point(&md) {
            return Err(rejected(format!(
                "Destination '{}' is a link; refusing to extract through it",
                dst.display()
            )));
        }
    } else {
        fs::create_dir(dst).map_err(|e| CoreError::io("Cannot create directory", &e))?;
    }
    for entry in fs::read_dir(src).map_err(|e| CoreError::io("Cannot read staging", &e))? {
        let entry = entry.map_err(|e| CoreError::io("Cannot read staging", &e))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let md = entry
            .metadata()
            .map_err(|e| CoreError::io("Cannot inspect staging", &e))?;
        if md.is_dir() {
            move_merge(&from, &to, overwrite)?;
        } else {
            if let Ok(existing) = fs::symlink_metadata(&to) {
                if !overwrite {
                    return Err(CoreError::new(
                        ErrorCode::PathExists,
                        format!("'{}' already exists", to.display()),
                    ));
                }
                if existing.is_dir() && !is_reparse_point(&existing) {
                    return Err(CoreError::new(
                        ErrorCode::PathExists,
                        format!("'{}' is a directory", to.display()),
                    ));
                }
                fsx::remove_tree_no_follow(&to)?;
            }
            fs::rename(&from, &to).map_err(|e| CoreError::io("Cannot move file", &e))?;
        }
    }
    Ok(())
}

/// Create a ZIP at `dest_file` containing `entries` (files or directories that share
/// the parent `base`). Links are skipped; highly sensitive files are skipped and counted.
pub fn create_zip(
    base: &SafePath,
    entries: &[SafePath],
    dest_file: &Path,
) -> CoreResult<ZipReport> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest_file)
        .map_err(|e| CoreError::io("Cannot create archive", &e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let mut report = ZipReport::default();
    let dest_canonical = dest_file.to_path_buf();

    let result = (|| -> CoreResult<()> {
        for entry in entries {
            if !entry.starts_with(base) || entry.components().len() != base.components().len() + 1 {
                return Err(CoreError::invalid(
                    "All entries must be in the same directory",
                ));
            }
            entry.ensure_no_reparse_points()?;
            add_to_zip(
                &mut writer,
                options,
                entry.absolute(),
                entry.components().to_vec(),
                base.components().len(),
                &dest_canonical,
                &mut report,
            )?;
        }
        Ok(())
    })();
    match result.and_then(|_| {
        writer
            .finish()
            .map(|_| ())
            .map_err(|e| CoreError::internal(format!("Cannot finish archive: {e}")))
    }) {
        Ok(()) => Ok(report),
        Err(e) => {
            let _ = fs::remove_file(dest_file);
            Err(e)
        }
    }
}

fn add_to_zip(
    writer: &mut zip::ZipWriter<fs::File>,
    options: zip::write::SimpleFileOptions,
    abs: PathBuf,
    components: Vec<String>,
    strip: usize,
    dest: &Path,
    report: &mut ZipReport,
) -> CoreResult<()> {
    let md = fs::symlink_metadata(&abs).map_err(|e| CoreError::io("Cannot inspect file", &e))?;
    if is_reparse_point(&md) {
        report.skipped_links += 1;
        return Ok(());
    }
    if abs == dest {
        return Ok(());
    }
    let name = components[strip..].join("/");
    if md.is_dir() {
        writer
            .add_directory(format!("{name}/"), options)
            .map_err(|e| CoreError::internal(format!("Cannot write archive: {e}")))?;
        let mut children: Vec<_> = fs::read_dir(&abs)
            .map_err(|e| CoreError::io("Cannot read directory", &e))?
            .filter_map(Result::ok)
            .collect();
        children.sort_by_key(|e| e.file_name());
        for child in children {
            let child_name = child.file_name().to_string_lossy().to_string();
            let mut c = components.clone();
            c.push(child_name);
            add_to_zip(writer, options, child.path(), c, strip, dest, report)?;
        }
    } else {
        if classify(&components) == Sensitivity::HighlySensitive {
            report.skipped_sensitive += 1;
            return Ok(());
        }
        writer
            .start_file(name, options)
            .map_err(|e| CoreError::internal(format!("Cannot write archive: {e}")))?;
        let mut f = fs::File::open(&abs).map_err(|e| CoreError::io("Cannot read file", &e))?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = f
                .read(&mut buf)
                .map_err(|e| CoreError::io("Cannot read file", &e))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .map_err(|e| CoreError::io("Cannot write archive", &e))?;
            report.bytes += n as u64;
        }
        report.files += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::safepath::SafeRoot;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            if name.ends_with('/') {
                w.add_directory(name.to_string(), o).unwrap();
            } else {
                w.start_file(name.to_string(), o).unwrap();
                w.write_all(data).unwrap();
            }
        }
        w.finish().unwrap();
    }

    fn setup() -> (tempfile::TempDir, SafeRoot, tempfile::TempDir) {
        let server = tempfile::tempdir().unwrap();
        let root = SafeRoot::open(server.path()).unwrap();
        let other = tempfile::tempdir().unwrap();
        (server, root, other)
    }

    #[test]
    fn extracts_normal_archive() {
        let (_s, root, other) = setup();
        let zip_path = other.path().join("a.zip");
        make_zip(
            &zip_path,
            &[("dir/", b""), ("dir/a.txt", b"hello"), ("b.txt", b"x")],
        );
        let dest = root.root_path();
        let staging = root.path().join(".mcpanel").join("staging").join("t1");
        let r = extract_zip(&zip_path, &dest, &staging, ExtractLimits::default(), false).unwrap();
        assert_eq!(r.files, 2);
        assert_eq!(fs::read(root.path().join("dir/a.txt")).unwrap(), b"hello");
        assert!(!staging.exists());
    }

    #[test]
    fn rejects_zip_slip() {
        let (_s, root, other) = setup();
        for evil in [
            "../evil.txt",
            "a/../../evil.txt",
            "C:/evil.txt",
            "/abs.txt",
            "con.txt",
            "a:b",
        ] {
            let zip_path = other.path().join("evil.zip");
            let _ = fs::remove_file(&zip_path);
            make_zip(&zip_path, &[(evil, b"pwn")]);
            let staging = other.path().join("staging");
            let err = extract_zip(
                &zip_path,
                &root.root_path(),
                &staging,
                ExtractLimits::default(),
                false,
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::ArchiveRejected, "{evil}");
        }
        assert!(!other.path().join("evil.txt").exists());
    }

    #[test]
    fn rejects_case_collisions() {
        let (_s, root, other) = setup();
        let zip_path = other.path().join("dup.zip");
        make_zip(&zip_path, &[("File.txt", b"a"), ("file.TXT", b"b")]);
        let err = extract_zip(
            &zip_path,
            &root.root_path(),
            &other.path().join("st"),
            ExtractLimits::default(),
            false,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ArchiveRejected);
    }

    #[test]
    fn enforces_limits() {
        let (_s, root, other) = setup();
        let zip_path = other.path().join("big.zip");
        make_zip(&zip_path, &[("a.txt", &[b'a'; 4096]), ("b.txt", b"b")]);
        let limits = ExtractLimits {
            max_entries: 1,
            ..ExtractLimits::default()
        };
        assert!(
            extract_zip(
                &zip_path,
                &root.root_path(),
                &other.path().join("st"),
                limits,
                false
            )
            .is_err()
        );
        let limits = ExtractLimits {
            max_total_bytes: 1000,
            ..ExtractLimits::default()
        };
        assert!(
            extract_zip(
                &zip_path,
                &root.root_path(),
                &other.path().join("st"),
                limits,
                false
            )
            .is_err()
        );
    }

    #[test]
    fn reports_conflicts_without_overwrite() {
        let (_s, root, other) = setup();
        fs::write(root.path().join("a.txt"), b"old").unwrap();
        let zip_path = other.path().join("c.zip");
        make_zip(&zip_path, &[("a.txt", b"new")]);
        let err = extract_zip(
            &zip_path,
            &root.root_path(),
            &other.path().join("st"),
            ExtractLimits::default(),
            false,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::PathExists);
        extract_zip(
            &zip_path,
            &root.root_path(),
            &other.path().join("st"),
            ExtractLimits::default(),
            true,
        )
        .unwrap();
        assert_eq!(fs::read(root.path().join("a.txt")).unwrap(), b"new");
    }

    #[test]
    fn zip_skips_sensitive_files() {
        let (_s, root, other) = setup();
        fs::create_dir_all(root.path().join("plugins/floodgate")).unwrap();
        fs::write(root.path().join("plugins/floodgate/key.pem"), b"secret").unwrap();
        fs::write(
            root.path().join("plugins/floodgate/config.yml"),
            b"ok: true",
        )
        .unwrap();
        let base = root.root_path();
        let entries = vec![root.resolve("plugins").unwrap()];
        let out = other.path().join("out.zip");
        let r = create_zip(&base, &entries, &out).unwrap();
        assert_eq!(r.skipped_sensitive, 1);
        assert_eq!(r.files, 1);
        let mut z = zip::ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
        assert!(z.by_name("plugins/floodgate/key.pem").is_err());
        assert!(z.by_name("plugins/floodgate/config.yml").is_ok());
    }
}
