//! Bounded, ordered delivery of queued temporal append intents, and the
//! mutation barrier over them.
//!
//! An Entity's history is applied in the order its writes committed (clauses
//! 4.5.7, 4.5.8 and 4.8). A claim takes only the head of an Entity's queued
//! intents and the run after it, and an application refuses a set holding an
//! intent behind an earlier one of its Entity still queued outside it.
//!
//! A pass claims a batch and applies it in one transaction. When the batch
//! does not apply, it is applied again one intent per transaction: an intent
//! that cannot be applied is rescheduled, holds back the later intents of its
//! Entity, and holds back no other Entity's. A held-back intent is released at
//! the end of the pass without counting an attempt, and follows its head once
//! the head applies.
//!
//! The barrier applies one entity's queued intents before a Temporal API
//! mutation that keeps part of its history: an attribute deletion, or an
//! instance's modification or deletion. A Temporal Evolution deletion keeps
//! none of it and removes the queued intents instead. The barrier waits only for the intents queued when it
//! starts, up to the entity's newest key at that moment: a later intent is a
//! concurrent write, and waiting for it too would never end under sustained
//! writes. It claims them whatever their retry delay, claims again while a
//! claim leaves some behind, and waits for another owner's lease on them. Its
//! bound covers the claims, their applications and the wait together: once it
//! runs out with any of them still queued, the barrier answers
//! [`TemporalError::HistoryUnsettled`], and what it applied stays applied.

use crate::{
    append_intent::TemporalIntentKey,
    error::TemporalError,
    intent_retry_policy::TemporalIntentRetryPolicy,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        temporal_intent_application::{TemporalIntentApplication, TemporalIntentApplicationOutcome},
        temporal_intent_queue::{ClaimedTemporalIntent, EntityRelease, TemporalIntentBatchSize, TemporalIntentQueue, TemporalIntentRetry},
    },
    temporal_invariant::TemporalInvariant,
    unsettled_history::UnsettledHistory,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, write_id::WriteId};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
use async_lock::Mutex;
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use std::{
    slice,
    sync::Arc,
    time::{Duration, Instant},
};

/// Outcome of one bounded temporal intent drain pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalIntentDrainOutcome {
    /// Intents applied to history and removed from the queue.
    pub acknowledged: usize,
    /// Intents retained with retry metadata after their application failed.
    pub retried: usize,
    /// Intents released unapplied, without counting an attempt, because an
    /// earlier intent of their Entity was still queued.
    pub held_back: usize,
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
/// Only the live branch queues its temporal history, so the drain delivers the
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
    /// The longest the mutation barrier spends applying the entity's queued
    /// history and waiting for another owner's lease on it.
    barrier_wait: Duration,
    /// Executor-independent gate preventing scheduled and request-driven passes from overlapping.
    execution_gate: Mutex<()>,
}

/// How one worker claims queued intents and reschedules what it cannot apply.
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
    /// Construct one bounded temporal intent delivery operation whose mutation
    /// barrier spends at most `barrier_wait` settling an entity's history.
    pub const fn new(
        queue: Arc<dyn TemporalIntentQueue>,
        application: Arc<dyn TemporalIntentApplication>,
        claim: TemporalIntentClaim,
        barrier_wait: Duration,
    ) -> TemporalIntentDrain {
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
            barrier_wait,
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
        let claimed = self.queue.claim(self.owner, self.batch_size, now + self.lease_delta()?).await?;
        Ok(self.process_claimed(&claimed, now).await?.outcome)
    }

    /// The claim lease as a calendar duration.
    fn lease_delta(&self) -> Result<TimeDelta, TemporalError> {
        TimeDelta::from_std(self.lease_duration).map_err(|error| TemporalError::from(TemporalInvariant::LeaseOutOfRange(error)))
    }

    /// Apply a claimed batch in one transaction, and when it does not apply,
    /// each of its intents in one of its own, in durable sequence order,
    /// releasing at the end the intents held back behind an earlier one.
    async fn process_claimed(&self, claimed: &[ClaimedTemporalIntent], now: DateTime<Utc>) -> Result<ClaimedPass, TemporalError> {
        let work_may_remain = usize::try_from(self.batch_size.get()).is_ok_and(|batch_size| claimed.len() == batch_size);
        let mut pass = ClaimedPass {
            outcome: TemporalIntentDrainOutcome {
                work_may_remain,
                ..TemporalIntentDrainOutcome::default()
            },
            first_failure: None,
        };
        // A batch of one is applied once, by the per-intent pass below.
        if claimed.len() > 1
            && let Ok(TemporalIntentApplicationOutcome::Applied) = self.application.apply(self.owner, claimed).await
        {
            pass.outcome.acknowledged = claimed.len();
            record_queue_outcome(
                QueueName::TemporalAppendIntents,
                QueueOutcome::Completed,
                u64::try_from(claimed.len()).unwrap_or(u64::MAX),
            );
            return Ok(pass);
        }
        let mut held_back = Vec::new();
        for claim in claimed {
            let ended = self.apply_alone(claim, now, &mut pass.outcome.stuck).await?;
            record_queue_outcome(QueueName::TemporalAppendIntents, ended.queue_outcome(), 1);
            match ended {
                IntentEnd::Applied => pass.outcome.acknowledged += 1,
                IntentEnd::Retried(failure) => {
                    pass.outcome.retried += 1;
                    pass.first_failure.get_or_insert(failure);
                }
                IntentEnd::LeaseLost => pass.outcome.lease_lost += 1,
                IntentEnd::Behind => held_back.push(claim.clone()), // clone: the release names the claims it gives back.
            }
        }
        pass.outcome.held_back = held_back.len();
        self.release_held_back(&held_back).await?;
        Ok(pass)
    }

    /// Give back the leases of the intents held back in a pass.
    ///
    /// A release that finds a lease lapsed leaves that intent with whoever
    /// claimed it since, which is where it belongs, so it is not an outcome of
    /// its own.
    async fn release_held_back(&self, held_back: &[ClaimedTemporalIntent]) -> Result<(), TemporalError> {
        if held_back.is_empty() {
            return Ok(());
        }
        match self.queue.release(self.owner, held_back).await? {
            LeaseAcknowledgement::Applied | LeaseAcknowledgement::Lost => Ok(()),
        }
    }

    /// Apply one claimed intent in a transaction of its own, rescheduling it
    /// when it does not apply and adding it to `stuck` when it has failed as
    /// often as the threshold names.
    async fn apply_alone(&self, claim: &ClaimedTemporalIntent, now: DateTime<Utc>, stuck: &mut Vec<StuckTemporalIntent>) -> Result<IntentEnd, TemporalError> {
        match self.application.apply(self.owner, slice::from_ref(claim)).await {
            Ok(TemporalIntentApplicationOutcome::Applied) => Ok(IntentEnd::Applied),
            Ok(TemporalIntentApplicationOutcome::LeaseLost) => Ok(IntentEnd::LeaseLost),
            Ok(TemporalIntentApplicationOutcome::Behind) => Ok(IntentEnd::Behind),
            Err(error) => {
                stuck.extend(self.report_if_stuck(claim, &error));
                let retry = TemporalIntentRetry {
                    eligible_at: self.retry_policy.retry_at(now, claim.attempts)?,
                    summary: TemporalIntentRetryPolicy::summary(&error),
                };
                Ok(match self.queue.retry(self.owner, claim, retry).await? {
                    LeaseAcknowledgement::Applied => IntentEnd::Retried(error),
                    LeaseAcknowledgement::Lost => IntentEnd::LeaseLost,
                })
            }
        }
    }
}

/// What one pass over a claimed batch did.
struct ClaimedPass {
    /// The counts a drain pass reports.
    outcome: TemporalIntentDrainOutcome,
    /// The failure of the first intent rescheduled, [`None`] when every
    /// intent applied or was left to another owner.
    first_failure: Option<TemporalError>,
}

/// How the delivery of one claimed intent ended.
#[derive(Debug)]
enum IntentEnd {
    /// The intent's entries are history and the intent left the queue.
    Applied,
    /// The intent did not apply, for the reason carried, and is rescheduled.
    Retried(TemporalError),
    /// The lease lapsed before the outcome could be recorded.
    LeaseLost,
    /// An earlier intent of the Entity is still queued, so the intent waits
    /// for it and is released without counting an attempt.
    Behind,
}

impl IntentEnd {
    /// The queue outcome the end is counted as.
    const fn queue_outcome(&self) -> QueueOutcome {
        match self {
            IntentEnd::Applied => QueueOutcome::Completed,
            IntentEnd::Retried(_) => QueueOutcome::Retried,
            IntentEnd::LeaseLost => QueueOutcome::LeaseLost,
            IntentEnd::Behind => QueueOutcome::HeldBack,
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
    /// Apply every intent the live branch queued for `entity_id` before the
    /// barrier started, ahead of an immediate mutation of its history.
    ///
    /// The execution gate is held around each claim and its application only,
    /// never across the wait, so this replica's drain keeps running while the
    /// barrier waits on another owner.
    async fn synchronize_live(&self, entity_id: &Identifier) -> Result<(), TemporalError> {
        let Some(through) = self.queue.last_intent_for(entity_id).await? else {
            return Ok(());
        };
        let lease = self.lease_delta()?;
        let started = Instant::now();
        loop {
            let pass = {
                let _execution = self.execution_gate.lock().await;
                let now = Utc::now();
                let claimed = self
                    .queue
                    .claim_for_entity(self.owner, entity_id, through, self.batch_size, now + lease)
                    .await?;
                self.process_claimed(&claimed, now).await?
            };
            // The retry is recorded, so the background drain keeps its delay;
            // the mutation answers with what kept the history from applying.
            // The intents after the failing one were held back, not applied.
            if let Some(failure) = pass.first_failure {
                return Err(failure);
            }
            // The bound covers applying as well as waiting, so a queue longer
            // than the bound can apply answers before the request deadline
            // instead of at it. Every pass committed, so a retry resumes
            // after the last.
            let remaining = self.barrier_wait.saturating_sub(started.elapsed());
            if pass.outcome.acknowledged > 0 && !remaining.is_zero() {
                continue;
            }
            match self.queue.await_entity_release(entity_id, through, remaining).await? {
                EntityRelease::Clear => return Ok(()),
                // Intents still claimable once the bound is spent are either
                // more than it could apply or rows another transaction still
                // locks, which the claim skips; both are answered as unsettled
                // rather than claimed again without end.
                EntityRelease::Claimable if remaining.is_zero() => return Err(TemporalError::HistoryUnsettled(UnsettledHistory::after(Duration::ZERO))),
                EntityRelease::Claimable => (),
                EntityRelease::Held { remaining_lease } => return Err(TemporalError::HistoryUnsettled(UnsettledHistory::after(remaining_lease))),
            }
        }
    }
}
