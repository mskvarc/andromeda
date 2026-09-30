//! Bounded, ordered delivery of deferred temporal append intents.

use crate::{
    error::TemporalError,
    intent_retry_policy::TemporalIntentRetryPolicy,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_history_sink::TemporalHistorySink,
        temporal_intent_queue::{ClaimedTemporalAppendIntent, TemporalIntentBatchSize, TemporalIntentQueue, TemporalIntentRetry},
    },
    temporal_invariant::TemporalInvariant,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
use async_lock::Mutex;
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use std::{sync::Arc, time::Duration};

/// Outcome of one bounded temporal intent drain pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalIntentDrainOutcome {
    /// Intents acknowledged after durable sink acceptance.
    pub acknowledged: usize,
    /// Intents retained with retry metadata after sink rejection.
    pub retried: usize,
    /// Intents whose lease had lapsed before their outcome could be recorded,
    /// left to the worker that now holds them.
    pub lease_lost: usize,
    /// Whether a full claim indicates more durable work may remain.
    pub work_may_remain: bool,
}

/// Policy-owned bounded operation for ordered, idempotent intent delivery.
///
/// Only the live branch defers its temporal history, so the drain delivers the
/// live branch's queue and applies every intent to the live branch.
pub struct TemporalIntentDrain {
    /// Durable anchor queue.
    queue: Arc<dyn TemporalIntentQueue>,
    /// Read side of the anchor queue used to prove entity synchronization.
    pending: Arc<dyn PendingTemporalIntentReader>,
    /// Selected idempotent history sink.
    sink: Arc<dyn TemporalHistorySink>,
    /// Stable lease identity of this constructed drain.
    owner: LeaseHolder,
    /// Maximum intents claimed per pass.
    batch_size: TemporalIntentBatchSize,
    /// Claim lease duration.
    lease_duration: Duration,
    /// How a rejected intent is rescheduled and when it is reported as stuck.
    retry_policy: TemporalIntentRetryPolicy,
    /// Executor-independent gate preventing scheduled and request-driven passes from overlapping.
    execution_gate: Mutex<()>,
}

/// How one worker claims deferred intents and reschedules what it cannot deliver.
///
/// The four values describe a single claim: who holds it, how much of the queue
/// it takes, how long it is held before another worker may take it back, and
/// what happens to an intent the sink rejects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalIntentClaim {
    /// Identity recorded against every intent this worker claims.
    pub owner: LeaseHolder,
    /// Upper bound on the intents one pass claims.
    pub batch_size: TemporalIntentBatchSize,
    /// How long a claim is held before it may be reclaimed.
    pub lease_duration: Duration,
    /// How a rejected intent is rescheduled and when it is reported as stuck.
    pub retry_policy: TemporalIntentRetryPolicy,
}

impl TemporalIntentDrain {
    /// Construct one bounded temporal intent delivery operation.
    pub const fn new(
        queue: Arc<dyn TemporalIntentQueue>,
        pending: Arc<dyn PendingTemporalIntentReader>,
        sink: Arc<dyn TemporalHistorySink>,
        claim: TemporalIntentClaim,
    ) -> TemporalIntentDrain {
        let TemporalIntentClaim {
            owner,
            batch_size,
            lease_duration,
            retry_policy,
        } = claim;
        TemporalIntentDrain {
            queue,
            pending,
            sink,
            owner,
            batch_size,
            lease_duration,
            retry_policy,
            execution_gate: Mutex::new(()),
        }
    }

    /// Claim and process one bounded ordered batch.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Internal`] when the configured lease duration
    /// cannot be expressed as a calendar duration, and whatever the durable
    /// queue reports when a claim, acknowledgement or retry cannot be
    /// recorded. A sink that rejects an intent is not an error: the intent is
    /// rescheduled and counted in the returned outcome.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<TemporalIntentDrainOutcome, TemporalError> {
        let _execution = self.execution_gate.lock().await;
        let lease_duration = TimeDelta::from_std(self.lease_duration).map_err(|error| TemporalError::from(TemporalInvariant::LeaseOutOfRange(error)))?;
        let claimed = self.queue.claim(self.owner, self.batch_size, now + lease_duration).await?;
        self.process_claimed(claimed, now).await
    }

    /// Deliver a claimed batch in durable sequence order.
    async fn process_claimed(&self, claimed: Vec<ClaimedTemporalAppendIntent>, now: DateTime<Utc>) -> Result<TemporalIntentDrainOutcome, TemporalError> {
        let work_may_remain = claimed.len() == usize::try_from(self.batch_size.get()).unwrap_or(usize::MAX);
        let mut acknowledged = 0usize;
        let mut retried = 0usize;
        let mut lease_lost = 0usize;
        for claim in claimed {
            let (recorded, delivery) = match self.sink.append(&ContextTarget::Live, &claim.pending.intent).await {
                Ok(()) => (self.queue.acknowledge(self.owner, &claim.pending).await?, Delivery::Landed),
                Err(error) => {
                    self.report_if_stuck(&claim, &error);
                    let retry = TemporalIntentRetry {
                        eligible_at: self.retry_policy.retry_at(now, claim.attempts)?,
                        summary: TemporalIntentRetryPolicy::summary(&error),
                    };
                    (self.queue.retry(self.owner, &claim.pending, retry).await?, Delivery::Rejected)
                }
            };
            let outcome = match (recorded, delivery) {
                (LeaseAcknowledgement::Applied, Delivery::Landed) => {
                    acknowledged += 1;
                    QueueOutcome::Completed
                }
                (LeaseAcknowledgement::Applied, Delivery::Rejected) => {
                    retried += 1;
                    QueueOutcome::Retried
                }
                (LeaseAcknowledgement::Lost, Delivery::Landed | Delivery::Rejected) => {
                    lease_lost += 1;
                    QueueOutcome::LeaseLost
                }
            };
            record_queue_outcome(QueueName::TemporalAppendIntents, outcome, 1);
        }
        Ok(TemporalIntentDrainOutcome {
            acknowledged,
            retried,
            lease_lost,
            work_may_remain,
        })
    }
}

impl TemporalIntentDrain {
    /// Report an intent whose attempt count has reached the stuck threshold.
    ///
    /// There is no dead-letter for a temporal append intent: it is the durable
    /// record of history the broker accepted, so it stays claimable and is
    /// reported, never discarded.
    fn report_if_stuck(&self, claim: &ClaimedTemporalAppendIntent, error: &TemporalError) {
        // The attempt that just failed completes; the row reports the count
        // before it.
        let attempts = claim.attempts.saturating_add(1);
        if !self.retry_policy.is_stuck(attempts) {
            return;
        }
        tracing::warn!(
            intent_key = %claim.pending.key,
            write_id = %claim.pending.intent.record.write_id(),
            entity_id = %claim.pending.intent.entity_id,
            attempts,
            %error,
            event = "temporal_intent_stuck"
        );
        record_queue_outcome(QueueName::TemporalAppendIntents, QueueOutcome::Stuck, 1);
    }
}

/// Whether the sink accepted one claimed intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Delivery {
    /// The sink confirmed the intent durable.
    Landed,
    /// The sink rejected the intent and it is rescheduled.
    Rejected,
}

#[async_trait]
impl TemporalMutationBarrier for TemporalIntentDrain {
    async fn synchronize(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<(), TemporalError> {
        match target {
            ContextTarget::Live => self.synchronize_live(entity_id).await,
            // A Snapshot branch records its history in the write's own
            // transaction, so it never has a queued intent to wait for.
            ContextTarget::Snapshot(_) => Ok(()),
        }
    }
}

impl TemporalIntentDrain {
    /// Apply every intent the live branch still queues for `entity_id` before
    /// an immediate mutation of its history runs.
    async fn synchronize_live(&self, entity_id: &Identifier) -> Result<(), TemporalError> {
        let _execution = self.execution_gate.lock().await;
        if self.pending.pending_for_entity(entity_id).await?.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let lease_duration = TimeDelta::from_std(self.lease_duration).map_err(|error| TemporalError::from(TemporalInvariant::LeaseOutOfRange(error)))?;
        let claimed = self
            .queue
            .claim_for_entity(self.owner, entity_id, self.batch_size, now + lease_duration)
            .await?;
        if claimed.is_empty() {
            return Err(TemporalInvariant::PendingHistoryLeasedElsewhere.into());
        }
        let outcome = self.process_claimed(claimed, now).await?;
        if outcome.retried > 0 || outcome.lease_lost > 0 || !self.pending.pending_for_entity(entity_id).await?.is_empty() {
            return Err(TemporalInvariant::PendingHistoryRemains.into());
        }
        Ok(())
    }
}
