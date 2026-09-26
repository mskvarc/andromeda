//! The `store-verified` mode: no index narrows a query.

use crate::{
    spatial_acceleration::SpatialAcceleration,
    spatial_write_visibility::geo_query::{narrowed_ids, near_origin},
};
use pretty_assertions::assert_eq;

#[tokio::test]
async fn under_store_verified_the_store_evaluates_every_geo_query_unnarrowed() {
    assert_eq!(
        narrowed_ids(SpatialAcceleration::StoreVerified, &near_origin("urn:ngsi-ld:Vehicle:any")).await,
        None
    );
}
