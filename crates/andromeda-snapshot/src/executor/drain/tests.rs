use crate::{
    branch_condition::BranchCondition,
    executor::{
        attempts::{PreparationClaim, PreparationEnd, QueryCounts},
        drain::DrainWorker,
    },
    store::{SnapshotPreparationJob, SnapshotStore},
    store_double::{ClaimHold, CloneCopy, StoreDouble},
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::snapshot::{Snapshot, SnapshotPriority, SnapshotStatus};
use chrono::{Duration as ChronoDuration, Utc};
use futures::{executor::block_on, future::BoxFuture};
use pretty_assertions::assert_eq;
use std::{sync::Mutex, time::Duration};

const LEASE: Duration = Duration::from_secs(30);

fn source_id() -> Uri {
    "urn:ngsi-ld:Snapshot:clone-source".parse().expect("valid snapshot identifier")
}

fn clone_id() -> Uri {
    "urn:ngsi-ld:Snapshot:clone-target".parse().expect("valid snapshot identifier")
}

fn snapshot(id: Uri, status: SnapshotStatus) -> Snapshot {
    let now = Utc::now();
    Snapshot::builder()
        .id(id)
        .snapshot_priority(SnapshotPriority::default())
        .status(status)
        .created_at(now)
        .modified_at(now)
        .expires_at(now + ChronoDuration::hours(1))
        .build()
}

/// A source in `status` and a preparing clone of it.
fn source_and_clone(status: SnapshotStatus) -> StoreDouble {
    StoreDouble::new(snapshot(source_id(), status), BranchCondition::Current, ClaimHold::Held)
        .with_clone(snapshot(clone_id(), SnapshotStatus::Preparing), &source_id())
}

/// Prepare one claimed job the way the preparation does: a clone copies its
/// source, a capture promotes what it staged as `success`. `before_capture`
/// runs once a capture is claimed, before it promotes. Every claimed job is
/// logged in `claimed`.
fn prepare<'a>(
    store: &'a StoreDouble,
    worker: LeaseHolder,
    claimed: &'a Mutex<Vec<Uri>>,
    before_capture: &'a (dyn Fn() + Sync),
) -> impl Fn(SnapshotPreparationJob) -> BoxFuture<'a, Option<PreparationEnd>> {
    move |job| {
        Box::pin(async move {
            if let Ok(mut claimed) = claimed.lock() {
                claimed.push(job.snapshot.id.clone()); // clone: the log owns the identifiers it records.
            }
            let claim = PreparationClaim {
                store,
                id: &job.snapshot.id,
                worker,
                lease: LEASE,
                queries: QueryCounts::of(&job),
            };
            let end = if let Some(source) = &job.clone_source_id {
                claim.run(|| store.promote_clone(source, &job.snapshot.id, worker)).await
            } else {
                before_capture();
                claim
                    .run(|| store.promote_capture(&job.snapshot.id, worker, SnapshotStatus::Success, None, None))
                    .await
            };
            end.ok()
        })
    }
}

fn drain_worker(store: &StoreDouble, worker: LeaseHolder) -> DrainWorker<'_> {
    DrainWorker {
        store,
        worker,
        lease: LEASE,
        max_per_round: 2,
    }
}

/// The recorded terminal statuses, by Snapshot.
fn recorded(store: &StoreDouble) -> Vec<(Uri, SnapshotStatus)> {
    store.take_recorded().into_iter().map(|record| (record.id, record.status)).collect()
}

/// A clone created while its source prepares is not claimed until the source
/// ends: the round that prepares the source cannot claim it, and the same pass
/// claims and copies it in the next round, from the ended source, without an
/// attempt spent on waiting (clause 5.16.2.4).
#[tokio::test(start_paused = true)]
async fn a_clone_waits_for_its_preparing_source_and_copies_it_once_the_source_succeeds() {
    let store = source_and_clone(SnapshotStatus::Preparing);
    let worker = LeaseHolder::mint();
    let claimed = Mutex::new(Vec::new());

    let moved = drain_worker(&store, worker)
        .drain(prepare(&store, worker, &claimed, &|| {}))
        .await
        .expect("the drain completes");

    assert_eq!(moved, 2);
    assert_eq!(claimed.lock().expect("claim log").as_slice(), [source_id(), clone_id()]);
    assert_eq!(
        store.take_copies(),
        [CloneCopy {
            target: clone_id(),
            source_status: SnapshotStatus::Success,
        }]
    );
    assert_eq!(store.restarts(), 0, "no attempt of the clone was spent waiting");
    assert_eq!(
        recorded(&store),
        [(source_id(), SnapshotStatus::Success), (clone_id(), SnapshotStatus::Success)]
    );
}

/// A source deleted while its clone waits leaves nothing to copy, so the clone
/// ends `failure` rather than succeeding over an empty branch.
#[tokio::test(start_paused = true)]
async fn a_clone_whose_source_is_deleted_while_it_waits_ends_in_failure() {
    let store = source_and_clone(SnapshotStatus::Preparing);
    let worker = LeaseHolder::mint();
    let claimed = Mutex::new(Vec::new());
    let delete_source = || {
        let deleted = block_on(store.delete(&source_id())).expect("the source is deleted");
        assert_eq!(deleted, 1);
    };

    let moved = drain_worker(&store, worker)
        .drain(prepare(&store, worker, &claimed, &delete_source))
        .await
        .expect("the drain completes");

    assert_eq!(moved, 1, "only the clone was moved out of preparing by this worker");
    assert_eq!(claimed.lock().expect("claim log").as_slice(), [source_id(), clone_id()]);
    assert!(store.take_copies().is_empty(), "nothing is copied from a deleted source");
    assert_eq!(store.restarts(), 0, "a missing source is not retried");
    assert_eq!(recorded(&store), [(clone_id(), SnapshotStatus::Failure)]);
}

/// A clone of a source that already ended is claimed in the first round.
#[tokio::test(start_paused = true)]
async fn a_clone_of_an_ended_source_is_claimed_at_once() {
    let store = source_and_clone(SnapshotStatus::Empty);
    let worker = LeaseHolder::mint();
    let claimed = Mutex::new(Vec::new());

    let moved = drain_worker(&store, worker)
        .drain(prepare(&store, worker, &claimed, &|| {}))
        .await
        .expect("the drain completes");

    assert_eq!(moved, 1);
    assert_eq!(claimed.lock().expect("claim log").as_slice(), [clone_id()]);
    assert_eq!(
        store.take_copies(),
        [CloneCopy {
            target: clone_id(),
            source_status: SnapshotStatus::Empty,
        }]
    );
}
