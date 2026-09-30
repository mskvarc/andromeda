//! What a Snapshot control record that no longer reads back as a Snapshot looks
//! like.
//!
//! The broker only ever writes values it validated, so each variant is a column
//! of `snapshots` or `snapshot_preparation` holding something the broker never
//! wrote: a restore from another release, or manual surgery. Each names the
//! column and carries the parser's own rejection.

use andromeda_context_space::snapshot_key::SnapshotKeyError;
use andromeda_ngsi_ld_lexicon::identifier::IdentifierParseError;
use andromeda_ngsi_ld_model::snapshot::SnapshotPriorityParseError;
use andromeda_postgresql_codes::registry::UnknownCompactCode;
use std::num::TryFromIntError;
use thiserror::Error;

/// A stored Snapshot column that does not parse back into its domain type.
#[derive(Debug, Error)]
pub enum StoredSnapshotError {
    /// The `snapshot_key` column is not a surrogate key.
    #[error("a stored snapshot key is not a surrogate key: {0}")]
    Key(#[source] SnapshotKeyError),
    /// The `id` column is not a URI.
    #[error("a stored snapshot id is not a URI: {0}")]
    Id(#[source] IdentifierParseError),
    /// The `status` column holds a code the status registry never assigned.
    #[error("a stored snapshot status is not a known code: {0}")]
    Status(#[source] UnknownCompactCode),
    /// The `snapshot_lifetime` column is not an ISO 8601 duration.
    #[error("a stored snapshotLifetime is not an ISO 8601 duration past position {position}")]
    Lifetime {
        /// Offset of the first character the duration grammar rejected.
        position: usize,
    },
    /// The `snapshot_priority` column is outside the range a priority allows.
    #[error("a stored snapshotPriority is not a valid priority: {0}")]
    Priority(#[source] SnapshotPriorityParseError),
    /// The `snapshot_priority` column does not fit the byte a priority is.
    #[error("a stored snapshotPriority does not fit a priority: {0}")]
    PriorityWidth(#[source] TryFromIntError),
    /// The `endpoint` column is not a URL.
    #[error("a stored snapshot endpoint is not a URL: {0}")]
    Endpoint(#[source] url::ParseError),
    /// The `queries_details` column does not deserialize.
    #[error("the stored snapshot queriesDetails do not deserialize: {0}")]
    QueriesDetails(#[source] serde_json::Error),
    /// The `temporal_queries_details` column does not deserialize.
    #[error("the stored snapshot temporalQueriesDetails do not deserialize: {0}")]
    TemporalQueriesDetails(#[source] serde_json::Error),
    /// The `snapshot_queries` column does not deserialize.
    #[error("the stored snapshotQueries do not deserialize: {0}")]
    SnapshotQueries(#[source] serde_json::Error),
    /// The `snapshot_temporal_queries` column does not deserialize.
    #[error("the stored snapshotTemporalQueries do not deserialize: {0}")]
    SnapshotTemporalQueries(#[source] serde_json::Error),
    /// The `receiver_info` column does not deserialize.
    #[error("the stored snapshot receiverInfo does not deserialize: {0}")]
    ReceiverInfo(#[source] serde_json::Error),
}
