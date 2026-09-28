//! Minecraft account name → UUID (Mojang profile service).
//!
//! Verified 2026-09-28: `GET https://api.minecraftservices.com/minecraft/profile/lookup/name/{name}`
//! returns `{"id": "<32 hex>", "name": "<exact name>"}`, or 404 for an unknown name;
//! `https://api.mojang.com/users/profiles/minecraft/{name}` behaves the same and is the
//! fallback. The name is validated before it is put into the URL.

use crate::http::HttpClient;
use async_trait::async_trait;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::ProfileLookup;
use serde::Deserialize;
use uuid::Uuid;

const ENDPOINTS: [&str; 2] = [
    "https://api.minecraftservices.com/minecraft/profile/lookup/name/",
    "https://api.mojang.com/users/profiles/minecraft/",
];

#[derive(Deserialize)]
struct Profile {
    id: String,
    name: String,
}

pub struct MojangProfiles {
    http: HttpClient,
}

impl MojangProfiles {
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }
}

#[async_trait]
impl ProfileLookup for MojangProfiles {
    async fn uuid_for_name(&self, name: &str) -> CoreResult<Option<(Uuid, String)>> {
        if name.is_empty()
            || name.len() > 16
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(CoreError::invalid("Not a valid Java Edition player name"));
        }
        let mut last = None;
        for base in ENDPOINTS {
            match self
                .http
                .get_json::<Profile>(&format!("{base}{name}"))
                .await
            {
                Ok(p) => {
                    let uuid = Uuid::parse_str(&p.id).map_err(|_| {
                        CoreError::new(
                            ErrorCode::ProviderError,
                            "The profile service returned an invalid UUID",
                        )
                    })?;
                    return Ok(Some((uuid, p.name)));
                }
                Err(e) if e.code == ErrorCode::VersionNotFound => return Ok(None),
                Err(e) => last = Some(e),
            }
        }
        let mut e = last.unwrap_or_else(|| CoreError::internal("profile lookup failed"));
        e.message = format!(
            "Cannot look up the player at Mojang (needed to edit a stopped online-mode server): {}",
            e.message
        );
        Err(e)
    }
}
