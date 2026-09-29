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

/// Whether `p` is, or (for a folder) contains, a highly sensitive file. Renaming or
/// moving such a tree would change the names the protection is based on.
fn tree_has_sensitive(p: &SafePath) -> bool {
    fn walk(dir: &Path, comps: &mut Vec<String>, budget: &mut u32) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for e in entries.flatten() {
            if *budget == 0 {
                // Very large trees: be safe and treat them as possibly sensitive.
                return true;
            }
            *budget -= 1;
            let Ok(md) = fs::symlink_metadata(e.path()) else {
                continue;
            };
            if is_reparse_point(&md) {
                continue;
            }
            comps.push(e.file_name().to_string_lossy().to_string());
            let hit = if md.is_dir() {
                walk(&e.path(), comps, budget)
            } else {
                sensitivity::classify(comps) == Sensitivity::HighlySensitive
            };
            comps.pop();
            if hit {
                return true;
            }
        }
        false
    }
    if sensitivity_of(p) == Sensitivity::HighlySensitive {
        return true;
    }
    match fs::symlink_metadata(p.absolute()) {
        Ok(md) if md.is_dir() && !is_reparse_point(&md) => {
            let mut comps = p.components().to_vec();
            let mut budget = 200_000;
            walk(&p.absolute(), &mut comps, &mut budget)
        }
        _ => false,
    }
}

fn protected_error() -> CoreError {
    CoreError::new(
        ErrorCode::SensitiveFile,
        "This contains key material (e.g. the Floodgate key) and is protected by MCPanel; it cannot be renamed, moved or copied here",
    )
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

/// Shown instead of secret `server.properties` values in the text editor; saving the
/// placeholder keeps the stored value.
pub const HIDDEN_VALUE: &str = "<hidden by MCPanel>";

fn is_server_properties(p: &SafePath) -> bool {
    p.components().len() == 1 && p.components()[0].eq_ignore_ascii_case("server.properties")
}

fn secret_keys() -> Vec<String> {
    crate::config::schema::all_schemas()
        .iter()
        .filter(|s| s.sensitive)
        .map(|s| s.key.clone())
        .collect()
}

/// Replace non-empty secret values with [`HIDDEN_VALUE`].
pub fn mask_properties(text: &str, keys: &[String]) -> String {
    let mut doc = crate::config::PropertiesDocument::parse(text);
    let mut changed = false;
    for k in keys {
        if doc.get(k).is_some_and(|v| !v.trim().is_empty()) && doc.set(k, HIDDEN_VALUE).is_ok() {
            changed = true;
        }
    }
    if changed {
        doc.to_text()
    } else {
        text.to_string()
    }
}

/// Put the stored secret values back where the editor kept the placeholder.
fn unmask_properties(new_text: &str, current: &str, keys: &[String]) -> String {
    let stored = crate::config::PropertiesDocument::parse(current);
    let mut doc = crate::config::PropertiesDocument::parse(new_text);
    let mut changed = false;
    for k in keys {
        if doc.get(k).map(str::trim) == Some(HIDDEN_VALUE) {
            let value = stored.get(k).unwrap_or_default().to_string();
            if doc.set(k, &value).is_ok() {
                changed = true;
            }
        }
    }
    if changed {
        doc.to_text()
    } else {
        new_text.to_string()
    }
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
    let content = if is_server_properties(&p) {
        mask_properties(&decoded.text, &secret_keys())
    } else {
        decoded.text
    };
    Ok(TextDocument {
        path: p.relative(),
        content,
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
    let content = if is_server_properties(&p) {
        let current = fs::read(&abs)
            .map(|b| text::decode(&b).text)
            .unwrap_or_default();
        unmask_properties(&req.content, &current, &secret_keys())
    } else {
        req.content.clone()
    };
    let bytes = text::encode(&content, &req.encoding).ok_or_else(|| {
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
    if tree_has_sensitive(&p) || sensitivity_of(&target) == Sensitivity::HighlySensitive {
        return Err(protected_error());
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
        if tree_has_sensitive(src) {
            return Err(protected_error());
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
        if sensitivity_of(src) == Sensitivity::HighlySensitive {
            return Err(protected_error());
        }
        let name = src.name().unwrap_or_default().to_string();
        let target = unique_child(&dest, &name)?;
        // Sensitive files inside a copied folder are left out (the copy would be
        // readable under a different name).
        let base = src.components().to_vec();
        let keep = |rel: &Path| {
            let mut c = base.clone();
            c.extend(rel.iter().map(|s| s.to_string_lossy().to_string()));
            sensitivity::classify(&c) != Sensitivity::HighlySensitive
        };
        let stats = fsx::copy_tree_no_follow(&src.absolute(), &target.absolute(), &keep)?;
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
    let trash_root = root.path().join(RESERVED_DIR).join("trash");
    fsx::ensure_real_dir_chain(root.path(), &trash_root)?;
    let trash = trash_root.join(format!("{}", Timestamp::now().millis()));
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
    let staging_root = root.path().join(RESERVED_DIR).join("staging");
    fsx::ensure_real_dir_chain(root.path(), &staging_root)?;
    let staging = staging_root.join(uuid::Uuid::now_v7().simple().to_string());
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

    #[cfg(windows)]
    #[test]
    fn trash_is_not_followed_through_a_junctioned_mcpanel_folder() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(d.path().join(".mcpanel"))
            .arg(outside.path())
            .output()
            .expect("run mklink");
        assert!(status.status.success(), "mklink /J failed");
        std::fs::write(d.path().join("a.txt"), b"x").unwrap();
        let r = SafeRoot::open(d.path()).unwrap();
        let e = delete(&r, &["a.txt".into()], false).unwrap_err();
        assert_eq!(e.code, ErrorCode::PathRejected);
        assert!(d.path().join("a.txt").exists(), "file stays in place");
        assert_eq!(
            std::fs::read_dir(outside.path()).unwrap().count(),
            0,
            "nothing written outside"
        );
        std::fs::remove_dir(d.path().join(".mcpanel")).unwrap();
    }

    #[test]
    fn server_properties_secrets_are_hidden_in_the_editor_and_kept_on_save() {
        let d = tempfile::tempdir().unwrap();
        let r = SafeRoot::open(d.path()).unwrap();
        std::fs::write(
            d.path().join("server.properties"),
            "motd=Hi\nrcon.password=hunter2\nmanagement-server-secret=abc\n",
        )
        .unwrap();
        let doc = read_text(&r, "server.properties").unwrap();
        assert!(!doc.content.contains("hunter2") && !doc.content.contains("=abc"));
        assert!(doc.content.contains(HIDDEN_VALUE));
        let edited = doc.content.replace("motd=Hi", "motd=Hello");
        write_text(
            &r,
            "server.properties",
            &WriteText {
                content: edited,
                encoding: doc.encoding.clone(),
                expected_sha256: Some(doc.sha256.clone()),
            },
        )
        .unwrap();
        let disk = std::fs::read_to_string(d.path().join("server.properties")).unwrap();
        assert!(disk.contains("motd=Hello"));
        assert!(
            disk.contains("rcon.password=hunter2") && disk.contains("management-server-secret=abc")
        );
        assert!(!disk.contains(HIDDEN_VALUE));
    }

    #[test]
    fn protected_files_cannot_be_renamed_moved_or_copied_out() {
        let d = tempfile::tempdir().unwrap();
        let r = SafeRoot::open(d.path()).unwrap();
        std::fs::create_dir_all(d.path().join("plugins/floodgate")).unwrap();
        std::fs::create_dir_all(d.path().join("elsewhere")).unwrap();
        std::fs::write(d.path().join("plugins/floodgate/key.pem"), b"k").unwrap();
        std::fs::write(d.path().join("plugins/floodgate/config.yml"), b"c").unwrap();
        let code = |e: CoreError| e.code;
        assert_eq!(
            code(rename(&r, "plugins/floodgate/key.pem", "x.txt").unwrap_err()),
            ErrorCode::SensitiveFile
        );
        assert_eq!(
            code(rename(&r, "plugins/floodgate", "fg").unwrap_err()),
            ErrorCode::SensitiveFile
        );
        assert_eq!(
            code(rename(&r, "plugins", "p2").unwrap_err()),
            ErrorCode::SensitiveFile
        );
        assert_eq!(
            code(move_to(&r, &["plugins/floodgate/key.pem".into()], "elsewhere").unwrap_err()),
            ErrorCode::SensitiveFile
        );
        assert_eq!(
            code(move_to(&r, &["plugins/floodgate".into()], "elsewhere").unwrap_err()),
            ErrorCode::SensitiveFile
        );
        assert_eq!(
            code(copy_to(&r, &["plugins/floodgate/key.pem".into()], "elsewhere").unwrap_err()),
            ErrorCode::SensitiveFile
        );
        // Copying the folder leaves the key out.
        copy_to(&r, &["plugins/floodgate".into()], "elsewhere").unwrap();
        assert!(d.path().join("elsewhere/floodgate/config.yml").exists());
        assert!(!d.path().join("elsewhere/floodgate/key.pem").exists());
        // Ordinary files still work, and nothing can be renamed *to* a key name.
        std::fs::write(d.path().join("a.txt"), b"x").unwrap();
        rename(&r, "a.txt", "b.txt").unwrap();
        std::fs::create_dir_all(d.path().join("other/floodgate")).unwrap();
        std::fs::write(d.path().join("other/floodgate/k.txt"), b"x").unwrap();
        assert_eq!(
            code(rename(&r, "other/floodgate/k.txt", "key.pem").unwrap_err()),
            ErrorCode::SensitiveFile
        );
    }
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
