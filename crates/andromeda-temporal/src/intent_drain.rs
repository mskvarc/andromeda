//! Bounded, ordered delivery of deferred temporal append intents.

use crate::{
    error::TemporalError,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_history_sink::TemporalHistorySink,
        temporal_intent_queue::{ClaimedTemporalAppendIntent, TemporalIntentBatchSize, TemporalIntentLeaseOwner, TemporalIntentQueue, TemporalIntentRetry},
    },
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
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
    /// Whether a full claim indicates more durable work may remain.
    pub work_may_remain: bool,
}

/// Policy-owned bounded operation for ordered, idempotent intent delivery.
pub struct TemporalIntentDrain {
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
    /// Base delay multiplied by the next attempt number.
    retry_delay: Duration,
    /// Executor-independent gate preventing scheduled and request-driven passes from overlapping.
    execution_gate: Mutex<()>,
}

impl TemporalIntentDrain {
    /// Construct one bounded temporal intent delivery operation.
    pub const fn new(
        queue: Arc<dyn TemporalIntentQueue>,
        pending: Arc<dyn PendingTemporalIntentReader>,
        sink: Arc<dyn TemporalHistorySink>,
        owner: TemporalIntentLeaseOwner,
        batch_size: TemporalIntentBatchSize,
        lease_duration: Duration,
        retry_delay: Duration,
    ) -> TemporalIntentDrain {
        TemporalIntentDrain {
            queue,
            pending,
            sink,
            owner,
            batch_size,
            lease_duration,
            retry_delay,
            execution_gate: Mutex::new(()),
        }
    }

    /// Claim and process one bounded ordered batch.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<TemporalIntentDrainOutcome, TemporalError> {
        let _execution = self.execution_gate.lock().await;
        let lease_duration = TimeDelta::from_std(self.lease_duration)
            .map_err(|error| TemporalError::internal(format!("temporal intent lease duration is out of range: {error}")))?;
        let claimed = self.queue.claim(self.owner, self.batch_size, now + lease_duration).await?;
        self.process_claimed(claimed, now).await
    }

    /// Deliver a claimed batch in durable sequence order.
    async fn process_claimed(&self, claimed: Vec<ClaimedTemporalAppendIntent>, now: DateTime<Utc>) -> Result<TemporalIntentDrainOutcome, TemporalError> {
        let work_may_remain = claimed.len() == usize::try_from(self.batch_size.get()).unwrap_or(usize::MAX);
        let mut acknowledged = 0usize;
        let mut retried = 0usize;
        for claim in claimed {
            match self.sink.append(&claim.pending.intent).await {
                Ok(_) => {
                    self.queue.acknowledge(self.owner, &claim.pending).await?;
                    acknowledged += 1;
                }
                Err(error) => {
                    let retry_at = retry_at(now, self.retry_delay, claim.attempts)?;
                    self.queue
                        .retry(
                            self.owner,
                            &claim.pending,
                            TemporalIntentRetry {
                                eligible_at: retry_at,
                                summary: error.to_string(),
                            },
                        )
                        .await?;
                    retried += 1;
                }
            }
        }
        Ok(TemporalIntentDrainOutcome {
            acknowledged,
            retried,
            work_may_remain,
        })
    }
}

#[async_trait]
impl TemporalMutationBarrier for TemporalIntentDrain {
    async fn synchronize(&self, entity_id: &Uri) -> Result<(), TemporalError> {
        let _execution = self.execution_gate.lock().await;
        if self.pending.pending_for_entity(entity_id).await?.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let lease_duration = TimeDelta::from_std(self.lease_duration)
            .map_err(|error| TemporalError::internal(format!("temporal intent lease duration is out of range: {error}")))?;
        let claimed = self
            .queue
            .claim_for_entity(self.owner, entity_id, self.batch_size, now + lease_duration)
            .await?;
        if claimed.is_empty() {
            return Err(TemporalError::persistence("pending temporal history is leased by another delivery owner"));
        }
        let outcome = self.process_claimed(claimed, now).await?;
        if outcome.retried > 0 || !self.pending.pending_for_entity(entity_id).await?.is_empty() {
            return Err(TemporalError::persistence(
                "pending temporal history remains after the bounded mutation barrier",
            ));
        }
        Ok(())
    }
}

/// Calculate the next eligible claim time from the completed-attempt count.
fn retry_at(now: DateTime<Utc>, base: Duration, attempts: u32) -> Result<DateTime<Utc>, TemporalError> {
    let multiplier = attempts.saturating_add(1);
    let delay = base.saturating_mul(multiplier);
    let delay = TimeDelta::from_std(delay).map_err(|error| TemporalError::internal(format!("temporal intent retry delay is out of range: {error}")))?;
    Ok(now + delay)
}
