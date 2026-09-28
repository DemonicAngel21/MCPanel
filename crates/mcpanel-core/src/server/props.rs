//! `server.properties` editing service (version-aware, lossless).

use super::manager::ServerManager;
use super::runtime::Operation;
use crate::config::PropertiesDocument;
use crate::config::properties::{decode_bytes, validate_key};
use crate::config::schema::{self, PropertyView};
use crate::error::{CoreError, CoreResult};
use crate::events::DomainEvent;
use crate::files::{SafeRoot, fsx};
use crate::ids::ServerId;
use crate::model::AuditResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerProperties {
    pub file_exists: bool,
    pub game_version: String,
    pub properties: Vec<PropertyView>,
    /// Changes apply on the next start.
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyChange {
    pub key: String,
    /// `None` removes the key.
    pub value: Option<String>,
}

impl ServerManager {
    fn read_properties_doc(root: &SafeRoot) -> CoreResult<Option<PropertiesDocument>> {
        let p = root.resolve("server.properties")?;
        p.ensure_no_reparse_points()?;
        match std::fs::read(p.absolute()) {
            Ok(b) => Ok(Some(PropertiesDocument::parse(&decode_bytes(&b)))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CoreError::io("Cannot read server.properties", &e)),
        }
    }

    pub async fn properties(&self, id: ServerId) -> CoreResult<ServerProperties> {
        let server = self.get(id).await?;
        let root = SafeRoot::open(&server.directory)?;
        let doc = tokio::task::spawn_blocking(move || Self::read_properties_doc(&root))
            .await
            .map_err(|e| CoreError::internal(e.to_string()))??;
        let exists = doc.is_some();
        let doc = doc.unwrap_or_default();
        let views = schema::describe(&doc, &server.software.game_version, &self.versions).await;
        Ok(ServerProperties {
            file_exists: exists,
            game_version: server.software.game_version,
            properties: views,
            restart_required: self.runtime(id).state().has_process(),
        })
    }

    pub async fn update_properties(
        &self,
        id: ServerId,
        changes: Vec<PropertyChange>,
        actor: &str,
    ) -> CoreResult<ServerProperties> {
        if changes.is_empty() {
            return self.properties(id).await;
        }
        let server = self.get(id).await?;
        let rt = self.runtime(id);
        let _guard = rt.begin(Operation::EditingConfig)?;
        for c in &changes {
            validate_key(&c.key)?;
            if let Some(v) = &c.value {
                let s = schema::schema_for(&c.key, &server.software.game_version, &self.versions)
                    .await
                    .map(|(s, _)| s);
                schema::validate_value(s.as_ref(), &c.key, v)?;
            }
        }
        let root = SafeRoot::open(&server.directory)?;
        let keys: Vec<String> = changes.iter().map(|c| c.key.clone()).collect();
        tokio::task::spawn_blocking(move || -> CoreResult<()> {
            let mut doc = Self::read_properties_doc(&root)?.unwrap_or_default();
            for c in &changes {
                match &c.value {
                    Some(v) => doc.set(&c.key, v)?,
                    None => doc.remove(&c.key),
                }
            }
            let p = root.resolve("server.properties")?;
            p.ensure_no_reparse_points()?;
            fsx::atomic_write(&p.absolute(), doc.to_text().as_bytes())
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))??;
        drop(_guard);
        self.events
            .publish(DomainEvent::ServerUpdated { server_id: id });
        // Keys only: values may be secrets (rcon.password).
        self.audit
            .record(
                actor,
                "server.properties.update",
                Some(id),
                None,
                AuditResult::Success,
                serde_json::json!({ "keys": keys }),
            )
            .await;
        self.properties(id).await
    }
}
