//! Data-driven log dialects (`data/log-dialects.json`) and line prefix parsing.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "TRACE" => Self::Trace,
            "DEBUG" => Self::Debug,
            "INFO" => Self::Info,
            "WARN" | "WARNING" => Self::Warn,
            "ERROR" | "SEVERE" => Self::Error,
            "FATAL" => Self::Fatal,
            _ => return None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct DialectFile {
    dialects: HashMap<String, RawDialect>,
}

#[derive(Debug, Deserialize)]
struct RawDialect {
    ready: Vec<String>,
    stopping: Vec<String>,
    save_complete: Vec<String>,
    player_joined: Vec<String>,
    player_left: Vec<String>,
    fatal: Vec<RawFatal>,
}

#[derive(Debug, Deserialize)]
struct RawFatal {
    pattern: String,
    kind: String,
    message: String,
}

#[derive(Debug)]
pub struct FatalPattern {
    pub needle: String,
    pub kind: String,
    pub message: String,
}

#[derive(Debug)]
pub struct LogDialect {
    pub ready: Vec<Regex>,
    pub stopping: Vec<Regex>,
    pub save_complete: Vec<Regex>,
    pub player_joined: Vec<Regex>,
    pub player_left: Vec<Regex>,
    pub fatal: Vec<FatalPattern>,
}

fn compile(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|p| match Regex::new(p) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::error!("invalid dialect regex {p:?}: {e}");
                None
            }
        })
        .collect()
}

static DIALECTS: LazyLock<HashMap<String, LogDialect>> = LazyLock::new(|| {
    let raw = include_str!("../../../../data/log-dialects.json");
    let file: DialectFile = match serde_json::from_str(raw) {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("invalid log-dialects.json: {e}");
            return HashMap::new();
        }
    };
    file.dialects
        .into_iter()
        .map(|(k, d)| {
            (
                k,
                LogDialect {
                    ready: compile(&d.ready),
                    stopping: compile(&d.stopping),
                    save_complete: compile(&d.save_complete),
                    player_joined: compile(&d.player_joined),
                    player_left: compile(&d.player_left),
                    fatal: d
                        .fatal
                        .into_iter()
                        .map(|f| FatalPattern {
                            needle: f.pattern,
                            kind: f.kind,
                            message: f.message,
                        })
                        .collect(),
                },
            )
        })
        .collect()
});

/// Look up a dialect by id; falls back to the generic `minecraft` dialect.
pub fn dialect(id: &str) -> &'static LogDialect {
    DIALECTS
        .get(id)
        .or_else(|| DIALECTS.get("minecraft"))
        .unwrap_or_else(|| {
            static EMPTY: LazyLock<LogDialect> = LazyLock::new(|| LogDialect {
                ready: vec![],
                stopping: vec![],
                save_complete: vec![],
                player_joined: vec![],
                player_left: vec![],
                fatal: vec![],
            });
            &EMPTY
        })
}

static ANSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").expect("static regex"));

// `[12:34:56] [Server thread/INFO]: msg` (vanilla) and `[12:34:56 INFO]: msg` (Paper).
static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\[(?:\d{1,2}:\d{2}:\d{2})(?:\.\d+)?(?: (?P<l1>[A-Z]+))?\](?: \[(?P<thread>[^\]]*?)/(?P<l2>[A-Z]+)\])?(?: \[[^\]]*\])?:? ?",
    )
    .expect("static regex")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLine {
    pub level: Option<LogLevel>,
    /// Message with prefix and ANSI codes removed.
    pub message: String,
}

pub fn strip_ansi(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\x1b') {
        ANSI.replace_all(s, "")
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

pub fn parse_line(raw: &str) -> ParsedLine {
    let clean = strip_ansi(raw);
    // Cheap pre-check before running the regex.
    if clean.starts_with('[')
        && let Some(caps) = PREFIX.captures(&clean)
    {
        let level = caps
            .name("l2")
            .or_else(|| caps.name("l1"))
            .and_then(|m| LogLevel::parse(m.as_str()));
        let end = caps.get(0).map_or(0, |m| m.end());
        return ParsedLine {
            level,
            message: clean[end..].trim_end().to_string(),
        };
    }
    ParsedLine {
        level: None,
        message: clean.trim_end().to_string(),
    }
}

/// Stack-trace continuation lines inherit the level of the previous line.
pub fn is_continuation(raw: &str) -> bool {
    raw.starts_with('\t')
        || raw.starts_with("    at ")
        || raw.starts_with("Caused by:")
        || raw.starts_with("\tat ")
        || (raw.starts_with("... ") && raw.ends_with(" more"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vanilla_and_paper_prefixes() {
        let v =
            parse_line(r#"[12:34:56] [Server thread/INFO]: Done (3.123s)! For help, type "help""#);
        assert_eq!(v.level, Some(LogLevel::Info));
        assert!(v.message.starts_with("Done ("));
        let p = parse_line("[12:34:56 WARN]: Can't keep up!");
        assert_eq!(p.level, Some(LogLevel::Warn));
        assert_eq!(p.message, "Can't keep up!");
        let n = parse_line("Starting net.minecraft.server.Main");
        assert_eq!(n.level, None);
    }

    #[test]
    fn ready_and_player_patterns() {
        let d = dialect("minecraft");
        let m =
            parse_line(r#"[10:00:00] [Server thread/INFO]: Done (12,5s)! For help, type "help""#);
        assert!(d.ready.iter().any(|r| r.is_match(&m.message)));
        let j = parse_line("[10:00:00] [Server thread/INFO]: Steve joined the game");
        let caps = d
            .player_joined
            .iter()
            .find_map(|r| r.captures(&j.message))
            .unwrap();
        assert_eq!(&caps["name"], "Steve");
        // Chat cannot spoof a join.
        let chat = parse_line("[10:00:00] [Server thread/INFO]: <Alex> Steve joined the game");
        assert!(!d.player_joined.iter().any(|r| r.is_match(&chat.message)));
    }

    #[test]
    fn save_confirmation_across_versions() {
        let d = dialect("minecraft");
        let saved = |line: &str| {
            let m = parse_line(line).message;
            d.save_complete.iter().any(|r| r.is_match(&m))
        };
        // 1.13+ and 1.12 wording; 26.x logs command feedback as "System chat: …"
        // (observed with Vanilla 26.3).
        assert!(saved("[18:51:36] [Server thread/INFO]: Saved the game"));
        assert!(saved("[18:51:36] [Server thread/INFO]: Saved the world"));
        assert!(saved(
            "[18:51:36] [Server thread/INFO]: System chat: Saved the game"
        ));
        assert!(!saved(
            "[18:51:36] [Server thread/INFO]: <Alex> Saved the game"
        ));
        assert!(!saved(
            "[18:51:36] [Server thread/INFO]: Saving the game (this may take a moment!)"
        ));
    }

    #[test]
    fn strips_ansi() {
        let p = parse_line("\x1b[33m[12:00:00 WARN]: hi\x1b[0m");
        assert_eq!(p.level, Some(LogLevel::Warn));
        assert_eq!(p.message, "hi");
    }
}
