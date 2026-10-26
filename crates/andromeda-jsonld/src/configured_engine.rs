//! The engine a `[jsonld]` section describes.

use crate::{
    context_cache::ContextCache,
    engine::{builder::JsonLdEngineBuilder, processor::JsonLdEngine},
    error::JsonLdError,
    hosted_context_recognizer::HostedContextRecognizer,
    loader::cache_lifetime::CacheLifetime,
};
use andromeda_configuration::schema::jsonld::JsonLdConfig;
use std::sync::Arc;

/// Build the engine `config` describes for the tenant whose stored
/// `@context`s `hosted` recognises.
///
/// # Errors
///
/// Returns whatever [`JsonLdEngineBuilder::build`] returns: an embedded
/// context that will not parse, a preloaded file that cannot be read or
/// parsed, or an HTTP client that cannot be built.
pub fn build_engine(
    config: &JsonLdConfig,
    l2_cache: Arc<impl ContextCache + 'static>,
    hosted: Arc<dyn HostedContextRecognizer>,
) -> Result<JsonLdEngine, JsonLdError> {
    let mut builder = JsonLdEngineBuilder::new()
        .allow_remote(config.allow_remote_contexts)
        .timeout(config.timeout.duration())
        .max_redirects(config.max_redirects)
        .cache_capacity(config.cache_capacity)
        .cache_ttl(CacheLifetime::from(config.cache_ttl))
        .hosted_context_revalidation(CacheLifetime::from(config.hosted_context_revalidation))
        .max_context_bytes(config.max_context.as_u64())
        .with_l2_cache(l2_cache)
        .hosted_contexts(hosted);
    if !config.preload_contexts.is_empty() {
        builder = builder.preload_contexts(config.preload_contexts.clone()); // clone: the configuration is borrowed and stays valid
    }
    builder.build()
}
