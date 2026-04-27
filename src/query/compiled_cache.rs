//! Compiled-query cache.
//!
//! Entity-query compilation expands terms through the JSON-LD engine and
//! parses `q`, `type`, `geo`, and projection clauses into a
//! [`CompiledEntityQuery`]. For repeated queries (dashboards, polling
//! clients) these results can be cached behind a capacity-limited,
//! time-limited [`moka::future::Cache`] keyed on the context fingerprint and
//! every parameter that can influence the compiled output.
//!
//! Each cache instance is owned by an
//! [`EntityQueryService`](crate::service::entity::query::EntityQueryService),
//! which is already scoped to a single tenant, so tenant identity is implicit
//! in the cache owner and does not need to be part of the key.

use crate::{http::parameter::query::entity::EntityQueryParameters, jsonld::ContextSpec, query::compiled::CompiledEntityQuery};
use moka::future::Cache;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
    time::Duration,
};

/// Default capacity for the compiled-query cache.
const DEFAULT_CAPACITY: u64 = 1024;

/// Default idle time-to-live for cache entries.
const DEFAULT_TTL: Duration = Duration::from_secs(300);

/// Key identifying a unique compilation output.
///
/// Two compilations with identical keys produce identical [`CompiledEntityQuery`]
/// values (absent nondeterministic inputs). The `cap_limit` flag is part of the
/// key because `compile_query_uncapped` and `compile_query_public` yield
/// different paginations for the same parameters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompiledQueryKey {
    context_digest: u64,
    cap_limit: bool,
    id: Option<String>,
    entity_type: Option<String>,
    id_pattern: Option<String>,
    attrs: Option<String>,
    q: Option<String>,
    georel: Option<String>,
    geometry: Option<String>,
    coordinates: Option<String>,
    geoproperty: Option<String>,
    scope_q: Option<String>,
    order_by: Option<String>,
    order_from: Option<String>,
    order_geometry: Option<String>,
    collation: Option<String>,
    pick: Option<String>,
    omit: Option<String>,
    expand_values: Option<String>,
    json_keys: Option<String>,
    dataset_id: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    count: Option<bool>,
    options: Option<String>,
}

impl CompiledQueryKey {
    /// Construct a cache key from the inputs to
    /// [`EntityQueryService::compile_query`](crate::service::entity::query::EntityQueryService::compile_query).
    pub fn from_params(params: &EntityQueryParameters, context: &ContextSpec, cap_limit: bool) -> CompiledQueryKey {
        CompiledQueryKey {
            context_digest: context_digest(context),
            cap_limit,
            id: params.id.clone(),
            entity_type: params.entity_type.clone(),
            id_pattern: params.id_pattern.clone(),
            attrs: params.attrs.clone(),
            q: params.q.clone(),
            georel: params.georel.clone(),
            geometry: params.geometry.clone(),
            coordinates: params.coordinates.clone(),
            geoproperty: params.geoproperty.clone(),
            scope_q: params.scope_q.clone(),
            order_by: params.order_by.clone(),
            order_from: params.order_from.clone(),
            order_geometry: params.order_geometry.clone(),
            collation: params.collation.clone(),
            pick: params.pick.clone(),
            omit: params.omit.clone(),
            expand_values: params.expand_values.clone(),
            json_keys: params.json_keys.clone(),
            dataset_id: params.dataset_id.clone(),
            limit: params.limit,
            offset: params.offset,
            count: params.count,
            options: params.options.clone(),
        }
    }
}

/// Hash the JSON-LD context configuration into a 64-bit fingerprint.
///
/// Covers every dimension that affects term expansion: the list of link
/// contexts (order preserved) and the inline body context if any.
fn context_digest(context: &ContextSpec) -> u64 {
    let mut hasher = DefaultHasher::new();
    for url in context.link_contexts() {
        url.hash(&mut hasher);
    }
    if let Some(body) = context.body_context() {
        body.to_string().hash(&mut hasher);
    }
    hasher.finish()
}

/// Thread-safe cache of compiled entity queries.
///
/// Cloning is cheap: the underlying moka cache is `Arc`-shared internally.
#[derive(Debug, Clone)]
pub struct CompiledQueryCache {
    inner: Cache<CompiledQueryKey, Arc<CompiledEntityQuery>>,
}

impl CompiledQueryCache {
    /// Create a cache with the default capacity and TTL.
    pub fn new() -> CompiledQueryCache {
        CompiledQueryCache::with_capacity_and_ttl(DEFAULT_CAPACITY, DEFAULT_TTL)
    }

    /// Create a cache with explicit capacity and idle time-to-live.
    pub fn with_capacity_and_ttl(capacity: u64, ttl: Duration) -> CompiledQueryCache {
        let inner = Cache::builder().max_capacity(capacity).time_to_idle(ttl).build();
        CompiledQueryCache { inner }
    }

    /// Return the cached compiled query for `key`, or compute it through
    /// `init` and insert it on cache miss.
    pub async fn get_or_compile<F, Fut, E>(&self, key: CompiledQueryKey, init: F) -> Result<Arc<CompiledEntityQuery>, E>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<CompiledEntityQuery, E>> + Send,
    {
        if let Some(cached) = self.inner.get(&key).await {
            return Ok(cached);
        }
        let compiled = init().await?;
        let arc = Arc::new(compiled);
        self.inner.insert(key, Arc::clone(&arc)).await;
        Ok(arc)
    }
}

impl Default for CompiledQueryCache {
    fn default() -> CompiledQueryCache {
        CompiledQueryCache::new()
    }
}
