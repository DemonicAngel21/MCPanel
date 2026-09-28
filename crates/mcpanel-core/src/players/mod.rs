//! Player management: online players, operators, allowlist and bans.
//!
//! The server's own files (`ops.json`, `whitelist.json`, `banned-players.json`,
//! `banned-ips.json`, `usercache.json`) are the source of truth (ADR-0005). While a
//! server runs it owns them, so changes go through console commands; while it is
//! stopped MCPanel edits the files directly.

pub mod lists;
mod service;

pub use service::{
    ActionOutcome, AppliedVia, KnownPlayer, PlayerAction, PlayerService, ServerPlayers,
    validate_player_name,
};

use md5::Digest;
use uuid::Uuid;

/// The UUID an offline-mode server assigns to `name` (Java's
/// `UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`, a v3 UUID without namespace).
pub fn offline_uuid(name: &str) -> Uuid {
    let digest: [u8; 16] = md5::Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    uuid::Builder::from_md5_bytes(digest).into_uuid()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_java() {
        // UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(UTF_8))
        assert_eq!(
            offline_uuid("Notch").to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
    }
}
