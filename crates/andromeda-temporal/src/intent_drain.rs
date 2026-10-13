//! Bounded, ordered delivery of deferred temporal append intents.
//!
//! A pass claims a batch and applies it in one transaction. When the batch
//! does not apply, it is applied again one intent per transaction, so one
//! intent that cannot be applied holds back none of the others and is the only
//! one rescheduled.

use crate::{
    append_intent::TemporalIntentKey,
    error::TemporalError,
    intent_retry_policy::TemporalIntentRetryPolicy,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        temporal_intent_application::TemporalIntentApplication,
        temporal_intent_queue::{ClaimedTemporalIntent, TemporalIntentBatchSize, TemporalIntentQueue, TemporalIntentRetry},
    },
    temporal_invariant::TemporalInvariant,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, write_id::WriteId};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
use async_lock::Mutex;
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use std::{slice, sync::Arc, time::Duration};

/// Outcome of one bounded temporal intent drain pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalIntentDrainOutcome {
    /// Intents applied to history and removed from the queue.
    pub acknowledged: usize,
    /// Intents retained with retry metadata after their application failed.
    pub retried: usize,
    /// Intents whose lease had lapsed before their outcome could be recorded,
    /// left to the worker that now holds them.
    pub lease_lost: usize,
    /// Whether a full claim indicates more durable work may remain.
    pub work_may_remain: bool,
    /// The retried intents whose attempts reached the stuck threshold.
    pub stuck: Vec<StuckTemporalIntent>,
}

/// An intent that has failed as many times as the operator threshold names.
///
/// There is no dead-letter for a temporal append intent: it is the durable
/// record of history the broker accepted, so it stays claimable and is
/// reported by what names it, never discarded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StuckTemporalIntent {
    /// The intent's queue key.
    pub key: TemporalIntentKey,
    /// The write the intent belongs to.
    pub write_id: WriteId,
    /// The Entity whose history the intent holds.
    pub entity_id: Identifier,
    /// Completed attempts, the one that just failed included.
    pub attempts: u32,
}

/// Policy-owned bounded operation for ordered, idempotent intent delivery.
///
/// Only the live branch defers its temporal history, so the drain delivers the
/// live branch's queue and applies every intent to the live branch.
pub struct TemporalIntentDrain {
    /// Durable anchor queue.
    queue: Arc<dyn TemporalIntentQueue>,
    /// Application of claimed intents to history.
    application: Arc<dyn TemporalIntentApplication>,
    /// Stable lease identity of this constructed drain.
    owner: LeaseHolder,
    /// Maximum intents claimed per pass.
    batch_size: TemporalIntentBatchSize,
    /// Claim lease duration.
    lease_duration: Duration,
    /// How a failed intent is rescheduled and when it is reported as stuck.
    retry_policy: TemporalIntentRetryPolicy,
    /// Executor-independent gate preventing scheduled and request-driven passes from overlapping.
    execution_gate: Mutex<()>,
}

/// How one worker claims deferred intents and reschedules what it cannot apply.
///
/// The four values describe a single claim: who holds it, how much of the queue
/// it takes, how long it is held before another worker may take it back, and
/// what happens to an intent that cannot be applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalIntentClaim {
    /// Identity recorded against every intent this worker claims.
    pub owner: LeaseHolder,
    /// Upper bound on the intents one pass claims.
    pub batch_size: TemporalIntentBatchSize,
    /// How long a claim is held before it may be reclaimed.
    pub lease_duration: Duration,
    /// How a failed intent is rescheduled and when it is reported as stuck.
    pub retry_policy: TemporalIntentRetryPolicy,
}

impl TemporalIntentDrain {
    /// Construct one bounded temporal intent delivery operation.
    pub const fn new(queue: Arc<dyn TemporalIntentQueue>, application: Arc<dyn TemporalIntentApplication>, claim: TemporalIntentClaim) -> TemporalIntentDrain {
        let TemporalIntentClaim {
            owner,
            batch_size,
            lease_duration,
            retry_policy,
        } = claim;
        TemporalIntentDrain {
            queue,
            application,
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
    /// queue reports when a claim or retry cannot be recorded. An intent that
    /// cannot be applied is not an error: it is rescheduled and counted in the
    /// returned outcome.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<TemporalIntentDrainOutcome, TemporalError> {
        let _execution = self.execution_gate.lock().await;
        let lease_duration = TimeDelta::from_std(self.lease_duration).map_err(|error| TemporalError::from(TemporalInvariant::LeaseOutOfRange(error)))?;
        let claimed = self.queue.claim(self.owner, self.batch_size, now + lease_duration).await?;
        self.process_claimed(&claimed, now).await
    }

    /// Apply a claimed batch in one transaction, and when it does not apply,
    /// each of its intents in one of its own, in durable sequence order.
    async fn process_claimed(&self, claimed: &[ClaimedTemporalIntent], now: DateTime<Utc>) -> Result<TemporalIntentDrainOutcome, TemporalError> {
        let work_may_remain = claimed.len() == usize::try_from(self.batch_size.get()).unwrap_or(usize::MAX);
        let mut outcome = TemporalIntentDrainOutcome {
            work_may_remain,
            ..TemporalIntentDrainOutcome::default()
        };
        // A batch of one is applied once, by the per-intent pass below.
        if claimed.len() > 1
            && let Ok(LeaseAcknowledgement::Applied) = self.application.apply(self.owner, claimed).await
        {
            outcome.acknowledged = claimed.len();
            record_queue_outcome(
                QueueName::TemporalAppendIntents,
                QueueOutcome::Completed,
                u64::try_from(claimed.len()).unwrap_or(u64::MAX),
            );
            return Ok(outcome);
        }
        for claim in claimed {
            let ended = self.apply_alone(claim, now, &mut outcome.stuck).await?;
            match ended {
                IntentEnd::Applied => outcome.acknowledged += 1,
                IntentEnd::Retried => outcome.retried += 1,
                IntentEnd::LeaseLost => outcome.lease_lost += 1,
            }
            record_queue_outcome(QueueName::TemporalAppendIntents, ended.queue_outcome(), 1);
        }
        Ok(outcome)
    }

    /// Apply one claimed intent in a transaction of its own, rescheduling it
    /// when it does not apply and adding it to `stuck` when it has failed as
    /// often as the threshold names.
    async fn apply_alone(&self, claim: &ClaimedTemporalIntent, now: DateTime<Utc>, stuck: &mut Vec<StuckTemporalIntent>) -> Result<IntentEnd, TemporalError> {
        match self.application.apply(self.owner, slice::from_ref(claim)).await {
            Ok(LeaseAcknowledgement::Applied) => Ok(IntentEnd::Applied),
            Ok(LeaseAcknowledgement::Lost) => Ok(IntentEnd::LeaseLost),
            Err(error) => {
                stuck.extend(self.report_if_stuck(claim, &error));
                let retry = TemporalIntentRetry {
                    eligible_at: self.retry_policy.retry_at(now, claim.attempts)?,
                    summary: TemporalIntentRetryPolicy::summary(&error),
                };
                Ok(match self.queue.retry(self.owner, claim, retry).await? {
                    LeaseAcknowledgement::Applied => IntentEnd::Retried,
                    LeaseAcknowledgement::Lost => IntentEnd::LeaseLost,
                })
            }
        }
    }
}

/// How the delivery of one claimed intent ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IntentEnd {
    /// The intent's entries are history and the intent left the queue.
    Applied,
    /// The intent did not apply and is rescheduled.
    Retried,
    /// The lease lapsed before the outcome could be recorded.
    LeaseLost,
}

impl IntentEnd {
    /// The queue outcome the end is counted as.
    const fn queue_outcome(self) -> QueueOutcome {
        match self {
            IntentEnd::Applied => QueueOutcome::Completed,
            IntentEnd::Retried => QueueOutcome::Retried,
            IntentEnd::LeaseLost => QueueOutcome::LeaseLost,
        }
    }
}

impl TemporalIntentDrain {
    /// Report an intent whose attempt count has reached the stuck threshold.
    fn report_if_stuck(&self, claim: &ClaimedTemporalIntent, error: &TemporalError) -> Option<StuckTemporalIntent> {
        // The attempt that just failed completes; the row reports the count
        // before it.
        let attempts = claim.attempts.saturating_add(1);
        if !self.retry_policy.is_stuck(attempts) {
            return None;
        }
        tracing::warn!(
            intent_key = %claim.key,
            write_id = %claim.write_id,
            entity_id = %claim.entity_id,
            attempts,
            %error,
            event = "temporal_intent_stuck"
        );
        record_queue_outcome(QueueName::TemporalAppendIntents, QueueOutcome::Stuck, 1);
        Some(StuckTemporalIntent {
            key: claim.key,
            write_id: claim.write_id,
            entity_id: claim.entity_id.clone(), // clone: the report owns the entity it names.
            attempts,
        })
    }
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
        if !self.queue.holds_intent_for(entity_id).await? {
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
        let outcome = self.process_claimed(&claimed, now).await?;
        if outcome.retried > 0 || outcome.lease_lost > 0 || self.queue.holds_intent_for(entity_id).await? {
            return Err(TemporalInvariant::PendingHistoryRemains.into());
        }
        Ok(())
    }
}
