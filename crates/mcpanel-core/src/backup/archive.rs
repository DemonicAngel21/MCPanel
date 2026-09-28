//! Backup archive format v1: a plain ZIP (deflate) of the server directory plus
//! `mcpanel-manifest.json` listing every file with its size and SHA-256.
//!
//! Excluded: the reserved `.mcpanel` directory, `session.lock` files (held by a running
//! server and meaningless in a backup) and links (never followed). Highly sensitive
//! files are *included* and flagged (decision #6).

use super::{
    BackupManifest, MANIFEST_FORMAT, MANIFEST_FORMAT_VERSION, MANIFEST_NAME, ManifestFile,
    SkippedFile,
};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::files::archive::{ExtractLimits, open_zip, plan_entries};
use crate::files::fsx::is_reparse_point;
use crate::files::sensitivity::{Sensitivity, classify};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Largest manifest MCPanel will read (a manifest lists every file of a server).
const MAX_MANIFEST_BYTES: u64 = 256 * 1024 * 1024;
/// Windows `ERROR_SHARING_VIOLATION` / `ERROR_LOCK_VIOLATION`.
const LOCKED_OS_ERRORS: [i32; 2] = [32, 33];

#[derive(Debug, Clone)]
pub struct ScanEntry {
    pub components: Vec<String>,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

impl ScanEntry {
    pub fn key(&self) -> String {
        self.components.join("/")
    }
}

#[derive(Debug, Default)]
pub struct Scan {
    pub entries: Vec<ScanEntry>,
    pub skipped: Vec<SkippedFile>,
}

impl Scan {
    pub fn total_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.size)
            .sum()
    }
}

fn is_excluded(components: &[String]) -> bool {
    let Some(last) = components.last() else {
        return false;
    };
    (components.len() == 1 && last.eq_ignore_ascii_case(".mcpanel"))
        || last.eq_ignore_ascii_case("session.lock")
}

/// List the files and directories a backup of `root` contains, in a stable order.
pub fn scan(root: &Path) -> CoreResult<Scan> {
    let md = fs::symlink_metadata(root)
        .map_err(|e| CoreError::io("The server directory is not accessible", &e))?;
    if is_reparse_point(&md) || !md.is_dir() {
        return Err(CoreError::new(
            ErrorCode::PathNotFound,
            "The server directory is not a regular folder",
        ));
    }
    let mut scan = Scan::default();
    walk(root, &mut Vec::new(), &mut scan)?;
    Ok(scan)
}

fn walk(dir: &Path, prefix: &mut Vec<String>, scan: &mut Scan) -> CoreResult<()> {
    let mut children: Vec<_> = fs::read_dir(dir)
        .map_err(|e| CoreError::io("Cannot read directory", &e))?
        .collect::<Result<_, _>>()
        .map_err(|e| CoreError::io("Cannot read directory", &e))?;
    children.sort_by_key(|e| e.file_name());
    for child in children {
        let name = child.file_name().to_string_lossy().to_string();
        let mut components = prefix.clone();
        components.push(name);
        if is_excluded(&components) {
            continue;
        }
        let rel = components.join("/");
        let md = fs::symlink_metadata(child.path())
            .map_err(|e| CoreError::io("Cannot inspect file", &e))?;
        if is_reparse_point(&md) {
            scan.skipped.push(SkippedFile {
                path: rel,
                reason: "Links are not followed".into(),
            });
            continue;
        }
        if components.len() == 1 && components[0].eq_ignore_ascii_case(MANIFEST_NAME) {
            scan.skipped.push(SkippedFile {
                path: rel,
                reason: "The name is reserved for the backup manifest".into(),
            });
            continue;
        }
        if md.is_dir() {
            scan.entries.push(ScanEntry {
                components: components.clone(),
                path: child.path(),
                is_dir: true,
                size: 0,
            });
            prefix.push(components.pop().unwrap_or_default());
            let r = walk(&child.path(), prefix, scan);
            prefix.pop();
            r?;
        } else {
            scan.entries.push(ScanEntry {
                components,
                path: child.path(),
                is_dir: false,
                size: md.len(),
            });
        }
    }
    Ok(())
}

fn zip_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::internal(format!("Cannot write backup archive: {e}"))
}

fn is_locked(e: &std::io::Error) -> bool {
    e.raw_os_error()
        .is_some_and(|c| LOCKED_OS_ERRORS.contains(&c))
}

/// Write the archive to `dest` (created new) and return the completed manifest.
/// `manifest` carries the metadata; its file list, totals and skips are filled in here.
pub fn write_archive(
    scan: &Scan,
    mut manifest: BackupManifest,
    dest: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> CoreResult<BackupManifest> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| CoreError::io("Cannot create the backup file", &e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    manifest.files.clear();
    manifest.skipped = scan.skipped.clone();
    manifest.contains_sensitive = false;
    let mut buf = vec![0u8; 256 * 1024];

    for entry in &scan.entries {
        if cancelled() {
            return Err(CoreError::cancelled());
        }
        let name = entry.key();
        if entry.is_dir {
            zip.add_directory(format!("{name}/"), options)
                .map_err(zip_err)?;
            continue;
        }
        let mut f = match fs::File::open(&entry.path) {
            Ok(f) => f,
            Err(e) if is_locked(&e) => {
                manifest.skipped.push(SkippedFile {
                    path: name,
                    reason: "The file is locked by another program".into(),
                });
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Deleted by the running server between scan and archive.
                continue;
            }
            Err(e) => return Err(CoreError::io(format!("Cannot read '{name}'"), &e)),
        };
        zip.start_file(name.as_str(), options).map_err(zip_err)?;
        let mut hasher = Sha256::new();
        let mut size: u64 = 0;
        let mut locked = false;
        loop {
            let n = match f.read(&mut buf) {
                Ok(n) => n,
                Err(e) if is_locked(&e) => {
                    locked = true;
                    break;
                }
                Err(e) => return Err(CoreError::io(format!("Cannot read '{name}'"), &e)),
            };
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            zip.write_all(&buf[..n])
                .map_err(|e| CoreError::io("Cannot write the backup file", &e))?;
            size += n as u64;
            progress(n as u64);
        }
        if locked {
            zip.abort_file().map_err(zip_err)?;
            manifest.skipped.push(SkippedFile {
                path: name,
                reason: "The file is locked by another program".into(),
            });
            continue;
        }
        if classify(&entry.components) == Sensitivity::HighlySensitive {
            manifest.contains_sensitive = true;
        }
        manifest.files.push(ManifestFile {
            path: name,
            size,
            sha256: hex::encode(hasher.finalize()),
        });
    }
    manifest.total_bytes = manifest.files.iter().map(|f| f.size).sum();
    let json = serde_json::to_vec_pretty(&manifest).map_err(zip_err)?;
    zip.start_file(MANIFEST_NAME, options).map_err(zip_err)?;
    zip.write_all(&json)
        .map_err(|e| CoreError::io("Cannot write the backup file", &e))?;
    let file = zip.finish().map_err(zip_err)?;
    file.sync_all()
        .map_err(|e| CoreError::io("Cannot write the backup file", &e))?;
    Ok(manifest)
}

/// SHA-256 and size of a file.
pub fn hash_file(path: &Path) -> CoreResult<(u64, String)> {
    let mut f = fs::File::open(path).map_err(|e| CoreError::io("Cannot read file", &e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| CoreError::io("Cannot read file", &e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((size, hex::encode(hasher.finalize())))
}

fn corrupt(msg: impl Into<String>) -> CoreError {
    CoreError::new(ErrorCode::ArchiveRejected, msg.into())
}

/// Read and validate the manifest of a backup archive.
pub fn read_manifest(archive: &Path) -> CoreResult<BackupManifest> {
    let mut zip = open_zip(archive)?;
    read_manifest_from(&mut zip)
}

pub(crate) fn read_manifest_from(
    zip: &mut zip::ZipArchive<fs::File>,
) -> CoreResult<BackupManifest> {
    let mut entry = zip
        .by_name(MANIFEST_NAME)
        .map_err(|_| corrupt("This is not an MCPanel backup (no manifest)"))?;
    if entry.size() > MAX_MANIFEST_BYTES {
        return Err(corrupt("The backup manifest is too large"));
    }
    let mut bytes = Vec::new();
    (&mut entry)
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| corrupt(format!("Cannot read the backup manifest: {e}")))?;
    let manifest: BackupManifest = serde_json::from_slice(&bytes)
        .map_err(|e| corrupt(format!("The backup manifest is invalid: {e}")))?;
    if manifest.format != MANIFEST_FORMAT {
        return Err(corrupt("This is not an MCPanel backup"));
    }
    if manifest.format_version > MANIFEST_FORMAT_VERSION {
        return Err(CoreError::new(
            ErrorCode::Unsupported,
            "This backup was made by a newer version of MCPanel",
        ));
    }
    Ok(manifest)
}

/// Limits for extracting an MCPanel backup: bounded by the manifest (whose sizes are
/// checked entry by entry), not by a compression-ratio heuristic — worlds contain
/// highly compressible files.
pub(crate) fn backup_limits(manifest: &BackupManifest) -> ExtractLimits {
    ExtractLimits {
        max_entries: 5_000_000,
        max_total_bytes: manifest.total_bytes.saturating_add(MAX_MANIFEST_BYTES),
        max_ratio: u64::MAX,
    }
}

/// Check that the archive's entries match the manifest exactly (names and sizes).
pub(crate) fn check_entries_match(
    planned: &[crate::files::archive::PlannedEntry],
    manifest: &BackupManifest,
) -> CoreResult<()> {
    let expected: HashMap<String, u64> = manifest
        .files
        .iter()
        .map(|f| (f.path.to_lowercase(), f.size))
        .collect();
    let mut seen = 0usize;
    for p in planned.iter().filter(|p| !p.is_dir) {
        let key = p.key();
        if key.eq_ignore_ascii_case(MANIFEST_NAME) {
            continue;
        }
        match expected.get(&key.to_lowercase()) {
            Some(size) if *size == p.size => seen += 1,
            Some(_) => return Err(corrupt(format!("'{key}' has an unexpected size"))),
            None => return Err(corrupt(format!("'{key}' is not listed in the manifest"))),
        }
    }
    if seen != expected.len() {
        return Err(corrupt("Files listed in the manifest are missing"));
    }
    Ok(())
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct VerifyReport {
    pub ok: bool,
    pub files_checked: u64,
    pub bytes_checked: u64,
    pub problems: Vec<String>,
}

/// Read every file in the archive and compare it with the manifest.
pub fn verify_archive(
    archive: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> CoreResult<VerifyReport> {
    let mut zip = open_zip(archive)?;
    let manifest = read_manifest_from(&mut zip)?;
    let mut report = VerifyReport::default();
    let planned = match plan_entries(&mut zip, backup_limits(&manifest))
        .and_then(|p| check_entries_match(&p, &manifest).map(|_| p))
    {
        Ok(p) => p,
        Err(e) => {
            report.problems.push(e.message);
            return Ok(report);
        }
    };
    let expected: HashMap<String, &ManifestFile> = manifest
        .files
        .iter()
        .map(|f| (f.path.to_lowercase(), f))
        .collect();
    let mut buf = vec![0u8; 256 * 1024];
    for p in planned.iter().filter(|p| !p.is_dir) {
        if cancelled() {
            return Err(CoreError::cancelled());
        }
        let key = p.key();
        let Some(want) = expected.get(&key.to_lowercase()) else {
            continue; // the manifest itself
        };
        let mut entry = zip
            .by_index(p.index)
            .map_err(|e| corrupt(format!("Cannot read '{key}': {e}")))?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let read_ok = loop {
            match entry.read(&mut buf) {
                Ok(0) => break true,
                Ok(n) => {
                    hasher.update(&buf[..n]);
                    size += n as u64;
                    progress(n as u64);
                    if size > want.size {
                        break false;
                    }
                }
                Err(e) => {
                    report.problems.push(format!("'{key}' cannot be read: {e}"));
                    break false;
                }
            }
        };
        if read_ok && (size != want.size || hex::encode(hasher.finalize()) != want.sha256) {
            report
                .problems
                .push(format!("'{key}' does not match its recorded hash"));
        } else if !read_ok && size > want.size {
            report
                .problems
                .push(format!("'{key}' is larger than recorded"));
        }
        report.files_checked += 1;
        report.bytes_checked += size;
        if report.problems.len() >= 50 {
            break;
        }
    }
    report.ok = report.problems.is_empty();
    Ok(report)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::backup::{BackupKind, ManifestServer};
    use crate::ids::{BackupId, ServerId};
    use crate::time::Timestamp;

    pub(crate) fn manifest_template() -> BackupManifest {
        BackupManifest {
            format: MANIFEST_FORMAT.into(),
            format_version: MANIFEST_FORMAT_VERSION,
            generator: "test".into(),
            backup_id: BackupId::new(),
            created_at: Timestamp::now(),
            kind: BackupKind::Manual,
            live: false,
            server: ManifestServer {
                id: ServerId::new(),
                name: "Test".into(),
                software: crate::model::InstalledSoftware {
                    software_id: "vanilla".into(),
                    game_version: "1.21.4".into(),
                    build: None,
                    jar: "server.jar".into(),
                    java_min_major: Some(21),
                    java_recommended_major: None,
                },
            },
            contains_sensitive: false,
            total_bytes: 0,
            files: vec![],
            skipped: vec![],
        }
    }

    fn sample_server(root: &Path) {
        fs::create_dir_all(root.join("world/region")).unwrap();
        fs::create_dir_all(root.join("world/empty")).unwrap();
        fs::create_dir_all(root.join(".mcpanel/trash")).unwrap();
        fs::create_dir_all(root.join("plugins/floodgate")).unwrap();
        fs::write(root.join("server.properties"), b"motd=hi\n").unwrap();
        fs::write(root.join("world/level.dat"), vec![7u8; 5000]).unwrap();
        fs::write(
            root.join("world/region/r.0.0.mca"),
            vec![0u8; 3 * 1024 * 1024],
        )
        .unwrap();
        fs::write(root.join("world/session.lock"), b"x").unwrap();
        fs::write(root.join(".mcpanel/trash/old.txt"), b"old").unwrap();
        fs::write(root.join("plugins/floodgate/key.pem"), b"secret").unwrap();
    }

    fn build(root: &Path, dest: &Path) -> BackupManifest {
        let s = scan(root).unwrap();
        write_archive(&s, manifest_template(), dest, &|| false, &mut |_| {}).unwrap()
    }

    #[test]
    fn archive_contains_the_server_without_excluded_files_and_verifies() {
        let server = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        sample_server(server.path());
        let dest = out.path().join("b.zip");
        let m = build(server.path(), &dest);
        let paths: Vec<_> = m.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"server.properties"));
        assert!(paths.contains(&"world/region/r.0.0.mca"));
        assert!(
            paths.contains(&"plugins/floodgate/key.pem"),
            "sensitive files are included"
        );
        assert!(!paths.iter().any(|p| p.contains("session.lock")));
        assert!(!paths.iter().any(|p| p.starts_with(".mcpanel")));
        assert!(m.contains_sensitive);
        assert_eq!(m.total_bytes, 8 + 5000 + 3 * 1024 * 1024 + 6);

        assert_eq!(read_manifest(&dest).unwrap(), m);
        let report = verify_archive(&dest, &|| false, &mut |_| {}).unwrap();
        assert!(report.ok, "{:?}", report.problems);
        assert_eq!(report.files_checked, 4);
    }

    #[test]
    fn tampered_archive_fails_verification() {
        let server = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        sample_server(server.path());
        let good = out.path().join("good.zip");
        let m = build(server.path(), &good);

        // Same manifest, different content for one file.
        let bad = out.path().join("bad.zip");
        let mut w = zip::ZipWriter::new(fs::File::create(&bad).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        let mut src = open_zip(&good).unwrap();
        for i in 0..src.len() {
            let mut e = src.by_index(i).unwrap();
            let name = e.name().to_string();
            if name.ends_with('/') {
                w.add_directory(name, o).unwrap();
                continue;
            }
            let mut data = Vec::new();
            e.read_to_end(&mut data).unwrap();
            if name == "server.properties" {
                data = b"motd=XX\n".to_vec(); // same size, other bytes
            }
            w.start_file(name, o).unwrap();
            w.write_all(&data).unwrap();
        }
        w.finish().unwrap();
        let report = verify_archive(&bad, &|| false, &mut |_| {}).unwrap();
        assert!(!report.ok);
        assert!(
            report.problems[0].contains("server.properties"),
            "{:?}",
            report.problems
        );
        assert_eq!(read_manifest(&bad).unwrap(), m);
    }

    #[test]
    fn non_backup_zip_is_rejected() {
        let out = tempfile::tempdir().unwrap();
        let p = out.path().join("x.zip");
        let mut w = zip::ZipWriter::new(fs::File::create(&p).unwrap());
        w.start_file("a.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"a").unwrap();
        w.finish().unwrap();
        assert_eq!(
            read_manifest(&p).unwrap_err().code,
            ErrorCode::ArchiveRejected
        );
    }

    #[test]
    fn cancellation_stops_writing() {
        let server = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        sample_server(server.path());
        let s = scan(server.path()).unwrap();
        let err = write_archive(
            &s,
            manifest_template(),
            &out.path().join("c.zip"),
            &|| true,
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
    }
}
