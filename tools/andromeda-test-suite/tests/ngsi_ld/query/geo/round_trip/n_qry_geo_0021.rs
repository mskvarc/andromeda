//! A multi point geo property round trips in every representation and geo query.

use andromeda_test_suite::{
    broker,
    geometry_round_trip::{GeometryTrip, positions, rectangle, run},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::model::geometry::Shape;

/// Pins: A `MultiPoint` `GeoProperty` makes a full round trip: it reads back with its exact type and coordinates in
/// the normalized, concise, simplified and `GeoJSON` representations, an update to another `MultiPoint` reads back
/// the same way, and a within geo-query selects it where it lies inside and not where it does not
/// Clause: 4.7.1, 4.6.3, 4.5.4, 4.5.16, 4.10, 5.6.1, 5.6.2, 5.7.1, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_geo_0021_a_multi_point_geo_property_round_trips_in_every_representation_and_geo_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let trip = GeometryTrip {
        first: Shape::MultiPoint(positions(&[(13.4, 52.5), (13.5, 52.6)])?),
        second: Shape::MultiPoint(positions(&[(14.1, 51.2), (14.3, 51.4), (14.5, 51.6)])?),
        first_area: rectangle(13.0, 52.0, 14.0, 53.0)?,
        second_area: rectangle(14.0, 51.0, 15.0, 52.0)?,
        far_area: rectangle(100.0, 60.0, 101.0, 61.0)?,
    };
    run(&broker, &namespace, &trip).await
}
