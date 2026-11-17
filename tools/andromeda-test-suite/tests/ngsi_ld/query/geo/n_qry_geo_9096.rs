//! Near reads a location instance with a datasetId across the antimeridian and apart from an antipodal corner in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    geo_parity::{GeoCase, expect_geo_query_and_subscription_alike},
    geometry_round_trip::{polygon, rectangle},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        Entity,
        EntityId,
        GeoProperty,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: for Zones whose only location instance carries a datasetId and is the polygon (170,-10), (-170,-10),
/// (-170,10), (170,10) for the first and the square (-10,-10) to (10,10) for the second, Query Entities and a
/// Subscription with near;maxDistance==25000 around (180,0) select the first, with near;maxDistance==1000 around the
/// square (0,0) to (10,10) the second, with near;minDistance==1000 around it the first, with near;maxDistance==1000
/// around the square (170,0) to (180,10) the first and with near;minDistance==1000 around it the second; the first
/// polygon is the one 20 degrees wide around the antimeridian, and each square lies 16 890 km from the polygon one of
/// whose corners is the antipode of one of its own
/// Clause: 4.10, 5.7.2.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_geo_9096_near_reads_a_location_instance_with_a_dataset_id_across_the_antimeridian_and_apart_from_an_antipodal_corner_in_a_query_and_a_subscription_alike()
-> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let zone_type = namespace.entity_type("Zone")?;
    let antimeridian = namespace.urn("Zone", "antimeridian")?;
    let centre = namespace.urn("Zone", "centre")?;
    let dataset = namespace.dataset_id("Dataset", "survey")?;
    let zone = |id: &EntityId, location: Shape| {
        Entity::builder()
            .id(id.clone())
            .entity_type(zone_type.clone())
            .geo_property("location", GeoProperty::builder(location).dataset_id(dataset.clone()))
            .build()
    };
    let entities = [
        zone(
            &antimeridian,
            Shape::Polygon(polygon(&[&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)]])?),
        )?,
        zone(&centre, rectangle(-10.0, -10.0, 10.0, 10.0)?)?,
    ];
    let by_the_antimeridian = geoquery::near_max(25_000, Shape::Point(Position::try_new(180.0, 0.0)?)).build()?;
    let near_the_square = geoquery::near_max(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?;
    let beyond_the_square = geoquery::near_min(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?;
    let near_the_far_square = geoquery::near_max(1000, rectangle(170.0, 0.0, 180.0, 10.0)?).build()?;
    let beyond_the_far_square = geoquery::near_min(1000, rectangle(170.0, 0.0, 180.0, 10.0)?).build()?;
    let cases = [
        GeoCase {
            label: "near;maxDistance==25000 around (180,0)",
            geo_q: &by_the_antimeridian,
            selects: &[&antimeridian],
        },
        GeoCase {
            label: "near;maxDistance==1000 around the square (0,0) to (10,10)",
            geo_q: &near_the_square,
            selects: &[&centre],
        },
        GeoCase {
            label: "near;minDistance==1000 around the square (0,0) to (10,10)",
            geo_q: &beyond_the_square,
            selects: &[&antimeridian],
        },
        GeoCase {
            label: "near;maxDistance==1000 around the square (170,0) to (180,10)",
            geo_q: &near_the_far_square,
            selects: &[&antimeridian],
        },
        GeoCase {
            label: "near;minDistance==1000 around the square (170,0) to (180,10)",
            geo_q: &beyond_the_far_square,
            selects: &[&centre],
        },
    ];
    expect_geo_query_and_subscription_alike(&broker, &namespace, &zone_type, &entities, &cases).await
}
