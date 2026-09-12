//! Snapshot query execution for NGSI-LD clause 5.16.1.4.

/// Where a snapshot capture reads the context it freezes.
pub mod capture_sources;
mod execution;
mod status;
mod translation;

use crate::{
    error::SnapshotError,
    executor::capture_sources::SnapshotCaptureSources,
    status::validate_preparing_transition,
    store::{SnapshotPreparationJob, SnapshotStore},
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::{ContextSpec, CoreContextPolicy};
use andromeda_ngsi_ld_model::snapshot::{Snapshot, SnapshotStatus};
use chrono::Utc;
use futures::{StreamExt, future::ready, stream};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tracing::{debug, info};
use uuid::Uuid;

/// Executes a snapshot's typed queries and persists their frozen results.
pub struct SnapshotExecutor {
    sources: SnapshotCaptureSources,
    snapshot_repo: Arc<dyn SnapshotStore>,
    writes: Arc<dyn ContextCommitter>,
    capture_page_size: u32,
    tenant_count_quota: Option<usize>,
    tenant_byte_quota: Option<u64>,
}

impl SnapshotExecutor {
    /// Creates a snapshot executor wired to the query services.
    pub fn new(
        sources: SnapshotCaptureSources,
        snapshot_repo: Arc<dyn SnapshotStore>,
        writes: Arc<dyn ContextCommitter>,
        capture_page_size: usize,
        tenant_count_quota: Option<usize>,
        tenant_byte_quota: Option<u64>,
    ) -> SnapshotExecutor {
        SnapshotExecutor {
            sources,
            snapshot_repo,
            writes,
            capture_page_size: u32::try_from(capture_page_size).unwrap_or(u32::MAX),
            tenant_count_quota,
            tenant_byte_quota,
        }
    }

    /// Execute all queries for a snapshot and record its final status.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Internal`] when the snapshot owns no durable
    /// staging context branch, [`SnapshotError::Validation`] when the aggregated
    /// execution status is not a permitted transition out of `preparing`, and the
    /// store's own failure when promoting the capture or enforcing the tenant
    /// quotas does not complete.
    pub async fn execute(&self, snapshot: Snapshot, query_context: serde_json::Value, claimed_by: Uuid) -> Result<(), SnapshotError> {
        info!(snapshot_id = %snapshot.id, "Starting snapshot query execution");
        let target = self
            .snapshot_repo
            .staging_target(&snapshot.id)
            .await?
            .map(ContextTarget::Snapshot)
            .ok_or_else(|| SnapshotError::internal(format!("Snapshot {} has no durable staging context target", snapshot.id)))?;
        let mut aggregate = status::ExecutionAggregate::default();
        let context = ContextSpec::none()
            .with_body_context(Some(query_context))
            .with_core_policy(CoreContextPolicy::Omit);

        let mut materialized_entities = HashSet::new();
        for (index, query) in snapshot.snapshot_queries.unwrap_or_default().into_iter().enumerate() {
            let detail = self
                .run_entity_query(&target, &snapshot.id, query, index, &context, &mut materialized_entities)
                .await;
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
            .promote_capture(&snapshot.id, claimed_by, final_status, query_details, temporal_query_details)
            .await?;
        self.snapshot_repo.enforce_quotas(self.tenant_count_quota, self.tenant_byte_quota).await?;
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
    max_per_pass: usize,
    worker_id: Uuid,
    lease_duration: Duration,
}

/// Bounded execution and resource policies for Snapshot preparation.
#[derive(Clone, Copy, Debug)]
pub struct SnapshotPreparationConfig {
    /// Maximum source page size used during capture.
    pub capture_page_size: usize,
    /// Maximum preparations executed concurrently by one worker pass.
    pub max_per_pass: usize,
    /// Renewable durable-claim lifetime.
    pub lease_duration: Duration,
    /// Optional per-tenant Snapshot count quota.
    pub tenant_count_quota: Option<usize>,
    /// Optional per-tenant Snapshot byte quota.
    pub tenant_byte_quota: Option<u64>,
}

impl SnapshotPreparation {
    /// Construct preparation from focused policy ports and query operations.
    pub fn new(
        sources: SnapshotCaptureSources,
        store: Arc<dyn SnapshotStore>,
        writes: Arc<dyn ContextCommitter>,
        config: SnapshotPreparationConfig,
    ) -> SnapshotPreparation {
        SnapshotPreparation {
            executor: SnapshotExecutor::new(
                sources,
                Arc::clone(&store), // clone: the preparation claims the jobs its executor promotes through the same store.
                writes,
                config.capture_page_size,
                config.tenant_count_quota,
                config.tenant_byte_quota,
            ),
            store,
            max_per_pass: config.max_per_pass.max(1),
            worker_id: Uuid::new_v4(),
            lease_duration: config.lease_duration,
        }
    }

    /// Execute one bounded group of pending preparations.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Internal`] when the configured lease duration
    /// cannot be turned into a wall-clock deadline, and the store's own failure
    /// when claiming the next preparing snapshot does not complete. A failure of
    /// one individual preparation is logged and leaves that claim reclaimable
    /// instead of aborting the pass.
    pub async fn execute(&self) -> Result<SnapshotPreparationOutcome, SnapshotError> {
        let mut jobs = Vec::with_capacity(self.max_per_pass);
        while jobs.len() < self.max_per_pass {
            let Some(job) = self.store.next_preparing(self.worker_id, self.lease_deadline()?).await? else {
                break;
            };
            jobs.push(job);
        }
        let count = stream::iter(jobs)
            .map(|job| async move {
                let identifier = job.snapshot.id.clone(); // clone: execution consumes the job while diagnostics retain its identifier.
                // Boxed: one capture drives the whole query pipeline, so its future is far
                // too large to hold inline for every buffered job of the pass.
                if let Err(error) = Box::pin(self.execute_with_lease_renewal(job)).await {
                    tracing::warn!(snapshot_id = %identifier, %error, "Snapshot preparation failed; its renewable claim remains reclaimable");
                    return false;
                }
                true
            })
            .buffer_unordered(self.max_per_pass)
            .filter(|completed| ready(*completed))
            .count()
            .await;
        if count == 0 {
            Ok(SnapshotPreparationOutcome::Idle)
        } else {
            Ok(SnapshotPreparationOutcome::Prepared { count })
        }
    }

    /// Execute one capture while periodically extending this replica's durable claim.
    async fn execute_with_lease_renewal(&self, job: SnapshotPreparationJob) -> Result<(), SnapshotError> {
        let identifier = job.snapshot.id.clone(); // clone: lease renewal and execution need independent ownership.
        let execution = async {
            if let Some(source_id) = job.clone_source_id {
                if let Err(error) = self.store.promote_clone(&source_id, &job.snapshot.id, self.worker_id).await {
                    self.store
                        .update_status(&job.snapshot.id, self.worker_id, SnapshotStatus::Failure, None, None)
                        .await?;
                    tracing::warn!(snapshot_id = %job.snapshot.id, %error, "Snapshot clone failed atomically");
                    return Ok(());
                }
                self.store
                    .enforce_quotas(self.executor.tenant_count_quota, self.executor.tenant_byte_quota)
                    .await?;
                Ok(())
            } else {
                // Box the large execution future so it does not inflate the
                // enclosing select's state machine (clippy::large_futures).
                Box::pin(self.executor.execute(job.snapshot, job.query_context, self.worker_id)).await
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
