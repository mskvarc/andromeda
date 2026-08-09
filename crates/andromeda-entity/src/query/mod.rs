//! Reading entities: clause 5.7.2 query, clause 5.6.3 retrieval, and the
//! compilation that turns request parameters into a query a store can run.

/// Filtering an entity's attributes down to what the client asked for.
pub mod attribute_projection;
/// Turning an expanded payload back into a compacted [`Entity`].
pub mod compaction;
/// Compiling request parameters into a [`CompiledEntityQuery`].
pub mod compilation;
/// Narrowing an entity's attribute instances by `datasetId`.
pub mod dataset_filter;
/// Merging locally held entities with those a context source returned.
pub mod forwarded_merge;
/// Resolving the entities a result set links to (clause 4.5.20).
pub mod linked_entity;
/// Refusing a query the store cannot narrow at all.
pub mod pushdown_refusal;
/// Finishing a query a store did not finish.
pub mod residual;
/// Proof that declining a predicate does not change the answer.
pub mod residual_conformance;
/// The read operations themselves.
pub mod retrieval;
/// Compiling a query's entity selectors (clause 5.2.11) against the context.
pub mod selector_compilation;
/// Narrowing a geo-query to an identifier set through the spatial index.
pub mod spatial_prefilter;

use crate::{
    compiled_cache::CompiledQueryCache,
    port::{query::EntityQueryStore, reader::EntityReader},
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_tenancy::tenant_ref::TenantRef;
use std::sync::Arc;

/// Service for querying and retrieving NGSI-LD entities.
pub struct EntityQueryService {
    reader: Arc<dyn EntityReader>,
    query_store: Arc<dyn EntityQueryStore>,
    engine: JsonLdEngine,
    compiled_cache: CompiledQueryCache,
    spatial: Arc<SpatialEntityIndex>,
    tenant: TenantRef,
}

impl EntityQueryService {
    /// Create a new entity query service backed by the given store.
    pub fn new(
        reader: Arc<dyn EntityReader>,
        query_store: Arc<dyn EntityQueryStore>,
        engine: JsonLdEngine,
        spatial: Arc<SpatialEntityIndex>,
        tenant: TenantRef,
    ) -> EntityQueryService {
        EntityQueryService {
            reader,
            query_store,
            engine,
            compiled_cache: CompiledQueryCache::new(),
            spatial,
            tenant,
        }
    }

    /// Return a reference to the underlying JSON-LD engine.
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }
}
