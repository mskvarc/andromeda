use crate::{
    branch_condition::BranchCondition,
    error::SnapshotError,
    executor::{attempts::PreparationEnd, lease_renewal::LeaseRenewal},
    store::SnapshotStore,
    store_double::{ClaimHold, Renewal, StoreDouble},
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::{Snapshot, SnapshotPriority, SnapshotStatus};
use chrono::{Duration as ChronoDuration, Utc};
use futures::future::pending;
use pretty_assertions::assert_eq;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::Notify,
    time::{Instant, sleep, timeout},
};

/// The lease of the claim under test; it is renewed every 15 s.
const LEASE: Duration = Duration::from_secs(30);

fn snapshot_id() -> Identifier {
    "urn:ngsi-ld:Snapshot:lease-renewal".parse().expect("valid snapshot identifier")
}

/// A store holding one preparing Snapshot, claimed by the worker under test,
/// answering renewals as `renewal` says.
async fn store(renewal: Renewal) -> StoreDouble {
    let now = Utc::now();
    let snapshot = Snapshot::builder()
        .id(snapshot_id())
        .snapshot_priority(SnapshotPriority::default())
        .status(SnapshotStatus::Preparing)
        .created_at(now)
        .modified_at(now)
        .expires_at(now + ChronoDuration::hours(1))
        .build();
    let store = StoreDouble::new(snapshot, BranchCondition::Current, ClaimHold::Held).with_renewal(renewal);
    let job = store.next_preparing(LeaseHolder::mint(), now).await.expect("the double claims");
    assert!(job.is_some(), "the preparing Snapshot is claimed");
    store
}

fn renewal<'a>(store: &'a StoreDouble, id: &'a Identifier) -> LeaseRenewal<'a> {
    LeaseRenewal {
        store,
        id,
        worker: LeaseHolder::mint(),
        lease: LEASE,
    }
}

/// An execution that completes after `duration`.
async fn completing_after(duration: Duration) -> Result<PreparationEnd, SnapshotError> {
    sleep(duration).await;
    Ok(PreparationEnd::Completed)
}

/// Clause 5.16.2.4 sets the status to `success` once all information has been
/// stored. An execution holding the row lock a renewal waits on keeps being
/// polled while that renewal is pending, so it completes and releases the lock.
#[tokio::test(start_paused = true)]
async fn an_attempt_completes_while_its_lease_renewal_waits_on_a_lock_the_attempt_holds() {
    let gate = Arc::new(Notify::new());
    let store = store(Renewal::GatedOn(Arc::clone(&gate))).await;
    let id = snapshot_id();
    let started = Instant::now();
    let execution = async {
        sleep(LEASE).await;
        gate.notify_one();
        Ok(PreparationEnd::Completed)
    };

    let end = timeout(LEASE * 4, renewal(&store, &id).drive(execution)).await;

    assert_eq!(
        end.expect("the execution is not starved").expect("the execution completes"),
        PreparationEnd::Completed
    );
    assert!(store.renewals() >= 1, "a renewal was pending while the execution ran");
    assert!(started.elapsed() < LEASE * 2, "the execution took {:?}", started.elapsed());
}

/// A renewal that finds the claim gone ends the preparation as a lost claim,
/// whose outcome is not this worker's to record.
#[tokio::test(start_paused = true)]
async fn a_lease_renewal_that_finds_the_claim_lost_ends_the_preparation_as_claim_lost() {
    let store = store(Renewal::Lost).await;
    let id = snapshot_id();
    let started = Instant::now();

    let end = renewal(&store, &id).drive(pending()).await;

    assert_eq!(end.expect("a lost claim is not an error"), PreparationEnd::ClaimLost);
    assert_eq!(store.renewals(), 1);
    assert_eq!(started.elapsed(), LEASE / 2);
}

/// A renewal the store fails ends the preparation with the store's failure.
#[tokio::test(start_paused = true)]
async fn a_failed_lease_renewal_ends_the_preparation_with_its_error() {
    let store = store(Renewal::Fails).await;
    let id = snapshot_id();

    let end = renewal(&store, &id).drive(pending()).await;

    assert!(matches!(end, Err(SnapshotError::StoreSaturated)), "{end:?}");
    assert_eq!(store.renewals(), 1);
}

/// The claim is renewed every half lease for as long as the execution runs.
#[tokio::test(start_paused = true)]
async fn the_lease_is_renewed_every_half_lease_while_the_attempt_runs() {
    let store = store(Renewal::Retained).await;
    let id = snapshot_id();

    let end = renewal(&store, &id).drive(completing_after(LEASE * 9 / 4)).await;

    assert_eq!(end.expect("the execution completes"), PreparationEnd::Completed);
    assert_eq!(store.renewals(), 4, "renewed at 15, 30, 45 and 60 s");
}

/// An execution shorter than half a lease never renews its claim.
#[tokio::test(start_paused = true)]
async fn an_attempt_that_finishes_before_the_first_renewal_renews_nothing() {
    let store = store(Renewal::Retained).await;
    let id = snapshot_id();

    let end = renewal(&store, &id).drive(completing_after(LEASE / 4)).await;

    assert_eq!(end.expect("the execution completes"), PreparationEnd::Completed);
    assert_eq!(store.renewals(), 0);
}

/// An execution that ends in the instant a renewal is due keeps its result,
/// even when that renewal would find the claim lost.
#[tokio::test(start_paused = true)]
async fn an_attempt_result_wins_over_a_renewal_due_in_the_same_instant() {
    let store = store(Renewal::Lost).await;
    let id = snapshot_id();

    let end = renewal(&store, &id).drive(completing_after(LEASE / 2)).await;

    assert_eq!(end.expect("the execution completes"), PreparationEnd::Completed);
}
