//! Snapshot query executor (ETSI GS CIM 009 clause 5.16.1.4).
//!
//! Executes the queries stored in a snapshot's `snapshotQueries` and
//! `snapshotTemporalQueries` members, stores the results, and transitions
//! the snapshot to its final status.

use crate::{
    domain::snapshot::{ExecutionResultDetails, ExecutionResultStatus, Snapshot, SnapshotQuery, SnapshotStatus},
    repository::snapshot::SnapshotRepository,
};
use serde_json::Value;
use sqlx::postgres::PgPool;
use tracing::{debug, error, info, warn};

/// Executes snapshot queries and stores results.
///
/// After a snapshot is created in `Preparing` status, this executor runs each
/// query, stores the resulting entities, computes per-query execution details,
/// and transitions the snapshot to a terminal status.
pub struct SnapshotExecutor {
    snapshot_repo: SnapshotRepository,
}

impl SnapshotExecutor {
    /// Create a new snapshot executor backed by the given connection pool.
    pub fn new(pool: &PgPool) -> SnapshotExecutor {
        SnapshotExecutor {
            snapshot_repo: SnapshotRepository::new(pool),
        }
    }

    /// Execute all queries for a snapshot and update its status.
    ///
    /// Each query in `snapshot_queries` is parsed as a `SnapshotQuery` and
    /// executed (currently as a no-op placeholder for entity query execution).
    /// Results are stored in `snapshot_entities` / `snapshot_temporal_entities`.
    /// The final status is computed from per-query results:
    /// - All queries succeeded with results -> `Success`
    /// - At least one succeeded with results, others failed/empty -> `Partial`
    /// - All succeeded but no results -> `Empty`
    /// - All failed -> `Failure`
    pub async fn execute(&self, snapshot: &Snapshot) {
        info!(snapshot_id = %snapshot.id, "Starting snapshot query execution");

        let mut queries_details = Vec::new();
        let mut temporal_queries_details = Vec::new();
        let mut any_success_with_results = false;
        let mut any_failure = false;
        let mut all_empty = true;

        // Execute snapshot queries (non-temporal)
        if let Some(raw_queries) = &snapshot.snapshot_queries {
            for (idx, raw_query) in raw_queries.iter().enumerate() {
                match serde_json::from_value::<SnapshotQuery>(raw_query.clone()) {
                    Ok(query) => {
                        let result = self.execute_entity_query(snapshot, &query, idx).await;
                        match result {
                            QueryResult::Success(count) => {
                                if count > 0 {
                                    any_success_with_results = true;
                                    all_empty = false;
                                }
                                queries_details.push(ExecutionResultDetails {
                                    result_status: if count > 0 {
                                        ExecutionResultStatus::Success
                                    } else {
                                        ExecutionResultStatus::Empty
                                    },
                                    problem_details: None,
                                });
                            }
                            QueryResult::Failure(err) => {
                                any_failure = true;
                                queries_details.push(ExecutionResultDetails {
                                    result_status: ExecutionResultStatus::Failure,
                                    problem_details: Some(serde_json::json!({"detail": err})),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            snapshot_id = %snapshot.id,
                            query_index = idx,
                            error = %e,
                            "Failed to parse snapshot query"
                        );
                        any_failure = true;
                        queries_details.push(ExecutionResultDetails {
                            result_status: ExecutionResultStatus::Failure,
                            problem_details: Some(serde_json::json!({"detail": format!("Failed to parse query: {}", e)})),
                        });
                    }
                }
            }
        }

        // Execute snapshot temporal queries
        if let Some(raw_queries) = &snapshot.snapshot_temporal_queries {
            for (idx, raw_query) in raw_queries.iter().enumerate() {
                match serde_json::from_value::<SnapshotQuery>(raw_query.clone()) {
                    Ok(query) => {
                        let result = self.execute_temporal_query(snapshot, &query, idx).await;
                        match result {
                            QueryResult::Success(count) => {
                                if count > 0 {
                                    any_success_with_results = true;
                                    all_empty = false;
                                }
                                temporal_queries_details.push(ExecutionResultDetails {
                                    result_status: if count > 0 {
                                        ExecutionResultStatus::Success
                                    } else {
                                        ExecutionResultStatus::Empty
                                    },
                                    problem_details: None,
                                });
                            }
                            QueryResult::Failure(err) => {
                                any_failure = true;
                                temporal_queries_details.push(ExecutionResultDetails {
                                    result_status: ExecutionResultStatus::Failure,
                                    problem_details: Some(serde_json::json!({"detail": err})),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            snapshot_id = %snapshot.id,
                            query_index = idx,
                            error = %e,
                            "Failed to parse snapshot temporal query"
                        );
                        any_failure = true;
                        temporal_queries_details.push(ExecutionResultDetails {
                            result_status: ExecutionResultStatus::Failure,
                            problem_details: Some(serde_json::json!({"detail": format!("Failed to parse temporal query: {}", e)})),
                        });
                    }
                }
            }
        }

        // Compute final status
        let final_status = compute_final_status(any_success_with_results, any_failure, all_empty);
        debug!(
            snapshot_id = %snapshot.id,
            status = %final_status,
            "Snapshot query execution completed"
        );

        // Update snapshot status
        let qd = if queries_details.is_empty() { None } else { Some(queries_details.as_slice()) };
        let tqd = if temporal_queries_details.is_empty() {
            None
        } else {
            Some(temporal_queries_details.as_slice())
        };

        if let Err(e) = self.snapshot_repo.update_status(&snapshot.id, final_status, qd, tqd).await {
            error!(
                snapshot_id = %snapshot.id,
                error = %e,
                "Failed to update snapshot status after execution"
            );
        }
    }

    /// Execute a single non-temporal snapshot query.
    ///
    /// Stores matching entities in `snapshot_entities` via the repository.
    /// Currently executes as a direct SQL query against the entities table;
    /// future iterations will integrate with the full entity query pipeline.
    async fn execute_entity_query(&self, snapshot: &Snapshot, query: &SnapshotQuery, idx: usize) -> QueryResult {
        debug!(
            snapshot_id = %snapshot.id,
            query_index = idx,
            query = ?query,
            "Executing snapshot entity query"
        );

        // Log unsupported query parameters (full pipeline integration planned)
        if let Some(ref q) = query.q {
            warn!(snapshot_id = %snapshot.id, query_index = idx, q = %q, "Snapshot query q filter not yet applied");
        }
        if let Some(ref geo) = query.geo_q {
            warn!(
                snapshot_id = %snapshot.id, query_index = idx,
                georel = %geo.georel, geometry = %geo.geometry,
                geoproperty = geo.geoproperty.as_deref().unwrap_or("location"),
                coordinates = %geo.coordinates,
                "Snapshot geoQ filter not yet applied"
            );
        }
        if let Some(ref scope) = query.scope_q {
            warn!(snapshot_id = %snapshot.id, query_index = idx, scope_q = %scope, "Snapshot scopeQ filter not yet applied");
        }
        if let Some(ref attrs) = query.attrs {
            debug!(snapshot_id = %snapshot.id, query_index = idx, attrs_count = attrs.len(), "Snapshot attrs filter not yet applied");
        }
        if let Some(ref omit) = query.omit {
            debug!(snapshot_id = %snapshot.id, query_index = idx, omit_count = omit.len(), "Snapshot omit filter not yet applied");
        }

        // Extract entity type and IDs from selectors
        let entity_type = query
            .entities
            .as_ref()
            .and_then(|entities| entities.first())
            .and_then(|selector| selector.type_.as_deref());

        let entity_ids: Option<Vec<String>> = query.entities.as_ref().and_then(|entities| {
            entities.first().and_then(|selector| {
                // Also check idPattern for logging
                if let Some(ref pattern) = selector.id_pattern {
                    warn!(
                        snapshot_id = %snapshot.id,
                        query_index = idx,
                        id_pattern = %pattern,
                        "Snapshot query contains idPattern -- not yet applied"
                    );
                }
                selector.id.clone()
            })
        });

        // Query entities from the local database
        match self.query_local_entities(entity_type, entity_ids.as_deref()).await {
            Ok(entities) => {
                let count = entities.len();
                if !entities.is_empty() {
                    let entity_pairs: Vec<(String, Value)> = entities
                        .into_iter()
                        .filter_map(|v| {
                            let id = v.get("id").and_then(Value::as_str).map(String::from)?;
                            Some((id, v))
                        })
                        .collect();
                    if let Err(e) = self.snapshot_repo.insert_entities(&snapshot.id, &entity_pairs).await {
                        error!(
                            snapshot_id = %snapshot.id,
                            query_index = idx,
                            error = %e,
                            "Failed to store snapshot entity query results"
                        );
                        return QueryResult::Failure(format!("Failed to store results: {}", e));
                    }
                }
                QueryResult::Success(count)
            }
            Err(e) => {
                warn!(
                    snapshot_id = %snapshot.id,
                    query_index = idx,
                    error = %e,
                    "Snapshot entity query failed"
                );
                QueryResult::Failure(e.to_string())
            }
        }
    }

    /// Execute a single temporal snapshot query.
    async fn execute_temporal_query(&self, snapshot: &Snapshot, query: &SnapshotQuery, idx: usize) -> QueryResult {
        debug!(
            snapshot_id = %snapshot.id,
            query_index = idx,
            "Executing snapshot temporal query"
        );

        // Temporal queries require a temporalQ section
        let temporal_q = match &query.temporal_q {
            Some(tq) => tq,
            None => return QueryResult::Failure("Temporal query missing temporalQ section".to_string()),
        };

        debug!(
            snapshot_id = %snapshot.id,
            query_index = idx,
            timerel = %temporal_q.timerel,
            time_at = %temporal_q.time_at,
            end_time_at = temporal_q.end_time_at.as_deref().unwrap_or("none"),
            last_n = ?temporal_q.last_n,
            timeproperty = temporal_q.timeproperty.as_deref().unwrap_or("observedAt"),
            aggr_methods = temporal_q.aggr_methods.as_deref().unwrap_or("none"),
            aggr_period = temporal_q.aggr_period_duration.as_deref().unwrap_or("none"),
            "Temporal query parameters (not yet fully applied)"
        );

        let entity_type = query
            .entities
            .as_ref()
            .and_then(|entities| entities.first())
            .and_then(|selector| selector.type_.as_deref());

        let entity_ids: Option<Vec<String>> = query
            .entities
            .as_ref()
            .and_then(|entities| entities.first())
            .and_then(|selector| selector.id.clone());

        // For temporal queries, we query the temporal tables
        match self.query_local_temporal_entities(entity_type, entity_ids.as_deref()).await {
            Ok(entities) => {
                let count = entities.len();
                if !entities.is_empty() {
                    let entity_pairs: Vec<(String, Value)> = entities
                        .into_iter()
                        .filter_map(|v| {
                            let id = v.get("id").and_then(Value::as_str).map(String::from)?;
                            Some((id, v))
                        })
                        .collect();
                    if let Err(e) = self.snapshot_repo.insert_temporal_entities(&snapshot.id, &entity_pairs).await {
                        error!(
                            snapshot_id = %snapshot.id,
                            query_index = idx,
                            error = %e,
                            "Failed to store snapshot temporal query results"
                        );
                        return QueryResult::Failure(format!("Failed to store results: {}", e));
                    }
                }
                QueryResult::Success(count)
            }
            Err(e) => {
                warn!(
                    snapshot_id = %snapshot.id,
                    query_index = idx,
                    error = %e,
                    "Snapshot temporal query failed"
                );
                QueryResult::Failure(e.to_string())
            }
        }
    }

    /// Query entities from the local entity table.
    ///
    /// Basic implementation that retrieves entities by type and/or ID.
    /// Full query pipeline integration (q, geoQ, scopeQ, attrs, omit)
    /// will be added in a subsequent iteration.
    async fn query_local_entities(&self, entity_type: Option<&str>, entity_ids: Option<&[String]>) -> Result<Vec<Value>, crate::core::error::AndromedaError> {
        self.snapshot_repo.query_entities_from_main(entity_type, entity_ids).await
    }

    /// Query temporal entities from the local temporal tables.
    async fn query_local_temporal_entities(
        &self,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
    ) -> Result<Vec<Value>, crate::core::error::AndromedaError> {
        self.snapshot_repo.query_temporal_entities_from_main(entity_type, entity_ids).await
    }
}

/// Internal result of executing a single query.
enum QueryResult {
    /// Query succeeded; contains the number of entities found.
    Success(usize),
    /// Query failed; contains an error description.
    Failure(String),
}

/// Compute the final snapshot status from per-query execution results.
///
/// - All queries succeeded with results -> `Success`
/// - At least one succeeded with results, others failed/empty -> `Partial`
/// - All succeeded but no results -> `Empty`
/// - All failed -> `Failure`
const fn compute_final_status(any_success_with_results: bool, any_failure: bool, all_empty: bool) -> SnapshotStatus {
    if any_success_with_results && !any_failure {
        SnapshotStatus::Success
    } else if any_success_with_results && any_failure {
        SnapshotStatus::Partial
    } else if all_empty && !any_failure {
        SnapshotStatus::Empty
    } else {
        SnapshotStatus::Failure
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn status_all_success() {
        let status = compute_final_status(true, false, false);
        pretty_assertions::assert_eq!(status, SnapshotStatus::Success);
    }

    #[test]
    fn status_all_empty() {
        let status = compute_final_status(false, false, true);
        pretty_assertions::assert_eq!(status, SnapshotStatus::Empty);
    }

    #[test]
    fn status_mixed() {
        let status = compute_final_status(true, true, false);
        pretty_assertions::assert_eq!(status, SnapshotStatus::Partial);
    }

    #[test]
    fn status_all_failure() {
        let status = compute_final_status(false, true, false);
        pretty_assertions::assert_eq!(status, SnapshotStatus::Failure);
    }

    #[test]
    fn status_failure_with_all_empty() {
        // All queries failed but also marked as empty (edge case)
        let status = compute_final_status(false, true, true);
        pretty_assertions::assert_eq!(status, SnapshotStatus::Failure);
    }
}
