use crate::{
    append_intent::TemporalIntentKey,
    error::TemporalError,
    intent_drain::{StuckTemporalIntent, TemporalIntentClaim, TemporalIntentDrain},
    intent_retry_policy::TemporalIntentRetryPolicy,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        temporal_intent_application::TemporalIntentApplication,
        temporal_intent_queue::{ClaimedTemporalIntent, EntityRelease, TemporalIntentBatchSize, TemporalIntentQueue, TemporalIntentRetry},
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
use std::{
    collections::{HashMap, HashSet, VecDeque},
    mem,
    num::NonZeroU32,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

/// What the holder of a lease another owner keeps does while the barrier
/// waits on it.
#[derive(Clone, Copy)]
enum HolderMove {
    /// The holder applies the intents it holds, removing them.
    Applies,
    /// The holder's lease lapses, leaving its intents claimable.
    Lapses,
    /// The holder keeps its lease past the wait, with this much of it left.
    Keeps(Duration),
}

#[derive(Default)]
struct RecordingQueueState {
    pending: Vec<ClaimedTemporalIntent>,
    retries: Vec<TemporalIntentRetry>,
    /// The entities the high-water probe was asked about.
    probed: Vec<Identifier>,
    /// The entity and mark of every barrier claim.
    entity_claims: Vec<(Identifier, i64)>,
    /// The wait every release wait was given.
    waits: Vec<Duration>,
    /// The keys not claimable before the instant recorded, as a retry delay
    /// leaves them.
    not_before: HashMap<i64, DateTime<Utc>>,
    /// The keys another owner holds under a live lease.
    held_elsewhere: HashSet<i64>,
    /// What the holder does at each release wait, in order.
    holder_moves: VecDeque<HolderMove>,
    /// The lease the holder has left when the release wait ends with it held.
    remaining_lease: Duration,
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

    async fn entity_claims(&self) -> Vec<(Identifier, i64)> {
        self.state.lock().await.entity_claims.clone()
    }

    async fn waits(&self) -> Vec<Duration> {
        self.state.lock().await.waits.clone()
    }

    async fn lapse_leases(&self) {
        self.state.lock().await.lease_lapsed = true;
    }

    /// Hold `key` back from every claim until `instant`, as a retry delay does.
    async fn defer(&self, key: i64, instant: DateTime<Utc>) {
        self.state.lock().await.not_before.insert(key, instant);
    }

    /// Let another owner hold `key`, doing `moves` at each release wait.
    async fn hold_elsewhere(&self, key: i64, moves: &[HolderMove]) {
        let mut state = self.state.lock().await;
        state.held_elsewhere.insert(key);
        state.holder_moves.extend(moves.iter().copied());
    }
}

impl RecordingQueueState {
    /// The pending intents a claim of `limit` takes under `admits`, skipping
    /// what another owner holds.
    fn claimable(&self, limit: TemporalIntentBatchSize, admits: impl Fn(&ClaimedTemporalIntent) -> bool) -> Vec<ClaimedTemporalIntent> {
        let limit = usize::try_from(limit.get()).unwrap_or(usize::MAX);
        self.pending
            .iter()
            .filter(|claim| !self.held_elsewhere.contains(&claim.key.get()) && admits(claim))
            .take(limit)
            .cloned()
            .collect()
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
        let state = self.state.lock().await;
        let now = Utc::now();
        Ok(state.claimable(limit, |claim| state.not_before.get(&claim.key.get()).is_none_or(|due| *due <= now)))
    }

    async fn claim_for_entity(
        &self,
        _owner: LeaseHolder,
        entity_id: &Identifier,
        through: TemporalIntentKey,
        limit: TemporalIntentBatchSize,
        _lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalIntent>, TemporalError> {
        let mut state = self.state.lock().await;
        state.entity_claims.push((entity_id.clone(), through.get())); // clone: the recording owns the entity it was asked about.
        Ok(state.claimable(limit, |claim| claim.entity_id == *entity_id && claim.key <= through))
    }

    async fn last_intent_for(&self, entity_id: &Identifier) -> Result<Option<TemporalIntentKey>, TemporalError> {
        let mut state = self.state.lock().await;
        state.probed.push(entity_id.clone()); // clone: the recording owns the entity it was asked about.
        Ok(state.pending.iter().filter(|claim| claim.entity_id == *entity_id).map(|claim| claim.key).max())
    }

    async fn await_entity_release(&self, entity_id: &Identifier, through: TemporalIntentKey, wait: Duration) -> Result<EntityRelease, TemporalError> {
        let mut state = self.state.lock().await;
        state.waits.push(wait);
        match state.holder_moves.pop_front() {
            Some(HolderMove::Applies) => {
                let held = mem::take(&mut state.held_elsewhere);
                state.pending.retain(|claim| !held.contains(&claim.key.get()));
            }
            Some(HolderMove::Lapses) => state.held_elsewhere.clear(),
            Some(HolderMove::Keeps(remaining_lease)) => state.remaining_lease = remaining_lease,
            None => (),
        }
        let mut up_to_mark = state
            .pending
            .iter()
            .filter(|claim| claim.entity_id == *entity_id && claim.key <= through)
            .peekable();
        if up_to_mark.peek().is_none() {
            return Ok(EntityRelease::Clear);
        }
        if up_to_mark.any(|claim| !state.held_elsewhere.contains(&claim.key.get())) {
            return Ok(EntityRelease::Claimable);
        }
        Ok(EntityRelease::Held {
            remaining_lease: state.remaining_lease,
        })
    }

    async fn retry(&self, _owner: LeaseHolder, intent: &ClaimedTemporalIntent, retry: TemporalIntentRetry) -> Result<LeaseAcknowledgement, TemporalError> {
        let mut state = self.state.lock().await;
        if state.lease_lapsed {
            return Ok(LeaseAcknowledgement::Lost);
        }
        if let Some(claim) = state.pending.iter_mut().find(|claim| claim.key == intent.key) {
            claim.attempts = claim.attempts.saturating_add(1);
        }
        state.not_before.insert(intent.key.get(), retry.eligible_at);
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
    /// An intent a concurrent write commits while the first call runs.
    committed_during_first_call: Option<ClaimedTemporalIntent>,
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

    /// Commit `intent` to the queue while the first call runs.
    async fn commit_during_first_call(&self, intent: ClaimedTemporalIntent) {
        self.state.lock().await.committed_during_first_call = Some(intent);
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
        let committed = state.committed_during_first_call.take();
        drop(state);
        self.queue.remove(&keys).await;
        if let Some(intent) = committed {
            self.queue.insert(intent).await;
        }
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
        "urn:ngsi-ld:Snapshot:queued".parse().expect("a valid snapshot URI"),
    ))
}

/// The retry schedule the drains under test use: two seconds a step, capped
/// at five, stuck from the third completed attempt.
fn retry_policy() -> TemporalIntentRetryPolicy {
    TemporalIntentRetryPolicy::new(Duration::from_secs(2), Duration::from_secs(5), NonZeroU32::new(3).expect("non-zero threshold"))
        .expect("the ceiling is above the base delay")
}

/// The longest the barrier of the drains under test waits on another owner.
const BARRIER_WAIT: Duration = Duration::from_secs(15);

fn drain(queue: &Arc<RecordingQueue>, application: &Arc<RecordingApplication>) -> TemporalIntentDrain {
    drain_claiming(queue, application, 8)
}

/// A drain over `queue` and `application` claiming `batch_size` intents a pass.
fn drain_claiming(queue: &Arc<RecordingQueue>, application: &Arc<RecordingApplication>, batch_size: u32) -> TemporalIntentDrain {
    TemporalIntentDrain::new(
        Arc::clone(queue) as Arc<dyn TemporalIntentQueue>,
        Arc::clone(application) as Arc<dyn TemporalIntentApplication>,
        TemporalIntentClaim {
            owner: LeaseHolder::mint(),
            batch_size: TemporalIntentBatchSize::new(NonZeroU32::new(batch_size).expect("non-zero batch")),
            lease_duration: Duration::from_secs(30),
            retry_policy: retry_policy(),
        },
        BARRIER_WAIT,
    )
}

/// A queue holding one intent of `vehicle(1)` under key 1, and an
/// application over it.
async fn one_intent() -> (Arc<RecordingQueue>, Arc<RecordingApplication>) {
    let queue = Arc::new(RecordingQueue::default());
    queue.insert(claimed(1, &vehicle(1))).await;
    let application = Arc::new(RecordingApplication::over(&queue));
    (queue, application)
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

/// The barrier asks the queue for the entity's newest key, and claims nothing
/// when it holds none.
#[tokio::test]
async fn the_mutation_barrier_claims_nothing_for_an_entity_without_intents() {
    let (queue, application) = three_intents().await;
    let drain = drain(&queue, &application);
    drain.synchronize(&live_target(), &vehicle(9)).await.expect("nothing to synchronize");
    assert!(application.calls().await.is_empty(), "an entity without intents claims nothing");
    assert_eq!(queue.entity_claims().await, Vec::<(Identifier, i64)>::new());
    drain.synchronize(&live_target(), &vehicle(1)).await.expect("entity synchronization succeeds");
    assert_eq!(queue.probed().await, vec![vehicle(9), vehicle(1)]);
    assert_eq!(queue.entity_claims().await[0], (vehicle(1), 1), "the barrier claims up to the newest key");
}

/// The retry delay paces the background drain, not a client waiting on its
/// own mutation.
#[tokio::test]
async fn the_mutation_barrier_applies_an_intent_waiting_in_retry_backoff() {
    let (queue, application) = one_intent().await;
    queue.defer(1, Utc::now() + TimeDelta::minutes(5)).await;
    let drain = drain(&queue, &application);
    let background = drain.execute(Utc::now()).await.expect("the pass runs");
    assert_eq!(background.acknowledged, 0, "the background drain keeps the backoff");
    drain.synchronize(&live_target(), &vehicle(1)).await.expect("the barrier applies the intent");
    assert_eq!(application.calls().await, vec![vec![1]]);
    assert_eq!(queue.pending_keys().await, Vec::<i64>::new());
}

#[tokio::test]
async fn the_mutation_barrier_applies_more_intents_than_one_batch() {
    let queue = Arc::new(RecordingQueue::default());
    for key in 1..=5 {
        queue.insert(claimed(key, &vehicle(1))).await;
    }
    let application = Arc::new(RecordingApplication::over(&queue));
    drain_claiming(&queue, &application, 2)
        .synchronize(&live_target(), &vehicle(1))
        .await
        .expect("the barrier claims until none is left");
    assert_eq!(application.calls().await, vec![vec![1, 2], vec![3, 4], vec![5]]);
    assert_eq!(queue.pending_keys().await, Vec::<i64>::new());
}

/// An intent committed after the barrier started is a concurrent write: the
/// mutation does not wait for it, or sustained writes would hold it forever.
#[tokio::test]
async fn the_mutation_barrier_does_not_wait_for_intents_queued_after_it_started() {
    let (queue, application) = one_intent().await;
    application.commit_during_first_call(claimed(9, &vehicle(1))).await;
    drain(&queue, &application)
        .synchronize(&live_target(), &vehicle(1))
        .await
        .expect("the barrier settles what was queued when it started");
    assert_eq!(application.calls().await, vec![vec![1]]);
    assert_eq!(queue.pending_keys().await, vec![9], "the later intent stays queued");
}

/// An unreachable store answers 503 and a fault 500, so the mutation is
/// answered with the failure of the intent rather than one of its own.
#[tokio::test]
async fn an_intent_that_still_fails_answers_with_its_own_error() {
    let (queue, application) = three_intents().await;
    application.fail_key(2).await;
    let before = Utc::now();
    let refused = drain(&queue, &application).synchronize(&live_target(), &vehicle(2)).await;
    let after = Utc::now();
    assert!(matches!(refused, Err(TemporalError::StoreUnavailable)), "{refused:?}");
    let retries = queue.retries().await;
    assert_eq!(retries.len(), 1, "the failure is recorded for the background drain");
    let eligible_at = retries[0].eligible_at;
    assert!(
        eligible_at >= before + TimeDelta::seconds(2) && eligible_at <= after + TimeDelta::seconds(2),
        "the first retry waits the two-second base delay: {eligible_at}"
    );
    assert_eq!(queue.pending_keys().await, vec![1, 2, 3]);
}

#[tokio::test]
async fn an_intent_held_elsewhere_and_applied_by_its_holder_lets_the_barrier_through() {
    let (queue, application) = one_intent().await;
    queue.hold_elsewhere(1, &[HolderMove::Applies]).await;
    drain(&queue, &application)
        .synchronize(&live_target(), &vehicle(1))
        .await
        .expect("the holder applied the intent");
    assert!(application.calls().await.is_empty(), "the barrier applied nothing itself");
}

#[tokio::test]
async fn an_intent_whose_holder_lease_lapses_is_applied_by_the_barrier() {
    let (queue, application) = one_intent().await;
    queue.hold_elsewhere(1, &[HolderMove::Lapses]).await;
    drain(&queue, &application)
        .synchronize(&live_target(), &vehicle(1))
        .await
        .expect("the barrier claims the lapsed intent");
    assert_eq!(application.calls().await, vec![vec![1]]);
    assert_eq!(queue.pending_keys().await, Vec::<i64>::new());
}

/// `Retry-After` carries whole seconds, so 2.3 s of lease left asks for 3.
#[tokio::test]
async fn an_intent_held_past_the_wait_answers_unsettled_with_the_remaining_lease() {
    let (queue, application) = one_intent().await;
    queue.hold_elsewhere(1, &[HolderMove::Keeps(Duration::from_millis(2300))]).await;
    let refused = drain(&queue, &application).synchronize(&live_target(), &vehicle(1)).await;
    let Err(TemporalError::HistoryUnsettled(unsettled)) = refused else {
        panic!("expected an unsettled history, got {refused:?}");
    };
    assert_eq!(unsettled.retry_after(), Duration::from_secs(3));
    assert_eq!(application.calls().await, Vec::<Vec<i64>>::new());
}

#[tokio::test]
async fn the_barrier_waits_no_longer_than_its_bound() {
    let (queue, application) = one_intent().await;
    queue.hold_elsewhere(1, &[HolderMove::Keeps(Duration::from_secs(20))]).await;
    let refused = drain(&queue, &application).synchronize(&live_target(), &vehicle(1)).await;
    assert!(matches!(refused, Err(TemporalError::HistoryUnsettled(_))), "{refused:?}");
    let waits = queue.waits().await;
    assert_eq!(waits.len(), 1);
    assert!(waits.iter().all(|wait| *wait <= BARRIER_WAIT), "{waits:?}");
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
