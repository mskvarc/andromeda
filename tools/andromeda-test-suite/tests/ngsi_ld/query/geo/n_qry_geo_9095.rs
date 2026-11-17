//! Near reads every edge as a great-circle arc and every polygon within one hemisphere in a query and a subscription alike.

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
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: for Zones located at the polygon (170,-10), (-170,-10), (-170,10), (170,10), at the square (-30,50) to
/// (30,60) and at the square (-10,-10) to (10,10), Query Entities and a Subscription with near;maxDistance==25000
/// around (180,0) select the first, with near;maxDistance==1000 around the square (0,0) to (10,10) the third, with
/// near;minDistance==1000 around it the first two, with near;maxDistance==1000 around the square (170,0) to (180,10)
/// the first, with near;minDistance==1000 around it the last two, and with near;maxDistance==1000 around (0,63) the
/// second; an edge goes the short way round, so the first polygon is the one 20 degrees wide around the antimeridian
/// and 16 890 km from the square (0,0) to (10,10) whose corner (10,10) is the antipode of its corner (-170,-10), the
/// north edge of the second bows north to 63.43 over (0,63), and the third is 16 890 km from the square (170,0) to
/// (180,10) whose corner (170,10) is the antipode of its corner (-10,-10)
/// Clause: 4.10, 5.7.2.4, 5.8.6
/// Recorded: the polygon a ring encloses is the one inside the hemisphere its positions lie in, as `PostGIS` reads
/// `geography`
#[gtest]
#[tokio::test]
async fn n_qry_geo_9095_near_reads_every_edge_as_a_great_circle_arc_and_every_polygon_within_one_hemisphere_in_a_query_and_a_subscription_alike() -> Result<()>
{
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let zone_type = namespace.entity_type("Zone")?;
    let antimeridian = namespace.urn("Zone", "antimeridian")?;
    let bowed = namespace.urn("Zone", "bowed")?;
    let centre = namespace.urn("Zone", "centre")?;
    let zone = |id: &EntityId, location: Shape| {
        Entity::builder()
            .id(id.clone())
            .entity_type(zone_type.clone())
            .geo_property("location", location)
            .build()
    };
    let entities = [
        zone(
            &antimeridian,
            Shape::Polygon(polygon(&[&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)]])?),
        )?,
        zone(&bowed, rectangle(-30.0, 50.0, 30.0, 60.0)?)?,
        zone(&centre, rectangle(-10.0, -10.0, 10.0, 10.0)?)?,
    ];
    let by_the_antimeridian = geoquery::near_max(25_000, Shape::Point(Position::try_new(180.0, 0.0)?)).build()?;
    let near_the_square = geoquery::near_max(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?;
    let beyond_the_square = geoquery::near_min(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?;
    let near_the_far_square = geoquery::near_max(1000, rectangle(170.0, 0.0, 180.0, 10.0)?).build()?;
    let beyond_the_far_square = geoquery::near_min(1000, rectangle(170.0, 0.0, 180.0, 10.0)?).build()?;
    let under_the_bow = geoquery::near_max(1000, Shape::Point(Position::try_new(0.0, 63.0)?)).build()?;
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
            selects: &[&antimeridian, &bowed],
        },
        GeoCase {
            label: "near;maxDistance==1000 around the square (170,0) to (180,10)",
            geo_q: &near_the_far_square,
            selects: &[&antimeridian],
        },
        GeoCase {
            label: "near;minDistance==1000 around the square (170,0) to (180,10)",
            geo_q: &beyond_the_far_square,
            selects: &[&bowed, &centre],
        },
        GeoCase {
            label: "near;maxDistance==1000 around (0,63)",
            geo_q: &under_the_bow,
            selects: &[&bowed],
        },
    ];
    expect_geo_query_and_subscription_alike(&broker, &namespace, &zone_type, &entities, &cases).await
}
