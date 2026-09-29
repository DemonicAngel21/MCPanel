//! # mcpanel-providers
//!
//! Adapters for external services, behind the provider traits defined in
//! `mcpanel-core`. Every external API used here is recorded in
//! `docs/architecture/verification-log.md`.

mod detect;
pub mod fabric;
pub mod forge;
pub mod hangar;
pub mod http;
pub mod modrinth;
pub mod mojang;
pub mod paper;
pub mod profiles;
pub mod purpur;

use mcpanel_core::error::CoreResult;
use mcpanel_core::software::ProviderRegistry;
use std::sync::Arc;

pub use http::{HttpClient, HttpDownloader};
pub use profiles::MojangProfiles;

/// Build the registry of built-in providers: server software (Vanilla, Paper, Purpur,
/// Fabric, Quilt, NeoForge, Forge) and content (Modrinth, Hangar).
pub fn builtin_registry(http: &HttpClient) -> ProviderRegistry {
    let mojang = mojang::MojangClient::new(http.clone());
    let mut r = ProviderRegistry::new();
    r.set_reference_catalog(Arc::new(mojang::MojangCatalog(Arc::clone(&mojang))));
    r.register_software(paper::PaperProvider::provider(http.clone()));
    r.register_software(purpur::PurpurProvider::provider(
        http.clone(),
        Arc::clone(&mojang),
    ));
    r.register_software(fabric::FabricProvider::provider(
        http.clone(),
        Arc::clone(&mojang),
    ));
    r.register_software(fabric::FabricProvider::quilt(
        http.clone(),
        Arc::clone(&mojang),
    ));
    r.register_software(forge::ForgeProvider::neoforge(
        http.clone(),
        Arc::clone(&mojang),
    ));
    r.register_software(forge::ForgeProvider::forge(
        http.clone(),
        Arc::clone(&mojang),
    ));
    r.register_software(mojang::VanillaProvider::provider(mojang));
    r.register_content(Arc::new(modrinth::Modrinth::new(http.clone())));
    r.register_content(Arc::new(hangar::Hangar::new(http.clone())));
    r
}

pub fn http_client() -> CoreResult<HttpClient> {
    HttpClient::new()
}

#[cfg(all(test, feature = "live-tests"))]
mod live_tests;
