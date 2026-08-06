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

use crate::{
    core::error::AndromedaError,
    service::entity::{query_outcome::QueryOutcome, storage_profile::StorageProfile},
};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

/// One entity as it stood at the moment it was deleted.
///
/// Carries the payload and system attributes the notification path needs, which
/// are no longer readable once the row is gone.
#[derive(Debug)]
pub struct DeletedEntity {
    /// Entity identifier.
    pub id: String,
    /// The expanded JSON-LD payload at the time of deletion.
    pub payload: Value,
    /// Timestamp when the entity was originally created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the last modification before deletion.
    pub modified_at: DateTime<Utc>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
}

/// One entity's payload together with the version token a
/// [`Precondition::UnchangedSince`](crate::service::entity::precondition::Precondition::UnchangedSince)
/// compares against.
///
/// A read-modify-write reads this, computes its result, and states the
/// `modified_at` it assumed; the store refuses the write if the entity has
/// moved on since. That is what replaces reading inside the writing
/// transaction.
#[derive(Debug)]
pub struct EntitySnapshot {
    /// The expanded JSON-LD payload as it stood when it was read.
    pub payload: Value,
    /// The entity's clause 4.8 `modifiedAt` at the same instant.
    pub modified_at: DateTime<Utc>,
}

/// Aggregated metadata for a single attribute across all entities.
///
/// Backs the clause 5.7.3 attribute discovery endpoints.
#[derive(Debug)]
pub struct AttributeSummary {
    /// The expanded IRI of the attribute.
    pub attr_iri: String,
    /// Number of distinct entities that carry this attribute.
    pub entity_count: i64,
    /// Entity type IRIs that contain this attribute.
    pub entity_types: Vec<String>,
    /// NGSI-LD attribute types observed for this attribute.
    pub attribute_types: Vec<String>,
}

/// Reading entities, and the two bulk removals that stand alone.
#[async_trait]
pub trait EntityStore: Send + Sync + 'static {
    /// What this store can be asked to do with a compiled query.
    ///
    /// Asked through the port rather than threaded alongside it, so a profile
    /// can never describe a different store from the one it was wired to, and
    /// so it is per-pool for free.
    fn profile(&self) -> &StorageProfile;

    /// The expanded payload of one entity, if it exists.
    async fn get(&self, id: &str) -> Result<Option<Value>, AndromedaError>;

    /// The expanded payloads of several entities, keyed by identifier.
    ///
    /// Identifiers with no entity are simply absent from the map.
    async fn get_batch(&self, ids: &[String]) -> Result<HashMap<String, Value>, AndromedaError>;

    /// One entity's payload and the version token a read-modify-write states
    /// back to the store.
    async fn get_snapshot(&self, id: &str) -> Result<Option<EntitySnapshot>, AndromedaError>;

    /// The same for several entities, keyed by identifier.
    async fn get_snapshot_batch(&self, ids: &[String]) -> Result<HashMap<String, EntitySnapshot>, AndromedaError>;

    /// Every entity carrying at least one attribute instance whose `expiresAt`
    /// has passed (clause 4.22), with the version token needed to rewrite it.
    ///
    /// The removal itself is not the store's to make: which instances have
    /// expired and what a payload looks like without them is specification
    /// semantics, and the tombstones the removal implies have to commit with
    /// the rewritten payload.
    async fn entities_with_expired_attributes(&self) -> Result<Vec<(String, EntitySnapshot)>, AndromedaError>;

    /// The clause 4.8 system attributes of one entity.
    async fn get_sys_attrs(&self, id: &str) -> Result<Option<SysAttrsSnapshot>, AndromedaError>;

    /// The clause 4.8 system attributes of several entities, keyed by identifier.
    async fn get_sys_attrs_batch(&self, ids: &[String]) -> Result<HashMap<String, SysAttrsSnapshot>, AndromedaError>;

    /// Every entity type IRI the broker currently holds an entity for.
    async fn types(&self) -> Result<Vec<String>, AndromedaError>;

    /// One page of `(type IRI, attribute IRIs)` pairs, ordered by type IRI.
    ///
    /// `after` is the last type IRI of the previous page, exclusive.
    async fn get_entity_type_summaries(&self, after: Option<&str>, limit: u32) -> Result<Vec<(String, Vec<String>)>, AndromedaError>;

    /// One page of attribute summaries, ordered by attribute IRI.
    ///
    /// `after` is the last attribute IRI of the previous page, exclusive.
    async fn get_attribute_summary(&self, after: Option<&str>, limit: u32) -> Result<Vec<AttributeSummary>, AndromedaError>;

    /// The summary for one attribute IRI, if any entity carries it.
    async fn get_attribute_info(&self, attr_iri: &str) -> Result<Option<AttributeSummary>, AndromedaError>;

    /// Every entity of one type, paginated.
    async fn get_entities_by_type(&self, entity_type: &str, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError>;

    /// Every entity the broker holds, paginated.
    async fn get_all_entities(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError>;

    /// How many entities carry one type IRI.
    async fn count_by_type(&self, entity_type: &str) -> Result<i64, AndromedaError>;

    /// Run a compiled entity query, returning payloads together with whatever
    /// the store could not apply.
    ///
    /// A store that applies every predicate and the result window says so in
    /// the outcome; one that cannot names what it left behind, and the caller
    /// finishes the filter in memory rather than returning under-filtered
    /// results.
    async fn query_compiled(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, AndromedaError>;

    /// Run a compiled entity query for identifiers only (clause 5.2.39 entity maps).
    ///
    /// Carries the same residual contract as [`EntityStore::query_compiled`],
    /// which costs the caller a payload read: a predicate cannot be evaluated
    /// against an identifier.
    async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<String>, AndromedaError>;

    /// Delete every entity whose `expiresAt` has passed, returning their identifiers.
    async fn expire_due_entities(&self) -> Result<Vec<String>, AndromedaError>;
}
