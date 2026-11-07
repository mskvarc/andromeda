//! Reading entities: clause 5.7.2 query, clause 5.6.3 retrieval, and the
//! compilation that turns request parameters into a query a store can run.

/// Expanding, once per request, the attribute member names a response carries.
pub mod attribute_expansion;
/// Filtering an entity's attributes down to what the client asked for.
pub mod attribute_projection;
/// Compiling request parameters into a [`CompiledEntityQuery`].
pub mod compilation;
/// Where the Core reads and notifications read their Linked Entities from
/// (clause 4.5.23).
pub mod core_linked_source;
/// Narrowing an entity's attribute instances by `datasetId`.
pub mod dataset_filter;
/// Locally held entities as versions to merge, and merged versions compacted.
pub mod entity_versions;
/// Merging locally held entities with those a context source returned.
pub mod forwarded_merge;
/// Gathering the entity states the hops of an in-memory filter cross into
/// (clause 4.9).
pub mod linked_filter_states;
/// The Linked Entities a subscription's `join` reaches from the entities of
/// one notification (clause 4.5.23).
pub mod notification_join;
/// Compiling raw `pick`/`omit` terms into the clause 4.21 projection.
pub mod projection_compilation;
/// What a clause 4.21 projection leaves of an entity.
pub mod projection_remainder;
/// Refusing a query the store cannot narrow at all.
pub mod pushdown_refusal;
/// Resolving a Linked Entity the local store does not hold (clause 4.5.23.1).
pub mod remote_linked;
/// Finishing a query a store did not finish.
pub mod residual;
/// Proof that declining a predicate does not change the answer.
pub mod residual_conformance;
/// The read operations themselves.
pub mod retrieval;
/// Clause 5.6.3 retrieval over a store holding one entity.
#[cfg(test)]
mod retrieval_tests;
/// Compiling a query's entity selectors (clause 5.2.33) against the context.
pub mod selector_compilation;
/// The service the read-path unit tests are built on.
#[cfg(test)]
pub(crate) mod service_test_double;
/// The Attribute names the clause 4.5.4 simplified representation renders.
pub mod simplified_names;
/// Narrowing a geo-query to an identifier set through the spatial index.
pub mod spatial_prefilter;
/// Narrowing a stored state to what a read selected, before compaction.
pub(crate) mod state_projection;

use crate::{
    compiled_cache::CompiledQueryCache,
    port::{query::EntityQueryStore, reader::EntityReader},
    query::remote_linked::RemoteLinkedEntitySource,
    spatial_acceleration::SpatialAcceleration,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_query::{
    filter::{complexity::FilterComplexityLimits, language_targeting::LanguageTargeting},
    geo::admission::GeoQueryAdmission,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// Service for querying and retrieving NGSI-LD entities.
///
/// Built with [`EntityQueryService::builder`]; `remote_linked` is the source
/// consulted for an `objectType`-annotated Linked Entity the store does not
/// hold, `None` resolving links against the local store alone.
#[derive(TypedBuilder)]
pub struct EntityQueryService {
    /// The store entities are retrieved from.
    reader: Arc<dyn EntityReader>,
    /// The store queries are answered from.
    query_store: Arc<dyn EntityQueryStore>,
    /// The JSON-LD engine every term is expanded and compacted with.
    engine: JsonLdEngine,
    /// The compiled queries the service reuses, empty when it is built.
    #[builder(default = CompiledQueryCache::new(), setter(skip))]
    compiled_cache: CompiledQueryCache,
    /// Which spatial index, if any, a geo query may be narrowed through.
    spatial: SpatialAcceleration,
    /// The tenant whose partition every read stays inside.
    tenant: TenantRef,
    /// Where a Linked Entity the local store does not hold is fetched from
    /// (clause 4.5.23.1). Absent in a composition that resolves links purely
    /// locally.
    remote_linked: Option<Arc<dyn RemoteLinkedEntitySource>>,
    /// What a query's geo-query and distance ordering are admitted under:
    /// the reference geometry limits past which the query is too complex to
    /// resolve (clause 5.5.6), and whether a `near` distance band is served.
    geo_admission: GeoQueryAdmission,
    /// What a query's target languages may be and how they select
    /// languageMap keys: the broker's `q_target_languages` level.
    language_targeting: LanguageTargeting,
    /// The most a query's `q` filter may ask of the broker before it is too
    /// complex to resolve (clause 5.5.6).
    filter_complexity: FilterComplexityLimits,
}

impl EntityQueryService {
    /// Return a reference to the underlying JSON-LD engine.
    #[must_use]
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    /// What a query's geo-query is admitted under.
    #[must_use]
    pub const fn geo_admission(&self) -> GeoQueryAdmission {
        self.geo_admission
    }

    /// What a query's target languages may be and how they select languageMap keys.
    #[must_use]
    pub const fn language_targeting(&self) -> LanguageTargeting {
        self.language_targeting
    }

    /// The most a query's `q` filter may ask of the broker.
    #[must_use]
    pub const fn filter_complexity(&self) -> FilterComplexityLimits {
        self.filter_complexity
    }
}
