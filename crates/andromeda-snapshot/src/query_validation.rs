//! Ingest-time validation of a snapshot's stored queries (clause 5.2.23).
//!
//! The canonical query resource validates member syntax while this module
//! enforces which query family may occur in each snapshot collection.

use crate::{error::SnapshotError, snapshot_validation_reason::SnapshotValidationReason};
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;

/// Reject requests that omit both collections and queries placed in the wrong collection.
///
/// # Errors
///
/// Returns [`SnapshotError::Validation`] when both collections are absent, when
/// a member of `snapshot_queries` carries a `temporalQ`, when a member of
/// `snapshot_temporal_queries` omits its `temporalQ`, and when one carries a
/// `temporalQ` naming no interval (clause 4.11, Table 5.2.21-1).
pub fn validate_snapshot_query_shapes(snapshot_queries: Option<&[Query]>, snapshot_temporal_queries: Option<&[Query]>) -> Result<(), SnapshotError> {
    if snapshot_queries.is_none() && snapshot_temporal_queries.is_none() {
        return Err(SnapshotValidationReason::QueryCollectionsMissing.into());
    }
    if let Some(queries) = snapshot_queries
        && let Some(index) = queries.iter().position(|query| query.temporal_q.is_some())
    {
        return Err(SnapshotValidationReason::TemporalQOnEntityQuery { index }.into());
    }
    if let Some(queries) = snapshot_temporal_queries
        && let Some(index) = queries.iter().position(|query| query.temporal_q.is_none())
    {
        return Err(SnapshotValidationReason::TemporalQueryWithoutTemporalQ { index }.into());
    }
    // The capture builds its window by the same rule, so an admitted query
    // always runs rather than recording a failure once the Snapshot exists.
    for (index, temporal_q) in snapshot_temporal_queries
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, query)| query.temporal_q.as_ref().map(|temporal_q| (index, temporal_q)))
    {
        TimeInterval::from_parts(Some(temporal_q.timerel), Some(temporal_q.time_at), temporal_q.end_time_at)
            .map_err(|source| SnapshotValidationReason::TemporalQWindow { index, source })?;
    }
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_malformed_q_filter_is_rejected_at_ingest() {
        // Clause 5.2.23: snapshotQueries carry a clause-4.9 `q`. A malformed one
        // must be a 400 at ingest, not a snapshot created 201 whose execution
        // later records a failure detail.
        let query = serde_json::from_value::<Query>(json!({"type": "Query", "entities": [{"type": "Vehicle"}], "q": "=="}));
        assert!(query.is_err());
    }

    #[test]
    fn a_malformed_geometry_is_rejected_at_ingest() {
        let query = serde_json::from_value::<Query>(
            json!({"type": "Query", "geoQ": {"georel": "within", "geometry": "Hexagon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}}),
        );
        assert!(query.is_err());
    }

    #[test]
    fn a_malformed_temporal_relation_is_rejected_at_ingest() {
        let query = serde_json::from_value::<Query>(
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "temporalQ": {"timerel": "during", "timeAt": "2024-01-01T00:00:00Z"}}),
        );
        assert!(query.is_err());
    }

    #[test]
    fn well_formed_queries_pass_ingest_validation() {
        let entity =
            [serde_json::from_value::<Query>(json!({"type": "Query", "entities": [{"type": "Vehicle"}], "q": "speed>50"})).expect("valid entity query")];
        let temporal = [serde_json::from_value::<Query>(
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "temporalQ": {"timerel": "after", "timeAt": "2024-01-01T00:00:00Z"}}),
        )
        .expect("valid temporal query")];
        assert!(validate_snapshot_query_shapes(Some(&entity), Some(&temporal)).is_ok());
    }

    #[test]
    fn omitted_query_collections_are_rejected_but_present_empty_collections_are_accepted() {
        assert!(validate_snapshot_query_shapes(None, None).is_err());
        assert!(validate_snapshot_query_shapes(Some(&[]), None).is_ok());
        assert!(validate_snapshot_query_shapes(None, Some(&[])).is_ok());
        assert!(validate_snapshot_query_shapes(Some(&[]), Some(&[])).is_ok());
    }

    /// Clause 4.11 and Table 5.2.21-1: `endTimeAt` comes with `between` alone,
    /// and a `between` closes after it opens.
    #[test]
    fn a_temporal_query_naming_no_window_is_rejected_at_ingest() {
        for temporal_q in [
            json!({"timerel": "between", "timeAt": "2024-01-01T00:00:00Z"}),
            json!({"timerel": "before", "timeAt": "2024-01-01T00:00:00Z", "endTimeAt": "2024-02-01T00:00:00Z"}),
            json!({"timerel": "between", "timeAt": "2024-02-01T00:00:00Z", "endTimeAt": "2024-01-01T00:00:00Z"}),
            json!({"timerel": "between", "timeAt": "2024-01-01T00:00:00Z", "endTimeAt": "2024-01-01T00:00:00Z"}),
        ] {
            let query = serde_json::from_value::<Query>(json!({"type": "Query", "entities": [{"type": "Vehicle"}], "temporalQ": temporal_q}))
                .expect("a decodable temporal query");
            let error = validate_snapshot_query_shapes(None, Some(&[query])).expect_err("no window");
            assert!(error.to_string().contains("snapshotTemporalQueries[0]"), "{error}");
        }
    }

    #[test]
    fn temporal_members_are_rejected_in_entity_collection() {
        let query = serde_json::from_value::<Query>(json!({"type": "Query", "temporalQ": {"timerel": "after", "timeAt": "2024-01-01T00:00:00Z"}}))
            .expect("valid temporal query");
        assert!(validate_snapshot_query_shapes(Some(&[query]), None).is_err());
    }
}
