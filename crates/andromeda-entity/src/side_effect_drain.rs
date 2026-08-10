//! Bounded at-least-once drain for durable entity side effects.

use crate::{
    change_sink::EntityChangeSink,
    error::EntityError,
    side_effect_queue::{LeasedSideEffect, SideEffectLeaseOwner, SideEffectQueue},
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    batch_side_effect_job::BatchSideEffectJob,
    committer::ContextCommitter,
    entity_mutation::EntityMutation,
    history_record::HistoryRecord,
};
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};

/// Policy-owned retry and lease configuration for durable side effects.
#[derive(Debug)]
pub struct EntitySideEffectDrainConfig {
    /// Lease duration stamped on claimed rows.
    pub lease_seconds: i32,
    /// Maximum rows claimed by one bounded pass.
    pub batch_size: i32,
    /// Attempts permitted before dead-lettering.
    pub max_attempts: u16,
    /// Base exponential retry delay.
    pub base_backoff: Duration,
    /// Stable owner recorded on queue leases.
    pub lease_owner: SideEffectLeaseOwner,
}

/// Result of one bounded side-effect pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntitySideEffectDrainOutcome {
    /// Successfully acknowledged jobs.
    pub completed: usize,
    /// Whether a full claim means additional work may remain.
    pub work_may_remain: bool,
}

/// Durable entity side-effect delivery policy.
pub struct EntitySideEffectDrain {
    queue: Arc<dyn SideEffectQueue>,
    writes: Arc<dyn ContextCommitter>,
    changes: Option<Arc<dyn EntityChangeSink>>,
    config: EntitySideEffectDrainConfig,
}

impl EntitySideEffectDrain {
    /// Construct the drain over policy-owned ports.
    pub const fn new(
        queue: Arc<dyn SideEffectQueue>,
        writes: Arc<dyn ContextCommitter>,
        changes: Option<Arc<dyn EntityChangeSink>>,
        config: EntitySideEffectDrainConfig,
    ) -> EntitySideEffectDrain {
        EntitySideEffectDrain {
            queue,
            writes,
            changes,
            config,
        }
    }

    /// Claim and process one bounded batch at `now`.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<EntitySideEffectDrainOutcome, EntityError> {
        let leased = self
            .queue
            .lease_batch(&self.config.lease_owner, self.config.batch_size, self.config.lease_seconds)
            .await?;
        let batch_capacity = match usize::try_from(self.config.batch_size) {
            Ok(value) => value,
            Err(_) => usize::MAX,
        };
        let work_may_remain = leased.len() == batch_capacity;
        let mut completed = 0;
        for lease in leased {
            if self.process(lease, now).await? {
                completed += 1;
            }
        }
        Ok(EntitySideEffectDrainOutcome { completed, work_may_remain })
    }

    async fn process(&self, lease: LeasedSideEffect, now: DateTime<Utc>) -> Result<bool, EntityError> {
        let LeasedSideEffect { id, attempts, job } = lease;
        match self.deliver(job).await {
            Ok(()) => {
                self.queue.ack(id).await?;
                Ok(true)
            }
            Err(error) => {
                let attempt = u16::try_from(i32::from(attempts).saturating_add(1))
                    .map_err(|conversion| EntityError::internal(format!("invalid durable side-effect attempt count: {conversion}")))?;
                let detail = error.to_string();
                if attempt >= self.config.max_attempts {
                    self.queue.mark_dead(id, &detail).await?;
                } else {
                    let exponent = u32::from(attempt.saturating_sub(1)).min(15);
                    let multiplier = 1_u32 << exponent;
                    let backoff = self.config.base_backoff.saturating_mul(multiplier);
                    let retry_at = now + chrono::Duration::from_std(backoff).map_err(|conversion| EntityError::internal(conversion.to_string()))?;
                    self.queue.nack(id, &detail, retry_at).await?;
                }
                Ok(false)
            }
        }
    }

    async fn deliver(&self, job: BatchSideEffectJob) -> Result<(), EntityError> {
        let BatchSideEffectJob {
            temporal_entities,
            change_events,
        } = job;
        let writes = temporal_entities
            .into_iter()
            .map(|(identifier, expanded)| {
                AtomicWrite::builder()
                    .entity_id(identifier)
                    .entity(EntityMutation::None)
                    .history(vec![HistoryRecord::Observations(expanded)])
                    .build()
            })
            .collect();
        self.writes.commit(writes).await?;
        if let Some(ref sink) = self.changes {
            for event in change_events {
                sink.publish(event);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_context_commit::{committer::CommitFuture, error::CommitError, write_outcome::WriteOutcome};
    use futures::future::BoxFuture;
    use std::sync::Mutex;
    use uuid::Uuid;

    #[derive(Clone, Copy)]
    enum CommitDisposition {
        Applied,
        Failed,
    }

    struct FixedCommitter(CommitDisposition);

    impl ContextCommitter for FixedCommitter {
        fn commit(&self, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            Box::pin(async move {
                match self.0 {
                    CommitDisposition::Applied => Ok(writes.into_iter().map(|_| WriteOutcome::NoEntityChange).collect()),
                    CommitDisposition::Failed => Err(CommitError::Persistence {
                        detail: "scripted failure".to_string(),
                    }),
                }
            })
        }
    }

    #[derive(Default)]
    struct RecordingQueue {
        leased: Mutex<Vec<LeasedSideEffect>>,
        acknowledged: Mutex<Vec<Uuid>>,
        retried: Mutex<Vec<(Uuid, DateTime<Utc>)>>,
        dead: Mutex<Vec<Uuid>>,
    }

    impl RecordingQueue {
        fn with_lease(lease: LeasedSideEffect) -> RecordingQueue {
            RecordingQueue {
                leased: Mutex::new(vec![lease]),
                ..RecordingQueue::default()
            }
        }
    }

    impl SideEffectQueue for RecordingQueue {
        fn lease_batch<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _worker_id: &'life1 SideEffectLeaseOwner,
            _batch_size: i32,
            _lease_seconds: i32,
        ) -> BoxFuture<'async_trait, Result<Vec<LeasedSideEffect>, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let mut leased = self.leased.lock().map_err(|error| EntityError::internal(error.to_string()))?;
                Ok(std::mem::take(&mut *leased))
            })
        }

        fn ack<'life0, 'async_trait>(&'life0 self, id: Uuid) -> BoxFuture<'async_trait, Result<bool, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                self.acknowledged.lock().map_err(|error| EntityError::internal(error.to_string()))?.push(id);
                Ok(true)
            })
        }

        fn nack<'life0, 'life1, 'async_trait>(
            &'life0 self,
            id: Uuid,
            _error: &'life1 str,
            next_attempt_at: DateTime<Utc>,
        ) -> BoxFuture<'async_trait, Result<bool, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                self.retried
                    .lock()
                    .map_err(|error| EntityError::internal(error.to_string()))?
                    .push((id, next_attempt_at));
                Ok(true)
            })
        }

        fn mark_dead<'life0, 'life1, 'async_trait>(&'life0 self, id: Uuid, _error: &'life1 str) -> BoxFuture<'async_trait, Result<bool, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                self.dead.lock().map_err(|error| EntityError::internal(error.to_string()))?.push(id);
                Ok(true)
            })
        }
    }

    fn lease(attempts: i16) -> LeasedSideEffect {
        LeasedSideEffect {
            id: Uuid::new_v4(),
            attempts,
            job: BatchSideEffectJob {
                temporal_entities: Vec::new(),
                change_events: Vec::new(),
            },
        }
    }

    fn drain(queue: Arc<RecordingQueue>, disposition: CommitDisposition) -> Result<EntitySideEffectDrain, EntityError> {
        let source_id = "urn:ngsi-ld:ContextSourceIdentity:test"
            .parse()
            .map_err(|error| EntityError::internal(format!("invalid test source identifier: {error}")))?;
        Ok(EntitySideEffectDrain::new(
            queue,
            Arc::new(FixedCommitter(disposition)),
            None,
            EntitySideEffectDrainConfig {
                lease_seconds: 30,
                batch_size: 1,
                max_attempts: 3,
                base_backoff: Duration::from_secs(5),
                lease_owner: SideEffectLeaseOwner::for_tenant(&source_id, Uuid::nil()),
            },
        ))
    }

    #[tokio::test]
    async fn successful_delivery_is_acknowledged_after_commit() -> Result<(), EntityError> {
        let lease = lease(0);
        let id = lease.id;
        let queue = Arc::new(RecordingQueue::with_lease(lease));
        let outcome = drain(Arc::clone(&queue), CommitDisposition::Applied)?.execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 1);
        let acknowledged = queue.acknowledged.lock().map_err(|error| EntityError::internal(error.to_string()))?;
        pretty_assertions::assert_eq!(acknowledged.as_slice(), &[id]);
        Ok(())
    }

    #[tokio::test]
    async fn transient_failure_releases_the_lease_at_the_policy_backoff() -> Result<(), EntityError> {
        let lease = lease(0);
        let id = lease.id;
        let queue = Arc::new(RecordingQueue::with_lease(lease));
        let now = Utc::now();
        let outcome = drain(Arc::clone(&queue), CommitDisposition::Failed)?.execute(now).await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        let retried = queue.retried.lock().map_err(|error| EntityError::internal(error.to_string()))?;
        pretty_assertions::assert_eq!(retried.as_slice(), &[(id, now + chrono::Duration::seconds(5))]);
        Ok(())
    }

    #[tokio::test]
    async fn exhausted_failure_is_dead_lettered() -> Result<(), EntityError> {
        let lease = lease(2);
        let id = lease.id;
        let queue = Arc::new(RecordingQueue::with_lease(lease));
        let outcome = drain(Arc::clone(&queue), CommitDisposition::Failed)?.execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        let dead = queue.dead.lock().map_err(|error| EntityError::internal(error.to_string()))?;
        pretty_assertions::assert_eq!(dead.as_slice(), &[id]);
        Ok(())
    }
}
