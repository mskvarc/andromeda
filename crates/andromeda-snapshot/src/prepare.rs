//! Service-side helpers that materialise snapshot JSON columns for
//! persistence, so the repository receives already-serialised blobs.

use crate::{error::SnapshotError, store::InsertSnapshotParams};
use andromeda_ngsi_ld_model::snapshot::Snapshot;
use serde_json::Value;

/// Build an [`InsertSnapshotParams`] from a domain [`Snapshot`].
///
/// Serialises every nullable JSON column up-front so the repository call is
/// infallible with respect to encoding.
///
/// # Errors
///
/// Returns [`SnapshotError::Internal`] when the receiver info, the entity query
/// execution details or the temporal query execution details cannot be
/// serialised to JSON.
pub fn to_insert_params(snapshot: &Snapshot, query_context_json: Value) -> Result<InsertSnapshotParams<'_>, SnapshotError> {
    let receiver_info_json = match snapshot.receiver_info.as_ref() {
        Some(v) => Some(to_value(v)?),
        None => None,
    };
    let queries_details_json = match snapshot.queries_details.as_ref() {
        Some(v) => Some(to_value(v)?),
        None => None,
    };
    let temporal_queries_details_json = match snapshot.temporal_queries_details.as_ref() {
        Some(v) => Some(to_value(v)?),
        None => None,
    };

    Ok(InsertSnapshotParams {
        id: &snapshot.id,
        snapshot_queries: snapshot.snapshot_queries.as_deref(),
        snapshot_temporal_queries: snapshot.snapshot_temporal_queries.as_deref(),
        query_context_json,
        snapshot_lifetime: snapshot.snapshot_lifetime.as_ref(),
        snapshot_priority: i16::from(snapshot.snapshot_priority.as_u8()),
        endpoint: snapshot.endpoint.as_ref(),
        receiver_info_json,
        status: snapshot.status,
        clone_source_id: None,
        queries_details_json,
        temporal_queries_details_json,
        created_at: snapshot.created_at,
        modified_at: snapshot.modified_at,
        expires_at: snapshot.expires_at,
        last_used_at: snapshot.last_used_at,
    })
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, SnapshotError> {
    serde_json::to_value(value).map_err(SnapshotError::Representation)
}
