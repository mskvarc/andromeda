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

use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    entity::compiled::CompiledEntityQuery,
    filter::{language_targeting::LanguageTargeting, serialize::serialize_attr_path},
    geo::{input::GeoQueryInput, reference_geometry::ReferenceGeometry},
    ordering::{
        collation::IcuCollation,
        geometry::OrderingGeometry,
        term::{OrderDirection, OrderTerm},
    },
    projection::grammar::RawProjectionTerm,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{query::entity::parameters::EntityQueryParameters, representation_parameter::WrittenValue};
use moka::future::Cache;
use std::{
    fmt::Write,
    future::Future,
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
    /// Canonical rendering of every parameter that influences compilation.
    parameters: Box<str>,
    /// The level the compiled `q`'s target languages were admitted and are
    /// matched under.
    language_targeting: LanguageTargeting,
}

impl CompiledQueryKey {
    /// Construct a cache key from the inputs to
    /// [`EntityQueryService::compile_query`](crate::query::EntityQueryService::compile_query).
    ///
    /// The page the query answers with is not part of the key: it never takes
    /// part in compilation, so every page of one query shares one entry.
    pub fn from_params(params: &EntityQueryParameters, context: &ContextSpec, language_targeting: LanguageTargeting) -> CompiledQueryKey {
        let mut parameters = String::new();
        let id_pattern = params.id_pattern.as_ref().map(ToString::to_string);
        let q = params.q.as_ref().map(ToString::to_string);
        let entity_type = params.entity_type.as_ref().map(ToString::to_string);
        let options = params.options.as_ref().and_then(WrittenValue::if_supported);
        let option_format = options.and_then(|options| options.format).map(|format| format.to_string());
        let option_sys_attrs = options.map(|options| match options.sys_attrs {
            SysAttrsInclusion::Include => "include",
            SysAttrsInclusion::Strip => "strip",
        });
        push_uri_list(&mut parameters, params.id.as_deref());
        push_attribute_list(&mut parameters, params.attrs.as_deref());
        push_projection_term_list(&mut parameters, params.pick.as_deref());
        push_projection_term_list(&mut parameters, params.omit.as_deref());
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

        // The structured POST selectors (clause 5.2.33) are part of the key: two
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
            parameters: parameters.into_boxed_str(),
            language_targeting,
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

/// Append an ordered `pick` / `omit` raw projection-term list in its
/// canonical clause 4.21 text rendering, including any linked-entity
/// sub-projection each term carries.
fn push_projection_term_list(buffer: &mut String, value: Option<&[RawProjectionTerm]>) {
    match value {
        Some(terms) => {
            let rendered = terms.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
            push_field(buffer, Some(&rendered));
        }
        None => push_field(buffer, None),
    }
}

/// Append a typed entity identifier list in its canonical URI rendering.
fn push_uri_list(buffer: &mut String, value: Option<&[Identifier]>) {
    match value {
        Some(values) => {
            let rendered = values.iter().map(Identifier::as_str).collect::<Vec<_>>().join(",");
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

/// Append a decoded geo-query without retaining URL component strings.
///
/// Coordinates still encoded are keyed by their text, so two spellings of one
/// geometry compile once each.
fn push_geo_query(buffer: &mut String, value: Option<&GeoQueryInput>) {
    if let Some(value) = value {
        push_field(buffer, Some(&value.georel.to_string()));
        let geometry = match &value.reference {
            ReferenceGeometry::Read(geometry) => geometry.to_geojson().to_string(),
            ReferenceGeometry::Encoded { geometry_type, coordinates } => format!("{geometry_type}{coordinates}"),
        };
        push_field(buffer, Some(&geometry));
        push_field(buffer, value.geoproperty.as_ref().map(AsRef::<str>::as_ref));
    } else {
        push_field(buffer, None);
        push_field(buffer, None);
        push_field(buffer, None);
    }
}

/// Append parsed ordering terms without reconstructing a request query string.
fn push_order_terms(buffer: &mut String, terms: &[OrderTerm]) {
    push_field(buffer, Some(&terms.len().to_string()));
    for term in terms {
        // The whole path, not just its head: `a.b`, `a[b]` and `a` are three
        // different orderings, and a key spelling only `a` would serve one
        // compiled query for all of them.
        push_field(buffer, Some(&serialize_attr_path(&term.attr)));
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
/// Covers every dimension that affects term expansion: where the request named
/// its context and what it named there, and the core-context policy.
fn context_digest(context: &ContextSpec) -> u64 {
    let mut hasher = DefaultHasher::new();
    match context.request_context() {
        RequestContext::Default => 0_u8.hash(&mut hasher),
        RequestContext::Link(url) => {
            1_u8.hash(&mut hasher);
            url.hash(&mut hasher);
        }
        RequestContext::Body(body) => {
            2_u8.hash(&mut hasher);
            body.to_string().hash(&mut hasher);
        }
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
    #[must_use]
    pub fn new() -> CompiledQueryCache {
        CompiledQueryCache::with_capacity_and_ttl(DEFAULT_CAPACITY, DEFAULT_TTL)
    }

    /// Create a cache with explicit capacity and idle time-to-live.
    #[must_use]
    pub fn with_capacity_and_ttl(capacity: u64, ttl: Duration) -> CompiledQueryCache {
        let inner = Cache::builder().max_capacity(capacity).time_to_idle(ttl).build();
        CompiledQueryCache { inner }
    }

    /// Return the cached compiled query for `key`, or compute it through
    /// `init` and insert it on cache miss.
    ///
    /// # Errors
    ///
    /// Propagates whatever `init` reports on a cache miss; nothing is inserted
    /// when the compilation fails. A cache hit never fails.
    pub async fn get_or_compile<F, Fut, E>(&self, key: CompiledQueryKey, init: F) -> Result<Arc<CompiledEntityQuery>, E>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<CompiledEntityQuery, E>> + Send,
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
    use crate::compiled_cache::{CompiledQueryKey, context_digest};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;
    use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
    use iri_rs::IriBuf;
    use pretty_assertions::{assert_eq, assert_ne};
    use serde_json::json;

    fn params(query: &str) -> EntityQueryParameters {
        serde_urlencoded::from_str(query).expect("valid query string")
    }

    fn key(query: &str) -> CompiledQueryKey {
        CompiledQueryKey::from_params(&params(query), &ContextSpec::default(), LanguageTargeting::Specification)
    }

    #[test]
    fn identical_parameters_produce_one_key() {
        assert_eq!(key("q=speed>50"), key("q=speed>50"));
    }

    /// One `q` compiled at two `q_target_languages` levels may select
    /// different languageMap keys, so the two are two compilations.
    #[test]
    fn the_target_language_level_is_part_of_the_key() {
        let filtered = CompiledQueryKey::from_params(&params("q=name[en]==\"Lift\""), &ContextSpec::default(), LanguageTargeting::LanguageRanges);
        assert_ne!(key("q=name[en]==\"Lift\""), filtered);
    }

    /// The page never takes part in compilation, so two pages of one query
    /// share one compilation.
    #[test]
    fn two_pages_of_one_query_share_one_compilation() {
        assert_eq!(key("q=speed>50&limit=10"), key("q=speed>50&limit=20&offset=40"));
        assert_eq!(key("q=speed>50"), key("q=speed>50&cursor="));
    }

    /// The rendering is length-prefixed precisely so adjacent parameter values
    /// cannot impersonate a different parameter boundary.
    #[test]
    fn a_value_containing_the_separator_cannot_impersonate_another_parameter() {
        assert_ne!(key("attrs=ab&q=c"), key("attrs=a&q=bc"));
    }

    #[test]
    fn an_empty_filter_is_rejected_before_cache_key_construction() {
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("q=").is_err());
    }

    /// Clause 4.9: a bare `time` and a quoted `quotedStr` in the same form are
    /// different operands, so they compile apart.
    #[test]
    fn a_bare_time_and_a_quoted_time_form_string_produce_different_keys() {
        assert_ne!(key("q=opens%3D%3D09:30:00Z"), key("q=opens%3D%3D%2209:30:00Z%22"));
    }

    #[test]
    fn different_parameters_produce_different_keys() {
        assert_ne!(key("q=speed>50"), key("q=speed>10"));
    }

    #[test]
    fn typed_identifier_and_dataset_values_are_part_of_the_key() {
        assert_ne!(key("id=urn%3Angsi-ld%3AEntity%3Aa"), key("id=urn%3Angsi-ld%3AEntity%3Ab"));
        assert_ne!(
            key("type=Vehicle&datasetId=urn%3Angsi-ld%3ADataset%3Aa"),
            key("type=Vehicle&datasetId=urn%3Angsi-ld%3ADataset%3Ab")
        );
    }

    fn context_iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid context IRI")
    }

    /// The same context named by no carrier, by a `Link` header and by the
    /// body is three different requests (clause 6.3.5), so each compiles to its
    /// own entry.
    #[test]
    fn where_the_request_named_its_context_is_part_of_the_digest() {
        let url = "https://example.com/context.jsonld";
        let default = context_digest(&ContextSpec::none());
        let link = context_digest(&ContextSpec::from_link(context_iri(url)));
        let body = context_digest(&ContextSpec::from_body(
            JsonLiteral::admit(&json!(url).to_string()).expect("an admissible literal"),
        ));

        assert_ne!(default, link);
        assert_ne!(default, body);
        assert_ne!(link, body);
    }

    /// JSON-LD processes an `@context` array in order, repeats included, so
    /// `[A, B, A]` leaves A's definitions in force where `[A, B]` leaves B's.
    #[test]
    fn a_repeated_context_is_part_of_the_digest() {
        let repeated = ContextSpec::from_body(
            JsonLiteral::admit(&json!(["https://example.com/a.jsonld", "https://example.com/b.jsonld", "https://example.com/a.jsonld"]).to_string())
                .expect("an admissible literal"),
        );
        let once = ContextSpec::from_body(
            JsonLiteral::admit(&json!(["https://example.com/a.jsonld", "https://example.com/b.jsonld"]).to_string()).expect("an admissible literal"),
        );

        assert_ne!(context_digest(&repeated), context_digest(&once));
    }

    #[test]
    fn equal_contexts_have_equal_digests() {
        let first = ContextSpec::from_link(context_iri("https://example.com/context.jsonld"));
        let second = ContextSpec::from_link(context_iri("https://example.com/context.jsonld"));

        assert_eq!(context_digest(&first), context_digest(&second));
        assert_eq!(context_digest(&ContextSpec::none()), context_digest(&ContextSpec::none()));
    }
}
