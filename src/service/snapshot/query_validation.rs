//! Ingest-time validation of a snapshot's stored queries (clause 5.2.23).
//!
//! A `Snapshot` persists its queries as raw JSON and the executor parses them at
//! run time. Without an ingest check a malformed `q`, `timerel` or geometry is
//! accepted with 201 and only surfaces later as an execution `failure` detail.
//! Clause 5.16.1 has the broker reject a request it cannot act on, so the query
//! shapes are parsed here -- into the same typed [`SnapshotQuery`] the executor
//! demands -- before persistence, and a malformed one is a 400.

use crate::core::error::AndromedaError;
use andromeda_ngsi_ld_model::snapshot_query::SnapshotQuery;
use serde::Deserialize;
use serde_json::Value;

/// Reject a snapshot whose stored queries do not parse into the typed shapes the
/// executor will later demand.
///
/// Each element of `snapshotQueries` and `snapshotTemporalQueries` is parsed as
/// a [`SnapshotQuery`], exactly as the executor parses it at run time. That
/// deserialisation is where the `q` filter (clause 4.9), the geometry
/// (clause 4.10) and the temporal relation (clause 4.11) are validated, so a
/// success here means the executor can act on the query.
pub fn validate_snapshot_query_shapes(snapshot_queries: Option<&[Value]>, snapshot_temporal_queries: Option<&[Value]>) -> Result<(), AndromedaError> {
    if let Some(queries) = snapshot_queries {
        for (index, raw) in queries.iter().enumerate() {
            SnapshotQuery::deserialize(raw).map_err(|error| AndromedaError::snapshot_validation(format!("Invalid snapshotQueries[{index}]: {error}")))?;
        }
    }
    if let Some(queries) = snapshot_temporal_queries {
        for (index, raw) in queries.iter().enumerate() {
            SnapshotQuery::deserialize(raw).map_err(|error| AndromedaError::snapshot_validation(format!("Invalid snapshotTemporalQueries[{index}]: {error}")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_malformed_q_filter_is_rejected_at_ingest() {
        // Clause 5.2.23: snapshotQueries carry a clause-4.9 `q`. A malformed one
        // must be a 400 at ingest, not a snapshot created 201 whose execution
        // later records a failure detail.
        let queries = [json!({"entities": [{"type": "Vehicle"}], "q": "=="})];
        assert!(validate_snapshot_query_shapes(Some(&queries), None).is_err());
    }

    #[test]
    fn a_malformed_geometry_is_rejected_at_ingest() {
        let queries = [json!({"geoQ": {"georel": "within", "geometry": "Hexagon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}})];
        assert!(validate_snapshot_query_shapes(Some(&queries), None).is_err());
    }

    #[test]
    fn a_malformed_temporal_relation_is_rejected_at_ingest() {
        let queries = [json!({"entities": [{"type": "Vehicle"}], "temporalQ": {"timerel": "during", "timeAt": "2024-01-01T00:00:00Z"}})];
        assert!(validate_snapshot_query_shapes(None, Some(&queries)).is_err());
    }

    #[test]
    fn well_formed_queries_pass_ingest_validation() {
        let entity = [json!({"entities": [{"type": "Vehicle"}], "q": "speed>50"})];
        let temporal = [json!({"entities": [{"type": "Vehicle"}], "temporalQ": {"timerel": "after", "timeAt": "2024-01-01T00:00:00Z"}})];
        assert!(validate_snapshot_query_shapes(Some(&entity), Some(&temporal)).is_ok());
    }
}
