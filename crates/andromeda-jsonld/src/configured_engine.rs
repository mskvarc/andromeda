//! The engine a `[jsonld]` table describes.

use crate::{
    context_cache::ContextCache,
    engine::{builder::JsonLdEngineBuilder, processor::JsonLdEngine},
    error::JsonLdError,
    hosted_context_recognizer::HostedContextRecognizer,
    loader::cache_lifetime::CacheLifetime,
};
use andromeda_configuration::schema::jsonld::JsonLdConfig;
use andromeda_outbound_http::address_policy::AddressPolicy;
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
        .allow_remote(config.fetch.is_enabled())
        .timeout(config.fetch.timeout.duration())
        .max_redirects(config.fetch.max_redirects)
        .addresses(AddressPolicy::new(
            config.fetch.private_addresses,
            config.fetch.allowed_addresses.clone(), // clone: the configuration keeps its lists
            config.fetch.denied_addresses.clone(),  // clone: the configuration keeps its lists
        ))
        .cache_capacity(config.cache.max_entries.get())
        .cache_ttl(CacheLifetime::from(config.cache.default_lifetime))
        .hosted_context_revalidation(CacheLifetime::from(config.cache.hosted_revalidation_interval))
        .max_context_bytes(config.fetch.max_size.as_u64())
        .with_l2_cache(l2_cache)
        .hosted_contexts(hosted);
    if !config.preloaded_contexts.is_empty() {
        // clone: the configuration is borrowed and stays valid; the engine owns its preload table.
        let preloaded = config
            .preloaded_contexts
            .iter()
            .map(|context| (context.url.clone(), context.path.clone()))
            .collect();
        builder = builder.preload_contexts(preloaded);
    }
    builder.build()
}
