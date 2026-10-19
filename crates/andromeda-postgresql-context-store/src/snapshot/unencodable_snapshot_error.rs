//! A Snapshot document the store could not write as the JSON text its column
//! holds.
//!
//! Each document is a typed value the broker built, so a serializer refusing it
//! is the store failing to encode its own value: the write fails as
//! unencodable rather than storing the column as SQL `NULL`, which would read
//! back as a Snapshot without that document. Each variant names the column and
//! carries the serializer's own rejection.

use thiserror::Error;

/// A Snapshot document that does not serialize to JSON.
#[derive(Debug, Error)]
pub enum UnencodableSnapshotError {
    /// The `snapshot_queries` document does not serialize.
    #[error("the snapshotQueries do not serialize: {0}")]
    SnapshotQueries(#[source] serde_json::Error),
    /// The `snapshot_temporal_queries` document does not serialize.
    #[error("the snapshotTemporalQueries do not serialize: {0}")]
    SnapshotTemporalQueries(#[source] serde_json::Error),
    /// The `query_context` document does not serialize.
    #[error("the snapshot query context does not serialize: {0}")]
    QueryContext(#[source] serde_json::Error),
    /// The `receiver_info` document does not serialize.
    #[error("the snapshot receiverInfo does not serialize: {0}")]
    ReceiverInfo(#[source] serde_json::Error),
    /// The `queries_details` document does not serialize.
    #[error("the snapshot queriesDetails do not serialize: {0}")]
    QueriesDetails(#[source] serde_json::Error),
    /// The `temporal_queries_details` document does not serialize.
    #[error("the snapshot temporalQueriesDetails do not serialize: {0}")]
    TemporalQueriesDetails(#[source] serde_json::Error),
    /// The clause 5.3.4 notification queued in the status outbox does not
    /// serialize.
    #[error("the snapshot notification does not serialize: {0}")]
    Notification(#[source] serde_json::Error),
}
