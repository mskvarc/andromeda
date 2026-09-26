//! The `watermark` mode: the index catches up from the store before it narrows.

use crate::{
    port::spatial_snapshot::EntitySpatialSnapshotSource,
    query::{service_test_double::service_narrowing_through, spatial_prefilter::SpatialNarrowing},
    spatial_acceleration::SpatialAcceleration,
    spatial_watermark::SpatialWatermark,
    spatial_write_visibility::{
        geo_query::{narrowed_ids, near_origin, placement, uri},
        replicated_store::ReplicatedStore,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use pretty_assertions::assert_eq;
use std::sync::Arc;

#[tokio::test]
async fn under_watermark_an_entity_another_process_moved_into_range_is_a_candidate() {
    let store = Arc::new(ReplicatedStore::default());
    store.write("urn:ngsi-ld:Vehicle:parked", 90.0);
    store.write("urn:ngsi-ld:Vehicle:remote", 50.0);
    let watermark = Arc::new(SpatialWatermark::new(
        Arc::new(SpatialEntityIndex::new()),
        Arc::clone(&store) as Arc<dyn EntitySpatialSnapshotSource>, // clone: the watermark reads the store the test writes
        placement(),
    ));
    watermark.rebuild().await.expect("the store is readable");
    let query = near_origin("urn:ngsi-ld:Vehicle:remote");
    assert_eq!(narrowed_ids(SpatialAcceleration::Watermark(Arc::clone(&watermark)), &query).await, None); // clone: the query service shares the watermark

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
    let watermark = Arc::new(SpatialWatermark::new(
        Arc::new(SpatialEntityIndex::new()),
        Arc::clone(&store) as Arc<dyn EntitySpatialSnapshotSource>, // clone: the watermark reads the store the test writes
        placement(),
    ));
    watermark.rebuild().await.expect("the store is readable");
    *store.unreachable.lock().expect("the reachability lock is not poisoned") = true;

    let query = near_origin("urn:ngsi-ld:Vehicle:remote");
    match service_narrowing_through(SpatialAcceleration::Watermark(watermark))
        .spatial_prefilter(&ContextTarget::Live, &query)
        .await
    {
        SpatialNarrowing::Query(unchanged) => assert!(Arc::ptr_eq(&unchanged, &query), "the query reaches the store as the client wrote it"),
        SpatialNarrowing::EmptyResult => panic!("an index that could not catch up must not answer"),
    }
}
