//! Server file operations. Every function takes a [`SafeRoot`] and client-relative paths;
//! nothing here accepts an arbitrary host path except `import_from`/`export_to`, whose
//! host paths come exclusively from native-dialog grants issued by the host.
//!
//! These functions do blocking I/O; callers run them on a blocking thread.

use super::archive::{self, ExtractLimits, ExtractReport, ZipReport};
use super::fsx::{self, is_reparse_point};
use super::safepath::{SafePath, SafeRoot, validate_name};
use super::sensitivity::{self, Sensitivity};
use super::text::{self, LineEnding, TextEncoding};
use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Files larger than this open read-only (first `PREVIEW_BYTES` shown).
pub const MAX_EDITABLE_BYTES: u64 = 5 * 1024 * 1024;
pub const PREVIEW_BYTES: u64 = 1024 * 1024;
/// Reserved MCPanel metadata directory inside each server (trash, staging, disabled).
pub const RESERVED_DIR: &str = ".mcpanel";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    /// Symbolic link or junction (shown, never followed).
    Link,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub modified: Option<Timestamp>,
    pub sensitive: bool,
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDocument {
    pub path: String,
    pub content: String,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub sha256: String,
    pub size: u64,
    pub modified: Option<Timestamp>,
    /// Decoding replaced invalid sequences.
    pub lossy: bool,
    /// Set when the document can only be viewed (too large, lossy, read-only file).
    pub read_only_reason: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteText {
    pub content: String,
    pub encoding: TextEncoding,
    /// SHA-256 of the file as it was read. `None` means "create new / overwrite".
    pub expected_sha256: Option<String>,
}

fn is_reserved(path: &SafePath) -> bool {
    path.components()
        .first()
        .is_some_and(|c| c.eq_ignore_ascii_case(RESERVED_DIR))
}

/// Resolve a client path: validated, not in the reserved area, no reparse points.
pub fn resolve_user_path(root: &SafeRoot, rel: &str) -> CoreResult<SafePath> {
    let p = root.resolve(rel)?;
    if is_reserved(&p) {
        return Err(CoreError::new(
            ErrorCode::PathRejected,
            "The .mcpanel directory is managed by MCPanel",
        ));
    }
    p.ensure_no_reparse_points()?;
    Ok(p)
}

fn sensitivity_of(p: &SafePath) -> Sensitivity {
    sensitivity::classify(p.components())
}

fn ensure_not_sensitive(p: &SafePath) -> CoreResult<()> {
    if sensitivity_of(p) == Sensitivity::HighlySensitive {
        return Err(CoreError::new(
            ErrorCode::SensitiveFile,
            "This file contains key material and is protected by MCPanel",
        ));
    }
    Ok(())
}

fn entry_for(p: &SafePath, md: &fs::Metadata) -> FileEntry {
    let kind = if is_reparse_point(md) {
        EntryKind::Link
    } else if md.is_dir() {
        EntryKind::Directory
    } else {
        EntryKind::File
    };
    FileEntry {
        name: p.name().unwrap_or("").to_string(),
        path: p.relative(),
        kind,
        size: (kind == EntryKind::File).then_some(md.len()),
        modified: md.modified().ok().and_then(Timestamp::from_system_time),
        sensitive: kind == EntryKind::File && sensitivity_of(p) == Sensitivity::HighlySensitive,
        readonly: md.permissions().readonly(),
    }
}

pub fn stat(root: &SafeRoot, rel: &str) -> CoreResult<FileEntry> {
    let p = resolve_user_path(root, rel)?;
    let md =
        fs::symlink_metadata(p.absolute()).map_err(|e| CoreError::io("Cannot read file", &e))?;
    Ok(entry_for(&p, &md))
}

pub fn list_dir(root: &SafeRoot, rel: &str) -> CoreResult<Vec<FileEntry>> {
    let dir = resolve_user_path(root, rel)?;
    let abs = dir.absolute();
    let md = fs::symlink_metadata(&abs).map_err(|e| CoreError::io("Cannot open directory", &e))?;
    if !md.is_dir() {
        return Err(CoreError::invalid("Not a directory"));
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&abs).map_err(|e| CoreError::io("Cannot read directory", &e))? {
        let entry = entry.map_err(|e| CoreError::io("Cannot read directory", &e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if dir.is_root() && name.eq_ignore_ascii_case(RESERVED_DIR) {
            continue;
        }
        // Names that fail validation (exotic on-disk names) are still listed but
        // cannot be targeted by operations; skip them to keep the contract simple.
        let Ok(child) = dir.join(&name) else {
            tracing::debug!(target: "mcpanel::files", "skipping unaddressable entry {name:?}");
            continue;
        };
        let Ok(md) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        out.push(entry_for(&child, &md));
    }
    out.sort_by(|a, b| {
        let da = a.kind == EntryKind::Directory;
        let db = b.kind == EntryKind::Directory;
        db.cmp(&da)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn read_text(root: &SafeRoot, rel: &str) -> CoreResult<TextDocument> {
    let p = resolve_user_path(root, rel)?;
    ensure_not_sensitive(&p)?;
    let abs = p.absolute();
    let md = fs::metadata(&abs).map_err(|e| CoreError::io("Cannot open file", &e))?;
    if !md.is_file() {
        return Err(CoreError::invalid("Not a file"));
    }
    let size = md.len();
    let truncated = size > MAX_EDITABLE_BYTES;
    let bytes = if truncated {
        use std::io::Read;
        let mut buf = Vec::with_capacity(PREVIEW_BYTES as usize);
        fs::File::open(&abs)
            .and_then(|f| f.take(PREVIEW_BYTES).read_to_end(&mut buf))
            .map_err(|e| CoreError::io("Cannot read file", &e))?;
        buf
    } else {
        fs::read(&abs).map_err(|e| CoreError::io("Cannot read file", &e))?
    };
    if text::looks_binary(&bytes) {
        return Err(CoreError::new(
            ErrorCode::Unsupported,
            "This looks like a binary file and cannot be opened in the text editor",
        ));
    }
    let sha = sha256_hex(&bytes);
    let decoded = text::decode(&bytes);
    let read_only_reason = if truncated {
        Some(format!(
            "File is larger than {} MiB; showing the first {} MiB read-only",
            MAX_EDITABLE_BYTES / 1024 / 1024,
            PREVIEW_BYTES / 1024 / 1024
        ))
    } else if decoded.lossy {
        Some("File contains bytes that are not valid in the detected encoding".into())
    } else if md.permissions().readonly() {
        Some("File is marked read-only".into())
    } else {
        None
    };
    Ok(TextDocument {
        path: p.relative(),
        content: decoded.text,
        encoding: decoded.encoding,
        line_ending: decoded.line_ending,
        sha256: sha,
        size,
        modified: md.modified().ok().and_then(Timestamp::from_system_time),
        lossy: decoded.lossy,
        read_only_reason,
        truncated,
    })
}

pub fn write_text(root: &SafeRoot, rel: &str, req: &WriteText) -> CoreResult<TextDocument> {
    let p = resolve_user_path(root, rel)?;
    if p.is_root() {
        return Err(CoreError::invalid("Cannot write to the server root"));
    }
    ensure_not_sensitive(&p)?;
    let abs = p.absolute();
    match (fs::read(&abs), &req.expected_sha256) {
        (Ok(current), Some(expected)) => {
            if &sha256_hex(&current) != expected {
                return Err(CoreError::new(
                    ErrorCode::FileChangedOnDisk,
                    "The file was changed outside the editor since it was opened",
                ));
            }
        }
        (Ok(_), None) => {}
        (Err(e), Some(_)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CoreError::new(
                ErrorCode::FileChangedOnDisk,
                "The file was deleted outside the editor since it was opened",
            ));
        }
        (Err(e), _) if e.kind() == std::io::ErrorKind::NotFound => {}
        (Err(e), _) => return Err(CoreError::io("Cannot read file", &e)),
    }
    let bytes = text::encode(&req.content, &req.encoding).ok_or_else(|| {
        CoreError::invalid(format!(
            "The text contains characters that cannot be saved as {}",
            req.encoding.name
        ))
    })?;
    if bytes.len() as u64 > MAX_EDITABLE_BYTES {
        return Err(CoreError::new(
            ErrorCode::FileTooLarge,
            "Document is too large to save",
        ));
    }
    fsx::atomic_write(&abs, &bytes)?;
    read_text(root, rel)
}

pub fn create_dir(root: &SafeRoot, rel: &str) -> CoreResult<FileEntry> {
    let p = resolve_user_path(root, rel)?;
    if p.is_root() {
        return Err(CoreError::invalid("Directory name required"));
    }
    fs::create_dir(p.absolute()).map_err(|e| CoreError::io("Cannot create directory", &e))?;
    stat(root, &p.relative())
}

pub fn create_file(root: &SafeRoot, rel: &str) -> CoreResult<FileEntry> {
    let p = resolve_user_path(root, rel)?;
    if p.is_root() {
        return Err(CoreError::invalid("File name required"));
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(p.absolute())
        .map_err(|e| CoreError::io("Cannot create file", &e))?;
    stat(root, &p.relative())
}

pub fn rename(root: &SafeRoot, rel: &str, new_name: &str) -> CoreResult<FileEntry> {
    validate_name(new_name)?;
    let p = resolve_user_path(root, rel)?;
    let parent = p
        .parent()
        .ok_or_else(|| CoreError::invalid("Cannot rename the server root"))?;
    let target = parent.join(new_name)?;
    if is_reserved(&target) {
        return Err(CoreError::new(ErrorCode::PathRejected, "Reserved name"));
    }
    let case_only = p.name().is_some_and(|n| n.eq_ignore_ascii_case(new_name));
    if !case_only && fs::symlink_metadata(target.absolute()).is_ok() {
        return Err(CoreError::new(
            ErrorCode::PathExists,
            "An entry with that name already exists",
        ));
    }
    fs::rename(p.absolute(), target.absolute()).map_err(|e| CoreError::io("Cannot rename", &e))?;
    stat(root, &target.relative())
}

fn resolve_many(root: &SafeRoot, rels: &[String]) -> CoreResult<Vec<SafePath>> {
    let mut out = Vec::with_capacity(rels.len());
    for r in rels {
        let p = resolve_user_path(root, r)?;
        if p.is_root() {
            return Err(CoreError::invalid("The server root cannot be selected"));
        }
        fs::symlink_metadata(p.absolute()).map_err(|e| CoreError::io("Cannot find entry", &e))?;
        out.push(p);
    }
    Ok(out)
}

fn ensure_dir(p: &SafePath) -> CoreResult<()> {
    let md = fs::symlink_metadata(p.absolute())
        .map_err(|e| CoreError::io("Destination not found", &e))?;
    if !md.is_dir() || is_reparse_point(&md) {
        return Err(CoreError::invalid("Destination must be a directory"));
    }
    Ok(())
}

pub fn move_to(root: &SafeRoot, rels: &[String], dest_dir: &str) -> CoreResult<()> {
    let sources = resolve_many(root, rels)?;
    let dest = resolve_user_path(root, dest_dir)?;
    ensure_dir(&dest)?;
    for src in &sources {
        if dest.starts_with(src) {
            return Err(CoreError::invalid("Cannot move a directory into itself"));
        }
        let target = dest.join(src.name().unwrap_or_default())?;
        if fs::symlink_metadata(target.absolute()).is_ok() {
            return Err(CoreError::new(
                ErrorCode::PathExists,
                format!("'{}' already exists in the destination", target.relative()),
            ));
        }
    }
    for src in &sources {
        let target = dest.join(src.name().unwrap_or_default())?;
        fs::rename(src.absolute(), target.absolute())
            .map_err(|e| CoreError::io("Cannot move", &e))?;
    }
    Ok(())
}

pub fn copy_to(root: &SafeRoot, rels: &[String], dest_dir: &str) -> CoreResult<fsx::CopyStats> {
    let sources = resolve_many(root, rels)?;
    let dest = resolve_user_path(root, dest_dir)?;
    ensure_dir(&dest)?;
    let mut total = fsx::CopyStats::default();
    for src in &sources {
        if dest.starts_with(src) {
            return Err(CoreError::invalid("Cannot copy a directory into itself"));
        }
        let name = src.name().unwrap_or_default().to_string();
        let target = unique_child(&dest, &name)?;
        let stats = fsx::copy_tree_no_follow(&src.absolute(), &target.absolute(), &|_| true)?;
        total.files += stats.files;
        total.bytes += stats.bytes;
        total.skipped_links += stats.skipped_links;
    }
    Ok(total)
}

/// `name`, or `name (copy)`, `name (copy 2)`, … if taken.
fn unique_child(dir: &SafePath, name: &str) -> CoreResult<SafePath> {
    let candidate = dir.join(name)?;
    if fs::symlink_metadata(candidate.absolute()).is_err() {
        return Ok(candidate);
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for i in 1..1000 {
        let suffix = if i == 1 {
            " (copy)".to_string()
        } else {
            format!(" (copy {i})")
        };
        let c = dir.join(&format!("{stem}{suffix}{ext}"))?;
        if fs::symlink_metadata(c.absolute()).is_err() {
            return Ok(c);
        }
    }
    Err(CoreError::new(ErrorCode::PathExists, "Too many copies"))
}

/// Move entries to `<root>/.mcpanel/trash/<timestamp>/` (recoverable), or delete them
/// permanently. Links are removed as links.
pub fn delete(root: &SafeRoot, rels: &[String], permanent: bool) -> CoreResult<()> {
    let targets = resolve_many(root, rels)?;
    if permanent {
        for t in &targets {
            fsx::remove_tree_no_follow(&t.absolute())?;
        }
        return Ok(());
    }
    let trash = root
        .path()
        .join(RESERVED_DIR)
        .join("trash")
        .join(format!("{}", Timestamp::now().millis()));
    fs::create_dir_all(&trash).map_err(|e| CoreError::io("Cannot create trash", &e))?;
    for t in &targets {
        let flat = t.components().join("__");
        fs::rename(t.absolute(), trash.join(flat))
            .map_err(|e| CoreError::io("Cannot move to trash", &e))?;
    }
    Ok(())
}

pub fn zip(
    root: &SafeRoot,
    rels: &[String],
    archive_name: &str,
) -> CoreResult<(FileEntry, ZipReport)> {
    validate_name(archive_name)?;
    if !archive_name.to_ascii_lowercase().ends_with(".zip") {
        return Err(CoreError::invalid("Archive name must end with .zip"));
    }
    let entries = resolve_many(root, rels)?;
    let base = entries
        .first()
        .and_then(SafePath::parent)
        .ok_or_else(|| CoreError::invalid("Select at least one entry"))?;
    let dest = base.join(archive_name)?;
    if fs::symlink_metadata(dest.absolute()).is_ok() {
        return Err(CoreError::new(
            ErrorCode::PathExists,
            "An archive with that name already exists",
        ));
    }
    let report = archive::create_zip(&base, &entries, &dest.absolute())?;
    Ok((stat(root, &dest.relative())?, report))
}

pub fn unzip(
    root: &SafeRoot,
    archive_rel: &str,
    dest_dir: &str,
    overwrite: bool,
) -> CoreResult<ExtractReport> {
    let archive_path = resolve_user_path(root, archive_rel)?;
    let dest = resolve_user_path(root, dest_dir)?;
    ensure_dir(&dest)?;
    let staging = root
        .path()
        .join(RESERVED_DIR)
        .join("staging")
        .join(uuid::Uuid::now_v7().simple().to_string());
    archive::extract_zip(
        &archive_path.absolute(),
        &dest,
        &staging,
        ExtractLimits::default(),
        overwrite,
    )
}

/// Copy a host file or directory (from a native-dialog grant) into `dest_dir`.
pub fn import_from(root: &SafeRoot, source: &Path, dest_dir: &str) -> CoreResult<FileEntry> {
    let dest = resolve_user_path(root, dest_dir)?;
    ensure_dir(&dest)?;
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| CoreError::invalid("Source has no file name"))?;
    let target = dest.join(&name)?;
    if fs::symlink_metadata(target.absolute()).is_ok() {
        return Err(CoreError::new(
            ErrorCode::PathExists,
            format!("'{name}' already exists"),
        ));
    }
    fsx::copy_tree_no_follow(source, &target.absolute(), &|_| true)?;
    stat(root, &target.relative())
}

/// Copy a server file to a host location chosen in a native save dialog.
pub fn export_to(root: &SafeRoot, rel: &str, dest: &Path) -> CoreResult<u64> {
    let p = resolve_user_path(root, rel)?;
    ensure_not_sensitive(&p)?;
    let md =
        fs::symlink_metadata(p.absolute()).map_err(|e| CoreError::io("Cannot find file", &e))?;
    if !md.is_file() {
        return Err(CoreError::invalid(
            "Only files can be exported; zip directories first",
        ));
    }
    fs::copy(p.absolute(), dest).map_err(|e| CoreError::io("Cannot export file", &e))
}

/// Case-insensitive file-name search below `rel`, bounded by `limit` results and a
/// visit budget so huge worlds cannot stall the UI.
pub fn search(root: &SafeRoot, rel: &str, query: &str, limit: usize) -> CoreResult<Vec<FileEntry>> {
    let start = resolve_user_path(root, rel)?;
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut budget: u32 = 100_000;
    let mut stack = vec![start];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(dir.absolute()) else {
            continue;
        };
        for entry in rd.flatten() {
            budget = budget.saturating_sub(1);
            if budget == 0 || out.len() >= limit {
                return Ok(out);
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if dir.is_root() && name.eq_ignore_ascii_case(RESERVED_DIR) {
                continue;
            }
            let Ok(child) = dir.join(&name) else { continue };
            let Ok(md) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if name.to_lowercase().contains(&q) {
                out.push(entry_for(&child, &md));
            }
            if md.is_dir() && !is_reparse_point(&md) {
                stack.push(child);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> (tempfile::TempDir, SafeRoot) {
        let d = tempfile::tempdir().unwrap();
        let r = SafeRoot::open(d.path()).unwrap();
        (d, r)
    }

    #[test]
    fn list_hides_reserved_and_sorts() {
        let (d, r) = root();
        fs::create_dir(d.path().join(".mcpanel")).unwrap();
        fs::create_dir(d.path().join("world")).unwrap();
        fs::write(d.path().join("a.txt"), b"x").unwrap();
        fs::write(d.path().join("Z.txt"), b"x").unwrap();
        let names: Vec<_> = list_dir(&r, "")
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["world", "a.txt", "Z.txt"]);
        assert!(list_dir(&r, ".mcpanel").is_err());
    }

    #[test]
    fn edit_roundtrip_with_conflict_detection() {
        let (d, r) = root();
        fs::write(d.path().join("server.properties"), b"motd=A\r\n").unwrap();
        let doc = read_text(&r, "server.properties").unwrap();
        assert_eq!(doc.line_ending, LineEnding::CrLf);
        let saved = write_text(
            &r,
            "server.properties",
            &WriteText {
                content: "motd=B\r\n".into(),
                encoding: doc.encoding.clone(),
                expected_sha256: Some(doc.sha256.clone()),
            },
        )
        .unwrap();
        assert_eq!(
            fs::read(d.path().join("server.properties")).unwrap(),
            b"motd=B\r\n"
        );
        // Stale hash → conflict.
        let err = write_text(
            &r,
            "server.properties",
            &WriteText {
                content: "motd=C\r\n".into(),
                encoding: saved.encoding.clone(),
                expected_sha256: Some(doc.sha256),
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::FileChangedOnDisk);
    }

    #[test]
    fn sensitive_files_cannot_be_read_or_exported() {
        let (d, r) = root();
        fs::create_dir_all(d.path().join("plugins/floodgate")).unwrap();
        fs::write(d.path().join("plugins/floodgate/key.pem"), b"k").unwrap();
        let entries = list_dir(&r, "plugins/floodgate").unwrap();
        assert!(entries[0].sensitive);
        assert_eq!(
            read_text(&r, "plugins/floodgate/key.pem").unwrap_err().code,
            ErrorCode::SensitiveFile
        );
        let out = tempfile::tempdir().unwrap();
        assert_eq!(
            export_to(&r, "plugins/floodgate/key.pem", &out.path().join("k.pem"))
                .unwrap_err()
                .code,
            ErrorCode::SensitiveFile
        );
    }

    #[test]
    fn delete_moves_to_trash_and_move_rejects_into_self() {
        let (d, r) = root();
        fs::create_dir_all(d.path().join("a/b")).unwrap();
        assert!(move_to(&r, &["a".into()], "a/b").is_err());
        delete(&r, &["a".into()], false).unwrap();
        assert!(!d.path().join("a").exists());
        assert!(d.path().join(".mcpanel/trash").exists());
    }

    #[test]
    fn copy_creates_unique_names() {
        let (d, r) = root();
        fs::write(d.path().join("x.txt"), b"1").unwrap();
        copy_to(&r, &["x.txt".into()], "").unwrap();
        copy_to(&r, &["x.txt".into()], "").unwrap();
        assert!(d.path().join("x (copy).txt").exists());
        assert!(d.path().join("x (copy 2).txt").exists());
    }

    #[test]
    fn zip_and_unzip_roundtrip() {
        let (d, r) = root();
        fs::create_dir_all(d.path().join("cfg")).unwrap();
        fs::write(d.path().join("cfg/a.yml"), b"a: 1").unwrap();
        let (entry, report) = zip(&r, &["cfg".into()], "cfg.zip").unwrap();
        assert_eq!(report.files, 1);
        fs::create_dir(d.path().join("out")).unwrap();
        unzip(&r, &entry.path, "out", false).unwrap();
        assert_eq!(fs::read(d.path().join("out/cfg/a.yml")).unwrap(), b"a: 1");
    }

    #[test]
    fn search_finds_names() {
        let (d, r) = root();
        fs::create_dir_all(d.path().join("plugins/Essentials")).unwrap();
        fs::write(d.path().join("plugins/Essentials/config.yml"), b"").unwrap();
        let hits = search(&r, "", "config", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "plugins/Essentials/config.yml");
    }
}
