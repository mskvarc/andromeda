//! Repository input parameters for snapshot persistence.

use andromeda_domain::{ngsi_ld_urn::SnapshotId, snapshot::SnapshotStatus};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use url::Url;

/// Pre-serialised columns for a `snapshots` INSERT. The service layer
/// materialises JSON/lifetime/priority representations once and hands them
/// to the repo; the repo binds them verbatim.
#[derive(Debug)]
pub struct InsertSnapshotParams<'a> {
    /// Snapshot identifier.
    pub id: &'a SnapshotId,
    /// Pre-serialised JSON for `snapshot_queries`.
    pub snapshot_queries_json: Option<Value>,
    /// Pre-serialised JSON for `snapshot_temporal_queries`.
    pub snapshot_temporal_queries_json: Option<Value>,
    /// Stringified ISO-8601 duration for `snapshot_lifetime`.
    pub snapshot_lifetime: Option<String>,
    /// Numeric encoding of [`SnapshotPriority`].
    pub snapshot_priority: i16,
    /// Endpoint URL.
    pub endpoint: Option<&'a Url>,
    /// Pre-serialised JSON for `receiver_info`.
    pub receiver_info_json: Option<Value>,
    /// Current lifecycle status.
    pub status: SnapshotStatus,
    /// Pre-serialised JSON for `queries_details`.
    pub queries_details_json: Option<Value>,
    /// Pre-serialised JSON for `temporal_queries_details`.
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

/// A single (entity_id, payload) pair destined for a snapshot's frozen
/// entity/temporal-entity table.
#[derive(Debug)]
pub struct SnapshotEntityInput<'a> {
    /// NGSI-LD entity identifier.
    pub entity_id: &'a str,
    /// Frozen payload (entity or temporal entity).
    pub data: &'a Value,
}

/// Parameters for a paginated snapshot table query.
pub(crate) struct SnapshotQueryParams<'a> {
    pub pool: &'a PgPool,
    pub table: &'a str,
    pub data_column: &'a str,
    pub snapshot_id: &'a SnapshotId,
    pub entity_type: Option<&'a str>,
    pub entity_ids: Option<&'a [String]>,
    pub limit: i64,
    pub offset: i64,
}
