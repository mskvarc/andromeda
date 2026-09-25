//! Bounded at-least-once drain for durable entity side effects.
//!
//! Delivery replays through
//! [`ContextCommitter::commit_replayed`](andromeda_context_commit::committer::ContextCommitter::commit_replayed)
//! under the job's stored write identity and ordinal base, which is what makes
//! a crash-retried temporal write and notification event idempotent: the
//! replay's rows collide with the first attempt's on their row-level `ON
//! CONFLICT ... DO NOTHING` constraints. The in-memory [`EntityChangeSink`]
//! republication has no equivalent row-level guard -- it is a local fan-out,
//! not a durable insert -- so it is held back until the acknowledgement is
//! confirmed [`LeaseAcknowledgement::Applied`]: a job whose commit succeeded but
//! whose acknowledgement was then lost to a lease reclaim publishes nothing
//! here, and the reclaiming worker performs the single publication. The change
//! events therefore reach the live channel exactly once per committed job, while
//! the durable temporal and notification consequences keep their at-least-once,
//! idempotent-on-replay contract.

use crate::{
    change_sink::EntityChangeSink,
    committed_side_effect::CommittedSideEffect,
    error::EntityError,
    side_effect_queue::{LeasedSideEffect, SideEffectQueue},
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    batch_side_effect_job::BatchSideEffectJob,
    commit_replay::CommitReplay,
    committer::ContextCommitter,
    context_change_event::ContextChangeEvent,
    entity_mutation::EntityMutation,
    history_record::HistoryRecord,
    lease_acknowledgement::LeaseAcknowledgement,
    lease_holder::LeaseHolder,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
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
    /// The worker identity recorded on queue leases.
    pub lease_holder: LeaseHolder,
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
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the queue cannot be leased or
    /// a job's outcome cannot be recorded, and whatever the replayed commit
    /// reported for the first job that could not be applied.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<EntitySideEffectDrainOutcome, EntityError> {
        let leased = self
            .queue
            .lease_batch(&self.config.lease_holder, self.config.batch_size, self.config.lease_seconds)
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
        let LeasedSideEffect {
            id,
            attempts,
            job,
            record,
            consequence_base,
        } = lease;
        match self.deliver(job, record, consequence_base).await {
            Ok(committed) => match self.queue.ack(id, &self.config.lease_holder).await? {
                LeaseAcknowledgement::Applied => {
                    // The commit is durably owned by this worker, so its live
                    // change events are published exactly here, once.
                    self.publish_changes(committed);
                    record_queue_outcome(QueueName::PendingSideEffects, QueueOutcome::Completed, 1);
                    Ok(true)
                }
                LeaseAcknowledgement::Lost => {
                    // Another worker reclaimed the lease between the commit and
                    // this acknowledgement and will redeliver the job; it owns
                    // the single publication now, so nothing is published here.
                    // The durable temporal and notification rows are idempotent
                    // on replay, so its redelivery adds no duplicate there.
                    record_queue_outcome(QueueName::PendingSideEffects, QueueOutcome::LeaseLost, 1);
                    tracing::warn!(job_id = %id, lease_holder = %self.config.lease_holder, event = "pending_side_effect_lease_lost_on_ack");
                    Ok(false)
                }
            },
            Err(error) => {
                let attempt = u16::try_from(attempts.saturating_add(1))
                    .map_err(|conversion| EntityError::internal(format!("invalid durable side-effect attempt count: {conversion}")))?;
                let detail = error.to_string();
                let (acknowledgement, outcome) = if attempt >= self.config.max_attempts {
                    (self.queue.mark_dead(id, &self.config.lease_holder, &detail).await?, QueueOutcome::Exhausted)
                } else {
                    let exponent = u32::from(attempt.saturating_sub(1)).min(15);
                    let multiplier = 1_u32 << exponent;
                    let backoff = self.config.base_backoff.saturating_mul(multiplier);
                    let retry_at = now + chrono::Duration::from_std(backoff).map_err(|conversion| EntityError::internal(conversion.to_string()))?;
                    (self.queue.nack(id, &self.config.lease_holder, &detail, retry_at).await?, QueueOutcome::Retried)
                };
                let outcome = match acknowledgement {
                    LeaseAcknowledgement::Applied => outcome,
                    LeaseAcknowledgement::Lost => {
                        tracing::warn!(job_id = %id, lease_holder = %self.config.lease_holder, event = "pending_side_effect_lease_lost_on_retry");
                        QueueOutcome::LeaseLost
                    }
                };
                record_queue_outcome(QueueName::PendingSideEffects, outcome, 1);
                Ok(false)
            }
        }
    }

    /// Replay the job's durable consequences and hand back the live change
    /// events to publish only once the acknowledgement is confirmed applied.
    async fn deliver(&self, job: BatchSideEffectJob, record: WriteRecord, consequence_base: WriteOrdinal) -> Result<CommittedSideEffect, EntityError> {
        let BatchSideEffectJob {
            target,
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
        self.writes
            .commit_replayed(&target, CommitReplay::new(record, consequence_base), writes)
            .await?;
        Ok(CommittedSideEffect { target, change_events })
    }

    /// Publish a committed side effect's change events to the in-memory sink.
    ///
    /// Called only on the acknowledged-applied branch, so a delivery whose lease
    /// was reclaimed publishes nothing. This mirrors the online inline path,
    /// which never republishes a change it did not commit under its own lease,
    /// and is why the best-effort live channel carries one publication per
    /// committed job even though the durable queue is at-least-once.
    fn publish_changes(&self, committed: CommittedSideEffect) {
        let Some(ref sink) = self.changes else {
            return;
        };
        let CommittedSideEffect { target, change_events } = committed;
        for event in change_events {
            sink.publish(ContextChangeEvent {
                target: target.clone(), // clone: every queued consumer owns the immutable branch selection.
                event,
            });
        }
    }

    /// Remove dead-lettered jobs created before `cutoff`, returning how many
    /// were removed.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the queue could not be swept.
    pub async fn purge_dead(&self, cutoff: DateTime<Utc>) -> Result<u64, EntityError> {
        self.queue.purge_dead_before(cutoff).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_context_commit::{
        change_event::ChangeEvent,
        committer::CommitFuture,
        error::CommitError,
        lease_holder::LeaseHolder,
        side_effect_job_id::SideEffectJobId,
        write_id::WriteId,
        write_outcome::WriteOutcome,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use chrono::TimeZone;
    use futures::future::BoxFuture;
    use std::{mem::take, sync::Mutex};
    use uuid::Uuid;

    /// Records every change event published to the sink, so a test can assert
    /// how many publications one delivery produced.
    #[derive(Default)]
    struct RecordingSink {
        published: Mutex<Vec<ContextChangeEvent>>,
    }

    impl RecordingSink {
        fn count(&self) -> Result<usize, EntityError> {
            Ok(self.published.lock().map_err(|error| EntityError::internal(error.to_string()))?.len())
        }
    }

    impl EntityChangeSink for RecordingSink {
        fn publish(&self, event: ContextChangeEvent) {
            if let Ok(mut published) = self.published.lock() {
                published.push(event);
            }
        }
    }

    #[derive(Clone, Copy)]
    enum CommitDisposition {
        Applied,
        Failed,
    }

    /// Records every replay continuation `commit_replayed` was invoked with,
    /// so a test can assert delivery resumed the job's stored identity and
    /// ordinal base rather than minting a fresh one.
    struct FixedCommitter {
        disposition: CommitDisposition,
        replays: Mutex<Vec<CommitReplay>>,
    }

    impl FixedCommitter {
        fn new(disposition: CommitDisposition) -> FixedCommitter {
            FixedCommitter {
                disposition,
                replays: Mutex::new(Vec::new()),
            }
        }
    }

    impl ContextCommitter for FixedCommitter {
        fn commit(&self, _target: &ContextTarget, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            Box::pin(async move {
                Err(CommitError::Invariant {
                    detail: "the side-effect drain must replay, never mint a fresh commit".to_owned(),
                })
            })
        }

        fn commit_replayed(&self, _target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            Box::pin(async move {
                self.replays
                    .lock()
                    .map_err(|error| CommitError::Invariant { detail: error.to_string() })?
                    .push(replay);
                match self.disposition {
                    CommitDisposition::Applied => Ok(writes.into_iter().map(|_| WriteOutcome::NoEntityChange).collect()),
                    CommitDisposition::Failed => Err(CommitError::Persistence {
                        detail: "scripted failure".to_string(),
                    }),
                }
            })
        }
    }

    struct RecordingQueue {
        leased: Mutex<Vec<LeasedSideEffect>>,
        acknowledged: Mutex<Vec<SideEffectJobId>>,
        retried: Mutex<Vec<(SideEffectJobId, DateTime<Utc>)>>,
        dead: Mutex<Vec<SideEffectJobId>>,
        /// The outcome every guarded acknowledgement reports, as if another
        /// worker had already reclaimed the lease.
        lease_outcome: LeaseAcknowledgement,
    }

    // Manual: `lease_outcome`'s type is defined in another crate, so this test
    // double cannot derive `Default` for it and must state the applied default
    // explicitly.
    impl Default for RecordingQueue {
        fn default() -> RecordingQueue {
            RecordingQueue {
                leased: Mutex::new(Vec::new()),
                acknowledged: Mutex::new(Vec::new()),
                retried: Mutex::new(Vec::new()),
                dead: Mutex::new(Vec::new()),
                lease_outcome: LeaseAcknowledgement::Applied,
            }
        }
    }

    impl RecordingQueue {
        fn with_lease(lease: LeasedSideEffect) -> RecordingQueue {
            RecordingQueue {
                leased: Mutex::new(vec![lease]),
                ..RecordingQueue::default()
            }
        }

        fn with_lease_lost_on_acknowledgement(lease: LeasedSideEffect) -> RecordingQueue {
            RecordingQueue {
                leased: Mutex::new(vec![lease]),
                lease_outcome: LeaseAcknowledgement::Lost,
                ..RecordingQueue::default()
            }
        }
    }

    impl SideEffectQueue for RecordingQueue {
        fn lease_batch<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _worker_id: &'life1 LeaseHolder,
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
                Ok(take(&mut *leased))
            })
        }

        fn ack<'life0, 'life1, 'async_trait>(
            &'life0 self,
            id: SideEffectJobId,
            _owner: &'life1 LeaseHolder,
        ) -> BoxFuture<'async_trait, Result<LeaseAcknowledgement, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                if self.lease_outcome == LeaseAcknowledgement::Applied {
                    self.acknowledged.lock().map_err(|error| EntityError::internal(error.to_string()))?.push(id);
                }
                Ok(self.lease_outcome)
            })
        }

        fn nack<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            id: SideEffectJobId,
            _owner: &'life1 LeaseHolder,
            _error: &'life2 str,
            next_attempt_at: DateTime<Utc>,
        ) -> BoxFuture<'async_trait, Result<LeaseAcknowledgement, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                if self.lease_outcome == LeaseAcknowledgement::Applied {
                    self.retried
                        .lock()
                        .map_err(|error| EntityError::internal(error.to_string()))?
                        .push((id, next_attempt_at));
                }
                Ok(self.lease_outcome)
            })
        }

        fn mark_dead<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            id: SideEffectJobId,
            _owner: &'life1 LeaseHolder,
            _error: &'life2 str,
        ) -> BoxFuture<'async_trait, Result<LeaseAcknowledgement, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                if self.lease_outcome == LeaseAcknowledgement::Applied {
                    self.dead.lock().map_err(|error| EntityError::internal(error.to_string()))?.push(id);
                }
                Ok(self.lease_outcome)
            })
        }

        fn purge_dead_before<'life0, 'async_trait>(&'life0 self, _cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { Ok(0) })
        }
    }

    fn fixed_record() -> WriteRecord {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        WriteRecord::restored(WriteId::mint(), recorded_at)
    }

    fn lease(attempts: i32) -> LeasedSideEffect {
        LeasedSideEffect {
            id: SideEffectJobId::mint(),
            attempts,
            job: BatchSideEffectJob {
                target: live_target(),
                temporal_entities: Vec::new(),
                change_events: Vec::new(),
            },
            record: fixed_record(),
            consequence_base: WriteOrdinal::at(3),
        }
    }

    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    fn lease_holder() -> LeaseHolder {
        LeaseHolder::from(Uuid::nil())
    }

    fn drain_with_committer(queue: Arc<RecordingQueue>, writes: Arc<FixedCommitter>) -> EntitySideEffectDrain {
        EntitySideEffectDrain::new(
            queue,
            writes,
            None,
            EntitySideEffectDrainConfig {
                lease_seconds: 30,
                batch_size: 1,
                max_attempts: 3,
                base_backoff: Duration::from_secs(5),
                lease_holder: lease_holder(),
            },
        )
    }

    fn drain(queue: Arc<RecordingQueue>, disposition: CommitDisposition) -> EntitySideEffectDrain {
        drain_with_committer(queue, Arc::new(FixedCommitter::new(disposition)))
    }

    fn drain_with_sink(queue: Arc<RecordingQueue>, disposition: CommitDisposition, sink: Arc<RecordingSink>) -> EntitySideEffectDrain {
        EntitySideEffectDrain::new(
            queue,
            Arc::new(FixedCommitter::new(disposition)),
            Some(sink),
            EntitySideEffectDrainConfig {
                lease_seconds: 30,
                batch_size: 1,
                max_attempts: 3,
                base_backoff: Duration::from_secs(5),
                lease_holder: lease_holder(),
            },
        )
    }

    /// A lease whose job carries exactly one change event, so a test can count
    /// how many times it reaches the sink.
    fn lease_with_change_event() -> Result<LeasedSideEffect, EntityError> {
        let entity_id = "urn:ngsi-ld:Building:side-effect"
            .parse()
            .map_err(|error| EntityError::internal(format!("invalid test entity id: {error}")))?;
        let event = ChangeEvent::new(entity_id, Vec::new(), NotificationTrigger::EntityCreated);
        Ok(LeasedSideEffect {
            id: SideEffectJobId::mint(),
            attempts: 0,
            job: BatchSideEffectJob {
                target: live_target(),
                temporal_entities: Vec::new(),
                change_events: vec![event],
            },
            record: fixed_record(),
            consequence_base: WriteOrdinal::at(3),
        })
    }

    #[tokio::test]
    async fn successful_delivery_is_acknowledged_after_commit() -> Result<(), EntityError> {
        let lease = lease(0);
        let id = lease.id;
        let queue = Arc::new(RecordingQueue::with_lease(lease));
        let outcome = drain(Arc::clone(&queue), CommitDisposition::Applied).execute(Utc::now()).await?;
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
        let outcome = drain(Arc::clone(&queue), CommitDisposition::Failed).execute(now).await?;
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
        let outcome = drain(Arc::clone(&queue), CommitDisposition::Failed).execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        let dead = queue.dead.lock().map_err(|error| EntityError::internal(error.to_string()))?;
        pretty_assertions::assert_eq!(dead.as_slice(), &[id]);
        Ok(())
    }

    /// Delivery must resume the job's stored write identity and ordinal base
    /// through `commit_replayed`, not mint a fresh identity through `commit`:
    /// that is what keeps a crash-retried delivery of the same job idempotent.
    #[tokio::test]
    async fn delivery_replays_under_the_leased_jobs_stored_record_and_consequence_base() -> Result<(), EntityError> {
        let leased = lease(0);
        let expected_record = leased.record;
        let expected_base = leased.consequence_base;
        let queue = Arc::new(RecordingQueue::with_lease(leased));
        let committer = Arc::new(FixedCommitter::new(CommitDisposition::Applied));
        drain_with_committer(queue, Arc::clone(&committer)).execute(Utc::now()).await?;
        let replays = committer.replays.lock().map_err(|error| EntityError::internal(error.to_string()))?;
        pretty_assertions::assert_eq!(replays.as_slice(), &[CommitReplay::new(expected_record, expected_base)]);
        Ok(())
    }

    /// A lease reclaimed by another worker between a successful commit replay
    /// and this worker's acknowledgement must not be reported as completed:
    /// the reclaiming worker, not this one, owns the job's outcome now.
    #[tokio::test]
    async fn a_lost_lease_on_acknowledgement_is_logged_and_not_treated_as_completed() -> Result<(), EntityError> {
        let leased = lease(0);
        let queue = Arc::new(RecordingQueue::with_lease_lost_on_acknowledgement(leased));
        let outcome = drain(Arc::clone(&queue), CommitDisposition::Applied).execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        let acknowledged = queue.acknowledged.lock().map_err(|error| EntityError::internal(error.to_string()))?;
        assert!(acknowledged.is_empty(), "a lost lease must not be recorded as an applied acknowledgement");
        Ok(())
    }

    /// A delivery whose acknowledgement is confirmed applied publishes its
    /// change events to the live sink exactly once.
    #[tokio::test]
    async fn an_applied_delivery_publishes_its_change_events_once() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_lease(lease_with_change_event()?));
        let sink = Arc::new(RecordingSink::default());
        let outcome = drain_with_sink(queue, CommitDisposition::Applied, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;
        pretty_assertions::assert_eq!(outcome.completed, 1);
        pretty_assertions::assert_eq!(sink.count()?, 1, "an applied delivery must publish its one change event exactly once");
        Ok(())
    }

    /// A delivery whose lease is reclaimed on acknowledgement publishes nothing:
    /// the durable commit still succeeded and stays idempotent on replay, but
    /// the live change events are the reclaiming worker's to publish.
    #[tokio::test]
    async fn a_lost_lease_publishes_no_change_events() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_lease_lost_on_acknowledgement(lease_with_change_event()?));
        let sink = Arc::new(RecordingSink::default());
        let outcome = drain_with_sink(queue, CommitDisposition::Applied, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        pretty_assertions::assert_eq!(sink.count()?, 0, "a lease lost on acknowledgement must publish no change event");
        Ok(())
    }

    /// End to end: the same job is delivered twice -- once by a worker that
    /// commits and then loses its lease, and once by the worker that reclaims
    /// and acknowledges it. Both commits are idempotent on replay, and the live
    /// change event reaches the sink exactly once across the two deliveries.
    #[tokio::test]
    async fn a_reclaimed_redelivery_publishes_the_change_event_exactly_once() -> Result<(), EntityError> {
        let sink = Arc::new(RecordingSink::default());

        // First worker: commits, then finds its lease already reclaimed.
        let lost_queue = Arc::new(RecordingQueue::with_lease_lost_on_acknowledgement(lease_with_change_event()?));
        drain_with_sink(lost_queue, CommitDisposition::Applied, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;
        pretty_assertions::assert_eq!(sink.count()?, 0, "the worker that lost its lease publishes nothing");

        // Reclaiming worker: replays the same job and acknowledges it.
        let applied_queue = Arc::new(RecordingQueue::with_lease(lease_with_change_event()?));
        drain_with_sink(applied_queue, CommitDisposition::Applied, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;
        pretty_assertions::assert_eq!(sink.count()?, 1, "the reclaiming worker performs the single publication");
        Ok(())
    }
}
