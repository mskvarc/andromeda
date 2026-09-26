//! Reading entities: clause 5.7.2 query, clause 5.6.3 retrieval, and the
//! compilation that turns request parameters into a query a store can run.

/// Expanding, once per request, the attribute member names a response carries.
pub mod attribute_expansion;
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
/// Preparing the nested projections in the shape the join mode applies them.
pub mod join_projection;
/// Resolving the entities a result set links to (clause 4.5.23).
pub mod linked_entity;
/// Deriving the nested projection that applies to each linked entity
/// (clause 4.21).
pub(crate) mod linked_projection;
/// Which Linked Entities a payload's Relationships still name (clause 4.5.23).
pub mod linked_reachability;
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
/// Compiling a query's entity selectors (clause 5.2.33) against the context.
pub mod selector_compilation;
/// The service the read-path unit tests are built on.
#[cfg(test)]
pub(crate) mod service_test_double;
/// The Attribute names the clause 4.5.4 simplified representation renders.
pub mod simplified_names;
/// Narrowing a geo-query to an identifier set through the spatial index.
pub mod spatial_prefilter;

use crate::{
    compiled_cache::CompiledQueryCache,
    error::EntityError,
    port::{query::EntityQueryStore, reader::EntityReader},
    query::remote_linked::RemoteLinkedEntitySource,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, scoped_context_admission::reject_scoped_user_contexts};
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
    /// Where a Linked Entity the local store does not hold is fetched from
    /// (clause 4.5.23.1). Absent in a composition that resolves links purely
    /// locally.
    remote_linked: Option<Arc<dyn RemoteLinkedEntitySource>>,
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
        spatial: Arc<SpatialEntityIndex>,
        tenant: TenantRef,
        remote_linked: Option<Arc<dyn RemoteLinkedEntitySource>>,
    ) -> EntityQueryService {
        EntityQueryService {
            reader,
            query_store,
            engine,
            compiled_cache: CompiledQueryCache::new(),
            spatial,
            tenant,
            remote_linked,
        }
    }

    /// Return a reference to the underlying JSON-LD engine.
    #[must_use]
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    /// Refuse a request `@context` clause 5.5.7 does not admit, before a read or
    /// query expands anything against it.
    ///
    /// Clause 5.5.7 states that the user `@context` "shall not contain JSON-LD
    /// Scoped Contexts", because they "could be used to modify terms defined in
    /// the Core @context or to reshape NGSI-LD Elements during the expansion of
    /// terms". The clause addresses the request's `@context` on every operation
    /// that expands one, not only the write path, so a retrieval and a query
    /// screen it exactly as a create does.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] carrying
    /// [`JsonLdError::ScopedUserContext`](andromeda_jsonld::error::JsonLdError::ScopedUserContext),
    /// which the HTTP binding answers with `BadRequestData`, and the typed
    /// context-loading failure when one of the request's contexts does not
    /// resolve.
    pub async fn admit_read_context(&self, context: &ContextSpec) -> Result<(), EntityError> {
        reject_scoped_user_contexts(&self.engine, context).await?;
        Ok(())
    }
}
