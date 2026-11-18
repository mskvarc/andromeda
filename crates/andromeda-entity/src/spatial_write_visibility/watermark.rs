//! The `watermark` mode: the index catches up from the store before it narrows.

use crate::{
    port::{blocking_executor::BlockingExecutor, spatial_catch_up_signal::SpatialCatchUpSignal, spatial_snapshot::EntitySpatialSnapshotSource},
    recording_ports::{InlineBlockingExecutor, RecordingSignal},
    spatial_acceleration::SpatialAcceleration,
    spatial_watermark::{QUERY_CATCH_UP_BOUND, SpatialWatermark},
    spatial_write_visibility::{
        geo_query::{narrowed_ids, near_origin, placement, uri},
        replicated_store::ReplicatedStore,
    },
};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use pretty_assertions::assert_eq;
use std::sync::Arc;

/// A watermark over `store`, seeded from it.
async fn seeded_watermark(store: &Arc<ReplicatedStore>) -> Arc<SpatialWatermark> {
    let watermark = Arc::new(SpatialWatermark::new(
        Arc::new(SpatialEntityIndex::new()),
        Arc::clone(store) as Arc<dyn EntitySpatialSnapshotSource>,
        placement(),
        Arc::new(InlineBlockingExecutor) as Arc<dyn BlockingExecutor>,
        Arc::new(RecordingSignal::default()) as Arc<dyn SpatialCatchUpSignal>,
    ));
    watermark.rebuild().await.expect("the store is readable");
    watermark
}

#[tokio::test]
async fn under_watermark_an_entity_another_process_moved_into_range_is_a_candidate() {
    let store = Arc::new(ReplicatedStore::default());
    store.write("urn:ngsi-ld:Vehicle:parked", 90.0);
    store.write("urn:ngsi-ld:Vehicle:remote", 50.0);
    let watermark = seeded_watermark(&store).await;
    let query = near_origin("urn:ngsi-ld:Vehicle:remote");
    assert_eq!(
        narrowed_ids(SpatialAcceleration::Watermark(Arc::clone(&watermark)), &query).await,
        Some(Vec::new())
    );

    store.write("urn:ngsi-ld:Vehicle:remote", 0.001);

    assert_eq!(
        narrowed_ids(SpatialAcceleration::Watermark(watermark), &query).await,
        Some(vec![uri("urn:ngsi-ld:Vehicle:remote")])
    );
}

#[tokio::test]
async fn under_watermark_an_unreadable_store_leaves_the_query_unnarrowed() {
    let store = Arc::new(ReplicatedStore::default());
    store.write("urn:ngsi-ld:Vehicle:parked", 90.0);
    let watermark = seeded_watermark(&store).await;
    *store.unreachable.lock().expect("the reachability lock is not poisoned") = true;

    assert_eq!(
        narrowed_ids(SpatialAcceleration::Watermark(watermark), &near_origin("urn:ngsi-ld:Vehicle:remote")).await,
        None,
        "an index that could not catch up must not narrow"
    );
}

/// More changes than a query takes in itself are left to the background
/// catch-up, and the store answers the query in full meanwhile.
#[tokio::test]
async fn under_watermark_a_backlog_beyond_a_query_s_bound_leaves_it_unnarrowed() {
    let store = Arc::new(ReplicatedStore::default());
    store.write("urn:ngsi-ld:Vehicle:parked", 90.0);
    let watermark = seeded_watermark(&store).await;
    store.write_many("urn:ngsi-ld:Vehicle:load", QUERY_CATCH_UP_BOUND.get() + 1, 40.0);
    store.write("urn:ngsi-ld:Vehicle:remote", 0.001);

    assert_eq!(
        narrowed_ids(SpatialAcceleration::Watermark(watermark), &near_origin("urn:ngsi-ld:Vehicle:remote")).await,
        None,
        "an index behind the store must not narrow"
    );
}
