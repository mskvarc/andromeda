//! Bounded at-least-once drain for durable entity side effects.
//!
//! Delivery replays through
//! [`ContextCommitter::commit_replayed`](andromeda_context_commit::committer::ContextCommitter::commit_replayed)
//! under the job's stored write identity and ordinal base, so the temporal
//! history and notification events the replay writes belong to the write that
//! deferred them and are numbered after the consequences that write already
//! committed, never over them.
//!
//! The replay also removes the job, as the first statement of the same
//! transaction and only while this worker still holds its lease, so a job's
//! consequences and its removal land together and exactly once: only one
//! replay can remove the job, and every other one rolls back whatever it wrote
//! with it. A worker whose lease passed to another commits nothing, and the
//! reclaiming worker's replay is the only one that lands. Once removed, the job
//! stays locked for the rest of the replay, so the lease has to outlast only
//! the wait between a job's claim and the start of its replay. The drain
//! therefore claims one job at a time, right before replaying it, rather than a
//! whole batch whose later jobs would wait out their leases behind the earlier
//! ones.
//!
//! A replay that fails -- even one that ran past its lease -- releases the job
//! with its diagnostic and the policy backoff, stamped with the instant of the
//! release rather than the start of the pass. The release is guarded by this
//! drain still being the job's claimant, not by the lease still being live,
//! which is what lets a failure after a lapsed lease still be recorded.
//!
//! Every attempt counts toward the configured ceiling exactly once, including
//! one that ends without a word -- a crash, or a lease that lapsed before the
//! replay could lock its job. The claim counts those, and a job whose permitted
//! attempts all ended that way is dead-lettered when it is next claimed,
//! without counting an attempt it did not make, instead of being reclaimed
//! forever.
//!
//! The notifications a job still owes are settled by this broker's own delivery
//! mode ([`SideEffectNotificationRoute`]). Under durable delivery they become
//! notification events and deliveries of the replay commit itself. Under live
//! delivery they are published to the in-memory
//! [`EntityChangeSink`](crate::change_sink::EntityChangeSink) once the replay
//! commit landed, which is what makes that best-effort channel carry one
//! publication per job: only the worker whose replay removed the job publishes.

use crate::{
    committed_side_effect::CommittedSideEffect,
    entity_invariant::EntityInvariant,
    error::EntityError,
    side_effect_notification::{SideEffectNotificationRoute, owed_subscription_matches},
    side_effect_queue::{DeadLetterAttempt, LeasedSideEffect, SideEffectQueue},
    side_effect_replay::replay_writes,
};
use andromeda_context_commit::{
    batch_side_effect_job::BatchSideEffectJob,
    commit_replay::CommitReplay,
    committer::ContextCommitter,
    context_change_event::ContextChangeEvent,
    error::CommitError,
    job_lease::JobLease,
    lease_acknowledgement::LeaseAcknowledgement,
    lease_holder::LeaseHolder,
    lease_pass_clock::LeasePassClock,
    side_effect_job_id::SideEffectJobId,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};

/// Policy-owned retry and lease configuration for durable side effects.
#[derive(Debug)]
pub struct EntitySideEffectDrainConfig {
    /// Lease duration stamped on a claimed row.
    ///
    /// It has to cover only the wait between a job's claim and the start of
    /// its replay -- chiefly acquiring the replay transaction's connection --
    /// since the replay locks the job for as long as it runs.
    pub lease_seconds: i32,
    /// Maximum jobs one bounded pass claims, one after another.
    pub batch_size: i32,
    /// Attempts permitted before dead-lettering, counting attempts that ended
    /// without settling or releasing the job.
    pub max_attempts: u16,
    /// Base exponential retry delay.
    pub base_backoff: Duration,
    /// The worker identity recorded on queue leases.
    ///
    /// Minted for this drain alone and never shared: releases are guarded by
    /// the claimant rather than the clock, which is sound only while no other
    /// worker claims under the same identity.
    pub lease_holder: LeaseHolder,
}

/// Result of one bounded side-effect pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntitySideEffectDrainOutcome {
    /// Jobs whose replay committed and removed them.
    pub completed: usize,
    /// Whether the pass stopped at its batch bound rather than at an empty
    /// queue, so additional work may remain.
    pub work_may_remain: bool,
}

/// Durable entity side-effect delivery policy.
pub struct EntitySideEffectDrain {
    queue: Arc<dyn SideEffectQueue>,
    writes: Arc<dyn ContextCommitter>,
    notifications: SideEffectNotificationRoute,
    config: EntitySideEffectDrainConfig,
}

/// Whether one claimed job left the queue through its replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JobSettlement {
    /// The replay committed and removed the job.
    Settled,
    /// The job was retried, dead-lettered, or left to the worker holding it now.
    Unsettled,
}

/// How one replay of a leased job ended.
enum ReplaySettlement {
    /// The replay committed and removed the job; its live change events are
    /// this worker's to publish.
    Settled(CommittedSideEffect),
    /// The job's lease had passed to another worker, so the replay committed
    /// nothing.
    LeaseLost,
}

impl EntitySideEffectDrain {
    /// Construct the drain over policy-owned ports, settling the notifications
    /// a job still owes through `notifications`.
    pub const fn new(
        queue: Arc<dyn SideEffectQueue>,
        writes: Arc<dyn ContextCommitter>,
        notifications: SideEffectNotificationRoute,
        config: EntitySideEffectDrainConfig,
    ) -> EntitySideEffectDrain {
        EntitySideEffectDrain {
            queue,
            writes,
            notifications,
            config,
        }
    }

    /// Claim and process up to one bounded batch of jobs, one job at a time,
    /// in a pass that starts at `now`.
    ///
    /// Each job is claimed immediately before its replay, so its lease starts
    /// when the replay is about to begin, however long the jobs before it took.
    /// A drain runs one pass at a time, which the claimant-guarded releases
    /// rely on.
    ///
    /// A replay that fails is not an error of the pass: the job is released for
    /// a retry, or dead-lettered, with the failure as its diagnostic.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the queue cannot be leased or
    /// a job's release cannot be recorded; the pass ends there.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<EntitySideEffectDrainOutcome, EntityError> {
        let clock = LeasePassClock::starting_at(now);
        let mut completed = 0;
        for _ in 0..self.config.batch_size {
            let claimed = self
                .queue
                .lease_batch(&self.config.lease_holder, 1, self.config.lease_seconds)
                .await?
                .into_iter()
                .next();
            let Some(lease) = claimed else {
                return Ok(EntitySideEffectDrainOutcome {
                    completed,
                    work_may_remain: false,
                });
            };
            if self.process(lease, &clock).await? == JobSettlement::Settled {
                completed += 1;
            }
        }
        Ok(EntitySideEffectDrainOutcome {
            completed,
            work_may_remain: true,
        })
    }

    /// Replay one claimed job, or dead-letter it when its permitted attempts
    /// were already spent, and record how it ended.
    async fn process(&self, lease: LeasedSideEffect, clock: &LeasePassClock) -> Result<JobSettlement, EntityError> {
        let LeasedSideEffect {
            id,
            attempts,
            job,
            record,
            base_ordinal,
        } = lease;
        if attempts >= i32::from(self.config.max_attempts) {
            self.retire_lapsed(id, attempts).await?;
            return Ok(JobSettlement::Unsettled);
        }
        let lease = JobLease {
            job: id,
            holder: self.config.lease_holder,
        };
        match self.deliver(job, CommitReplay::new(record, base_ordinal, lease)).await {
            Ok(ReplaySettlement::Settled(committed)) => {
                // The replay removed the job in its own transaction, under this
                // worker's lease, so its live change events are published
                // exactly here, once.
                self.publish_changes(committed);
                record_queue_outcome(QueueName::PendingSideEffects, QueueOutcome::Completed, 1);
                Ok(JobSettlement::Settled)
            }
            Ok(ReplaySettlement::LeaseLost) => {
                // The lease lapsed before the replay could lock the job, so the
                // replay wrote nothing. Whichever worker claims the job next
                // counts this attempt and replays it; nothing is left to record
                // here, and this worker no longer may.
                record_queue_outcome(QueueName::PendingSideEffects, QueueOutcome::LeaseLost, 1);
                tracing::warn!(job_id = %id, lease_holder = %self.config.lease_holder, event = "pending_side_effect_lease_lost_on_replay");
                Ok(JobSettlement::Unsettled)
            }
            Err(error) => {
                self.release_failed(id, attempts, &error, clock).await?;
                Ok(JobSettlement::Unsettled)
            }
        }
    }

    /// Dead-letter a job whose permitted attempts all ended without settling
    /// or releasing it.
    ///
    /// Such attempts leave no diagnostic of their own -- the worker crashed, or
    /// its lease lapsed before its replay could lock the job -- so the reason
    /// recorded is the ceiling itself. No attempt is made here, and every one
    /// the job had is already counted, so the dead letter keeps `attempts` as
    /// its exact count.
    async fn retire_lapsed(&self, id: SideEffectJobId, attempts: i32) -> Result<(), EntityError> {
        let detail = format!(
            "{attempts} attempts ended without settling the job, the configured ceiling is {}",
            self.config.max_attempts
        );
        let acknowledgement = self
            .queue
            .mark_dead(id, &self.config.lease_holder, &detail, DeadLetterAttempt::AlreadyCounted)
            .await?;
        self.record_release(id, acknowledgement, QueueOutcome::Exhausted);
        Ok(())
    }

    /// Release a job whose replay failed: back to the queue at the policy
    /// backoff from the instant of the release, or into the dead letters once
    /// the attempt that failed was the last one permitted.
    async fn release_failed(&self, id: SideEffectJobId, attempts: i32, error: &EntityError, clock: &LeasePassClock) -> Result<(), EntityError> {
        let attempt = u16::try_from(attempts.saturating_add(1)).map_err(|conversion| EntityError::from(EntityInvariant::AttemptCountOutOfRange(conversion)))?;
        let detail = error.to_string();
        let (acknowledgement, outcome) = if attempt >= self.config.max_attempts {
            (
                self.queue.mark_dead(id, &self.config.lease_holder, &detail, DeadLetterAttempt::Failed).await?,
                QueueOutcome::Exhausted,
            )
        } else {
            let exponent = u32::from(attempt.saturating_sub(1)).min(15);
            let multiplier = 1_u32 << exponent;
            let backoff = self.config.base_backoff.saturating_mul(multiplier);
            let retry_at =
                clock.now() + chrono::Duration::from_std(backoff).map_err(|conversion| EntityError::from(EntityInvariant::BackoffOutOfRange(conversion)))?;
            (self.queue.nack(id, &self.config.lease_holder, &detail, retry_at).await?, QueueOutcome::Retried)
        };
        self.record_release(id, acknowledgement, outcome);
        Ok(())
    }

    /// Record how a release ended, or that the lease it needed was lost.
    fn record_release(&self, id: SideEffectJobId, acknowledgement: LeaseAcknowledgement, outcome: QueueOutcome) {
        let outcome = match acknowledgement {
            LeaseAcknowledgement::Applied => outcome,
            LeaseAcknowledgement::Lost => {
                tracing::warn!(job_id = %id, lease_holder = %self.config.lease_holder, event = "pending_side_effect_lease_lost_on_release");
                QueueOutcome::LeaseLost
            }
        };
        record_queue_outcome(QueueName::PendingSideEffects, outcome, 1);
    }

    /// Replay the job's durable consequences, removing the job in the same
    /// transaction under its lease, and hand back the live change events to
    /// publish once that commit landed.
    ///
    /// Under durable delivery the owed notifications are among those durable
    /// consequences and nothing is handed back; under live delivery the owed
    /// changes are handed back whole. A replay whose lease was lost committed
    /// nothing and hands back nothing.
    async fn deliver(&self, job: BatchSideEffectJob, replay: CommitReplay) -> Result<ReplaySettlement, EntityError> {
        let BatchSideEffectJob {
            temporal_entities,
            change_events,
        } = job;
        let (owed, change_events) = match &self.notifications {
            SideEffectNotificationRoute::Live(_) => (Vec::new(), change_events),
            SideEffectNotificationRoute::Durable(recording) => (owed_subscription_matches(recording, &ContextTarget::Live, &change_events), Vec::new()),
        };
        let writes = replay_writes(temporal_entities, owed);
        match self.writes.commit_replayed(&ContextTarget::Live, replay, writes).await {
            Ok(_) => Ok(ReplaySettlement::Settled(CommittedSideEffect { change_events })),
            Err(CommitError::ReplayLeaseLost { .. }) => Ok(ReplaySettlement::LeaseLost),
            Err(
                failure @ (CommitError::DuplicateEntity { .. }
                | CommitError::Submission { .. }
                | CommitError::Persistence(_)
                | CommitError::StoreSaturated
                | CommitError::StoreUnavailable
                | CommitError::TransactionContended
                | CommitError::Invariant(_)
                | CommitError::UnexpectedOutcomeCount { .. }),
            ) => Err(failure.into()),
        }
    }

    /// Publish a committed side effect's change events to the in-memory sink.
    ///
    /// Called only once the replay that removed the job under this worker's
    /// lease has committed, so a delivery whose lease was reclaimed publishes
    /// nothing. This mirrors the online inline path, which never republishes a
    /// change it did not commit, and is why the best-effort live channel
    /// carries one publication per job. A
    /// durable-delivery broker publishes nothing: its notification worker does
    /// not evaluate in-process changes, and the replay commit already queued the
    /// deliveries.
    fn publish_changes(&self, committed: CommittedSideEffect) {
        let SideEffectNotificationRoute::Live(sink) = &self.notifications else {
            return;
        };
        let CommittedSideEffect { change_events } = committed;
        for event in change_events {
            sink.publish(ContextChangeEvent {
                target: ContextTarget::Live,
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
    use crate::{
        NotificationWriteConfig,
        building_subscription_fixture::{BUILDING_TYPE_IRI, building_subscription_cache, subscription_key},
        change_sink::EntityChangeSink,
        error::EntityError,
        side_effect_drain::{EntitySideEffectDrain, EntitySideEffectDrainConfig},
        side_effect_notification::SideEffectNotificationRoute,
        side_effect_queue::{DeadLetterAttempt, LeasedSideEffect, SideEffectQueue},
    };
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        batch_side_effect_job::BatchSideEffectJob,
        change_event::ChangeEvent,
        commit_replay::CommitReplay,
        committer::{CommitFuture, ContextCommitter},
        context_change_event::ContextChangeEvent,
        error::CommitError,
        job_lease::JobLease,
        lease_acknowledgement::LeaseAcknowledgement,
        lease_holder::LeaseHolder,
        side_effect_job_id::SideEffectJobId,
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_outcome::WriteOutcome,
        write_record::WriteRecord,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        expanded::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            instance_key::AttributeInstanceKey,
        },
        notification::ChangeOrigin,
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use chrono::{DateTime, TimeZone, Utc};
    use futures::future::BoxFuture;
    use serde_json::{Value, json};
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::time::sleep;
    use uuid::Uuid;

    /// Records every change event published to the sink, so a test can assert
    /// how many publications one delivery produced.
    #[derive(Default)]
    struct RecordingSink {
        published: Mutex<Vec<ContextChangeEvent>>,
    }

    impl RecordingSink {
        fn count(&self) -> usize {
            self.published.lock().expect("the test lock is not poisoned").len()
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
        /// The replay commits and removes the job.
        Applied,
        /// The replay fails for a reason worth retrying.
        Failed,
        /// The job's lease passed to another worker before the replay could
        /// commit, so the replay committed nothing.
        LeaseLost,
    }

    /// Records every replay continuation `commit_replayed` was invoked with,
    /// so a test can assert delivery resumed the job's stored identity and
    /// ordinal base rather than minting a fresh one, and the writes it replayed.
    struct FixedCommitter {
        disposition: CommitDisposition,
        /// How long every replay runs before it ends as `disposition` says.
        duration: Duration,
        replays: Mutex<Vec<CommitReplay>>,
        replayed_writes: Mutex<Vec<AtomicWrite>>,
    }

    impl FixedCommitter {
        fn new(disposition: CommitDisposition) -> FixedCommitter {
            FixedCommitter::lasting(disposition, Duration::ZERO)
        }

        fn lasting(disposition: CommitDisposition, duration: Duration) -> FixedCommitter {
            FixedCommitter {
                disposition,
                duration,
                replays: Mutex::new(Vec::new()),
                replayed_writes: Mutex::new(Vec::new()),
            }
        }
    }

    impl ContextCommitter for FixedCommitter {
        fn commit(&self, _target: &ContextTarget, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            Box::pin(async move { panic!("the side-effect drain must replay, never mint a fresh commit") })
        }

        fn commit_replayed(&self, _target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            Box::pin(async move {
                self.replays.lock().expect("the test lock is not poisoned").push(replay);
                sleep(self.duration).await;
                let outcomes = writes.iter().map(|_| WriteOutcome::NoEntityChange).collect();
                self.replayed_writes.lock().expect("the test lock is not poisoned").extend(writes);
                match self.disposition {
                    CommitDisposition::Applied => Ok(outcomes),
                    CommitDisposition::Failed => Err(CommitError::Persistence(PersistenceFailure::Unavailable)),
                    CommitDisposition::LeaseLost => Err(CommitError::ReplayLeaseLost { job: replay.lease.job }),
                }
            })
        }
    }

    /// Records every queue call the drain makes. A job that succeeded is not
    /// released through the queue: its replay commit removes it.
    #[derive(Default)]
    struct RecordingQueue {
        leased: Mutex<VecDeque<LeasedSideEffect>>,
        claim_widths: Mutex<Vec<i32>>,
        retried: Mutex<Vec<(SideEffectJobId, DateTime<Utc>)>>,
        dead: Mutex<Vec<(SideEffectJobId, String, DeadLetterAttempt)>>,
    }

    impl RecordingQueue {
        fn with_lease(lease: LeasedSideEffect) -> RecordingQueue {
            RecordingQueue::with_leases([lease])
        }

        fn with_leases(leases: impl IntoIterator<Item = LeasedSideEffect>) -> RecordingQueue {
            RecordingQueue {
                leased: Mutex::new(leases.into_iter().collect()),
                ..RecordingQueue::default()
            }
        }

        fn dead_jobs(&self) -> Vec<SideEffectJobId> {
            self.dead.lock().expect("the test lock is not poisoned").iter().map(|(id, _, _)| *id).collect()
        }
    }

    impl SideEffectQueue for RecordingQueue {
        fn lease_batch<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _worker_id: &'life1 LeaseHolder,
            batch_size: i32,
            _lease_seconds: i32,
        ) -> BoxFuture<'async_trait, Result<Vec<LeasedSideEffect>, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                self.claim_widths.lock().expect("the test lock is not poisoned").push(batch_size);
                let mut leased = self.leased.lock().expect("the test lock is not poisoned");
                let width = usize::try_from(batch_size).expect("a non-negative claim width");
                let taken = width.min(leased.len());
                Ok(leased.drain(..taken).collect())
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
                self.retried.lock().expect("the test lock is not poisoned").push((id, next_attempt_at));
                Ok(LeaseAcknowledgement::Applied)
            })
        }

        fn mark_dead<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            id: SideEffectJobId,
            _owner: &'life1 LeaseHolder,
            error: &'life2 str,
            attempt: DeadLetterAttempt,
        ) -> BoxFuture<'async_trait, Result<LeaseAcknowledgement, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                self.dead.lock().expect("the test lock is not poisoned").push((id, error.to_owned(), attempt));
                Ok(LeaseAcknowledgement::Applied)
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
                temporal_entities: Vec::new(),
                change_events: Vec::new(),
            },
            record: fixed_record(),
            base_ordinal: WriteOrdinal::at(3),
        }
    }

    fn lease_holder() -> LeaseHolder {
        LeaseHolder::from(Uuid::nil())
    }

    fn drain_config() -> EntitySideEffectDrainConfig {
        EntitySideEffectDrainConfig {
            lease_seconds: 30,
            batch_size: 1,
            max_attempts: 3,
            base_backoff: Duration::from_secs(5),
            lease_holder: lease_holder(),
        }
    }

    fn drain_with_committer(queue: Arc<RecordingQueue>, writes: Arc<FixedCommitter>) -> EntitySideEffectDrain {
        EntitySideEffectDrain::new(
            queue,
            writes,
            SideEffectNotificationRoute::Live(Arc::new(RecordingSink::default())),
            drain_config(),
        )
    }

    fn drain(queue: Arc<RecordingQueue>, disposition: CommitDisposition) -> EntitySideEffectDrain {
        drain_with_committer(queue, Arc::new(FixedCommitter::new(disposition)))
    }

    fn drain_with_sink(queue: Arc<RecordingQueue>, disposition: CommitDisposition, sink: Arc<RecordingSink>) -> EntitySideEffectDrain {
        EntitySideEffectDrain::new(
            queue,
            Arc::new(FixedCommitter::new(disposition)),
            SideEffectNotificationRoute::Live(sink),
            drain_config(),
        )
    }

    /// A lease whose job carries exactly one change event, so a test can count
    /// how many times it reaches the sink.
    fn lease_with_change_event() -> LeasedSideEffect {
        let entity_id = "urn:ngsi-ld:Building:side-effect".parse().expect("a valid test entity id");
        let event = ChangeEvent::builder()
            .entity_id(entity_id)
            .change(EntityChange::new(EntityLifecycle::Created, Vec::new()))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build();
        LeasedSideEffect {
            id: SideEffectJobId::mint(),
            attempts: 0,
            job: BatchSideEffectJob {
                temporal_entities: Vec::new(),
                change_events: vec![event],
            },
            record: fixed_record(),
            base_ordinal: WriteOrdinal::at(3),
        }
    }

    fn job_lease(job: SideEffectJobId) -> JobLease {
        JobLease { job, holder: lease_holder() }
    }

    /// The replay removes the job under this worker's lease in its own commit:
    /// removing it in a transaction of its own after the replay committed would
    /// leave a window in which a lost lease let another worker replay a job
    /// whose consequences had already landed.
    #[tokio::test]
    async fn a_successful_replay_settles_the_job_under_this_workers_lease_in_its_own_commit() -> Result<(), EntityError> {
        let lease = lease(0);
        let id = lease.id;
        let queue = Arc::new(RecordingQueue::with_lease(lease));
        let committer = Arc::new(FixedCommitter::new(CommitDisposition::Applied));
        let outcome = drain_with_committer(Arc::clone(&queue), Arc::clone(&committer)).execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 1);
        let replays = committer.replays.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(replays.iter().map(|replay| replay.lease).collect::<Vec<JobLease>>(), vec![job_lease(id)]);
        assert!(
            queue.retried.lock().expect("the test lock is not poisoned").is_empty(),
            "a settled job is not released for another attempt"
        );
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
        let retried = queue.retried.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(retried.iter().map(|(job, _)| *job).collect::<Vec<_>>(), vec![id]);
        let backoff = chrono::Duration::seconds(5);
        let retry_at = retried[0].1;
        assert!(
            retry_at >= now + backoff && retry_at < now + backoff + chrono::Duration::seconds(1),
            "the retry is due one backoff after its release: {retry_at}"
        );
        Ok(())
    }

    /// A retry is due one backoff after the release that scheduled it, not
    /// after the start of the pass: a replay that ran long would otherwise be
    /// due again before its pass ended, and be reclaimed with no backoff.
    #[tokio::test]
    async fn a_retry_is_scheduled_from_the_instant_of_its_release() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_lease(lease(0)));
        let now = Utc::now();
        let replay_duration = Duration::from_millis(50);
        drain_with_committer(
            Arc::clone(&queue),
            Arc::new(FixedCommitter::lasting(CommitDisposition::Failed, replay_duration)),
        )
        .execute(now)
        .await?;
        let retried = queue.retried.lock().expect("the test lock is not poisoned");
        let earliest = now + chrono::Duration::seconds(5) + chrono::Duration::from_std(replay_duration).expect("a representable replay duration");
        assert!(retried[0].1 >= earliest, "{} is earlier than {earliest}", retried[0].1);
        Ok(())
    }

    #[tokio::test]
    async fn exhausted_failure_is_dead_lettered() -> Result<(), EntityError> {
        let lease = lease(2);
        let id = lease.id;
        let queue = Arc::new(RecordingQueue::with_lease(lease));
        let outcome = drain(Arc::clone(&queue), CommitDisposition::Failed).execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        pretty_assertions::assert_eq!(queue.dead_jobs(), vec![id]);
        let dead = queue.dead.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(
            dead[0].2,
            DeadLetterAttempt::Failed,
            "the attempt that failed is counted as it dead-letters the job"
        );
        Ok(())
    }

    /// A job whose every permitted attempt ended without settling or releasing
    /// it -- each lease lapsed before its replay could lock the job, and each
    /// reclaim counted the attempt -- is dead-lettered when it is next claimed,
    /// not replayed and reclaimed forever, and without counting an attempt it
    /// did not make.
    #[tokio::test]
    async fn a_job_whose_permitted_attempts_all_lapsed_is_dead_lettered_without_a_replay() -> Result<(), EntityError> {
        let lease = lease(3);
        let id = lease.id;
        let queue = Arc::new(RecordingQueue::with_lease(lease));
        let committer = Arc::new(FixedCommitter::new(CommitDisposition::Applied));
        let outcome = drain_with_committer(Arc::clone(&queue), Arc::clone(&committer)).execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        assert!(
            committer.replays.lock().expect("the test lock is not poisoned").is_empty(),
            "a job past its ceiling is not replayed again"
        );
        let dead = queue.dead.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(dead.iter().map(|(job, _, _)| *job).collect::<Vec<_>>(), vec![id]);
        pretty_assertions::assert_eq!(dead[0].2, DeadLetterAttempt::AlreadyCounted, "no attempt was made, so none is counted");
        assert!(
            dead[0].1.contains("ceiling is 3"),
            "the dead letter names the ceiling it reached: {}",
            dead[0].1
        );
        Ok(())
    }

    /// One attempt below the ceiling still earns a replay.
    #[tokio::test]
    async fn a_job_below_its_attempt_ceiling_is_still_replayed() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_lease(lease(2)));
        let committer = Arc::new(FixedCommitter::new(CommitDisposition::Applied));
        let outcome = drain_with_committer(Arc::clone(&queue), Arc::clone(&committer)).execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 1);
        pretty_assertions::assert_eq!(queue.dead_jobs(), Vec::<SideEffectJobId>::new());
        Ok(())
    }

    /// Each job is claimed on its own, immediately before its replay, so none
    /// waits out its lease behind the replays claimed with it.
    #[tokio::test]
    async fn the_drain_claims_one_job_at_a_time_up_to_its_batch_bound() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_leases([lease(0), lease(0), lease(0)]));
        let drain = EntitySideEffectDrain::new(
            Arc::clone(&queue) as Arc<dyn SideEffectQueue>,
            Arc::new(FixedCommitter::new(CommitDisposition::Applied)),
            SideEffectNotificationRoute::Live(Arc::new(RecordingSink::default())),
            EntitySideEffectDrainConfig {
                batch_size: 2,
                ..drain_config()
            },
        );
        let outcome = drain.execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 2);
        assert!(outcome.work_may_remain, "the pass stopped at its bound with a job still queued");
        let widths = queue.claim_widths.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(widths.as_slice(), &[1, 1]);
        Ok(())
    }

    /// A pass that empties the queue before its bound reports that no work
    /// remains.
    #[tokio::test]
    async fn a_pass_that_empties_the_queue_reports_no_remaining_work() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_leases([lease(0), lease(0)]));
        let drain = EntitySideEffectDrain::new(
            Arc::clone(&queue) as Arc<dyn SideEffectQueue>,
            Arc::new(FixedCommitter::new(CommitDisposition::Applied)),
            SideEffectNotificationRoute::Live(Arc::new(RecordingSink::default())),
            EntitySideEffectDrainConfig {
                batch_size: 5,
                ..drain_config()
            },
        );
        let outcome = drain.execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 2);
        assert!(!outcome.work_may_remain);
        let widths = queue.claim_widths.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(widths.as_slice(), &[1, 1, 1], "the third claim found the queue empty");
        Ok(())
    }

    /// Delivery must resume the job's stored write identity and ordinal base
    /// through `commit_replayed`, not mint a fresh identity through `commit`:
    /// that is what keeps a crash-retried delivery of the same job idempotent.
    #[tokio::test]
    async fn delivery_replays_under_the_leased_jobs_stored_record_and_base_ordinal() -> Result<(), EntityError> {
        let leased = lease(0);
        let expected_record = leased.record;
        let expected_base = leased.base_ordinal;
        let expected_lease = job_lease(leased.id);
        let queue = Arc::new(RecordingQueue::with_lease(leased));
        let committer = Arc::new(FixedCommitter::new(CommitDisposition::Applied));
        drain_with_committer(queue, Arc::clone(&committer)).execute(Utc::now()).await?;
        let replays = committer.replays.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(replays.as_slice(), &[CommitReplay::new(expected_record, expected_base, expected_lease)]);
        Ok(())
    }

    /// A replay whose lease passed to another worker commits nothing, so it is
    /// neither completed nor retried nor dead-lettered here: the reclaiming
    /// worker, not this one, owns the job's outcome.
    #[tokio::test]
    async fn a_replay_refused_for_a_lost_lease_is_neither_completed_nor_retried() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_lease(lease(0)));
        let outcome = drain(Arc::clone(&queue), CommitDisposition::LeaseLost).execute(Utc::now()).await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        assert!(queue.retried.lock().expect("the test lock is not poisoned").is_empty());
        pretty_assertions::assert_eq!(queue.dead_jobs(), Vec::<SideEffectJobId>::new());
        Ok(())
    }

    /// A replay that committed and removed its job publishes its change events
    /// to the live sink exactly once.
    #[tokio::test]
    async fn an_applied_delivery_publishes_its_change_events_once() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_lease(lease_with_change_event()));
        let sink = Arc::new(RecordingSink::default());
        let outcome = drain_with_sink(queue, CommitDisposition::Applied, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;
        pretty_assertions::assert_eq!(outcome.completed, 1);
        pretty_assertions::assert_eq!(sink.count(), 1, "an applied delivery must publish its one change event exactly once");
        Ok(())
    }

    /// A replay refused for a lost lease publishes nothing: it committed
    /// nothing, and the live change events are the reclaiming worker's to
    /// publish.
    #[tokio::test]
    async fn a_lost_lease_publishes_no_change_events() -> Result<(), EntityError> {
        let queue = Arc::new(RecordingQueue::with_lease(lease_with_change_event()));
        let sink = Arc::new(RecordingSink::default());
        let outcome = drain_with_sink(queue, CommitDisposition::LeaseLost, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;
        pretty_assertions::assert_eq!(outcome.completed, 0);
        pretty_assertions::assert_eq!(sink.count(), 0, "a replay refused for a lost lease must publish no change event");
        Ok(())
    }

    /// End to end: the same job is delivered twice -- once by a worker whose
    /// lease passed to another before its replay could commit, and once by the
    /// worker that reclaimed it. Only the reclaiming worker's replay lands, and
    /// the live change event reaches the sink exactly once.
    #[tokio::test]
    async fn a_reclaimed_redelivery_publishes_the_change_event_exactly_once() -> Result<(), EntityError> {
        let sink = Arc::new(RecordingSink::default());

        // First worker: its replay is refused, the lease having passed on.
        let lost_queue = Arc::new(RecordingQueue::with_lease(lease_with_change_event()));
        drain_with_sink(lost_queue, CommitDisposition::LeaseLost, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;
        pretty_assertions::assert_eq!(sink.count(), 0, "the worker that lost its lease publishes nothing");

        // Reclaiming worker: replays the same job and settles it.
        let applied_queue = Arc::new(RecordingQueue::with_lease(lease_with_change_event()));
        drain_with_sink(applied_queue, CommitDisposition::Applied, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;
        pretty_assertions::assert_eq!(sink.count(), 1, "the reclaiming worker performs the single publication");
        Ok(())
    }

    /// A durable-delivery drain over the one Building subscription, recording
    /// what it replays.
    async fn durable_drain(queue: Arc<RecordingQueue>, committer: Arc<FixedCommitter>) -> EntitySideEffectDrain {
        EntitySideEffectDrain::new(
            queue,
            committer,
            SideEffectNotificationRoute::Durable(NotificationWriteConfig {
                subscription_cache: building_subscription_cache().await,
            }),
            drain_config(),
        )
    }

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("a valid URI")
    }

    fn building(id: &str) -> Value {
        json!([{
            "@id": id,
            "@type": [BUILDING_TYPE_IRI],
            "https://uri.etsi.org/ngsi-ld/default-context/category": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "office"}]
            }]
        }])
    }

    /// The change a live-delivery broker defers for a Building it created.
    fn owed_creation(id: &str) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id(uri(id))
            .entity_types(vec![BUILDING_TYPE_IRI.parse::<EntityTypeName>().expect("a valid type name")])
            .change(EntityChange::new(
                EntityLifecycle::Created,
                vec![AttributeChange {
                    key: AttributeInstanceKey {
                        attr: "https://uri.etsi.org/ngsi-ld/default-context/category"
                            .parse::<AttributeName>()
                            .expect("a valid attribute name"),
                        dataset_id: None,
                    },
                    kind: AttributeChangeKind::Created,
                }],
            ))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .current_payload(Some(Arc::new(building(id))))
            .build()
    }

    fn lease_of(job: BatchSideEffectJob) -> LeasedSideEffect {
        LeasedSideEffect {
            id: SideEffectJobId::mint(),
            attempts: 0,
            job,
            record: fixed_record(),
            base_ordinal: WriteOrdinal::at(3),
        }
    }

    /// A job a live-delivery broker queued owes its changes' notifications. A
    /// durable-delivery broker draining it queues the matched deliveries in the
    /// replay commit, joined to the entity's observation write, rather than
    /// publishing the change to a live channel its notification worker never
    /// reads (clause 5.8.6: every matching change is notified).
    #[tokio::test]
    async fn a_durable_drain_queues_the_deliveries_a_live_producers_job_still_owes() -> Result<(), EntityError> {
        let id = "urn:ngsi-ld:Building:owed-by-live";
        let job = BatchSideEffectJob {
            temporal_entities: vec![(Arc::new(uri(id)), Arc::new(building(id)))],
            change_events: vec![owed_creation(id)],
        };
        let queue = Arc::new(RecordingQueue::with_lease(lease_of(job)));
        let committer = Arc::new(FixedCommitter::new(CommitDisposition::Applied));

        let outcome = durable_drain(queue, Arc::clone(&committer)).await.execute(Utc::now()).await?;

        pretty_assertions::assert_eq!(outcome.completed, 1);
        let writes = committer.replayed_writes.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(writes.len(), 1, "the owed match joins the entity's observation write");
        pretty_assertions::assert_eq!(writes[0].history.len(), 1);
        pretty_assertions::assert_eq!(writes[0].subscription_matches.len(), 1);
        pretty_assertions::assert_eq!(writes[0].subscription_matches[0].subscription_key, subscription_key());
        pretty_assertions::assert_eq!(writes[0].subscription_matches[0].change.lifecycle, EntityLifecycle::Created);
        Ok(())
    }

    /// A job a durable-delivery broker queued carries no change: its
    /// notifications were queued at commit. Draining it replays the
    /// observations alone and queues nothing a second time.
    #[tokio::test]
    async fn a_durable_drain_of_a_job_without_owed_changes_queues_no_delivery() -> Result<(), EntityError> {
        let id = "urn:ngsi-ld:Building:queued-at-commit";
        let job = BatchSideEffectJob {
            temporal_entities: vec![(Arc::new(uri(id)), Arc::new(building(id)))],
            change_events: Vec::new(),
        };
        let queue = Arc::new(RecordingQueue::with_lease(lease_of(job)));
        let committer = Arc::new(FixedCommitter::new(CommitDisposition::Applied));

        durable_drain(queue, Arc::clone(&committer)).await.execute(Utc::now()).await?;

        let writes = committer.replayed_writes.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(writes.len(), 1);
        assert!(writes[0].subscription_matches.is_empty(), "nothing is owed, so nothing is queued");
        Ok(())
    }

    /// A job a durable-delivery broker queued carries no change, so a
    /// live-delivery broker draining it publishes nothing: publishing would
    /// notify the subscribers the committing broker already queued deliveries
    /// for a second time.
    #[tokio::test]
    async fn a_live_drain_of_a_job_without_owed_changes_publishes_nothing() -> Result<(), EntityError> {
        let id = "urn:ngsi-ld:Building:queued-at-commit";
        let job = BatchSideEffectJob {
            temporal_entities: vec![(Arc::new(uri(id)), Arc::new(building(id)))],
            change_events: Vec::new(),
        };
        let queue = Arc::new(RecordingQueue::with_lease(lease_of(job)));
        let sink = Arc::new(RecordingSink::default());

        let outcome = drain_with_sink(queue, CommitDisposition::Applied, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;

        pretty_assertions::assert_eq!(outcome.completed, 1);
        pretty_assertions::assert_eq!(sink.count(), 0);
        Ok(())
    }

    /// A live-delivery broker draining a job a live-delivery broker queued hands
    /// the owed change, with the entity the write left behind, to its
    /// in-process notification worker.
    #[tokio::test]
    async fn a_live_drain_publishes_the_owed_change_with_its_resulting_entity() -> Result<(), EntityError> {
        let id = "urn:ngsi-ld:Building:owed-to-live";
        let job = BatchSideEffectJob {
            temporal_entities: Vec::new(),
            change_events: vec![owed_creation(id)],
        };
        let queue = Arc::new(RecordingQueue::with_lease(lease_of(job)));
        let sink = Arc::new(RecordingSink::default());

        drain_with_sink(queue, CommitDisposition::Applied, Arc::clone(&sink))
            .execute(Utc::now())
            .await?;

        let published = sink.published.lock().expect("the test lock is not poisoned");
        pretty_assertions::assert_eq!(published.len(), 1);
        pretty_assertions::assert_eq!(published[0].event.current_payload.as_deref(), Some(&building(id)));
        Ok(())
    }
}
