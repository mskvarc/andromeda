//! Bounded, ordered delivery of deferred temporal append intents.

use crate::{
    error::TemporalError,
    intent_retry_policy::TemporalIntentRetryPolicy,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_history_sink::TemporalHistorySink,
        temporal_intent_queue::{ClaimedTemporalAppendIntent, TemporalIntentBatchSize, TemporalIntentLeaseOwner, TemporalIntentQueue, TemporalIntentRetry},
    },
};
use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
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
pub struct TemporalIntentDrain {
    /// Context branch whose queue this drain processes.
    target: ContextTarget,
    /// Durable anchor queue.
    queue: Arc<dyn TemporalIntentQueue>,
    /// Read side of the anchor queue used to prove entity synchronization.
    pending: Arc<dyn PendingTemporalIntentReader>,
    /// Selected idempotent history sink.
    sink: Arc<dyn TemporalHistorySink>,
    /// Stable lease identity of this constructed drain.
    owner: TemporalIntentLeaseOwner,
    /// Maximum intents claimed per pass.
    batch_size: TemporalIntentBatchSize,
    /// Claim lease duration.
    lease_duration: Duration,
    /// How a rejected intent is rescheduled and when it is reported as stuck.
    retry_policy: TemporalIntentRetryPolicy,
    /// Executor-independent gate preventing scheduled and request-driven passes from overlapping.
    execution_gate: Mutex<()>,
}

impl TemporalIntentDrain {
    /// Construct one bounded temporal intent delivery operation.
    pub const fn new(
        target: ContextTarget,
        queue: Arc<dyn TemporalIntentQueue>,
        pending: Arc<dyn PendingTemporalIntentReader>,
        sink: Arc<dyn TemporalHistorySink>,
        owner: TemporalIntentLeaseOwner,
        batch_size: TemporalIntentBatchSize,
        lease_duration: Duration,
        retry_policy: TemporalIntentRetryPolicy,
    ) -> TemporalIntentDrain {
        TemporalIntentDrain {
            target,
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
        let lease_duration = TimeDelta::from_std(self.lease_duration)
            .map_err(|error| TemporalError::internal(format!("temporal intent lease duration is out of range: {error}")))?;
        let claimed = self.queue.claim(&self.target, self.owner, self.batch_size, now + lease_duration).await?;
        self.process_claimed(claimed, now).await
    }

    /// Deliver a claimed batch in durable sequence order.
    async fn process_claimed(&self, claimed: Vec<ClaimedTemporalAppendIntent>, now: DateTime<Utc>) -> Result<TemporalIntentDrainOutcome, TemporalError> {
        let work_may_remain = claimed.len() == usize::try_from(self.batch_size.get()).unwrap_or(usize::MAX);
        let mut acknowledged = 0usize;
        let mut retried = 0usize;
        let mut lease_lost = 0usize;
        for claim in claimed {
            let (recorded, delivery) = match self.sink.append(&self.target, &claim.pending.intent).await {
                Ok(()) => (self.queue.acknowledge(&self.target, self.owner, &claim.pending).await?, Delivery::Landed),
                Err(error) => {
                    self.report_if_stuck(&claim, &error);
                    let retry = TemporalIntentRetry {
                        eligible_at: self.retry_policy.retry_at(now, claim.attempts)?,
                        summary: TemporalIntentRetryPolicy::summary(&error),
                    };
                    (self.queue.retry(&self.target, self.owner, &claim.pending, retry).await?, Delivery::Rejected)
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
            sequence = %claim.pending.sequence,
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
    async fn synchronize(&self, target: &ContextTarget, entity_id: &Uri) -> Result<(), TemporalError> {
        let _execution = self.execution_gate.lock().await;
        if self.pending.pending_for_entity(target, entity_id).await?.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let lease_duration = TimeDelta::from_std(self.lease_duration)
            .map_err(|error| TemporalError::internal(format!("temporal intent lease duration is out of range: {error}")))?;
        let claimed = self
            .queue
            .claim_for_entity(target, self.owner, entity_id, self.batch_size, now + lease_duration)
            .await?;
        if claimed.is_empty() {
            return Err(TemporalError::persistence("pending temporal history is leased by another delivery owner"));
        }
        let outcome = self.process_claimed(claimed, now).await?;
        if outcome.retried > 0 || outcome.lease_lost > 0 || !self.pending.pending_for_entity(target, entity_id).await?.is_empty() {
            return Err(TemporalError::persistence(
                "pending temporal history remains after the bounded mutation barrier",
            ));
        }
        Ok(())
    }
}
