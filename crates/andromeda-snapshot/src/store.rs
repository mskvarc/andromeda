//! The snapshot store port, and the parameter shapes it takes (clause 5.16).
//!
//! A snapshot freezes query results, so the store holds three things: the
//! snapshot record and its lifecycle, the frozen entities, and the frozen
//! temporal entities. The port keeps them apart rather than exposing a table
//! name, because a store that keeps frozen payloads somewhere else entirely
//! still answers the same questions.

use crate::error::SnapshotError;
use andromeda_context_space::snapshot_target::SnapshotTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    query::body::Query,
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

/// Pre-serialised columns for a snapshot record.
///
/// The policy layer materialises the JSON, lifetime and priority
/// representations once and hands them over; the store writes them verbatim.
#[derive(Debug)]
pub struct InsertSnapshotParams<'a> {
    /// Snapshot identifier.
    pub id: &'a Uri,
    /// Typed entity query definitions, serialized by the SQL adapter.
    pub snapshot_queries: Option<&'a [Query]>,
    /// Typed temporal query definitions, serialized by the SQL adapter.
    pub snapshot_temporal_queries: Option<&'a [Query]>,
    /// Resolved request context used by the asynchronous query executor.
    pub query_context_json: Value,
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
    /// Source Snapshot identifier for clone preparation; absent for capture.
    pub clone_source_id: Option<&'a Uri>,
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

/// One durably claimed preparation job.
#[derive(Debug)]
pub struct SnapshotPreparationJob {
    /// Snapshot control resource being prepared.
    pub snapshot: Snapshot,
    /// Source Snapshot for clone jobs; absent for query capture jobs.
    pub clone_source_id: Option<Uri>,
    /// Resolved request context persisted with the preparation job.
    pub query_context: Value,
}

/// Storing Snapshot control resources and their lifecycle state.
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

    /// Resolve the durable context branch owned by a Snapshot identifier.
    fn target<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<SnapshotTarget>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Resolve the private context branch populated during query capture.
    fn staging_target<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<SnapshotTarget>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Return the next snapshot awaiting bounded preparation, if one exists.
    fn next_preparing<'life0, 'async_trait>(
        &'life0 self,
        claimed_by: Uuid,
        lease_until: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Option<SnapshotPreparationJob>, SnapshotError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Renew a preparation lease if it is still held by `claimed_by`.
    fn renew_preparation_lease<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        claimed_by: Uuid,
        lease_until: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<bool, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Apply mutable Snapshot metadata and expiry. Returns rows affected.
    fn update_metadata<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        lifetime: Option<&'life2 Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        endpoint: Option<Option<&'life3 Url>>,
        receiver_info: Option<Option<Value>>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Advance a snapshot out of `preparing`, recording the execution details.
    ///
    /// The transition itself is validated by the policy layer; this is
    /// unconditional apart from the guard on the stored status still being
    /// `preparing`, which is what makes a concurrent second executor a no-op.
    fn update_status<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        claimed_by: Uuid,
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

    /// Atomically publish staged current and temporal rows and leave preparation.
    fn promote_capture<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        claimed_by: Uuid,
        status: SnapshotStatus,
        queries_details: Option<&'life2 [ExecutionResultDetails]>,
        temporal_queries_details: Option<&'life3 [ExecutionResultDetails]>,
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
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

    /// Atomically copy a source branch into a claimed preparing clone and promote it.
    fn promote_clone<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        source_id: &'life1 Uri,
        target_id: &'life2 Uri,
        claimed_by: Uuid,
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
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

    /// Enforce optional tenant-wide Snapshot count and materialized-byte quotas.
    ///
    /// Victims are removed in deterministic policy order: lower priority,
    /// earlier expiry, older last use, then lexicographically smaller URI.
    fn enforce_quotas<'life0, 'async_trait>(
        &'life0 self,
        count_quota: Option<usize>,
        byte_quota: Option<u64>,
    ) -> BoxFuture<'async_trait, Result<Vec<Uri>, SnapshotError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Record that a snapshot was read.
    fn touch_last_used<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
