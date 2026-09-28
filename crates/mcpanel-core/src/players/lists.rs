//! The server's player list files (`ops.json`, `whitelist.json`, `banned-players.json`,
//! `banned-ips.json`, `usercache.json`; JSON since Minecraft 1.7.6).
//!
//! Entries are kept as JSON objects so fields MCPanel does not know survive an edit.
//! Output uses the vanilla layout (two-space indentation).

use crate::error::{CoreError, CoreResult, ErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

pub const OPS: &str = "ops.json";
pub const WHITELIST: &str = "whitelist.json";
pub const BANNED_PLAYERS: &str = "banned-players.json";
pub const BANNED_IPS: &str = "banned-ips.json";
pub const USER_CACHE: &str = "usercache.json";
/// Pre-1.7.6 plain-text lists: detected, never edited.
pub const LEGACY_FILES: &[&str] = &[
    "ops.txt",
    "white-list.txt",
    "banned-players.txt",
    "banned-ips.txt",
];

/// A list file's entries (objects only; anything else in the array is kept verbatim).
#[derive(Debug, Clone, Default)]
pub struct ListFile {
    pub entries: Vec<Value>,
}

impl ListFile {
    /// Parse a list file. A missing or empty file is an empty list; invalid JSON is an
    /// error (MCPanel never overwrites a file it cannot read).
    pub fn parse(text: Option<&str>, file: &str) -> CoreResult<Self> {
        let text = text
            .map(|t| t.trim_start_matches('\u{feff}').trim())
            .unwrap_or("");
        if text.is_empty() {
            return Ok(Self::default());
        }
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(entries)) => Ok(Self { entries }),
            Ok(_) | Err(_) => Err(CoreError::new(
                ErrorCode::InvalidInput,
                format!(
                    "{file} is not a valid player list; fix or remove it before editing players"
                ),
            )),
        }
    }

    pub fn to_text(&self) -> String {
        let mut s = serde_json::to_string_pretty(&Value::Array(self.entries.clone()))
            .unwrap_or_else(|_| "[]".into());
        s.push('\n');
        s
    }

    fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
        v.get(key).and_then(Value::as_str)
    }

    fn matches(v: &Value, key: &str, wanted: &str) -> bool {
        Self::str_field(v, key).is_some_and(|s| s.eq_ignore_ascii_case(wanted))
    }

    /// Index of the entry for a player (by UUID, else by name, case-insensitive).
    pub fn find_player(&self, name: &str, uuid: Option<Uuid>) -> Option<usize> {
        self.entries.iter().position(|e| {
            uuid.is_some_and(|u| Self::matches(e, "uuid", &u.to_string()))
                || Self::matches(e, "name", name)
        })
    }

    pub fn find_ip(&self, ip: &str) -> Option<usize> {
        self.entries.iter().position(|e| Self::matches(e, "ip", ip))
    }

    /// Insert or replace (keeping unknown fields of an existing entry).
    pub fn upsert(&mut self, index: Option<usize>, fields: Map<String, Value>) -> bool {
        match index {
            Some(i) => {
                let before = self.entries[i].clone();
                if let Value::Object(o) = &mut self.entries[i] {
                    o.extend(fields);
                } else {
                    self.entries[i] = Value::Object(fields);
                }
                before != self.entries[i]
            }
            None => {
                self.entries.push(Value::Object(fields));
                true
            }
        }
    }

    pub fn remove(&mut self, index: Option<usize>) -> bool {
        match index {
            Some(i) => {
                self.entries.remove(i);
                true
            }
            None => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedPlayer {
    pub name: String,
    pub uuid: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operator {
    pub name: String,
    pub uuid: Option<Uuid>,
    pub level: u8,
    pub bypasses_player_limit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ban {
    /// Player name, or the IP address for IP bans.
    pub target: String,
    pub uuid: Option<Uuid>,
    pub reason: Option<String>,
    pub source: Option<String>,
    pub created: Option<String>,
    /// `None` = permanent ("forever").
    pub expires: Option<String>,
}

fn uuid_of(v: &Value) -> Option<Uuid> {
    ListFile::str_field(v, "uuid").and_then(|s| Uuid::parse_str(s).ok())
}

fn string_of(v: &Value, key: &str) -> Option<String> {
    ListFile::str_field(v, key)
        .map(String::from)
        .filter(|s| !s.is_empty())
}

impl ListFile {
    pub fn players(&self) -> Vec<ListedPlayer> {
        self.entries
            .iter()
            .filter_map(|e| {
                Some(ListedPlayer {
                    name: string_of(e, "name")?,
                    uuid: uuid_of(e),
                })
            })
            .collect()
    }

    pub fn operators(&self) -> Vec<Operator> {
        self.entries
            .iter()
            .filter_map(|e| {
                Some(Operator {
                    name: string_of(e, "name")?,
                    uuid: uuid_of(e),
                    level: e.get("level").and_then(Value::as_u64).unwrap_or(4).min(4) as u8,
                    bypasses_player_limit: e
                        .get("bypassesPlayerLimit")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect()
    }

    pub fn bans(&self, target_key: &str) -> Vec<Ban> {
        self.entries
            .iter()
            .filter_map(|e| {
                Some(Ban {
                    target: string_of(e, target_key)?,
                    uuid: uuid_of(e),
                    reason: string_of(e, "reason"),
                    source: string_of(e, "source"),
                    created: string_of(e, "created"),
                    expires: string_of(e, "expires").filter(|x| !x.eq_ignore_ascii_case("forever")),
                })
            })
            .collect()
    }

    /// `usercache.json`: name → UUID.
    pub fn cached_uuid(&self, name: &str) -> Option<Uuid> {
        self.entries
            .iter()
            .find(|e| Self::matches(e, "name", name))
            .and_then(uuid_of)
    }
}

/// Timestamp in the format vanilla writes (`2026-09-28 19:33:37 +0530`).
pub fn vanilla_timestamp(at: jiff::Zoned) -> String {
    at.strftime("%Y-%m-%d %H:%M:%S %z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITELIST_263: &str = r#"[
  {
    "uuid": "46b19af6-0da5-4034-8566-c1bd5b29625b",
    "name": "Nobody_Here"
  }
]"#;

    #[test]
    fn parses_vanilla_files_and_round_trips() {
        let w = ListFile::parse(Some(WHITELIST_263), WHITELIST).unwrap();
        assert_eq!(w.players()[0].name, "Nobody_Here");
        assert_eq!(w.to_text().trim(), WHITELIST_263);
        assert!(ListFile::parse(None, OPS).unwrap().entries.is_empty());
        assert!(
            ListFile::parse(Some("\u{feff}  "), OPS)
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(ListFile::parse(Some("{oops"), OPS).is_err());
        assert!(ListFile::parse(Some("{}"), OPS).is_err());
    }

    #[test]
    fn edits_keep_unknown_fields() {
        let mut ops = ListFile::parse(
            Some(r#"[{"uuid":"46b19af6-0da5-4034-8566-c1bd5b29625b","name":"A","level":2,"bypassesPlayerLimit":false,"custom":1}]"#),
            OPS,
        )
        .unwrap();
        let i = ops.find_player("a", None);
        assert_eq!(i, Some(0), "names match case-insensitively");
        let mut f = Map::new();
        f.insert("level".into(), 4.into());
        assert!(ops.upsert(i, f));
        assert_eq!(ops.entries[0]["custom"], 1);
        assert_eq!(ops.operators()[0].level, 4);
        assert!(ops.remove(ops.find_player("A", None)));
        assert!(!ops.remove(ops.find_player("A", None)));
    }

    #[test]
    fn bans_treat_forever_as_permanent() {
        let b = ListFile::parse(
            Some(r#"[{"uuid":"46b19af6-0da5-4034-8566-c1bd5b29625b","name":"X","created":"2026-09-28 19:33:33 +0530","source":"Server","expires":"forever","reason":"Griefing"}]"#),
            BANNED_PLAYERS,
        )
        .unwrap();
        let ban = &b.bans("name")[0];
        assert_eq!(ban.reason.as_deref(), Some("Griefing"));
        assert_eq!(ban.expires, None);
    }

    #[test]
    fn user_cache_lookup() {
        let c = ListFile::parse(
            Some(r#"[{"uuid":"320cbc6d-d142-3469-a44f-b1f125ec169b","name":"Leaver","expiresOn":"2026-10-28 19:33:37 +0530"}]"#),
            USER_CACHE,
        )
        .unwrap();
        assert_eq!(
            c.cached_uuid("leaver").unwrap().to_string(),
            "320cbc6d-d142-3469-a44f-b1f125ec169b"
        );
        // Observed on 26.3 (offline mode): the cached UUID is the offline UUID.
        assert_eq!(
            super::super::offline_uuid("Leaver").to_string(),
            "320cbc6d-d142-3469-a44f-b1f125ec169b"
        );
    }
}
