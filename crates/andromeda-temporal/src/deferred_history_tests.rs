use crate::{
    append_intent::TemporalIntentKey,
    error::TemporalError,
    intent_drain::{StuckTemporalIntent, TemporalIntentClaim, TemporalIntentDrain},
    intent_retry_policy::TemporalIntentRetryPolicy,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        temporal_intent_application::TemporalIntentApplication,
        temporal_intent_queue::{ClaimedTemporalIntent, TemporalIntentBatchSize, TemporalIntentQueue, TemporalIntentRetry},
    },
    temporal_not_found_reason::TemporalNotFoundReason,
};
use andromeda_context_commit::{
    failure_summary::FAILURE_SUMMARY_MAX_CHARACTERS,
    lease_acknowledgement::LeaseAcknowledgement,
    lease_holder::LeaseHolder,
    write_id::WriteId,
};
use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use std::{collections::HashSet, num::NonZeroU32, sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Default)]
struct RecordingQueueState {
    pending: Vec<ClaimedTemporalIntent>,
    retries: Vec<TemporalIntentRetry>,
    /// The entities the pending probe was asked about.
    probed: Vec<Identifier>,
    /// When set, every retry reports a lapsed lease.
    lease_lapsed: bool,
}

#[derive(Default)]
struct RecordingQueue {
    state: Mutex<RecordingQueueState>,
}

impl RecordingQueue {
    async fn insert(&self, claim: ClaimedTemporalIntent) {
        self.state.lock().await.pending.push(claim);
    }

    async fn pending_keys(&self) -> Vec<i64> {
        self.state.lock().await.pending.iter().map(|claim| claim.key.get()).collect()
    }

    async fn remove(&self, keys: &[i64]) {
        self.state.lock().await.pending.retain(|claim| !keys.contains(&claim.key.get()));
    }

    async fn retries(&self) -> Vec<TemporalIntentRetry> {
        self.state.lock().await.retries.clone()
    }

    async fn probed(&self) -> Vec<Identifier> {
        self.state.lock().await.probed.clone()
    }

    async fn lapse_leases(&self) {
        self.state.lock().await.lease_lapsed = true;
    }
}

#[async_trait]
impl TemporalIntentQueue for RecordingQueue {
    async fn claim(
        &self,
        _owner: LeaseHolder,
        limit: TemporalIntentBatchSize,
        _lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalIntent>, TemporalError> {
        let limit = usize::try_from(limit.get()).unwrap_or(usize::MAX);
        Ok(self.state.lock().await.pending.iter().take(limit).cloned().collect())
    }

    async fn claim_for_entity(
        &self,
        _owner: LeaseHolder,
        entity_id: &Identifier,
        limit: TemporalIntentBatchSize,
        _lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalIntent>, TemporalError> {
        let limit = usize::try_from(limit.get()).unwrap_or(usize::MAX);
        Ok(self
            .state
            .lock()
            .await
            .pending
            .iter()
            .filter(|claim| claim.entity_id == *entity_id)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn holds_intent_for(&self, entity_id: &Identifier) -> Result<bool, TemporalError> {
        let mut state = self.state.lock().await;
        state.probed.push(entity_id.clone()); // clone: the recording owns the entity it was asked about.
        Ok(state.pending.iter().any(|claim| claim.entity_id == *entity_id))
    }

    async fn retry(&self, _owner: LeaseHolder, intent: &ClaimedTemporalIntent, retry: TemporalIntentRetry) -> Result<LeaseAcknowledgement, TemporalError> {
        let mut state = self.state.lock().await;
        if state.lease_lapsed {
            return Ok(LeaseAcknowledgement::Lost);
        }
        if let Some(claim) = state.pending.iter_mut().find(|claim| claim.key == intent.key) {
            claim.attempts = claim.attempts.saturating_add(1);
        }
        state.retries.push(retry);
        Ok(LeaseAcknowledgement::Applied)
    }
}

/// How the recording application refuses an intent while it is told to.
#[derive(Clone, Copy)]
enum ApplicationFailure {
    /// The history store is unreachable.
    StoreUnavailable,
    /// A refusal whose rendering runs past the failure-summary bound: three
    /// identifiers near the identifier limit in one not-found reason.
    LongDiagnostic,
}

impl ApplicationFailure {
    fn error(self) -> TemporalError {
        match self {
            ApplicationFailure::StoreUnavailable => TemporalError::from(PersistenceFailure::Unavailable),
            ApplicationFailure::LongDiagnostic => {
                let long = |prefix: &str| format!("{prefix}{}", "x".repeat(1000));
                TemporalError::NotFound(TemporalNotFoundReason::AttributeDatasetOnEntity {
                    attribute: long("https://example.org/").parse().expect("a valid long attribute name"),
                    dataset_id: Box::new(long("urn:ngsi-ld:Dataset:").parse().expect("a valid long dataset identifier")),
                    entity: long("urn:ngsi-ld:Entity:").parse().expect("a valid long entity identifier"),
                })
            }
        }
    }
}

#[derive(Default)]
struct RecordingApplicationState {
    /// The keys of every call, in call order.
    calls: Vec<Vec<i64>>,
    /// The keys whose application fails, failing every call naming one.
    failing: HashSet<i64>,
    /// The failure every call reports while set.
    failure: Option<ApplicationFailure>,
    /// When set, every call reports a lapsed lease.
    lease_lapsed: bool,
}

/// An application that removes what it applies from the queue it was built
/// over, as the store's one transaction does.
struct RecordingApplication {
    queue: Arc<RecordingQueue>,
    state: Mutex<RecordingApplicationState>,
}

impl RecordingApplication {
    fn over(queue: &Arc<RecordingQueue>) -> RecordingApplication {
        RecordingApplication {
            queue: Arc::clone(queue),
            state: Mutex::new(RecordingApplicationState::default()),
        }
    }

    async fn fail(&self, failure: ApplicationFailure) {
        self.state.lock().await.failure = Some(failure);
    }

    async fn fail_key(&self, key: i64) {
        self.state.lock().await.failing.insert(key);
    }

    async fn lapse_leases(&self) {
        self.state.lock().await.lease_lapsed = true;
    }

    async fn calls(&self) -> Vec<Vec<i64>> {
        self.state.lock().await.calls.clone()
    }
}

#[async_trait]
impl TemporalIntentApplication for RecordingApplication {
    async fn apply(&self, _owner: LeaseHolder, intents: &[ClaimedTemporalIntent]) -> Result<LeaseAcknowledgement, TemporalError> {
        let keys: Vec<i64> = intents.iter().map(|intent| intent.key.get()).collect();
        let mut state = self.state.lock().await;
        state.calls.push(keys.clone()); // clone: the recording owns the keys of each call.
        if let Some(failure) = state.failure {
            return Err(failure.error());
        }
        if keys.iter().any(|key| state.failing.contains(key)) {
            return Err(ApplicationFailure::StoreUnavailable.error());
        }
        if state.lease_lapsed {
            return Ok(LeaseAcknowledgement::Lost);
        }
        drop(state);
        self.queue.remove(&keys).await;
        Ok(LeaseAcknowledgement::Applied)
    }
}

fn vehicle(index: u8) -> Identifier {
    format!("urn:ngsi-ld:Vehicle:{index}").parse().expect("valid entity ID")
}

/// One claimed intent of `entity_id` queued under `key`.
fn claimed(key: i64, entity_id: &Identifier) -> ClaimedTemporalIntent {
    ClaimedTemporalIntent {
        key: TemporalIntentKey::new(key).expect("positive key"),
        write_id: WriteId::mint(),
        entity_id: entity_id.clone(), // clone: the claim owns the entity it names.
        attempts: 0,
    }
}

fn instant() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant")
}

fn live_target() -> ContextTarget {
    ContextTarget::Live
}

fn snapshot_target() -> ContextTarget {
    ContextTarget::Snapshot(SnapshotTarget::published(
        SnapshotKey::try_from(7).expect("a positive key"),
        "urn:ngsi-ld:Snapshot:deferred".parse().expect("a valid snapshot URI"),
    ))
}

/// The retry schedule the drains under test use: two seconds a step, capped
/// at five, stuck from the third completed attempt.
fn retry_policy() -> TemporalIntentRetryPolicy {
    TemporalIntentRetryPolicy::new(Duration::from_secs(2), Duration::from_secs(5), NonZeroU32::new(3).expect("non-zero threshold"))
        .expect("the ceiling is above the base delay")
}

fn drain(queue: &Arc<RecordingQueue>, application: &Arc<RecordingApplication>) -> TemporalIntentDrain {
    TemporalIntentDrain::new(
        Arc::clone(queue) as Arc<dyn TemporalIntentQueue>,
        Arc::clone(application) as Arc<dyn TemporalIntentApplication>,
        TemporalIntentClaim {
            owner: LeaseHolder::mint(),
            batch_size: TemporalIntentBatchSize::new(NonZeroU32::new(8).expect("non-zero batch")),
            lease_duration: Duration::from_secs(30),
            retry_policy: retry_policy(),
        },
    )
}

/// A queue holding three intents of three entities, keys 1 to 3, and an
/// application over it.
async fn three_intents() -> (Arc<RecordingQueue>, Arc<RecordingApplication>) {
    let queue = Arc::new(RecordingQueue::default());
    for key in 1..=3 {
        queue.insert(claimed(key, &vehicle(u8::try_from(key).expect("a small key")))).await;
    }
    let application = Arc::new(RecordingApplication::over(&queue));
    (queue, application)
}

#[tokio::test]
async fn an_applied_batch_is_counted_as_acknowledged() {
    let (queue, application) = three_intents().await;
    let outcome = drain(&queue, &application).execute(instant()).await.expect("the pass runs");
    assert_eq!(outcome.acknowledged, 3);
    assert_eq!(outcome.retried, 0);
    assert_eq!(application.calls().await, vec![vec![1, 2, 3]], "one transaction for the batch");
    assert_eq!(queue.pending_keys().await, Vec::<i64>::new());
}

#[tokio::test]
async fn a_failed_batch_is_applied_again_one_intent_at_a_time() {
    let (queue, application) = three_intents().await;
    application.fail_key(2).await;
    drain(&queue, &application).execute(instant()).await.expect("the pass runs");
    assert_eq!(application.calls().await, vec![vec![1, 2, 3], vec![1], vec![2], vec![3]]);
}

#[tokio::test]
async fn one_failing_intent_does_not_hold_back_its_batch() {
    let (queue, application) = three_intents().await;
    application.fail_key(2).await;
    let outcome = drain(&queue, &application).execute(instant()).await.expect("the pass runs");
    assert_eq!(outcome.acknowledged, 2);
    assert_eq!(outcome.retried, 1);
    assert_eq!(queue.pending_keys().await, vec![2], "only the failing intent stays queued");
    assert_eq!(queue.retries().await.len(), 1);
}

/// A batch of one is applied once: its own transaction is the per-intent one.
#[tokio::test]
async fn a_single_claimed_intent_is_applied_once() {
    let queue = Arc::new(RecordingQueue::default());
    queue.insert(claimed(1, &vehicle(1))).await;
    let application = Arc::new(RecordingApplication::over(&queue));
    application.fail(ApplicationFailure::StoreUnavailable).await;
    let outcome = drain(&queue, &application).execute(instant()).await.expect("the pass runs");
    assert_eq!(application.calls().await, vec![vec![1]]);
    assert_eq!(outcome.retried, 1);
}

/// An application or retry whose lease has lapsed is counted as lost, not as
/// applied or retried, and the intent is left to the worker that now holds it.
#[tokio::test]
async fn a_lapsed_lease_is_reported_as_lost_rather_than_as_an_outcome() {
    let queue = Arc::new(RecordingQueue::default());
    queue.insert(claimed(1, &vehicle(1))).await;
    let application = Arc::new(RecordingApplication::over(&queue));
    application.lapse_leases().await;
    let outcome = drain(&queue, &application)
        .execute(instant())
        .await
        .expect("a lapsed lease is an outcome, not a failure");
    assert_eq!(outcome.acknowledged, 0);
    assert_eq!(outcome.retried, 0);
    assert_eq!(outcome.lease_lost, 1);
    assert_eq!(queue.pending_keys().await, vec![1], "the intent is left to its new owner");

    queue.lapse_leases().await;
    application.fail(ApplicationFailure::StoreUnavailable).await;
    let outcome = drain(&queue, &application)
        .execute(instant())
        .await
        .expect("a lapsed lease is an outcome, not a failure");
    assert_eq!(outcome.retried, 0);
    assert_eq!(outcome.lease_lost, 1, "a retry that lost its lease is lost too");
}

/// The retry delay grows a base step per completed attempt and stops at the
/// policy ceiling, so an intent that keeps failing is retried at a bounded
/// cadence instead of backing off without limit.
#[tokio::test]
async fn repeated_application_failures_back_off_linearly_up_to_the_configured_ceiling() {
    let queue = Arc::new(RecordingQueue::default());
    queue.insert(claimed(1, &vehicle(1))).await;
    let application = Arc::new(RecordingApplication::over(&queue));
    application.fail(ApplicationFailure::StoreUnavailable).await;
    let drain = drain(&queue, &application);
    for _ in 0..4 {
        drain.execute(instant()).await.expect("failure is retained per item");
    }
    let delays: Vec<TimeDelta> = queue.retries().await.iter().map(|retry| retry.eligible_at - instant()).collect();
    assert_eq!(
        delays,
        vec![TimeDelta::seconds(2), TimeDelta::seconds(4), TimeDelta::seconds(5), TimeDelta::seconds(5)],
        "two seconds a step, clamped at the five-second ceiling"
    );
}

/// The application's failure summary is cut to what the queue row can hold.
#[tokio::test]
async fn a_long_application_failure_summary_is_cut_to_the_column_limit() {
    let queue = Arc::new(RecordingQueue::default());
    queue.insert(claimed(1, &vehicle(1))).await;
    let application = Arc::new(RecordingApplication::over(&queue));
    application.fail(ApplicationFailure::LongDiagnostic).await;
    drain(&queue, &application).execute(instant()).await.expect("failure is retained per item");
    let summaries: Vec<usize> = queue.retries().await.iter().map(|retry| retry.summary.chars().count()).collect();
    assert_eq!(summaries, vec![usize::try_from(FAILURE_SUMMARY_MAX_CHARACTERS).expect("fits usize")]);
}

/// An intent is reported stuck from the third completed attempt by what names
/// it in the queue, read from the claim alone.
#[tokio::test]
async fn a_stuck_intent_is_reported_by_its_key_write_and_entity() {
    let queue = Arc::new(RecordingQueue::default());
    let intent = claimed(7, &vehicle(7));
    queue.insert(intent.clone()).await; // clone: the expected report is built from the claim after it is queued
    let application = Arc::new(RecordingApplication::over(&queue));
    application.fail(ApplicationFailure::StoreUnavailable).await;
    let drain = drain(&queue, &application);
    let mut reported = Vec::new();
    for _ in 0..3 {
        reported.push(drain.execute(instant()).await.expect("failure is retained per item").stuck);
    }
    assert_eq!(
        reported,
        vec![
            Vec::new(),
            Vec::new(),
            vec![StuckTemporalIntent {
                key: intent.key,
                write_id: intent.write_id,
                entity_id: intent.entity_id,
                attempts: 3,
            }],
        ]
    );
}

#[tokio::test]
async fn the_mutation_barrier_delivers_the_target_entity_before_returning() {
    let (queue, application) = three_intents().await;
    drain(&queue, &application)
        .synchronize(&live_target(), &vehicle(2))
        .await
        .expect("entity synchronization succeeds");
    assert_eq!(application.calls().await, vec![vec![2]]);
    assert_eq!(queue.pending_keys().await, vec![1, 3]);
}

/// The barrier asks the queue whether the entity holds an intent, before and
/// after delivering, and claims nothing when it holds none.
#[tokio::test]
async fn the_mutation_barrier_asks_the_queue_without_reading_intents() {
    let (queue, application) = three_intents().await;
    let drain = drain(&queue, &application);
    drain.synchronize(&live_target(), &vehicle(9)).await.expect("nothing to synchronize");
    assert!(application.calls().await.is_empty(), "an entity without intents claims nothing");
    drain.synchronize(&live_target(), &vehicle(1)).await.expect("entity synchronization succeeds");
    assert_eq!(queue.probed().await, vec![vehicle(9), vehicle(1), vehicle(1)]);
}

/// A barrier that cannot deliver every intent of the entity fails rather than
/// letting the mutation run ahead of history still queued.
#[tokio::test]
async fn a_mutation_barrier_that_leaves_an_intent_queued_fails() {
    let (queue, application) = three_intents().await;
    application.fail_key(2).await;
    let refused = drain(&queue, &application).synchronize(&live_target(), &vehicle(2)).await;
    assert!(matches!(refused, Err(TemporalError::Internal(_))), "{refused:?}");
}

/// A Snapshot branch records its history in the write's own transaction, so
/// its mutation barrier has nothing queued to wait for and leaves the live
/// branch's pending intent for the entity alone.
#[tokio::test]
async fn a_snapshot_mutation_barrier_neither_waits_for_nor_delivers_live_intents() {
    let (queue, application) = three_intents().await;
    drain(&queue, &application)
        .synchronize(&snapshot_target(), &vehicle(1))
        .await
        .expect("a Snapshot barrier has nothing to synchronize");
    assert_eq!(queue.pending_keys().await, vec![1, 2, 3], "the live branch's intent stays queued");
    assert!(application.calls().await.is_empty(), "nothing was delivered on the Snapshot's behalf");
    assert_eq!(queue.probed().await, Vec::<Identifier>::new());
}
