//! The entity store port, declared by the policy that calls it.
//!
//! Everything here is phrased in NGSI-LD terms and in this module's own value
//! types. No statement, dialect, connection or dictionary key appears in the
//! contract, so a store that is not PostgreSQL implements the same trait.
//!
//! Two things the previous shape exposed are deliberately gone. Resolving the
//! attribute IRIs a `q` filter references to their dictionary keys, and waiting
//! for whatever projection answers that filter, are both properties of how a
//! particular store pushes the predicate down; the caller passes the compiled
//! query and nothing else.
//!
//! The write path is absent for a different reason: a write that must land
//! atomically with its history, outbox and side-effect rows is described by an
//! [`AtomicWrite`] and committed through a [`WriteCoordinator`], not by calling
//! this port method by method.
//!
//! [`AtomicWrite`]: crate::service::entity::atomic_write::AtomicWrite
//! [`WriteCoordinator`]: crate::service::entity::write_coordinator::WriteCoordinator

use crate::error::AndromedaError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use andromeda_postgresql_context_store::entity::{
    attribute_summary::AttributeSummary,
    query_outcome::QueryOutcome,
    snapshot::EntitySnapshot,
    storage_profile::StorageProfile,
};
use futures::future::BoxFuture;
use serde_json::Value;
use std::collections::HashMap;

/// One entity type and the attributes carried by entities of that type.
pub type EntityTypeAttributes = (EntityTypeName, Vec<AttributeName>);

/// Reading entities, and the two bulk removals that stand alone.
pub trait EntityStore: Send + Sync + 'static {
    /// What this store can be asked to do with a compiled query.
    ///
    /// Asked through the port rather than threaded alongside it, so a profile
    /// can never describe a different store from the one it was wired to, and
    /// so it is per-pool for free.
    fn profile(&self) -> &StorageProfile;

    /// The expanded payload of one entity, if it exists.
    fn get<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<Value>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The expanded payloads of several entities, keyed by identifier.
    ///
    /// Identifiers with no entity are simply absent from the map.
    fn get_batch<'life0, 'life1, 'async_trait>(&'life0 self, ids: &'life1 [Uri]) -> BoxFuture<'async_trait, Result<HashMap<Uri, Value>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// One entity's payload and the version token a read-modify-write states
    /// back to the store.
    fn get_snapshot<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<EntitySnapshot>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The same for several entities, keyed by identifier.
    fn get_snapshot_batch<'life0, 'life1, 'async_trait>(
        &'life0 self,
        ids: &'life1 [Uri],
    ) -> BoxFuture<'async_trait, Result<HashMap<Uri, EntitySnapshot>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Every entity carrying at least one attribute instance whose `expiresAt`
    /// has passed (clause 4.22), with the version token needed to rewrite it.
    ///
    /// The removal itself is not the store's to make: which instances have
    /// expired and what a payload looks like without them is specification
    /// semantics, and the tombstones the removal implies have to commit with
    /// the rewritten payload.
    fn entities_with_expired_attributes<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<(Uri, EntitySnapshot)>, AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// The clause 4.8 system attributes of one entity.
    fn get_sys_attrs<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<SysAttrsSnapshot>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The clause 4.8 system attributes of several entities, keyed by identifier.
    fn get_sys_attrs_batch<'life0, 'life1, 'async_trait>(
        &'life0 self,
        ids: &'life1 [Uri],
    ) -> BoxFuture<'async_trait, Result<HashMap<Uri, SysAttrsSnapshot>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Every entity type IRI the broker currently holds an entity for.
    fn types<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<EntityTypeName>, AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// One page of `(type IRI, attribute IRIs)` pairs, ordered by type IRI.
    ///
    /// `after` is the last type IRI of the previous page, exclusive.
    fn get_entity_type_summaries<'life0, 'life1, 'async_trait>(
        &'life0 self,
        after: Option<&'life1 EntityTypeName>,
        limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<EntityTypeAttributes>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// One page of attribute summaries, ordered by attribute IRI.
    ///
    /// `after` is the last attribute IRI of the previous page, exclusive.
    fn get_attribute_summary<'life0, 'life1, 'async_trait>(
        &'life0 self,
        after: Option<&'life1 AttributeName>,
        limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<AttributeSummary>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The summary for one attribute IRI, if any entity carries it.
    fn get_attribute_info<'life0, 'life1, 'async_trait>(
        &'life0 self,
        attr_iri: &'life1 AttributeName,
    ) -> BoxFuture<'async_trait, Result<Option<AttributeSummary>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Every entity of one type, paginated.
    fn get_entities_by_type<'life0, 'life1, 'async_trait>(
        &'life0 self,
        entity_type: &'life1 EntityTypeName,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> BoxFuture<'async_trait, Result<Vec<Value>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Every entity the broker holds, paginated.
    fn get_all_entities<'life0, 'async_trait>(
        &'life0 self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> BoxFuture<'async_trait, Result<Vec<Value>, AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// How many entities carry one type IRI.
    fn count_by_type<'life0, 'life1, 'async_trait>(&'life0 self, entity_type: &'life1 EntityTypeName) -> BoxFuture<'async_trait, Result<i64, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Run a compiled entity query, returning payloads together with whatever
    /// the store could not apply.
    ///
    /// A store that applies every predicate and the result window says so in
    /// the outcome; one that cannot names what it left behind, and the caller
    /// finishes the filter in memory rather than returning under-filtered
    /// results.
    fn query_compiled<'life0, 'life1, 'async_trait>(
        &'life0 self,
        compiled: &'life1 CompiledEntityQuery,
    ) -> BoxFuture<'async_trait, Result<QueryOutcome<Value>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Run a compiled entity query for identifiers only (clause 5.2.39 entity maps).
    ///
    /// Carries the same residual contract as [`EntityStore::query_compiled`],
    /// which costs the caller a payload read: a predicate cannot be evaluated
    /// against an identifier.
    fn query_ids<'life0, 'life1, 'async_trait>(
        &'life0 self,
        compiled: &'life1 CompiledEntityQuery,
    ) -> BoxFuture<'async_trait, Result<QueryOutcome<Uri>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Delete every entity whose `expiresAt` has passed, returning their identifiers.
    fn expire_due_entities<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<Uri>, AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
