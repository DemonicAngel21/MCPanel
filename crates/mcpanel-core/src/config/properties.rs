//! Lossless Java `.properties` parser/writer.
//!
//! Unmodified lines (comments, blank lines, untouched entries, continuation lines) are
//! written back byte-for-byte; only changed entries are re-serialised. Non-ASCII
//! characters are written as `\uXXXX` escapes so the file is readable regardless of
//! which charset the server uses to load it.

use crate::error::{CoreError, CoreResult};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Raw(String),
    Entry {
        key: String,
        value: String,
        raw: String,
        dirty: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertiesDocument {
    lines: Vec<Line>,
    newline: &'static str,
    trailing_newline: bool,
}

impl Default for PropertiesDocument {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            newline: "\n",
            trailing_newline: true,
        }
    }
}

/// Decode file bytes: UTF-8 when valid, otherwise ISO-8859-1 (Java's historic default).
pub fn decode_bytes(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

fn ends_with_odd_backslashes(s: &str) -> bool {
    s.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1
}

fn is_comment_or_blank(s: &str) -> bool {
    let t = s.trim_start_matches([' ', '\t', '\x0c']);
    t.is_empty() || t.starts_with('#') || t.starts_with('!')
}

impl PropertiesDocument {
    pub fn parse(text: &str) -> Self {
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing_newline = text.ends_with('\n') || text.is_empty();
        let physical: Vec<&str> = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect();
        let physical = if trailing_newline && physical.last() == Some(&"") {
            &physical[..physical.len() - 1]
        } else {
            &physical[..]
        };
        let mut lines = Vec::new();
        let mut i = 0;
        while i < physical.len() {
            let first = physical[i];
            if is_comment_or_blank(first) {
                lines.push(Line::Raw(first.to_string()));
                i += 1;
                continue;
            }
            // Join continuation lines.
            let mut raw_parts = vec![first];
            let mut logical = String::new();
            let mut cur = first.to_string();
            loop {
                if ends_with_odd_backslashes(&cur) && i + 1 < physical.len() {
                    cur.pop();
                    logical.push_str(&cur);
                    i += 1;
                    raw_parts.push(physical[i]);
                    cur = physical[i]
                        .trim_start_matches([' ', '\t', '\x0c'])
                        .to_string();
                } else {
                    logical.push_str(&cur);
                    break;
                }
            }
            i += 1;
            let (key, value) = split_entry(&logical);
            lines.push(Line::Entry {
                key,
                value,
                raw: raw_parts.join(newline),
                dirty: false,
            });
        }
        Self {
            lines,
            newline,
            trailing_newline,
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().rev().find_map(|l| match l {
            Line::Entry { key: k, value, .. } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Entries in file order (last occurrence wins for duplicates, like Java).
    pub fn entries(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for l in &self.lines {
            if let Line::Entry { key, value, .. } = l {
                if let Some(existing) = out.iter_mut().find(|(k, _)| k == key) {
                    existing.1 = value.clone();
                } else {
                    out.push((key.clone(), value.clone()));
                }
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Set a value, updating the last occurrence in place or appending a new entry.
    pub fn set(&mut self, key: &str, value: &str) -> CoreResult<()> {
        validate_key(key)?;
        for l in self.lines.iter_mut().rev() {
            if let Line::Entry {
                key: k,
                value: v,
                dirty,
                ..
            } = l
                && k == key
            {
                if v != value {
                    *v = value.to_string();
                    *dirty = true;
                }
                return Ok(());
            }
        }
        // A last line ending in an unescaped backslash continues onto the next line; an
        // empty line ends that continuation so the new entry stays separate.
        let dangling = match self.lines.last() {
            Some(Line::Raw(s)) => ends_with_odd_backslashes(s),
            Some(Line::Entry {
                raw, dirty: false, ..
            }) => ends_with_odd_backslashes(raw),
            _ => false,
        };
        if dangling {
            self.lines.push(Line::Raw(String::new()));
        }
        self.lines.push(Line::Entry {
            key: key.to_string(),
            value: value.to_string(),
            raw: String::new(),
            dirty: true,
        });
        Ok(())
    }

    pub fn remove(&mut self, key: &str) {
        self.lines
            .retain(|l| !matches!(l, Line::Entry { key: k, .. } if k == key));
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push_str(self.newline);
            }
            match l {
                Line::Raw(s) => out.push_str(s),
                Line::Entry {
                    raw, dirty: false, ..
                } => out.push_str(raw),
                Line::Entry { key, value, .. } => {
                    out.push_str(&escape(key, true));
                    out.push('=');
                    out.push_str(&escape(value, false));
                }
            }
        }
        if self.trailing_newline && !self.lines.is_empty() {
            out.push_str(self.newline);
        }
        out
    }
}

pub fn validate_key(key: &str) -> CoreResult<()> {
    if key.is_empty()
        || key.len() > 128
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(CoreError::invalid(format!("Invalid property key '{key}'")));
    }
    Ok(())
}

fn split_entry(logical: &str) -> (String, String) {
    let s = logical.trim_start_matches([' ', '\t', '\x0c']);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut key_end = chars.len();
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == '=' || c == ':' || c == ' ' || c == '\t' || c == '\x0c' {
            key_end = i;
            break;
        }
        i += 1;
    }
    let key_raw: String = chars[..key_end.min(chars.len())].iter().collect();
    let mut j = key_end;
    while j < chars.len() && matches!(chars[j], ' ' | '\t' | '\x0c') {
        j += 1;
    }
    if j < chars.len() && (chars[j] == '=' || chars[j] == ':') {
        j += 1;
    }
    while j < chars.len() && matches!(chars[j], ' ' | '\t' | '\x0c') {
        j += 1;
    }
    let value_raw: String = chars[j.min(chars.len())..].iter().collect();
    (unescape(&key_raw), unescape(&value_raw))
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut pending_high: Option<u16> = None;
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\x0c'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                match u16::from_str_radix(&hex, 16) {
                    Ok(unit) if (0xD800..0xDC00).contains(&unit) => pending_high = Some(unit),
                    Ok(unit) if (0xDC00..0xE000).contains(&unit) => {
                        if let Some(high) = pending_high.take() {
                            out.extend(
                                char::decode_utf16([high, unit]).map(|r| r.unwrap_or('\u{FFFD}')),
                            );
                        } else {
                            out.push('\u{FFFD}');
                        }
                    }
                    Ok(unit) => out.push(char::from_u32(unit as u32).unwrap_or('\u{FFFD}')),
                    Err(_) => {
                        out.push_str("\\u");
                        out.push_str(&hex);
                    }
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn escape(s: &str, is_key: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\x0c' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            ' ' if is_key || i == 0 => out.push_str("\\ "),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04X}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "#Minecraft server properties\r\n#Mon Sep 28 12:00:00 CEST 2026\r\nmotd=A Minecraft Server\r\nresource-pack=https\\://example.com/pack.zip\r\nlevel-seed=\r\npvp=true\r\n";

    #[test]
    fn untouched_roundtrip_is_byte_identical() {
        let doc = PropertiesDocument::parse(SAMPLE);
        assert_eq!(doc.to_text(), SAMPLE);
        assert_eq!(
            doc.get("resource-pack"),
            Some("https://example.com/pack.zip")
        );
        assert_eq!(doc.get("level-seed"), Some(""));
    }

    #[test]
    fn modifying_one_key_preserves_everything_else() {
        let mut doc = PropertiesDocument::parse(SAMPLE);
        doc.set("pvp", "false").unwrap();
        doc.set("view-distance", "12").unwrap();
        let text = doc.to_text();
        assert!(text.starts_with("#Minecraft server properties\r\n#Mon Sep 28"));
        assert!(text.contains("resource-pack=https\\://example.com/pack.zip\r\n"));
        assert!(text.contains("pvp=false\r\n"));
        assert!(text.ends_with("view-distance=12\r\n"));
    }

    #[test]
    fn unicode_is_escaped_and_roundtrips() {
        let mut doc = PropertiesDocument::parse("");
        doc.set("motd", "§aHéllo 🌍\nline2").unwrap();
        let text = doc.to_text();
        assert!(text.is_ascii());
        let reparsed = PropertiesDocument::parse(&text);
        assert_eq!(reparsed.get("motd"), Some("§aHéllo 🌍\nline2"));
    }

    #[test]
    fn java_syntax_variants() {
        let doc =
            PropertiesDocument::parse("a : 1\nb   2\nc=multi \\\n    line\nkey\\ with\\ space=x\n");
        assert_eq!(doc.get("a"), Some("1"));
        assert_eq!(doc.get("b"), Some("2"));
        assert_eq!(doc.get("c"), Some("multi line"));
        assert_eq!(doc.get("key with space"), Some("x"));
    }

    #[test]
    fn duplicates_last_wins_and_set_updates_last() {
        let mut doc = PropertiesDocument::parse("a=1\na=2\n");
        assert_eq!(doc.get("a"), Some("2"));
        doc.set("a", "3").unwrap();
        assert_eq!(doc.to_text(), "a=1\na=3\n");
    }

    #[test]
    fn latin1_fallback() {
        assert_eq!(decode_bytes(b"motd=Caf\xE9"), "motd=Café");
    }

    #[test]
    fn rejects_bad_keys() {
        let mut doc = PropertiesDocument::default();
        assert!(doc.set("bad key", "x").is_err());
        assert!(doc.set("", "x").is_err());
    }

    #[test]
    fn appending_after_a_dangling_continuation_keeps_the_new_key() {
        let mut doc = PropertiesDocument::parse("motd=Hello \\\n");
        doc.set("pvp", "false").unwrap();
        let again = PropertiesDocument::parse(&doc.to_text());
        assert_eq!(again.get("pvp"), Some("false"));
        assert_eq!(again.get("motd").map(str::trim), Some("Hello"));
    }
}
