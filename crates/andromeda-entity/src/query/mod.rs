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
use andromeda_ngsi_ld_geometry::geometry_census::GeometryCensusLimits;
use andromeda_tenancy::tenant_ref::TenantRef;
use std::sync::Arc;

/// Service for querying and retrieving NGSI-LD entities.
pub struct EntityQueryService {
    reader: Arc<dyn EntityReader>,
    query_store: Arc<dyn EntityQueryStore>,
    engine: JsonLdEngine,
    compiled_cache: CompiledQueryCache,
    /// Which spatial index, if any, a geo query may be narrowed through.
    spatial: SpatialAcceleration,
    tenant: TenantRef,
    /// Where a Linked Entity the local store does not hold is fetched from
    /// (clause 4.5.23.1). Absent in a composition that resolves links purely
    /// locally.
    remote_linked: Option<Arc<dyn RemoteLinkedEntitySource>>,
    /// The limits a query's reference geometry stays within, past which the
    /// query is too complex to resolve (clause 5.5.6).
    reference_limits: GeometryCensusLimits,
}

impl EntityQueryService {
    /// Create a new entity query service backed by the given store.
    ///
    /// `remote_linked` is the source consulted for an `objectType`-annotated
    /// Linked Entity the store does not hold; `None` resolves links against the
    /// local store alone.
    pub fn new(
        reader: Arc<dyn EntityReader>,
        query_store: Arc<dyn EntityQueryStore>,
        engine: JsonLdEngine,
        spatial: SpatialAcceleration,
        tenant: TenantRef,
        remote_linked: Option<Arc<dyn RemoteLinkedEntitySource>>,
        reference_limits: GeometryCensusLimits,
    ) -> EntityQueryService {
        EntityQueryService {
            reader,
            query_store,
            engine,
            compiled_cache: CompiledQueryCache::new(),
            spatial,
            tenant,
            remote_linked,
            reference_limits,
        }
    }

    /// Return a reference to the underlying JSON-LD engine.
    #[must_use]
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    /// The limits a query's reference geometry stays within (clause 5.5.6).
    #[must_use]
    pub const fn reference_limits(&self) -> GeometryCensusLimits {
        self.reference_limits
    }
}
