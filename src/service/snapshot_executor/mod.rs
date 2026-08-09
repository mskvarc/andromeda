//! Snapshot query execution for NGSI-LD clause 5.16.1.4.

mod execution;
mod status;
mod translation;

use crate::service::{snapshot::store::SnapshotStore, snapshot_status::validate_preparing_transition, temporal::TemporalService};
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::FederationService;
use andromeda_ngsi_ld_model::snapshot::Snapshot;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Executes a snapshot's typed queries and persists their frozen results.
pub struct SnapshotExecutor {
    snapshot_repo: Arc<dyn SnapshotStore>,
    entity_query: Arc<EntityQueryService>,
    temporal: Arc<TemporalService>,
    forwarding: Arc<FederationService>,
}

impl SnapshotExecutor {
    /// Creates a snapshot executor wired to the query services.
    pub fn new(
        snapshot_repo: Arc<dyn SnapshotStore>,
        entity_query: Arc<EntityQueryService>,
        temporal: Arc<TemporalService>,
        forwarding: Arc<FederationService>,
    ) -> SnapshotExecutor {
        SnapshotExecutor {
            snapshot_repo,
            entity_query,
            temporal,
            forwarding,
        }
    }

    /// Executes all queries for a snapshot and records its final status.
    pub async fn execute(&self, snapshot: Snapshot) {
        info!(snapshot_id = %snapshot.id, "Starting snapshot query execution");
        let mut aggregate = status::ExecutionAggregate::default();

        for (index, query) in snapshot.snapshot_queries.unwrap_or_default().into_iter().enumerate() {
            let detail = self.run_entity_query(&snapshot.id, query, index).await;
            aggregate.push_entity(detail);
        }
        for (index, query) in snapshot.snapshot_temporal_queries.unwrap_or_default().into_iter().enumerate() {
            let detail = self.run_temporal_query(&snapshot.id, query, index).await;
            aggregate.push_temporal(detail);
        }

        let final_status = aggregate.final_status();
        debug!(snapshot_id = %snapshot.id, status = %final_status, "Snapshot query execution completed");
        if let Err(error) = validate_preparing_transition(final_status) {
            error!(snapshot_id = %snapshot.id, error = %error, "Invalid status transition requested");
            return;
        }

        let (query_details, temporal_query_details) = aggregate.detail_slices();
        if let Err(error) = self
            .snapshot_repo
            .update_status(&snapshot.id, final_status, query_details, temporal_query_details)
            .await
        {
            error!(snapshot_id = %snapshot.id, error = %error, "Failed to update snapshot status after execution");
        }
    }
}
