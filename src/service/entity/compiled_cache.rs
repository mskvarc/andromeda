//! Compiled-query cache.
//!
//! Entity-query compilation expands terms through the JSON-LD engine and
//! parses `q`, `type`, `geo`, and projection clauses into a compiled query.
//! For repeated queries (dashboards, polling clients) these results can be
//! cached behind a capacity-limited, time-limited cache keyed on the context
//! fingerprint and every parameter that can influence the compiled output.
//!
//! Each cache instance is owned by an entity query service, which is already
//! scoped to a single tenant, so tenant identity is implicit in the cache owner
//! and does not need to be part of the key.

use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    entity::compiled::CompiledEntityQuery,
    geo::input::GeoQueryInput,
    ordering::{
        collation::IcuCollation,
        geometry::OrderingGeometry,
        term::{OrderDirection, OrderTerm},
    },
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use moka::future::Cache;
use std::{
    fmt::Write,
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
    time::Duration,
};

/// Default capacity for the compiled-query cache.
const DEFAULT_CAPACITY: u64 = 1024;

/// Default idle time-to-live for cache entries.
const DEFAULT_TTL: Duration = Duration::from_secs(300);

/// Whether a compilation applies the broker's `limit` cap.
///
/// Part of the cache key: the capped and uncapped compilations of the same
/// parameters differ in their pagination, so they are two entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitCap {
    /// Apply the broker's maximum page size.
    Capped,
    /// Honour the requested limit unchanged, for internal callers that manage
    /// their own result-size constraints.
    Uncapped,
}

/// Key identifying a unique compilation output.
///
/// Two compilations with identical keys produce identical [`CompiledEntityQuery`]
/// values (absent nondeterministic inputs).
///
/// The parameters are held as one canonical rendering rather than as a field
/// per query parameter: the key is built on every lookup, and twenty-four
/// `Option<String>` clones per lookup is twenty-four allocations to answer a
/// cache hit. The rendering is length-prefixed, so no parameter value can
/// impersonate the boundary between two others.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompiledQueryKey {
    /// Fingerprint of the `@context` every term is expanded against.
    context_digest: u64,
    /// Which pagination cap the compilation applies.
    limit_cap: LimitCap,
    /// Canonical rendering of every parameter that influences compilation.
    parameters: Box<str>,
}

impl CompiledQueryKey {
    /// Construct a cache key from the inputs to
    /// [`EntityQueryService::compile_query`](crate::service::entity::query::EntityQueryService::compile_query).
    pub fn from_params(params: &EntityQueryParameters, context: &ContextSpec, limit_cap: LimitCap) -> CompiledQueryKey {
        let mut parameters = String::new();
        let id_pattern = params.id_pattern.as_ref().map(ToString::to_string);
        let q = params.q.as_ref().map(ToString::to_string);
        let entity_type = params.entity_type.as_ref().map(ToString::to_string);
        let option_format = params.options.and_then(|options| options.format).map(|format| format.to_string());
        let option_sys_attrs = params.options.map(|options| match options.sys_attrs {
            SysAttrsInclusion::Include => "include",
            SysAttrsInclusion::Strip => "strip",
        });
        push_uri_list(&mut parameters, params.id.as_deref());
        push_attribute_list(&mut parameters, params.attrs.as_deref());
        push_attribute_list(&mut parameters, params.pick.as_deref());
        push_attribute_list(&mut parameters, params.omit.as_deref());
        push_attribute_list(&mut parameters, params.expand_values.as_deref());
        push_attribute_list(&mut parameters, params.json_keys.as_deref());
        push_dataset_list(&mut parameters, params.dataset_id.as_deref());
        push_geo_query(&mut parameters, params.geo.0.as_ref());
        for field in [
            entity_type.as_deref(),
            id_pattern.as_deref(),
            q.as_deref(),
            params.scope_q.as_ref().map(ToString::to_string).as_deref(),
            params.ordering.collation.as_ref().map(IcuCollation::as_str),
            option_format.as_deref(),
            option_sys_attrs,
        ] {
            push_field(&mut parameters, field);
        }
        push_ordering_geometry(&mut parameters, params.ordering.geometry.as_ref());
        push_order_terms(&mut parameters, &params.ordering.order_by);
        for field in [params.limit, params.offset] {
            push_field(&mut parameters, field.map(|value| value.to_string()).as_deref());
        }
        push_field(&mut parameters, params.count.map(|value| value.to_string()).as_deref());

        // The structured POST selectors (clause 5.2.11) are part of the key: two
        // bodies that flatten to the same `id`/`type` summary but group their
        // selectors differently compile to different queries. The count is
        // length-prefixed like every field, so a selector value cannot
        // impersonate the boundary between two selectors.
        push_field(&mut parameters, Some(&params.entity_selectors.len().to_string()));
        for selector in &params.entity_selectors {
            let id_pattern = selector.id_pattern.as_ref().map(ToString::to_string);
            push_uri_list(&mut parameters, selector.id.as_deref());
            push_field(&mut parameters, id_pattern.as_deref());
            push_field(&mut parameters, selector.type_.as_ref().map(ToString::to_string).as_deref());
        }

        CompiledQueryKey {
            context_digest: context_digest(context),
            limit_cap,
            parameters: parameters.into_boxed_str(),
        }
    }
}

/// Append an ordered attribute-name list in its canonical rendering.
fn push_attribute_list(buffer: &mut String, value: Option<&[AttributeName]>) {
    match value {
        Some(values) => {
            let rendered = values.iter().map(AttributeName::as_str).collect::<Vec<_>>().join(",");
            push_field(buffer, Some(&rendered));
        }
        None => push_field(buffer, None),
    }
}

/// Append a typed entity identifier list in its canonical URI rendering.
fn push_uri_list(buffer: &mut String, value: Option<&[Uri]>) {
    match value {
        Some(values) => {
            let rendered = values.iter().map(Uri::as_str).collect::<Vec<_>>().join(",");
            push_field(buffer, Some(&rendered));
        }
        None => push_field(buffer, None),
    }
}

/// Append an ordered dataset-selector list in canonical rendering.
fn push_dataset_list(buffer: &mut String, value: Option<&[DatasetSelector]>) {
    match value {
        Some(values) => {
            let rendered = values.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
            push_field(buffer, Some(&rendered));
        }
        None => push_field(buffer, None),
    }
}

/// Append a parsed geo-query without retaining URL component strings.
fn push_geo_query(buffer: &mut String, value: Option<&GeoQueryInput>) {
    match value {
        Some(value) => {
            push_field(buffer, Some(&value.georel.to_string()));
            let geometry = value.geometry.to_geojson().to_string();
            push_field(buffer, Some(&geometry));
            push_field(buffer, value.geoproperty.as_ref().map(AsRef::<str>::as_ref));
        }
        None => {
            push_field(buffer, None);
            push_field(buffer, None);
            push_field(buffer, None);
        }
    }
}

/// Append parsed ordering terms without reconstructing a request query string.
fn push_order_terms(buffer: &mut String, terms: &[OrderTerm]) {
    push_field(buffer, Some(&terms.len().to_string()));
    for term in terms {
        push_field(buffer, Some(term.attr.as_str()));
        let direction = match term.direction {
            OrderDirection::Asc => "asc",
            OrderDirection::Desc => "desc",
            OrderDirection::DistAsc => "dist-asc",
            OrderDirection::DistDesc => "dist-desc",
        };
        push_field(buffer, Some(direction));
    }
}

/// Append the typed ordering reference geometry.
fn push_ordering_geometry(buffer: &mut String, geometry: Option<&OrderingGeometry>) {
    let rendered = geometry.map(|value| value.geometry.to_geojson().to_string());
    push_field(buffer, rendered.as_deref());
}

/// Append one parameter to the canonical rendering.
///
/// Length-prefixed so a value containing the separator cannot be read as two
/// parameters; an absent parameter is distinct from a present empty one.
fn push_field(buffer: &mut String, value: Option<&str>) {
    match value {
        Some(value) => {
            let _ = write!(buffer, "{}:{};", value.len(), value);
        }
        None => buffer.push_str("-;"),
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
    context.core_policy().hash(&mut hasher);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn params(query: &str) -> EntityQueryParameters {
        serde_urlencoded::from_str(query).expect("valid query string")
    }

    fn key(query: &str, limit_cap: LimitCap) -> CompiledQueryKey {
        CompiledQueryKey::from_params(&params(query), &ContextSpec::default(), limit_cap)
    }

    #[test]
    fn identical_parameters_produce_one_key() {
        assert_eq!(key("q=speed>50", LimitCap::Capped), key("q=speed>50", LimitCap::Capped));
    }

    #[test]
    fn the_pagination_cap_is_part_of_the_key() {
        assert_ne!(key("q=speed>50", LimitCap::Capped), key("q=speed>50", LimitCap::Uncapped));
    }

    /// The rendering is length-prefixed precisely so adjacent parameter values
    /// cannot impersonate a different parameter boundary.
    #[test]
    fn a_value_containing_the_separator_cannot_impersonate_another_parameter() {
        assert_ne!(key("attrs=ab&q=c", LimitCap::Capped), key("attrs=a&q=bc", LimitCap::Capped));
    }

    #[test]
    fn an_empty_filter_is_rejected_before_cache_key_construction() {
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("q=").is_err());
    }

    #[test]
    fn different_parameters_produce_different_keys() {
        assert_ne!(key("q=speed>50", LimitCap::Capped), key("q=speed>10", LimitCap::Capped));
        assert_ne!(key("limit=10", LimitCap::Capped), key("limit=20", LimitCap::Capped));
    }

    #[test]
    fn typed_identifier_and_dataset_values_are_part_of_the_key() {
        assert_ne!(
            key("id=urn%3Angsi-ld%3AEntity%3Aa", LimitCap::Capped),
            key("id=urn%3Angsi-ld%3AEntity%3Ab", LimitCap::Capped)
        );
        assert_ne!(
            key("type=Vehicle&datasetId=urn%3Angsi-ld%3ADataset%3Aa", LimitCap::Capped),
            key("type=Vehicle&datasetId=urn%3Angsi-ld%3ADataset%3Ab", LimitCap::Capped)
        );
    }
}
