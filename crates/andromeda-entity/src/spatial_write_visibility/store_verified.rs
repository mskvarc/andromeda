//! The `store-verified` mode: no index narrows a query.

use crate::{
    query::{service_test_double::service_narrowing_through, spatial_prefilter::SpatialNarrowing},
    spatial_acceleration::SpatialAcceleration,
    spatial_write_visibility::geo_query::near_origin,
};
use andromeda_context_space::context_target::ContextTarget;
use std::sync::Arc;

#[tokio::test]
async fn under_store_verified_the_store_evaluates_every_geo_query_unnarrowed() {
    let query = near_origin("urn:ngsi-ld:Vehicle:any");
    match service_narrowing_through(SpatialAcceleration::StoreVerified)
        .spatial_prefilter(&ContextTarget::Live, &query)
        .await
    {
        SpatialNarrowing::Query(unchanged) => assert!(Arc::ptr_eq(&unchanged, &query), "the query reaches the store as the client wrote it"),
        SpatialNarrowing::EmptyResult => panic!("store-verified consistency never answers from an index"),
    }
}
