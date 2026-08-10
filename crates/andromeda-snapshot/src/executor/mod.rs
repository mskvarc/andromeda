//! Snapshot query execution for NGSI-LD clause 5.16.1.4.

mod execution;
mod status;
mod translation;

use crate::{error::SnapshotError, notification::SnapshotNotificationSink, status::validate_preparing_transition, store::SnapshotStore};
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::FederationService;
use andromeda_ngsi_ld_model::snapshot::Snapshot;
use andromeda_temporal::service::TemporalService;
use std::sync::Arc;
use tracing::{debug, info};

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

    /// Execute all queries for a snapshot and record its final status.
    pub async fn execute(&self, snapshot: Snapshot) -> Result<(), SnapshotError> {
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
        validate_preparing_transition(final_status)?;

        let (query_details, temporal_query_details) = aggregate.detail_slices();
        self.snapshot_repo
            .update_status(&snapshot.id, final_status, query_details, temporal_query_details)
            .await?;
        Ok(())
    }
}

/// Outcome of one bounded preparation sweep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotPreparationOutcome {
    /// No snapshot is waiting for preparation.
    Idle,
    /// One or more preparing snapshots were completed.
    Prepared {
        /// Number completed during this bounded pass.
        count: usize,
    },
}

/// Bounded operation that executes the next persisted preparing snapshot.
pub struct SnapshotPreparation {
    store: Arc<dyn SnapshotStore>,
    executor: SnapshotExecutor,
    notification_sink: Option<Arc<dyn SnapshotNotificationSink>>,
    max_per_pass: usize,
}

impl SnapshotPreparation {
    /// Construct preparation from focused policy ports and query operations.
    pub fn new(
        store: Arc<dyn SnapshotStore>,
        entity_query: Arc<EntityQueryService>,
        temporal: Arc<TemporalService>,
        forwarding: Arc<FederationService>,
        notification_sink: Option<Arc<dyn SnapshotNotificationSink>>,
        max_per_pass: usize,
    ) -> SnapshotPreparation {
        SnapshotPreparation {
            executor: SnapshotExecutor::new(Arc::clone(&store), entity_query, temporal, forwarding),
            store,
            notification_sink,
            max_per_pass: max_per_pass.max(1),
        }
    }

    /// Execute one bounded group of pending preparations.
    pub async fn execute(&self) -> Result<SnapshotPreparationOutcome, SnapshotError> {
        let mut count = 0;
        while count < self.max_per_pass {
            let Some(snapshot) = self.store.next_preparing().await? else {
                break;
            };
            let identifier = snapshot.id.clone(); // clone: the executor consumes the snapshot while the final lookup retains its identifier.
            self.executor.execute(snapshot).await?;
            if let Some(ref sink) = self.notification_sink
                && let Some(prepared) = self.store.get(&identifier).await?
                && prepared.endpoint.is_some()
            {
                sink.deliver(&prepared).await;
            }
            count += 1;
        }
        if count == 0 {
            Ok(SnapshotPreparationOutcome::Idle)
        } else {
            Ok(SnapshotPreparationOutcome::Prepared { count })
        }
    }
}
