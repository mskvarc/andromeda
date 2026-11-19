use crate::{
    error::EntityError,
    port::{
        blocking_executor::BlockingExecutor,
        spatial_catch_up_signal::SpatialCatchUpSignal,
        spatial_snapshot::{ChangeHorizon, EntitySpatialSnapshotSource},
    },
    recording_ports::{GatedBlockingExecutor, InlineBlockingExecutor, RecordingSignal},
    spatial_reconciliation::RebuildOutcome,
    spatial_watermark::{CatchUpPass, QUERY_CATCH_UP_BOUND, SpatialWatermark},
    spatial_write_visibility::{
        geo_query::{placement, uri, vehicle_key},
        replicated_store::{Read, ReplicatedStore},
    },
};
use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    geo_rel::GeoRel,
    linear_ring::LinearRing,
    position::Position,
};
use andromeda_ngsi_ld_lexicon::identifier_spelling::IdentifierSpelling;
use andromeda_spatial_index::entity::index::{Admitted, SpatialEntityIndex};
use futures::{channel::oneshot, future::BoxFuture, poll};
use pretty_assertions::assert_eq;
use std::{collections::HashSet, sync::Arc};

/// A watermark over a store another process writes, waking a recording
/// signal.
struct Harness {
    store: Arc<ReplicatedStore>,
    signal: Arc<RecordingSignal>,
    watermark: Arc<SpatialWatermark>,
}

fn harness(executor: Arc<dyn BlockingExecutor>) -> Harness {
    let store = Arc::new(ReplicatedStore::default());
    let signal = Arc::new(RecordingSignal::default());
    let watermark = Arc::new(SpatialWatermark::new(
        Arc::new(SpatialEntityIndex::new()),
        Arc::clone(&store) as Arc<dyn EntitySpatialSnapshotSource>,
        placement(),
        executor,
        Arc::clone(&signal) as Arc<dyn SpatialCatchUpSignal>,
    ));
    Harness { store, signal, watermark }
}

fn inline_harness() -> Harness {
    harness(Arc::new(InlineBlockingExecutor))
}

/// The square two degrees wide centred on `(longitude, 0)`.
fn square(longitude: f64) -> GeometryForm {
    let corner = |x: f64, y: f64| Position::try_new(longitude + x, y, None).expect("a test position lies on the CRS84 axes");
    GeometryForm::Polygon(PolygonRings::new(vec![
        LinearRing::try_new(vec![
            corner(-1.0, -1.0),
            corner(1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, 1.0),
            corner(-1.0, -1.0),
        ])
        .expect("a test square is a closed ring"),
    ]))
}

/// The vehicles the index holds within a degree of `(longitude, 0)`, read
/// without catching up.
fn held_near(watermark: &SpatialWatermark, longitude: f64) -> Option<HashSet<IdentifierSpelling>> {
    watermark
        .index
        .query(&vehicle_key(), &GeoRel::Within, &square(longitude), Admitted::Every, usize::MAX)
}

/// The vehicles a geo query within a degree of `(longitude, 0)` is narrowed
/// to, or `None` when the store must answer it in full.
async fn near(watermark: &SpatialWatermark, longitude: f64) -> Option<HashSet<IdentifierSpelling>> {
    watermark
        .narrow(|index| index.query(&vehicle_key(), &GeoRel::Within, &square(longitude), Admitted::Every, usize::MAX))
        .await
        .expect("the store is readable")
        .map(|found| found.expect("a seeded scope answers"))
}

fn ids(raw: &[&str]) -> HashSet<IdentifierSpelling> {
    raw.iter().map(|id| IdentifierSpelling::from(&uri(id))).collect()
}

/// More vehicles than a query takes in itself.
const BACKLOG: usize = QUERY_CATCH_UP_BOUND.get() + 1;

/// A harness seeded with one vehicle at the origin, then a backlog of
/// [`BACKLOG`] vehicles half a degree east committed in one transaction.
async fn behind_a_backlog() -> Harness {
    let harness = inline_harness();
    harness.store.write("urn:ngsi-ld:Vehicle:seed", 0.0);
    harness.watermark.rebuild().await.expect("the store is readable");
    harness.store.write_many("urn:ngsi-ld:Vehicle:load", BACKLOG, 0.5);
    harness
}

#[tokio::test]
async fn a_query_takes_in_a_change_another_process_made_and_narrows_to_it() {
    let harness = inline_harness();
    harness.store.write("urn:ngsi-ld:Vehicle:far", 50.0);
    harness.watermark.rebuild().await.expect("the store is readable");
    assert_eq!(near(&harness.watermark, 0.0).await, Some(HashSet::new()));

    harness.store.write("urn:ngsi-ld:Vehicle:far", 0.5);

    assert_eq!(near(&harness.watermark, 0.0).await, Some(ids(&["urn:ngsi-ld:Vehicle:far"])));
}

#[tokio::test]
async fn a_query_reads_only_the_changes_above_the_last_horizon() {
    let harness = inline_harness();
    harness.store.write("urn:ngsi-ld:Vehicle:a", 0.0);
    harness.watermark.rebuild().await.expect("the store is readable");
    harness.store.write("urn:ngsi-ld:Vehicle:b", 0.2);
    near(&harness.watermark, 0.0).await;
    harness.store.write("urn:ngsi-ld:Vehicle:c", 0.3);
    let found = near(&harness.watermark, 0.0).await;

    assert_eq!(
        harness.store.reads(),
        vec![
            (Read::Full, 1),
            (Read::Within(ChangeHorizon::new(2, Vec::new())), 1),
            (Read::Within(ChangeHorizon::new(3, Vec::new())), 1),
        ]
    );
    assert_eq!(found.map(|found| found.len()), Some(3));
}

#[tokio::test]
async fn nothing_narrows_or_reads_before_the_first_rebuild() {
    let harness = inline_harness();
    harness.store.write("urn:ngsi-ld:Vehicle:a", 0.0);

    assert_eq!(near(&harness.watermark, 0.0).await, None);
    assert_eq!(harness.watermark.catch_up().await.expect("nothing is read"), CatchUpPass::Settled);
    assert_eq!(harness.store.reads(), Vec::new());
}

#[tokio::test]
async fn a_query_finding_more_changes_than_it_takes_in_goes_unnarrowed_and_wakes_the_background() {
    let harness = behind_a_backlog().await;

    assert_eq!(near(&harness.watermark, 0.0).await, None);
    assert_eq!(harness.signal.wakes(), 1);
    assert_eq!(
        held_near(&harness.watermark, 0.0),
        Some(ids(&["urn:ngsi-ld:Vehicle:seed"])),
        "a query takes in no part of a backlog"
    );
}

#[tokio::test]
async fn a_query_after_a_backlog_was_found_reads_nothing_and_goes_unnarrowed() {
    let harness = behind_a_backlog().await;
    near(&harness.watermark, 0.0).await;
    let reads = harness.store.reads().len();

    assert_eq!(near(&harness.watermark, 0.0).await, None);
    assert_eq!(harness.store.reads().len(), reads, "a query behind a backlog reads nothing");
    assert_eq!(harness.signal.wakes(), 2);
}

#[tokio::test]
async fn the_background_takes_in_a_backlog_a_query_found_and_queries_narrow_again() {
    let harness = behind_a_backlog().await;
    near(&harness.watermark, 0.0).await;

    assert_eq!(harness.watermark.catch_up().await.expect("the store is readable"), CatchUpPass::MoreMayRemain);

    assert_eq!(harness.store.reads().last(), Some(&(Read::Since(ChangeHorizon::new(2, Vec::new())), BACKLOG)));
    assert_eq!(near(&harness.watermark, 0.0).await.map(|found| found.len()), Some(BACKLOG + 1));
    assert_eq!(harness.watermark.catch_up().await.expect("the store is readable"), CatchUpPass::Settled);
}

#[tokio::test]
async fn the_background_takes_in_a_backlog_its_own_bounded_read_finds() {
    let harness = behind_a_backlog().await;

    assert_eq!(harness.watermark.catch_up().await.expect("the store is readable"), CatchUpPass::MoreMayRemain);

    assert_eq!(
        harness.store.reads()[1..].to_vec(),
        vec![
            (Read::Within(ChangeHorizon::new(2, Vec::new())), BACKLOG),
            (Read::Since(ChangeHorizon::new(2, Vec::new())), BACKLOG),
        ]
    );
    assert_eq!(near(&harness.watermark, 0.0).await.map(|found| found.len()), Some(BACKLOG + 1));
}

#[tokio::test]
async fn a_failed_backlog_read_leaves_queries_unnarrowed_until_a_later_catch_up() {
    let harness = behind_a_backlog().await;
    near(&harness.watermark, 0.0).await;
    *harness.store.unreachable.lock().expect("the reachability lock is not poisoned") = true;
    assert!(harness.watermark.catch_up().await.is_err());
    *harness.store.unreachable.lock().expect("the reachability lock is not poisoned") = false;

    assert_eq!(near(&harness.watermark, 0.0).await, None, "an index still behind must not narrow");

    assert_eq!(harness.watermark.catch_up().await.expect("the store is readable"), CatchUpPass::MoreMayRemain);
    assert_eq!(near(&harness.watermark, 0.0).await.map(|found| found.len()), Some(BACKLOG + 1));
}

#[tokio::test]
async fn a_change_by_a_transaction_open_at_the_previous_read_is_taken_in_once_it_commits() {
    let harness = inline_harness();
    harness.watermark.rebuild().await.expect("the store is readable");
    let late = harness.store.begin("urn:ngsi-ld:Vehicle:late", 0.5);
    harness.store.write("urn:ngsi-ld:Vehicle:other", 90.0);
    assert_eq!(near(&harness.watermark, 0.0).await, Some(HashSet::new()));

    harness.store.commit(late);

    assert_eq!(near(&harness.watermark, 0.0).await, Some(ids(&["urn:ngsi-ld:Vehicle:late"])));
    assert_eq!(
        harness.store.reads().last(),
        Some(&(Read::Within(ChangeHorizon::new(3, vec![late])), 1)),
        "the catch-up hands back the horizon of the previous read, its unseen transaction included"
    );
}

/// Each read takes in only the newest write: the open transaction holds no
/// lower bound back, so nothing taken in is read again.
#[tokio::test]
async fn an_open_transaction_does_not_make_later_catch_ups_re_read_what_they_took_in() {
    let harness = inline_harness();
    harness.watermark.rebuild().await.expect("the store is readable");
    harness.store.begin("urn:ngsi-ld:Vehicle:open", 0.5);

    for (id, longitude) in [("urn:ngsi-ld:Vehicle:a", 0.1), ("urn:ngsi-ld:Vehicle:b", 0.2), ("urn:ngsi-ld:Vehicle:c", 0.3)] {
        harness.store.write(id, longitude);
        near(&harness.watermark, 0.0).await;
    }

    assert_eq!(
        harness.store.reads()[1..].iter().map(|(_, returned)| *returned).collect::<Vec<_>>(),
        vec![1, 1, 1]
    );
}

#[tokio::test]
async fn a_query_arriving_while_a_rebuild_installs_still_narrows() {
    let executor = Arc::new(GatedBlockingExecutor::open());
    let harness = harness(Arc::clone(&executor) as Arc<dyn BlockingExecutor>);
    harness.store.write("urn:ngsi-ld:Vehicle:v", 0.5);
    harness.watermark.rebuild().await.expect("the store is readable");
    executor.hold();
    let mut rebuild = Box::pin(harness.watermark.rebuild());
    assert!(poll!(&mut rebuild).is_pending());
    assert_eq!(executor.queued(), 2, "the install waits for its worker");

    assert_eq!(near(&harness.watermark, 0.0).await, Some(ids(&["urn:ngsi-ld:Vehicle:v"])));

    executor.release();
    rebuild.await.expect("the store is readable");
    assert_eq!(near(&harness.watermark, 0.0).await, Some(ids(&["urn:ngsi-ld:Vehicle:v"])));
}

/// A rebuild of `harness`'s index that has read `v` at longitude 50 and holds
/// its answer until `release` is used or dropped, while the store has since
/// moved `v` to the origin.
struct StaleRebuild {
    harness: Harness,
    rebuild: BoxFuture<'static, Result<RebuildOutcome, EntityError>>,
    release: oneshot::Sender<()>,
}

async fn rebuild_reading_a_stale_position() -> StaleRebuild {
    let harness = inline_harness();
    harness.store.write("urn:ngsi-ld:Vehicle:v", 50.0);
    harness.watermark.rebuild().await.expect("the store is readable");
    let release = harness.store.hold_next_full_read();
    let watermark = Arc::clone(&harness.watermark);
    let mut rebuild: BoxFuture<'static, Result<RebuildOutcome, EntityError>> = Box::pin(async move { watermark.rebuild().await });
    assert!(poll!(&mut rebuild).is_pending());
    assert_eq!(harness.store.reads().len(), 2, "the rebuild has read the store");
    harness.store.write("urn:ngsi-ld:Vehicle:v", 0.5);
    StaleRebuild { harness, rebuild, release }
}

#[tokio::test]
async fn a_query_arriving_while_a_rebuild_reads_the_store_still_narrows() {
    let StaleRebuild { harness, rebuild, release } = rebuild_reading_a_stale_position().await;

    assert_eq!(near(&harness.watermark, 0.0).await, Some(ids(&["urn:ngsi-ld:Vehicle:v"])));

    drop(release);
    rebuild.await.expect("the store is readable");
}

#[tokio::test]
async fn a_query_arriving_while_the_first_rebuild_reads_the_store_goes_unnarrowed() {
    let harness = inline_harness();
    harness.store.write("urn:ngsi-ld:Vehicle:v", 0.0);
    let release = harness.store.hold_next_full_read();
    let watermark = Arc::clone(&harness.watermark);
    let mut seed: BoxFuture<'static, Result<RebuildOutcome, EntityError>> = Box::pin(async move { watermark.rebuild().await });
    assert!(poll!(&mut seed).is_pending());
    let reads = harness.store.reads();

    assert_eq!(near(&harness.watermark, 0.0).await, None);
    assert_eq!(harness.store.reads(), reads, "a query before the first rebuild reads nothing");

    drop(release);
    seed.await.expect("the store is readable");
    assert_eq!(near(&harness.watermark, 0.0).await, Some(ids(&["urn:ngsi-ld:Vehicle:v"])));
}

#[tokio::test]
async fn a_rebuild_keeps_a_newer_position_a_query_took_in_during_its_read() {
    let StaleRebuild { harness, rebuild, release } = rebuild_reading_a_stale_position().await;
    near(&harness.watermark, 0.0).await;

    drop(release);
    rebuild.await.expect("the store is readable");

    assert_eq!(held_near(&harness.watermark, 0.0), Some(ids(&["urn:ngsi-ld:Vehicle:v"])));
    assert_eq!(held_near(&harness.watermark, 50.0), Some(HashSet::new()));
}

#[tokio::test]
async fn a_rebuild_moves_no_horizon_once_seeded() {
    let harness = inline_harness();
    harness.watermark.rebuild().await.expect("the store is readable");
    harness.store.write("urn:ngsi-ld:Vehicle:a", 0.1);
    near(&harness.watermark, 0.0).await;
    harness.store.write("urn:ngsi-ld:Vehicle:b", 0.2);
    harness.watermark.rebuild().await.expect("the store is readable");

    assert_eq!(
        near(&harness.watermark, 0.0).await,
        Some(ids(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]))
    );
    assert_eq!(
        harness.store.reads().last(),
        Some(&(Read::Within(ChangeHorizon::new(2, Vec::new())), 1)),
        "the catch-up after the rebuild reads above the horizon the last catch-up recorded"
    );
}

#[tokio::test]
async fn a_step_whose_query_is_dropped_still_completes() {
    let executor = Arc::new(GatedBlockingExecutor::open());
    let harness = harness(Arc::clone(&executor) as Arc<dyn BlockingExecutor>);
    harness.watermark.rebuild().await.expect("the store is readable");
    executor.hold();
    harness.store.write("urn:ngsi-ld:Vehicle:v", 0.5);
    let mut query = Box::pin(near(&harness.watermark, 0.0));
    assert!(poll!(&mut query).is_pending());
    assert_eq!(executor.queued(), 2, "the step's read waits for its worker");

    drop(query);
    executor.release();

    assert_eq!(near(&harness.watermark, 0.0).await, Some(ids(&["urn:ngsi-ld:Vehicle:v"])));
    assert_eq!(
        harness.store.reads().last(),
        Some(&(Read::Within(ChangeHorizon::new(2, Vec::new())), 0)),
        "the dropped step recorded its read"
    );
}

#[tokio::test]
async fn own_writes_wake_the_background_once_every_thousand() {
    let harness = inline_harness();
    harness.watermark.rebuild().await.expect("the store is readable");

    harness.watermark.writes_committed(999);
    assert_eq!(harness.signal.wakes(), 0);
    harness.watermark.writes_committed(1);
    assert_eq!(harness.signal.wakes(), 1);

    assert_eq!(harness.watermark.catch_up().await.expect("the store is readable"), CatchUpPass::Settled);
    harness.watermark.writes_committed(999);
    assert_eq!(harness.signal.wakes(), 1, "a catch-up counts from when it read");
    harness.watermark.writes_committed(1);
    assert_eq!(harness.signal.wakes(), 2);
}
