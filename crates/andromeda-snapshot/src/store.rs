//! The snapshot store port, and the parameter shapes it takes (clause 5.16).
//!
//! A snapshot freezes query results, so the store holds three things: the
//! snapshot record and its lifecycle, the frozen entities, and the frozen
//! temporal entities. The port keeps them apart rather than exposing a table
//! name, because a store that keeps frozen payloads somewhere else entirely
//! still answers the same questions.

use crate::error::SnapshotError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use url::Url;

/// An entity count and the attribute names exposed by one frozen entity type.
pub type SnapshotTypeInfo = (i64, Vec<AttributeName>);

/// An entity count and the entity type names exposing one frozen attribute.
pub type SnapshotAttributeInfo = (i64, Vec<EntityTypeName>);

/// Pre-serialised columns for a snapshot record.
///
/// The policy layer materialises the JSON, lifetime and priority
/// representations once and hands them over; the store writes them verbatim.
#[derive(Debug)]
pub struct InsertSnapshotParams<'a> {
    /// Snapshot identifier.
    pub id: &'a Uri,
    /// Typed entity query definitions, serialized by the SQL adapter.
    pub snapshot_queries: Option<&'a [andromeda_ngsi_ld_model::query::body::Query]>,
    /// Typed temporal query definitions, serialized by the SQL adapter.
    pub snapshot_temporal_queries: Option<&'a [andromeda_ngsi_ld_model::query::body::Query]>,
    /// Typed ISO-8601 duration for the snapshot's lifetime.
    pub snapshot_lifetime: Option<&'a Iso8601Duration>,
    /// Numeric encoding of [`SnapshotPriority`].
    pub snapshot_priority: i16,
    /// Endpoint URL.
    pub endpoint: Option<&'a Url>,
    /// Pre-serialised JSON for the notification receiver info.
    pub receiver_info_json: Option<Value>,
    /// Current lifecycle status.
    pub status: SnapshotStatus,
    /// Pre-serialised JSON for the entity-query execution details.
    pub queries_details_json: Option<Value>,
    /// Pre-serialised JSON for the temporal-query execution details.
    pub temporal_queries_details_json: Option<Value>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last-modified timestamp.
    pub modified_at: DateTime<Utc>,
    /// Expiration timestamp.
    pub expires_at: DateTime<Utc>,
    /// Last-used timestamp, if set.
    pub last_used_at: Option<DateTime<Utc>>,
}

/// A single `(entity_id, payload)` pair destined for a snapshot's frozen set.
#[derive(Debug)]
pub struct SnapshotEntityInput<'a> {
    /// NGSI-LD entity identifier.
    pub entity_id: &'a Uri,
    /// Frozen payload (entity or temporal entity).
    pub data: &'a Value,
}

/// Storing snapshots and the frozen payloads they hold.
pub trait SnapshotStore: Send + Sync + 'static {
    /// Persist a new snapshot record.
    fn create<'life0, 'life1, 'async_trait>(&'life0 self, params: InsertSnapshotParams<'life1>) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Retrieve a snapshot record.
    fn get<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<Snapshot>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Whether a snapshot with this identifier exists.
    fn exists<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<bool, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Return the next snapshot awaiting bounded preparation, if one exists.
    fn next_preparing<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Option<Snapshot>, SnapshotError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Move a snapshot's lifetime, priority and expiry. Returns rows affected.
    fn update_lifetime_priority<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        lifetime: Option<&'life2 Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Advance a snapshot out of `preparing`, recording the execution details.
    ///
    /// The transition itself is validated by the policy layer; this is
    /// unconditional apart from the guard on the stored status still being
    /// `preparing`, which is what makes a concurrent second executor a no-op.
    fn update_status<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        status: SnapshotStatus,
        queries_details: Option<&'life2 [ExecutionResultDetails]>,
        temporal_queries_details: Option<&'life3 [ExecutionResultDetails]>,
    ) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Delete a snapshot and everything frozen under it. Returns rows affected.
    fn delete<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Identifiers of all snapshots matching a purge filter (clause 5.16.7).
    fn match_by_filter<'life0, 'life1, 'async_trait>(
        &'life0 self,
        filter: &'life1 SnapshotPurgeFilter,
    ) -> BoxFuture<'async_trait, Result<Vec<Uri>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Delete one snapshot found by a purge filter. Returns rows affected.
    fn delete_one<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Copy a snapshot's frozen contents under a new identifier and expiry.
    fn clone_snapshot<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        source_id: &'life1 Uri,
        new_id: &'life2 Uri,
        new_expires_at: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Snapshot, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Delete snapshots due at or before an explicit cutoff, returning the count removed.
    fn delete_expired<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Record that a snapshot was read.
    fn touch_last_used<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Freeze entities under a snapshot.
    fn insert_entities<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
        entities: &'life2 [SnapshotEntityInput<'life3>],
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Freeze temporal entities under a snapshot.
    fn insert_temporal_entities<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
        entities: &'life2 [SnapshotEntityInput<'life3>],
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Retrieve one frozen entity.
    fn get_entity<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
        entity_id: &'life2 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<Value>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Query frozen entities, with optional type and identifier filters.
    fn query_entities<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
        entity_type: Option<&'life2 TypeExprRaw>,
        entity_ids: Option<&'life3 [Uri]>,
        limit: i64,
        offset: i64,
    ) -> BoxFuture<'async_trait, Result<(Vec<Value>, i64), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Retrieve one frozen temporal entity.
    fn get_temporal_entity<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
        entity_id: &'life2 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<Value>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Query frozen temporal entities, with optional type and identifier filters.
    fn query_temporal_entities<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
        entity_type: Option<&'life2 TypeExprRaw>,
        entity_ids: Option<&'life3 [Uri]>,
        limit: i64,
        offset: i64,
    ) -> BoxFuture<'async_trait, Result<(Vec<Value>, i64), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Distinct entity types frozen under a snapshot.
    fn snapshot_distinct_types<'life0, 'life1, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Vec<EntityTypeName>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Entity count and attribute names for one frozen type.
    fn snapshot_type_info<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
        type_name: &'life2 EntityTypeName,
    ) -> BoxFuture<'async_trait, Result<Option<SnapshotTypeInfo>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Distinct attribute names frozen under a snapshot.
    fn snapshot_distinct_attributes<'life0, 'life1, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Vec<AttributeName>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Entity count and type names for one frozen attribute.
    fn snapshot_attribute_info<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        snapshot_id: &'life1 Uri,
        attr_name: &'life2 AttributeName,
    ) -> BoxFuture<'async_trait, Result<Option<SnapshotAttributeInfo>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
