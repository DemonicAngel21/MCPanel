//! Sensitivity classification of server files (data-driven, see
//! `data/sensitive-files.json`).

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Normal,
    /// Key material / tokens. Local backups only; never exposed without explicit reveal.
    HighlySensitive,
}

#[derive(Debug, Deserialize)]
struct PatternFile {
    patterns: Vec<PatternEntry>,
}

#[derive(Debug, Deserialize)]
struct PatternEntry {
    pattern: String,
    #[allow(dead_code)]
    reason: String,
}

static PATTERNS: LazyLock<Vec<Vec<String>>> = LazyLock::new(|| {
    let raw = include_str!("../../../../data/sensitive-files.json");
    match serde_json::from_str::<PatternFile>(raw) {
        Ok(f) => f
            .patterns
            .into_iter()
            .map(|p| p.pattern.split('/').map(|s| s.to_lowercase()).collect())
            .collect(),
        Err(e) => {
            // The file is compiled in and covered by tests; failing closed is not
            // possible here, so log loudly. Tests guarantee this never happens.
            tracing::error!("invalid sensitive-files.json: {e}");
            Vec::new()
        }
    }
});

/// Classify a path given as validated components relative to the server root.
pub fn classify(components: &[String]) -> Sensitivity {
    let lower: Vec<String> = components.iter().map(|c| c.to_lowercase()).collect();
    if PATTERNS.iter().any(|p| matches(p, &lower)) {
        Sensitivity::HighlySensitive
    } else {
        Sensitivity::Normal
    }
}

/// Whether a directory (given as components) contains or could contain sensitive files
/// below it — used to warn before bulk operations. Conservative: true if any pattern
/// could match a descendant.
pub fn may_contain_sensitive(components: &[String]) -> bool {
    let lower: Vec<String> = components.iter().map(|c| c.to_lowercase()).collect();
    PATTERNS.iter().any(|p| prefix_could_match(p, &lower))
}

fn matches(pattern: &[String], path: &[String]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((head, rest)) if head == "**" => (0..=path.len()).any(|i| matches(rest, &path[i..])),
        Some((head, rest)) => match path.split_first() {
            Some((p, prest)) => glob_component(head, p) && matches(rest, prest),
            None => false,
        },
    }
}

fn prefix_could_match(pattern: &[String], dir: &[String]) -> bool {
    match (pattern.split_first(), dir.split_first()) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some((head, _)), Some(_)) if head == "**" => true,
        (Some((head, rest)), Some((d, drest))) => {
            glob_component(head, d) && prefix_could_match(rest, drest)
        }
    }
}

fn glob_component(pattern: &str, name: &str) -> bool {
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        name.len() >= prefix.len() + suffix.len()
            && name.starts_with(prefix)
            && name.ends_with(suffix)
    } else {
        pattern == name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(p: &str) -> Vec<String> {
        p.split('/').map(String::from).collect()
    }

    #[test]
    fn pattern_file_parses() {
        assert!(!PATTERNS.is_empty());
    }

    #[test]
    fn floodgate_key_is_sensitive_anywhere() {
        assert_eq!(
            classify(&c("plugins/floodgate/key.pem")),
            Sensitivity::HighlySensitive
        );
        assert_eq!(
            classify(&c("config/Floodgate/KEY.PEM")),
            Sensitivity::HighlySensitive
        );
        assert_eq!(
            classify(&c("plugins/Geyser-Spigot/saved-refresh-tokens.json")),
            Sensitivity::HighlySensitive
        );
    }

    #[test]
    fn normal_files_are_normal() {
        assert_eq!(classify(&c("server.properties")), Sensitivity::Normal);
        assert_eq!(
            classify(&c("plugins/floodgate/config.yml")),
            Sensitivity::Normal
        );
        assert_eq!(classify(&c("key.pem")), Sensitivity::Normal);
    }

    #[test]
    fn directories_that_may_contain_sensitive() {
        assert!(may_contain_sensitive(&c("plugins")));
        assert!(may_contain_sensitive(&c("plugins/floodgate")));
        assert!(may_contain_sensitive(&[]));
    }
}
