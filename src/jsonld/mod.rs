//! JSON-LD support — thin re-export layer over [`json_ld_engine`].

pub mod term_resolver;

// Re-export everything the application uses from the engine library.
pub use json_ld_engine::{
    CompactOptions,
    ContextSpec,
    CoreContextPolicy,
    Error,
    JsonLdEngine,
    core_context::{NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V19, is_ngsi_ld_core_context_url},
};

// The trait lives in json_ld_engine; re-export it so derive macros resolve.
pub use json_ld_engine::JsonLdExpandable;

// Re-export the derive macro for convenience.
pub use json_ld_expandable::JsonLdExpandable as JsonLdExpandableMacro;

/// Build a [`JsonLdEngine`] from application config and an L2 context cache.
pub fn build_engine(
    config: &crate::core::config::JsonLdConfig,
    l2_cache: std::sync::Arc<impl json_ld_engine::ContextCache + 'static>,
) -> Result<JsonLdEngine, Error> {
    let mut builder = JsonLdEngine::builder()
        .allow_remote(config.allow_remote_contexts)
        .timeout_seconds(config.timeout_seconds)
        .max_redirects(config.max_redirects)
        .cache_capacity(config.cache_capacity)
        .cache_ttl_seconds(config.cache_ttl_seconds)
        .max_context_bytes(config.max_context_bytes)
        .with_l2_cache(l2_cache);
    if let Some(preloads) = config.preload_contexts.clone() {
        builder = builder.preload_contexts(preloads);
    }
    builder.build()
}
