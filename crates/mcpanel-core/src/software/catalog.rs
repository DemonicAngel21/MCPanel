//! Cached Minecraft version catalog used for ordering and version-aware features.
//! Versions are opaque ids ordered by release time — never parsed as semver.

use super::{GameVersion, SoftwareCatalog};
use crate::error::CoreResult;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

struct Cached {
    fetched: Instant,
    versions: Arc<Vec<GameVersion>>,
    index: Arc<HashMap<String, usize>>,
}

pub struct VersionCatalog {
    source: Option<Arc<dyn SoftwareCatalog>>,
    ttl: Duration,
    cache: Mutex<Option<Cached>>,
}

impl VersionCatalog {
    pub fn new(source: Option<Arc<dyn SoftwareCatalog>>) -> Self {
        Self {
            source,
            ttl: Duration::from_secs(3600),
            cache: Mutex::new(None),
        }
    }

    /// All versions, newest first. Uses the cache; on fetch failure returns the stale
    /// cache if any.
    pub async fn versions(&self) -> CoreResult<Arc<Vec<GameVersion>>> {
        Ok(self.load().await?.0)
    }

    async fn load(&self) -> CoreResult<(Arc<Vec<GameVersion>>, Arc<HashMap<String, usize>>)> {
        let mut guard = self.cache.lock().await;
        if let Some(c) = guard.as_ref()
            && c.fetched.elapsed() < self.ttl
        {
            return Ok((c.versions.clone(), c.index.clone()));
        }
        let Some(source) = &self.source else {
            return Ok((Arc::new(Vec::new()), Arc::new(HashMap::new())));
        };
        match source.game_versions().await {
            Ok(mut versions) => {
                versions.sort_by_key(|v| std::cmp::Reverse(v.release_time));
                let index: HashMap<String, usize> = versions
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (v.id.clone(), i))
                    .collect();
                let c = Cached {
                    fetched: Instant::now(),
                    versions: Arc::new(versions),
                    index: Arc::new(index),
                };
                let out = (c.versions.clone(), c.index.clone());
                *guard = Some(c);
                Ok(out)
            }
            Err(e) => match guard.as_ref() {
                Some(c) => {
                    tracing::warn!(target: "mcpanel::catalog", "version refresh failed, using cache: {}", e.message);
                    Ok((c.versions.clone(), c.index.clone()))
                }
                None => Err(e),
            },
        }
    }

    /// Compare two version ids by release order. `None` if either is unknown.
    pub async fn compare(&self, a: &str, b: &str) -> Option<Ordering> {
        let (_, index) = self.load().await.ok()?;
        let ia = *index.get(a)?;
        let ib = *index.get(b)?;
        // Lower index = newer.
        Some(ib.cmp(&ia))
    }

    /// `Some(true)` if `version >= since`, `None` if ordering is unknown.
    pub async fn is_at_least(&self, version: &str, since: &str) -> Option<bool> {
        self.compare(version, since)
            .await
            .map(|o| o != Ordering::Less)
    }

    pub async fn get(&self, id: &str) -> Option<GameVersion> {
        let (versions, index) = self.load().await.ok()?;
        index.get(id).map(|i| versions[*i].clone())
    }
}
