//! Snapshot query execution for NGSI-LD clause 5.16.1.4.

mod execution;
mod status;
mod translation;

use crate::{error::SnapshotError, notification::SnapshotNotificationSink, status::validate_preparing_transition, store::SnapshotStore};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::FederationService;
use andromeda_jsonld::context_spec::{ContextSpec, CoreContextPolicy};
use andromeda_ngsi_ld_model::snapshot::Snapshot;
use andromeda_temporal::service::TemporalService;
use chrono::Utc;
use std::{sync::Arc, time::Duration};
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tracing::{debug, info};
use uuid::Uuid;

/// Executes a snapshot's typed queries and persists their frozen results.
pub struct SnapshotExecutor {
    live_target: ContextTarget,
    snapshot_repo: Arc<dyn SnapshotStore>,
    writes: Arc<dyn ContextCommitter>,
    entity_query: Arc<EntityQueryService>,
    temporal: Arc<TemporalService>,
    forwarding: Arc<FederationService>,
    capture_page_size: u32,
}

impl SnapshotExecutor {
    /// Creates a snapshot executor wired to the query services.
    pub fn new(
        live_target: ContextTarget,
        snapshot_repo: Arc<dyn SnapshotStore>,
        writes: Arc<dyn ContextCommitter>,
        entity_query: Arc<EntityQueryService>,
        temporal: Arc<TemporalService>,
        forwarding: Arc<FederationService>,
        capture_page_size: usize,
    ) -> SnapshotExecutor {
        SnapshotExecutor {
            live_target,
            snapshot_repo,
            writes,
            entity_query,
            temporal,
            forwarding,
            capture_page_size: u32::try_from(capture_page_size).unwrap_or(u32::MAX),
        }
    }

    /// Execute all queries for a snapshot and record its final status.
    pub async fn execute(&self, snapshot: Snapshot, query_context: serde_json::Value) -> Result<(), SnapshotError> {
        info!(snapshot_id = %snapshot.id, "Starting snapshot query execution");
        let target = self
            .snapshot_repo
            .target(&snapshot.id)
            .await?
            .map(ContextTarget::Snapshot)
            .ok_or_else(|| SnapshotError::internal(format!("Snapshot {} has no durable context target", snapshot.id)))?;
        let mut aggregate = status::ExecutionAggregate::default();
        let context = ContextSpec::none()
            .with_body_context(Some(query_context))
            .with_core_policy(CoreContextPolicy::Omit);

        for (index, query) in snapshot.snapshot_queries.unwrap_or_default().into_iter().enumerate() {
            let detail = self.run_entity_query(&target, &snapshot.id, query, index, &context).await;
            aggregate.push_entity(detail);
        }
        for (index, query) in snapshot.snapshot_temporal_queries.unwrap_or_default().into_iter().enumerate() {
            let detail = self.run_temporal_query(&target, &snapshot.id, query, index, &context).await;
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
    worker_id: Uuid,
    lease_duration: Duration,
}

impl SnapshotPreparation {
    /// Construct preparation from focused policy ports and query operations.
    pub fn new(
        live_target: ContextTarget,
        store: Arc<dyn SnapshotStore>,
        writes: Arc<dyn ContextCommitter>,
        entity_query: Arc<EntityQueryService>,
        temporal: Arc<TemporalService>,
        forwarding: Arc<FederationService>,
        notification_sink: Option<Arc<dyn SnapshotNotificationSink>>,
        capture_page_size: usize,
        max_per_pass: usize,
        lease_duration: Duration,
    ) -> SnapshotPreparation {
        SnapshotPreparation {
            executor: SnapshotExecutor::new(live_target, Arc::clone(&store), writes, entity_query, temporal, forwarding, capture_page_size),
            store,
            notification_sink,
            max_per_pass: max_per_pass.max(1),
            worker_id: Uuid::new_v4(),
            lease_duration,
        }
    }

    /// Execute one bounded group of pending preparations.
    pub async fn execute(&self) -> Result<SnapshotPreparationOutcome, SnapshotError> {
        let mut count = 0;
        while count < self.max_per_pass {
            let Some(job) = self.store.next_preparing(self.worker_id, self.lease_deadline()?).await? else {
                break;
            };
            let identifier = job.snapshot.id.clone(); // clone: execution consumes the job while notification retains its identifier.
            self.execute_with_lease_renewal(job).await?;
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

    /// Execute one capture while periodically extending this replica's durable claim.
    async fn execute_with_lease_renewal(&self, job: crate::store::SnapshotPreparationJob) -> Result<(), SnapshotError> {
        let identifier = job.snapshot.id.clone(); // clone: lease renewal and execution need independent ownership.
        let execution = async {
            if let Some(source_id) = job.clone_source_id {
                match self.store.promote_clone(&source_id, &job.snapshot.id).await {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        self.store
                            .update_status(&job.snapshot.id, andromeda_ngsi_ld_model::snapshot::SnapshotStatus::Failure, None, None)
                            .await?;
                        tracing::warn!(snapshot_id = %job.snapshot.id, %error, "Snapshot clone failed atomically");
                        Ok(())
                    }
                }
            } else {
                self.executor.execute(job.snapshot, job.query_context).await
            }
        };
        tokio::pin!(execution);
        let renewal_period = self.lease_duration / 2;
        let mut renewals = interval_at(Instant::now() + renewal_period, renewal_period);
        renewals.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                result = &mut execution => return result,
                _ = renewals.tick() => {
                    let retained = self.store.renew_preparation_lease(&identifier, self.worker_id, self.lease_deadline()?).await?;
                    if !retained {
                        return Err(SnapshotError::internal(format!("Preparation lease ownership was lost for {identifier}")));
                    }
                }
            }
        }
    }

    /// Compute a checked wall-clock lease deadline.
    fn lease_deadline(&self) -> Result<chrono::DateTime<Utc>, SnapshotError> {
        let lease = chrono::Duration::from_std(self.lease_duration)
            .map_err(|error| SnapshotError::internal(format!("Configured Snapshot preparation lease is invalid: {error}")))?;
        Utc::now()
            .checked_add_signed(lease)
            .ok_or_else(|| SnapshotError::internal("Snapshot preparation lease deadline exceeds the supported timestamp range"))
    }
}
