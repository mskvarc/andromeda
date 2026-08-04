//! Service-side helpers that materialise snapshot JSON columns for
//! persistence, so the repository receives already-serialised blobs.

use crate::{core::error::AndromedaError, repository::snapshot::params::InsertSnapshotParams};
use andromeda_domain::snapshot::Snapshot;
use serde_json::Value;

/// Build an [`InsertSnapshotParams`] from a domain [`Snapshot`].
///
/// Serialises every nullable JSON column up-front; any failure is surfaced as
/// an [`AndromedaError::internal`] so the repository call is infallible w.r.t.
/// encoding.
pub fn to_insert_params(snapshot: &Snapshot) -> Result<InsertSnapshotParams<'_>, AndromedaError> {
    let snapshot_queries_json = match snapshot.snapshot_queries.as_ref() {
        Some(v) => Some(to_value(v, "snapshot_queries")?),
        None => None,
    };
    let snapshot_temporal_queries_json = match snapshot.snapshot_temporal_queries.as_ref() {
        Some(v) => Some(to_value(v, "snapshot_temporal_queries")?),
        None => None,
    };
    let receiver_info_json = match snapshot.receiver_info.as_ref() {
        Some(v) => Some(to_value(v, "receiver_info")?),
        None => None,
    };
    let queries_details_json = match snapshot.queries_details.as_ref() {
        Some(v) => Some(to_value(v, "queries_details")?),
        None => None,
    };
    let temporal_queries_details_json = match snapshot.temporal_queries_details.as_ref() {
        Some(v) => Some(to_value(v, "temporal_queries_details")?),
        None => None,
    };

    Ok(InsertSnapshotParams {
        id: &snapshot.id,
        snapshot_queries_json,
        snapshot_temporal_queries_json,
        snapshot_lifetime: snapshot.snapshot_lifetime.as_ref().map(ToString::to_string),
        snapshot_priority: snapshot.snapshot_priority.as_i16(),
        endpoint: snapshot.endpoint.as_ref(),
        receiver_info_json,
        status: snapshot.status,
        queries_details_json,
        temporal_queries_details_json,
        created_at: snapshot.created_at,
        modified_at: snapshot.modified_at,
        expires_at: snapshot.expires_at,
        last_used_at: snapshot.last_used_at,
    })
}

fn to_value<T: serde::Serialize>(value: &T, column: &str) -> Result<Value, AndromedaError> {
    serde_json::to_value(value).map_err(|e| AndromedaError::internal(format!("Failed to serialize {column}: {e}")))
}
