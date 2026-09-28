//! # mcpanel-providers
//!
//! Adapters for external services, behind the provider traits defined in
//! `mcpanel-core`. Every external API used here is recorded in
//! `docs/architecture/verification-log.md`.

mod detect;
pub mod http;
pub mod mojang;
pub mod paper;
pub mod profiles;
pub mod purpur;

use mcpanel_core::error::CoreResult;
use mcpanel_core::software::ProviderRegistry;
use std::sync::Arc;

pub use http::{HttpClient, HttpDownloader};
pub use profiles::MojangProfiles;

/// Build the registry of built-in providers (MVP: Vanilla, Paper, Purpur).
pub fn builtin_registry(http: &HttpClient) -> ProviderRegistry {
    let mojang = mojang::MojangClient::new(http.clone());
    let mut r = ProviderRegistry::new();
    r.set_reference_catalog(Arc::new(mojang::MojangCatalog(Arc::clone(&mojang))));
    r.register_software(paper::PaperProvider::provider(http.clone()));
    r.register_software(purpur::PurpurProvider::provider(
        http.clone(),
        Arc::clone(&mojang),
    ));
    r.register_software(mojang::VanillaProvider::provider(mojang));
    r
}

pub fn http_client() -> CoreResult<HttpClient> {
    HttpClient::new()
}

#[cfg(all(test, feature = "live-tests"))]
mod live_tests;
