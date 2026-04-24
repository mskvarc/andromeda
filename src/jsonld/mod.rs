//! JSON-LD processing engine for NGSI-LD.
//!
//! Provides expand and compact operations, context management, multi-tier
//! caching (in-memory L1, optional database-backed L2, embedded registry),
//! and NGSI-LD core context handling for all released versions (v1.3–v1.9).

mod context;
pub mod core_context;
mod engine;
mod error;
mod expandable;
mod loader;
pub mod term_resolver;
pub mod vocab;

pub use context::{ContextSpec, CoreContextPolicy, default_core_context_url, set_default_core_context_url};
pub use engine::{BatchContextHandle, CompactOptions, ContextStripping, JsonLdEngine};
pub use error::Error;
pub use expandable::{JsonLdExpandable, JsonLdLanguageMap, JsonLdTypeValue};
pub use loader::ContextCache;

pub use json_ld_expandable::JsonLdExpandable as JsonLdExpandableMacro;

/// Build a [`JsonLdEngine`] from application config and an L2 context cache.
///
/// # Errors
///
/// Returns [`Error`] if the engine cannot be constructed (e.g. embedded
/// contexts fail to parse, or preloaded context files are missing).
pub fn build_engine(config: &crate::core::config::JsonLdConfig, l2_cache: std::sync::Arc<impl ContextCache + 'static>) -> Result<JsonLdEngine, Error> {
    let mut builder = JsonLdEngine::builder()
        .allow_remote(config.allow_remote_contexts)
        .timeout_seconds(config.timeout_seconds)
        .max_redirects(config.max_redirects)
        .cache_capacity(config.cache_capacity)
        .cache_ttl_seconds(config.cache_ttl_seconds)
        .max_context_bytes(config.max_context_bytes)
        .with_l2_cache(l2_cache);
    if !config.preload_contexts.is_empty() {
        builder = builder.preload_contexts(config.preload_contexts.clone());
    }
    builder.build()
}
