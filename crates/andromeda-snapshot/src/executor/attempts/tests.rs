use crate::{
    branch_condition::BranchCondition,
    error::SnapshotError,
    executor::attempts::{PREPARATION_ATTEMPTS, PreparationClaim, PreparationEnd, QueryCounts},
    snapshot_invariant::SnapshotInvariant,
    snapshot_validation_reason::SnapshotValidationReason,
    store::SnapshotStore,
    store_double::{ClaimHold, StoreDouble},
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultStatus, Snapshot, SnapshotPriority, SnapshotStatus};
use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemDetail};
use chrono::{Duration as ChronoDuration, Utc};
use futures::future::BoxFuture;
use pretty_assertions::assert_eq;
use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};
use tokio::time::Instant;

/// The lease a transient failure must never be left to wait out.
const LEASE: Duration = Duration::from_secs(30);

fn snapshot_id() -> Identifier {
    "urn:ngsi-ld:Snapshot:attempts".parse().expect("valid snapshot identifier")
}

/// A store holding one preparing Snapshot, claimed by the worker under test.
async fn store(claim: ClaimHold) -> StoreDouble {
    let now = Utc::now();
    let snapshot = Snapshot::builder()
        .id(snapshot_id())
        .snapshot_priority(SnapshotPriority::default())
        .status(SnapshotStatus::Preparing)
        .created_at(now)
        .modified_at(now)
        .expires_at(now + ChronoDuration::hours(1))
        .build();
    let store = StoreDouble::new(snapshot, BranchCondition::Current, claim);
    let job = store.next_preparing(LeaseHolder::mint(), now).await.expect("the double claims");
    assert!(job.is_some(), "the preparing Snapshot is claimed");
    store
}

fn claim<'a>(store: &'a StoreDouble, id: &'a Identifier) -> PreparationClaim<'a> {
    PreparationClaim {
        store,
        id,
        worker: LeaseHolder::mint(),
        lease: LEASE,
        queries: QueryCounts { entity: 2, temporal: 1 },
    }
}

/// An attempt that fails with `failure` the first `failing` times it runs,
/// counting every run in `runs`.
fn failing_first<'a>(runs: &'a AtomicU32, failing: u32, failure: fn() -> SnapshotError) -> impl FnMut() -> BoxFuture<'a, Result<(), SnapshotError>> {
    move || {
        let run = runs.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { if run < failing { Err(failure()) } else { Ok(()) } })
    }
}

/// A preparation that loses one deadlock race restarts at once under the same
/// claim and completes, rather than parking until its lease expires (clause
/// 5.16.1.4 moves every Snapshot out of `preparing`).
#[tokio::test(start_paused = true)]
async fn a_preparation_contended_once_restarts_promptly_and_completes() {
    let store = store(ClaimHold::Held).await;
    let id = snapshot_id();
    let runs = AtomicU32::new(0);
    let started = Instant::now();

    let end = claim(&store, &id).run(failing_first(&runs, 1, || SnapshotError::Contended)).await;

    assert_eq!(end.expect("the restarted preparation completes"), PreparationEnd::Completed);
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    assert_eq!(store.restarts(), 1, "the staging branch is reset once before the second attempt");
    assert!(store.take_recorded().is_empty(), "no failure status is recorded");
    assert!(started.elapsed() < Duration::from_secs(1), "the restart waited {:?}", started.elapsed());
}

/// A preparation that stays contended records `failure` with one failed
/// execution result per query (Table 5.2.41-2) once its budget is spent, well
/// inside the lease.
#[tokio::test(start_paused = true)]
async fn a_preparation_contended_on_every_attempt_records_failure_within_its_budget() {
    let store = store(ClaimHold::Held).await;
    let id = snapshot_id();
    let runs = AtomicU32::new(0);
    let started = Instant::now();

    let end = claim(&store, &id).run(failing_first(&runs, u32::MAX, || SnapshotError::Contended)).await;

    assert_eq!(end.expect("the failure is recorded"), PreparationEnd::Failed);
    assert_eq!(runs.load(Ordering::SeqCst), PREPARATION_ATTEMPTS);
    assert_eq!(store.restarts(), PREPARATION_ATTEMPTS - 1);
    let recorded = store.take_recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].status, SnapshotStatus::Failure);
    let entity = recorded[0].queries_details.as_deref().expect("one result per entity query");
    let temporal = recorded[0].temporal_queries_details.as_deref().expect("one result per temporal query");
    assert_eq!(entity.len(), 2);
    assert_eq!(temporal.len(), 1);
    // A preparation no attempt completed failed during the operation
    // execution, clause 5.5.2's `InternalError`.
    assert!(
        entity
            .iter()
            .chain(temporal)
            .all(|detail| detail.result_status == ExecutionResultStatus::Failure
                && detail.problem_details.as_ref().map(ProblemDetail::error_type) == Some(NgsiLdErrorType::InternalError)),
        "every execution result reports the failure"
    );
    assert!(started.elapsed() < Duration::from_secs(1), "the budget waited {:?}", started.elapsed());
}

/// A failure a fresh attempt would repeat is recorded at once, without
/// restarting the claim.
#[tokio::test(start_paused = true)]
async fn a_persistent_preparation_failure_is_recorded_without_a_restart() {
    let store = store(ClaimHold::Held).await;
    let id = snapshot_id();
    let runs = AtomicU32::new(0);

    let end = claim(&store, &id)
        .run(failing_first(&runs, u32::MAX, || {
            SnapshotError::from(SnapshotValidationReason::StatusTransition {
                status: SnapshotStatus::Preparing,
            })
        }))
        .await;

    assert_eq!(end.expect("the failure is recorded"), PreparationEnd::Failed);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(store.restarts(), 0);
    assert_eq!(store.take_recorded().len(), 1);
}

/// A clone executes no queries of its own, so its recorded failure carries no
/// execution results.
#[tokio::test(start_paused = true)]
async fn a_failed_preparation_without_queries_records_no_execution_results() {
    let store = store(ClaimHold::Held).await;
    let id = snapshot_id();
    let runs = AtomicU32::new(0);
    let mut clone_claim = claim(&store, &id);
    clone_claim.queries = QueryCounts::default();

    let end = clone_claim
        .run(failing_first(&runs, u32::MAX, || {
            SnapshotError::from(SnapshotInvariant::PreparationChangedUnderPromotion)
        }))
        .await;

    assert_eq!(end.expect("the failure is recorded"), PreparationEnd::Failed);
    let recorded = store.take_recorded();
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0].queries_details.is_none() && recorded[0].temporal_queries_details.is_none());
}

/// A worker that lost its claim neither re-runs the preparation nor records a
/// status for it: the claim's new holder owns the outcome.
#[tokio::test(start_paused = true)]
async fn a_lost_claim_stops_the_restarts_without_recording_a_status() {
    let store = store(ClaimHold::Lost).await;
    let id = snapshot_id();
    let runs = AtomicU32::new(0);

    let end = claim(&store, &id).run(failing_first(&runs, u32::MAX, || SnapshotError::Contended)).await;

    assert_eq!(end.expect("a lost claim is not an error"), PreparationEnd::ClaimLost);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(store.restarts(), 1);
    assert!(store.take_recorded().is_empty());
}
