//! A polygon geo property round trips in every representation and geo query.

use andromeda_test_suite::{
    broker,
    geometry_round_trip::{GeometryTrip, polygon, rectangle, run},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::model::geometry::Shape;

/// Pins: A Polygon `GeoProperty` makes a full round trip: it reads back with its exact type and coordinates in the
/// normalized, concise, simplified and `GeoJSON` representations, an update to another Polygon reads back the same
/// way, and a within geo-query selects it where it lies inside and not where it does not
/// Clause: 4.7.1, 4.6.3, 4.5.4, 4.5.16, 4.10, 5.6.1, 5.6.2, 5.7.1, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_geo_0024_a_polygon_geo_property_round_trips_in_every_representation_and_geo_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let trip = GeometryTrip {
        first: Shape::Polygon(polygon(&[
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)],
            &[(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0), (2.0, 2.0)],
        ])?),
        second: Shape::Polygon(polygon(&[
            &[(20.0, 20.0), (30.0, 20.0), (30.0, 30.0), (20.0, 30.0), (20.0, 20.0)],
            &[(22.0, 22.0), (22.0, 24.0), (24.0, 24.0), (24.0, 22.0), (22.0, 22.0)],
        ])?),
        first_area: rectangle(-1.0, -1.0, 11.0, 11.0)?,
        second_area: rectangle(19.0, 19.0, 31.0, 31.0)?,
        far_area: rectangle(100.0, 60.0, 101.0, 61.0)?,
    };
    run(&broker, &namespace, &trip).await
}
