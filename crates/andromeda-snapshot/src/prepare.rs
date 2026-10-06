//! Service-side assembly of the typed columns a snapshot record is persisted
//! with.

use crate::store::InsertSnapshotParams;
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_model::snapshot::Snapshot;

/// Build an [`InsertSnapshotParams`] from a domain [`Snapshot`].
///
/// The receiver info and the execution details are borrowed typed; the store
/// encodes each JSON column from its type.
#[must_use]
pub fn to_insert_params(snapshot: &Snapshot, query_context: JsonLiteral) -> InsertSnapshotParams<'_> {
    InsertSnapshotParams {
        id: &snapshot.id,
        snapshot_queries: snapshot.snapshot_queries.as_deref(),
        snapshot_temporal_queries: snapshot.snapshot_temporal_queries.as_deref(),
        query_context,
        snapshot_lifetime: snapshot.snapshot_lifetime.as_ref(),
        snapshot_priority: i16::from(snapshot.snapshot_priority.as_u8()),
        endpoint: snapshot.endpoint.as_ref(),
        receiver_info: snapshot.receiver_info.as_deref(),
        status: snapshot.status,
        clone_source_id: None,
        queries_details: snapshot.queries_details.as_deref(),
        temporal_queries_details: snapshot.temporal_queries_details.as_deref(),
        created_at: snapshot.created_at,
        modified_at: snapshot.modified_at,
        expires_at: snapshot.expires_at,
        last_used_at: snapshot.last_used_at,
    }
}
