//! `SafePath`: the only path type server file operations accept.
//!
//! A `SafePath` is built from a server root plus a *relative* path supplied by a client.
//! Validation is purely lexical and conservative (Windows rules are applied on every
//! platform so behaviour is identical everywhere):
//!
//! - no absolute paths, drive letters, UNC or device prefixes
//! - no `.` / `..` / empty components
//! - no NUL or control characters, no `< > : " | ? *` (`:` also blocks NTFS alternate
//!   data streams)
//! - no reserved device names (`CON`, `NUL`, `COM1`, … with or without extension)
//! - no trailing dots or spaces, no 8.3 short-name aliases (`KEY~1.PEM`)
//! - bounded component and total length
//!
//! After lexical validation the joined path is guaranteed to be *lexically* inside the
//! root. [`SafePath::ensure_no_reparse_points`] additionally checks, component by
//! component, that no existing element below the root is a symlink/junction, so the
//! filesystem cannot redirect an operation outside the root.

use crate::error::{CoreError, CoreResult, ErrorCode};
use std::path::{Path, PathBuf};

const MAX_COMPONENT_LEN: usize = 255;
const MAX_TOTAL_LEN: usize = 4096;
const MAX_DEPTH: usize = 128;

const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

fn reject(reason: impl Into<String>) -> CoreError {
    CoreError::new(ErrorCode::PathRejected, reason.into())
}

/// Validate one path component (a file or directory name).
pub fn validate_name(name: &str) -> CoreResult<()> {
    if name.is_empty() {
        return Err(reject("Empty path component"));
    }
    if name == "." || name == ".." {
        return Err(reject("Relative path components are not allowed"));
    }
    if name.len() > MAX_COMPONENT_LEN {
        return Err(reject("Path component is too long"));
    }
    for ch in name.chars() {
        if (ch as u32) < 0x20 || ch == '\u{7f}' {
            return Err(reject("Control characters are not allowed in names"));
        }
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            return Err(reject(format!("Character '{ch}' is not allowed in names")));
        }
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(reject("Names must not end with a dot or a space"));
    }
    if name.starts_with(' ') {
        return Err(reject("Names must not start with a space"));
    }
    // Device names are reserved regardless of extension ("nul.txt" is still NUL).
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED_NAMES
        .iter()
        .any(|r| r.eq_ignore_ascii_case(stem) || r.to_uppercase() == stem.to_uppercase())
    {
        return Err(reject(format!("'{name}' is a reserved device name")));
    }
    // 8.3 short names (e.g. "PROGRA~1") could alias other entries; reject "~<digit>".
    let bytes = name.as_bytes();
    if bytes
        .windows(2)
        .any(|w| w[0] == b'~' && w[1].is_ascii_digit())
    {
        return Err(reject("Short-name aliases (~N) are not allowed"));
    }
    Ok(())
}

/// Split and validate a client-supplied relative path. `""` means the root.
pub fn parse_relative(input: &str) -> CoreResult<Vec<String>> {
    if input.len() > MAX_TOTAL_LEN {
        return Err(reject("Path is too long"));
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if input.starts_with('/') || input.starts_with('\\') {
        return Err(reject("Absolute paths are not allowed"));
    }
    let components: Vec<&str> = input.split(['/', '\\']).collect();
    // Allow a single trailing separator ("plugins/").
    let components = match components.split_last() {
        Some((last, rest)) if last.is_empty() && !rest.is_empty() => rest.to_vec(),
        _ => components,
    };
    if components.len() > MAX_DEPTH {
        return Err(reject("Path is too deep"));
    }
    let mut out = Vec::with_capacity(components.len());
    for c in components {
        validate_name(c)?;
        out.push(c.to_string());
    }
    Ok(out)
}

/// A validated server root directory.
#[derive(Debug, Clone)]
pub struct SafeRoot {
    root: PathBuf,
}

impl SafeRoot {
    /// `root` must be an existing directory; it is canonicalised.
    pub fn open(root: &Path) -> CoreResult<Self> {
        let canonical = std::fs::canonicalize(root)
            .map_err(|e| CoreError::io("Server directory is not accessible", &e))?;
        if !canonical.is_dir() {
            return Err(CoreError::new(
                ErrorCode::PathNotFound,
                "Server directory does not exist",
            ));
        }
        Ok(Self { root: canonical })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn resolve(&self, relative: &str) -> CoreResult<SafePath> {
        let components = parse_relative(relative)?;
        Ok(SafePath {
            root: self.root.clone(),
            components,
        })
    }

    pub fn root_path(&self) -> SafePath {
        SafePath {
            root: self.root.clone(),
            components: Vec::new(),
        }
    }
}

/// A path proven to be lexically inside a [`SafeRoot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafePath {
    root: PathBuf,
    components: Vec<String>,
}

impl SafePath {
    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    pub fn name(&self) -> Option<&str> {
        self.components.last().map(String::as_str)
    }

    /// Relative path using `/` separators (the canonical client representation).
    pub fn relative(&self) -> String {
        self.components.join("/")
    }

    pub fn absolute(&self) -> PathBuf {
        let mut p = self.root.clone();
        for c in &self.components {
            p.push(c);
        }
        p
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn parent(&self) -> Option<SafePath> {
        if self.components.is_empty() {
            return None;
        }
        Some(SafePath {
            root: self.root.clone(),
            components: self.components[..self.components.len() - 1].to_vec(),
        })
    }

    pub fn join(&self, name: &str) -> CoreResult<SafePath> {
        validate_name(name)?;
        let mut components = self.components.clone();
        components.push(name.to_string());
        if components.len() > MAX_DEPTH {
            return Err(reject("Path is too deep"));
        }
        Ok(SafePath {
            root: self.root.clone(),
            components,
        })
    }

    /// Whether `self` equals or lies below `other` (case-insensitive, as on Windows).
    pub fn starts_with(&self, other: &SafePath) -> bool {
        other.components.len() <= self.components.len()
            && other
                .components
                .iter()
                .zip(&self.components)
                .all(|(a, b)| a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase())
    }

    /// Reject if any *existing* component below the root is a reparse point (symlink or
    /// junction). Components that do not exist yet are fine (creation).
    pub fn ensure_no_reparse_points(&self) -> CoreResult<()> {
        let mut current = self.root.clone();
        for c in &self.components {
            current.push(c);
            match std::fs::symlink_metadata(&current) {
                Ok(md) => {
                    if super::fsx::is_reparse_point(&md) {
                        return Err(reject(format!(
                            "'{}' is a symbolic link or junction; MCPanel does not follow links",
                            c
                        )));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(CoreError::io("Cannot inspect path", &e)),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn accepts_normal_paths() {
        assert_eq!(parse_relative("").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_relative("plugins/Essentials/config.yml").unwrap(),
            vec!["plugins", "Essentials", "config.yml"]
        );
        assert_eq!(
            parse_relative("world\\region\\r.0.0.mca").unwrap(),
            vec!["world", "region", "r.0.0.mca"]
        );
        assert_eq!(parse_relative("plugins/").unwrap(), vec!["plugins"]);
        assert!(parse_relative(".mcpanel").is_ok());
        assert!(parse_relative("server.properties").is_ok());
    }

    #[test]
    fn rejects_traversal_and_absolute() {
        for bad in [
            "..",
            "../x",
            "a/../../b",
            "a/./b",
            "/etc/passwd",
            "\\windows",
            "C:\\Windows",
            "C:",
            "\\\\server\\share",
            "\\\\?\\C:\\x",
            "a//b",
            "a/\\b",
        ] {
            assert!(parse_relative(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn rejects_windows_specials() {
        for bad in [
            "file.txt:stream",
            "file.txt::$DATA",
            "CON",
            "con.txt",
            "Nul.log",
            "COM1",
            "lpt9.dat",
            "COM¹",
            "trailingdot.",
            "trailingspace ",
            " leading",
            "a<b",
            "a>b",
            "a|b",
            "a?b",
            "a*b",
            "a\"b",
            "tab\tname",
            "nul\0byte",
            "KEY~1.PEM",
            "PROGRA~1",
        ] {
            assert!(parse_relative(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn accepts_tilde_without_digit_and_unicode() {
        assert!(parse_relative("backup~old").is_ok());
        assert!(parse_relative("wörld/région").is_ok());
        assert!(parse_relative("console.log").is_ok());
        assert!(parse_relative("CONFIG").is_ok());
        assert!(parse_relative("COM10").is_ok());
    }

    #[test]
    fn starts_with_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let root = SafeRoot::open(dir.path()).unwrap();
        let a = root.resolve("Plugins/Floodgate/key.pem").unwrap();
        let b = root.resolve("plugins/floodgate").unwrap();
        assert!(a.starts_with(&b));
        assert!(!b.starts_with(&a));
    }

    #[test]
    fn absolute_stays_under_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = SafeRoot::open(dir.path()).unwrap();
        let p = root.resolve("a/b/c.txt").unwrap();
        assert!(p.absolute().starts_with(root.path()));
    }

    #[cfg(windows)]
    #[test]
    fn detects_junction_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = SafeRoot::open(dir.path()).unwrap();
        let link = dir.path().join("escape");
        // Directory junctions can be created without admin rights.
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .expect("run mklink");
        assert!(status.status.success(), "mklink /J failed");
        let p = root.resolve("escape/file.txt").unwrap();
        let err = p.ensure_no_reparse_points().unwrap_err();
        assert_eq!(err.code, ErrorCode::PathRejected);
        std::fs::remove_dir(&link).unwrap();
    }

    proptest! {
        #[test]
        fn any_accepted_path_is_lexically_contained(s in "[a-zA-Z0-9 ._~:/\\\\<>|?*-]{0,64}") {
            if let Ok(components) = parse_relative(&s) {
                for c in &components {
                    prop_assert!(c != ".." && c != "." && !c.is_empty());
                    prop_assert!(!c.contains(':') && !c.contains('/') && !c.contains('\\'));
                }
            }
        }

        #[test]
        fn never_panics(s in any::<String>()) {
            let _ = parse_relative(&s);
        }
    }
}
