//! The snapshot store port, and the parameter shapes it takes (clause 5.16).
//!
//! A snapshot freezes query results, so the store holds three things: the
//! snapshot record and its lifecycle, the frozen entities, and the frozen
//! temporal entities. The port keeps them apart rather than exposing a table
//! name, because a store that keeps frozen payloads somewhere else entirely
//! still answers the same questions.

use crate::core::error::AndromedaError;
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::SnapshotId;
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use url::Url;

/// Pre-serialised columns for a snapshot record.
///
/// The policy layer materialises the JSON, lifetime and priority
/// representations once and hands them over; the store writes them verbatim.
#[derive(Debug)]
pub struct InsertSnapshotParams<'a> {
    /// Snapshot identifier.
    pub id: &'a SnapshotId,
    /// Pre-serialised JSON for the snapshot's entity queries.
    pub snapshot_queries_json: Option<Value>,
    /// Pre-serialised JSON for the snapshot's temporal queries.
    pub snapshot_temporal_queries_json: Option<Value>,
    /// Stringified ISO-8601 duration for the snapshot's lifetime.
    pub snapshot_lifetime: Option<String>,
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
    pub entity_id: &'a str,
    /// Frozen payload (entity or temporal entity).
    pub data: &'a Value,
}

/// Storing snapshots and the frozen payloads they hold.
#[async_trait]
pub trait SnapshotStore: Send + Sync + 'static {
    /// Persist a new snapshot record.
    async fn create(&self, params: InsertSnapshotParams<'_>) -> Result<(), AndromedaError>;

    /// Retrieve a snapshot record.
    async fn get(&self, id: &SnapshotId) -> Result<Option<Snapshot>, AndromedaError>;

    /// Whether a snapshot with this identifier exists.
    async fn exists(&self, id: &SnapshotId) -> Result<bool, AndromedaError>;

    /// Move a snapshot's lifetime, priority and expiry. Returns rows affected.
    async fn update_lifetime_priority(
        &self,
        id: &SnapshotId,
        lifetime: Option<&Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> Result<u64, AndromedaError>;

    /// Advance a snapshot out of `preparing`, recording the execution details.
    ///
    /// The transition itself is validated by the policy layer; this is
    /// unconditional apart from the guard on the stored status still being
    /// `preparing`, which is what makes a concurrent second executor a no-op.
    async fn update_status(
        &self,
        id: &SnapshotId,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<u64, AndromedaError>;

    /// Delete a snapshot and everything frozen under it. Returns rows affected.
    async fn delete(&self, id: &SnapshotId) -> Result<u64, AndromedaError>;

    /// Identifiers of all snapshots matching a purge filter (clause 5.16.7).
    async fn match_by_filter(&self, filter: &SnapshotPurgeFilter) -> Result<Vec<SnapshotId>, AndromedaError>;

    /// Delete one snapshot found by a purge filter. Returns rows affected.
    async fn delete_one(&self, id: &SnapshotId) -> Result<u64, AndromedaError>;

    /// Copy a snapshot's frozen contents under a new identifier and expiry.
    async fn clone_snapshot(&self, source_id: &SnapshotId, new_id: &SnapshotId, new_expires_at: DateTime<Utc>) -> Result<Snapshot, AndromedaError>;

    /// Delete every expired snapshot, returning the count removed.
    async fn delete_expired(&self) -> Result<u64, AndromedaError>;

    /// Record that a snapshot was read.
    async fn touch_last_used(&self, id: &SnapshotId) -> Result<(), AndromedaError>;

    /// Freeze entities under a snapshot.
    async fn insert_entities(&self, snapshot_id: &SnapshotId, entities: &[SnapshotEntityInput<'_>]) -> Result<(), AndromedaError>;

    /// Freeze temporal entities under a snapshot.
    async fn insert_temporal_entities(&self, snapshot_id: &SnapshotId, entities: &[SnapshotEntityInput<'_>]) -> Result<(), AndromedaError>;

    /// Retrieve one frozen entity.
    async fn get_entity(&self, snapshot_id: &SnapshotId, entity_id: &str) -> Result<Option<Value>, AndromedaError>;

    /// Query frozen entities, with optional type and identifier filters.
    async fn query_entities(
        &self,
        snapshot_id: &SnapshotId,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError>;

    /// Retrieve one frozen temporal entity.
    async fn get_temporal_entity(&self, snapshot_id: &SnapshotId, entity_id: &str) -> Result<Option<Value>, AndromedaError>;

    /// Query frozen temporal entities, with optional type and identifier filters.
    async fn query_temporal_entities(
        &self,
        snapshot_id: &SnapshotId,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError>;

    /// Distinct entity types frozen under a snapshot.
    async fn snapshot_distinct_types(&self, snapshot_id: &SnapshotId) -> Result<Vec<String>, AndromedaError>;

    /// Entity count and attribute names for one frozen type.
    async fn snapshot_type_info(&self, snapshot_id: &SnapshotId, type_name: &str) -> Result<Option<(i64, Vec<String>)>, AndromedaError>;

    /// Distinct attribute names frozen under a snapshot.
    async fn snapshot_distinct_attributes(&self, snapshot_id: &SnapshotId) -> Result<Vec<String>, AndromedaError>;

    /// Entity count and type names for one frozen attribute.
    async fn snapshot_attribute_info(&self, snapshot_id: &SnapshotId, attr_name: &str) -> Result<Option<(i64, Vec<String>)>, AndromedaError>;
}
