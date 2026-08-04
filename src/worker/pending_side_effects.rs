//! Durable side-effect queue worker.
//!
//! Drains the `pending_side_effects` table with at-least-once semantics.
//! Each leased job is decoded into a [`BatchSideEffectJob`] and executed:
//! the temporal history rows are written in one unit of work, and any
//! buffered `ChangeEvent`s are re-emitted on the broadcast channel so
//! downstream notification consumers observe the batch. On success the row
//! is `ack`'ed (deleted); on failure the worker computes an exponential
//! backoff and `nack`s the row, and dead-letters once the attempt count
//! exceeds the configured maximum.

use crate::{
    core::error::AndromedaError,
    repository::{
        pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository, rows::LeasedSideEffect},
        uow::UnitOfWorkFactory,
    },
    service::entity::BatchSideEffectJob,
};
use andromeda_domain::notification::ChangeEvent;
use chrono::Utc;
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;

/// Tuning knobs for the pending-side-effects worker loop.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Polling cadence between lease attempts.
    pub poll_interval: Duration,
    /// Lease duration stamped on claimed rows. Must exceed the typical
    /// time to execute one job; short leases risk duplicate execution
    /// from concurrent workers picking up in-flight jobs, long leases
    /// delay recovery after a crash.
    pub lease_seconds: i32,
    /// Maximum rows leased per poll cycle.
    pub batch_size: i32,
    /// Maximum attempts before a job is dead-lettered.
    pub max_attempts: u16,
    /// Base interval for exponential retry backoff.
    pub base_backoff: Duration,
    /// Stable identifier for this worker instance. Stored on leased rows so
    /// operators can trace stuck jobs back to a specific process.
    pub worker_id: String,
}

/// Background worker that drains the durable side-effect queue.
pub struct PendingSideEffectsWorker {
    repo: PendingSideEffectsRepository,
    uow: Arc<UnitOfWorkFactory>,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    config: WorkerConfig,
}

impl PendingSideEffectsWorker {
    /// Create a new worker bound to the tenant's unit-of-work factory.
    pub const fn new(
        repo: PendingSideEffectsRepository,
        uow: Arc<UnitOfWorkFactory>,
        change_tx: Option<broadcast::Sender<ChangeEvent>>,
        config: WorkerConfig,
    ) -> PendingSideEffectsWorker {
        PendingSideEffectsWorker { repo, uow, change_tx, config }
    }

    /// Run the poll loop until the task is aborted.
    pub async fn run(self) {
        let mut ticker = tokio::time::interval(self.config.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = self.poll_once().await {
                tracing::warn!(error = %error, event = "pending_side_effects_poll_failed");
            }
        }
    }

    async fn poll_once(&self) -> Result<(), AndromedaError> {
        let leased = self
            .repo
            .lease_batch(&self.config.worker_id, self.config.batch_size, self.config.lease_seconds)
            .await?;
        for lease in leased {
            self.process_lease(lease).await;
        }
        Ok(())
    }

    async fn process_lease(&self, lease: LeasedSideEffect) {
        let id = lease.id;
        let attempts_before = lease.attempts;
        match self.execute(&lease).await {
            Ok(()) => {
                if let Err(error) = self.repo.ack(id).await {
                    tracing::warn!(error = %error, job_id = %id, event = "pending_side_effect_ack_failed");
                }
            }
            Err(error) => {
                let attempt_number = i32::from(attempts_before).saturating_add(1);
                let err_str = error.to_string();
                if u16::try_from(attempt_number).unwrap_or(u16::MAX) >= self.config.max_attempts {
                    if let Err(mark_error) = self.repo.mark_dead(id, &err_str).await {
                        tracing::warn!(error = %mark_error, job_id = %id, event = "pending_side_effect_mark_dead_failed");
                    } else {
                        tracing::warn!(job_id = %id, attempts = attempt_number, error = %err_str, event = "pending_side_effect_dead_lettered");
                    }
                } else {
                    let backoff = self.compute_backoff(attempt_number);
                    let next = Utc::now() + chrono::Duration::from_std(backoff).unwrap_or_else(|_| chrono::Duration::seconds(60));
                    if let Err(nack_error) = self.repo.nack(id, &err_str, next).await {
                        tracing::warn!(error = %nack_error, job_id = %id, event = "pending_side_effect_nack_failed");
                    } else {
                        tracing::info!(job_id = %id, attempts = attempt_number, retry_at = %next, error = %err_str, event = "pending_side_effect_retry_scheduled");
                    }
                }
            }
        }
    }

    fn compute_backoff(&self, attempt_number: i32) -> Duration {
        let shift = attempt_number.clamp(1, 16) as u32 - 1;
        let multiplier = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        self.config.base_backoff.saturating_mul(multiplier.min(u32::MAX as u64) as u32)
    }

    async fn execute(&self, lease: &LeasedSideEffect) -> Result<(), AndromedaError> {
        let persisted: PersistedBatchSideEffectJob =
            serde_json::from_value(lease.payload.clone()).map_err(|err| AndromedaError::internal(format!("invalid pending_side_effect payload: {err}")))?;
        let job: BatchSideEffectJob = persisted.try_into()?;
        let BatchSideEffectJob {
            temporal_entities,
            change_events,
        } = job;

        // One unit of work for the whole replayed job: the TEA upsert, the
        // history insert and the scope insert either all land or none do, so a
        // retry after a mid-job failure has nothing partial to heal.
        let mut uow = self.uow.begin().await?;
        uow.record_entities_batch(&temporal_entities).await?;
        uow.commit().await?;

        if let Some(tx) = &self.change_tx {
            for event in change_events {
                let _ = tx.send(event);
            }
        }

        Ok(())
    }
}
