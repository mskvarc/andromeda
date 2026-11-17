//! A temporal near reads a polygon across the antimeridian and apart from an antipodal corner.

use andromeda_test_suite::{
    broker,
    geometry_round_trip::{polygon, rectangle},
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        EntityId,
        GeoProperty,
        NgsiLdMediaType,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{geoquery, geoquery::GeoQuery, temporal_query},
};
use std::collections::HashSet;

/// Pins: for Zones whose location history holds the polygon (170,-10), (-170,-10), (-170,10), (170,10) for the first
/// and the square (-10,-10) to (10,10) for the second, a temporal query with near;maxDistance==25000 around (180,0)
/// answers the first, with near;maxDistance==1000 around the square (0,0) to (10,10) the second, with
/// near;minDistance==1000 around it the first, with near;maxDistance==1000 around the square (170,0) to (180,10) the
/// first and with near;minDistance==1000 around it the second; the first polygon is the one 20 degrees wide around
/// the antimeridian, and each square lies 16 890 km from the polygon one of whose corners is the antipode of one of
/// its own
/// Clause: 4.10, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9095_a_temporal_near_reads_a_polygon_across_the_antimeridian_and_apart_from_an_antipodal_corner() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let zone_type = namespace.entity_type("Zone")?;
    let antimeridian = namespace.urn("Zone", "antimeridian")?;
    let centre = namespace.urn("Zone", "centre")?;
    for (id, location) in [
        (
            &antimeridian,
            Shape::Polygon(polygon(&[&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)]])?),
        ),
        (&centre, rectangle(-10.0, -10.0, 10.0, 10.0)?),
    ] {
        let evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(zone_type.clone())
            .geo_property("location", GeoProperty::builder(location).observed_at("2020-08-01T12:03:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let cases: [(&str, GeoQuery, &[&EntityId]); 5] = [
        (
            "near;maxDistance==25000 around (180,0)",
            geoquery::near_max(25_000, Shape::Point(Position::try_new(180.0, 0.0)?)).build()?,
            &[&antimeridian],
        ),
        (
            "near;maxDistance==1000 around the square (0,0) to (10,10)",
            geoquery::near_max(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?,
            &[&centre],
        ),
        (
            "near;minDistance==1000 around the square (0,0) to (10,10)",
            geoquery::near_min(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?,
            &[&antimeridian],
        ),
        (
            "near;maxDistance==1000 around the square (170,0) to (180,10)",
            geoquery::near_max(1000, rectangle(170.0, 0.0, 180.0, 10.0)?).build()?,
            &[&antimeridian],
        ),
        (
            "near;minDistance==1000 around the square (170,0) to (180,10)",
            geoquery::near_min(1000, rectangle(170.0, 0.0, 180.0, 10.0)?).build()?,
            &[&centre],
        ),
    ];
    for (label, geo_q, selects) in cases {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
            .geo_q(geo_q)
            .entity_type(&zone_type)
            .send()
            .await?;
        let answered: HashSet<EntityId> = page.items().iter().map(|zone| zone.id.clone()).collect();
        let expected: HashSet<EntityId> = selects.iter().map(|id| (*id).clone()).collect();
        expect_that!(answered, eq(&expected), "a temporal query with {label}");
    }
    Ok(())
}
